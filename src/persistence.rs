use std::{
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

use parking_lot::{Condvar, Mutex};
use serde_json::{Map, Value};

use crate::{
    controls::{ControlState, Normalized, Revision, VisualIntensity},
    platform::macos::WindowPlacement,
    situations::{Registry, SituationIndex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveStage {
    CreateTemp,
    Write,
    SyncFile,
    Replace,
    SyncDirectory,
    ReadOnly,
    Serialize,
    Spawn,
    StartupChannel,
    WorkerStopped,
    WorkerPanicked,
    UnknownRevision,
    InvalidRevision,
}

#[derive(Clone, Debug)]
pub(crate) struct SaveError {
    pub stage: SaveStage,
    pub message: String,
}

impl SaveError {
    fn new(stage: SaveStage, message: impl ToString) -> Self {
        Self {
            stage,
            message: message.to_string(),
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.stage == SaveStage::SyncDirectory {
            write!(
                formatter,
                "state replaced, but directory durability is unconfirmed: {}",
                self.message
            )
        } else {
            write!(formatter, "{:?}: {}", self.stage, self.message)
        }
    }
}

impl Error for SaveError {}

#[derive(Debug)]
pub(crate) enum WritePolicy {
    Writable,
    ReadOnly(String),
}

#[derive(Debug)]
pub(crate) enum LoadProblem {
    Corrupt(String),
    Unreadable(String),
}

#[derive(Debug)]
pub(crate) enum BackupOutcome {
    Saved(PathBuf),
    Failed(String),
}

#[derive(Debug)]
pub(crate) enum LoadReason {
    Missing,
    FutureVersion(u64),
    Protected {
        problem: LoadProblem,
        backup: BackupOutcome,
    },
}

impl fmt::Display for LoadReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("no saved state"),
            Self::FutureVersion(version) => write!(
                formatter,
                "saved state version {version} is newer than supported version 1; original is read-only"
            ),
            Self::Protected { problem, backup } => {
                match problem {
                    LoadProblem::Corrupt(message) => {
                        write!(formatter, "saved state is corrupt: {message}")?
                    }
                    LoadProblem::Unreadable(message) => {
                        write!(formatter, "saved state is unreadable: {message}")?
                    }
                }
                match backup {
                    BackupOutcome::Saved(path) => write!(
                        formatter,
                        "; original backed up to {}; using defaults",
                        path.display()
                    ),
                    BackupOutcome::Failed(message) => write!(
                        formatter,
                        "; backup failed: {message}; original is read-only; using defaults"
                    ),
                }
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoadedState {
    pub controls: ControlState,
    pub window: Option<WindowPlacement>,
}

#[derive(Debug)]
pub(crate) struct StateDocument {
    raw: Value,
}

#[derive(Debug)]
pub(crate) enum LoadOutcome {
    Loaded {
        settings: LoadedState,
        document: StateDocument,
        write_policy: WritePolicy,
        notice: Option<String>,
    },
    Defaults {
        reason: LoadReason,
        document: StateDocument,
        write_policy: WritePolicy,
    },
}

pub(crate) struct SaveStartup {
    pub settings: LoadedState,
    pub notice: Option<String>,
}

pub(crate) struct SaveSnapshot {
    pub controls: ControlState,
    pub resume_target: SituationIndex,
    pub window: Option<WindowPlacement>,
}

pub(crate) struct SaveReceipt {
    pub revision: Revision,
    pub result: Result<(), SaveError>,
}

static UNIQUE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

#[derive(Default)]
struct IoHooks {
    #[cfg(test)]
    fail: Option<SaveStage>,
    #[cfg(test)]
    deny_backup: bool,
    #[cfg(test)]
    pauses: Vec<(Revision, Arc<IoPause>)>,
    #[cfg(test)]
    panic_revision: Option<Revision>,
    #[cfg(test)]
    startup_pause: Option<Arc<IoPause>>,
}

impl IoHooks {
    fn stage(&self, stage: SaveStage) -> Result<(), SaveError> {
        #[cfg(test)]
        if self.fail == Some(stage) {
            return Err(SaveError::new(stage, "injected filesystem-stage failure"));
        }
        let _ = stage;
        Ok(())
    }

    fn before_save(&self, revision: Revision) {
        #[cfg(test)]
        {
            assert_ne!(self.panic_revision, Some(revision), "injected worker panic");
            for (paused_revision, pause) in &self.pauses {
                if *paused_revision == revision {
                    pause.pause();
                }
            }
        }
        let _ = revision;
    }
}

pub(crate) fn load_state(path: &Path, registry: &Registry) -> LoadOutcome {
    load_with_hooks(path, registry, &IoHooks::default())
}

fn load_with_hooks(path: &Path, registry: &Registry, hooks: &IoHooks) -> LoadOutcome {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return defaults(LoadReason::Missing, WritePolicy::Writable);
        }
        Err(error) => {
            return protect_original(path, LoadProblem::Unreadable(error.to_string()), hooks);
        }
    };
    // In particular, do not block opening a FIFO or follow a symlink to one.
    if !metadata.file_type().is_file() {
        return protect_original(
            path,
            LoadProblem::Unreadable("state path is not a regular file".into()),
            hooks,
        );
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return protect_original(path, LoadProblem::Unreadable(error.to_string()), hooks);
        }
    };
    let raw: Value = match serde_json::from_slice(&bytes) {
        Ok(raw) => raw,
        Err(error) => {
            return protect_original(path, LoadProblem::Corrupt(error.to_string()), hooks);
        }
    };
    let Some(root) = raw.as_object() else {
        return protect_original(
            path,
            LoadProblem::Corrupt("root is not an object".into()),
            hooks,
        );
    };
    let Some(version) = root.get("version").and_then(Value::as_u64) else {
        return protect_original(
            path,
            LoadProblem::Corrupt("version is not an unsigned integer".into()),
            hooks,
        );
    };
    if version > 1 {
        let reason = LoadReason::FutureVersion(version);
        let policy = WritePolicy::ReadOnly(reason.to_string());
        return LoadOutcome::Defaults {
            reason,
            document: StateDocument { raw },
            write_policy: policy,
        };
    }
    if version != 1 {
        return protect_original(
            path,
            LoadProblem::Corrupt("unsupported version".into()),
            hooks,
        );
    }
    let (settings, notice) = normalize(root, registry);
    LoadOutcome::Loaded {
        settings,
        document: StateDocument { raw },
        write_policy: WritePolicy::Writable,
        notice,
    }
}

fn defaults(reason: LoadReason, write_policy: WritePolicy) -> LoadOutcome {
    LoadOutcome::Defaults {
        reason,
        document: StateDocument {
            raw: Value::Object(Map::new()),
        },
        write_policy,
    }
}

fn protect_original(path: &Path, problem: LoadProblem, hooks: &IoHooks) -> LoadOutcome {
    let backup = match backup_original(path, hooks) {
        Ok(path) => BackupOutcome::Saved(path),
        Err(error) => BackupOutcome::Failed(error.to_string()),
    };
    let writable = matches!(backup, BackupOutcome::Saved(_));
    let reason = LoadReason::Protected { problem, backup };
    let policy = if writable {
        WritePolicy::Writable
    } else {
        WritePolicy::ReadOnly(reason.to_string())
    };
    defaults(reason, policy)
}

fn backup_original(path: &Path, hooks: &IoHooks) -> io::Result<PathBuf> {
    #[cfg(test)]
    if hooks.deny_backup {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected backup denial",
        ));
    }
    let _ = hooks;
    // Reserve a unique directory with create_dir, which never overwrites another
    // backup. Rename into it works for files and nonregular originals alike.
    loop {
        let directory = parent_directory(path).join(unique_name("state-backup"));
        match fs::create_dir(&directory) {
            Ok(()) => {
                let destination = directory.join("state.json");
                match fs::rename(path, &destination) {
                    Ok(()) => return Ok(destination),
                    Err(error) => {
                        let _ = fs::remove_dir(&directory);
                        return Err(error);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn number(
    value: Option<&Value>,
    fallback: f64,
    low: f64,
    high: f64,
    name: &str,
    repairs: &mut Vec<String>,
) -> f64 {
    match value {
        None => fallback,
        Some(value) => match value.as_f64().filter(|value| value.is_finite()) {
            Some(value) => {
                let clamped = value.clamp(low, high);
                if clamped != value {
                    repairs.push(name.into());
                }
                clamped
            }
            None => {
                repairs.push(name.into());
                fallback
            }
        },
    }
}

fn boolean(value: Option<&Value>, fallback: bool, name: &str, repairs: &mut Vec<String>) -> bool {
    match value {
        None => fallback,
        Some(value) => match value.as_bool() {
            Some(value) => value,
            None => {
                repairs.push(name.into());
                fallback
            }
        },
    }
}

fn time_setting(
    root: &Map<String, Value>,
    key: &str,
    fallback: u32,
    max: u32,
    repairs: &mut Vec<String>,
) -> u32 {
    let value = number(
        root.get(key),
        f64::from(fallback),
        1.0,
        f64::from(max),
        key,
        repairs,
    );
    let rounded = value.round();
    if rounded != value {
        repairs.push(key.into());
    }
    rounded as u32
}

/// The state file under the user's Application Support folder; its directory also holds the
/// background image copy.
pub(crate) fn default_state_path() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/Ambien/state.json"))
        .ok_or_else(|| "HOME がなく、状態保存先を決められません".to_owned())
}

/// Replaces `path` with `bytes` through a same-directory temporary file (mode 0600).
pub(crate) fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    atomic_replace(path, bytes, &IoHooks::default())
}

/// The saved name of the background image: the file name inside the support folder, or null.
fn background_image(value: Option<&Value>, fallback: bool, repairs: &mut Vec<String>) -> bool {
    match value {
        None | Some(Value::Null) => value.map_or(fallback, |_| false),
        Some(Value::String(name)) if name == crate::background::FILE_NAME => true,
        Some(_) => {
            repairs.push("background_image".into());
            fallback
        }
    }
}

fn normalize(root: &Map<String, Value>, registry: &Registry) -> (LoadedState, Option<String>) {
    let mut controls = ControlState::new(registry);
    let mut repairs = Vec::new();
    controls.volume = Normalized::new(number(
        root.get("volume"),
        f64::from(controls.volume.get()),
        0.0,
        1.0,
        "volume",
        &mut repairs,
    ) as f32)
    .expect("clamped volume is normalized");
    controls.blur = boolean(root.get("blur"), controls.blur, "blur", &mut repairs);
    controls.eightbit = boolean(
        root.get("eightbit"),
        controls.eightbit,
        "eightbit",
        &mut repairs,
    );
    controls.background_image = background_image(
        root.get("background_image"),
        controls.background_image,
        &mut repairs,
    );
    controls.background_opacity = Normalized::new(number(
        root.get("background_opacity"),
        f64::from(controls.background_opacity.get()),
        0.0,
        1.0,
        "background_opacity",
        &mut repairs,
    ) as f32)
    .expect("clamped background opacity is normalized");
    controls.background_blur = Normalized::new(number(
        root.get("background_blur"),
        f64::from(controls.background_blur.get()),
        0.0,
        1.0,
        "background_blur",
        &mut repairs,
    ) as f32)
    .expect("clamped background blur is normalized");
    controls.visuals = match root.get("visuals") {
        None => controls.visuals,
        Some(value) => value
            .as_str()
            .and_then(VisualIntensity::from_name)
            .unwrap_or_else(|| {
                repairs.push("visuals".into());
                controls.visuals
            }),
    };
    controls.veil = Normalized::new(number(
        root.get("veil"),
        f64::from(controls.veil.get()),
        0.0,
        1.0,
        "veil",
        &mut repairs,
    ) as f32)
    .expect("clamped veil is normalized");
    controls.mute = boolean(root.get("mute"), controls.mute, "mute", &mut repairs);
    controls.wander = boolean(root.get("wander"), controls.wander, "wander", &mut repairs);
    controls.dwell_min_minutes = time_setting(
        root,
        "wander_min_minutes",
        controls.dwell_min_minutes,
        180,
        &mut repairs,
    );
    controls.dwell_max_minutes = time_setting(
        root,
        "wander_max_minutes",
        controls.dwell_max_minutes,
        180,
        &mut repairs,
    );
    controls.transition_seconds = time_setting(
        root,
        "transition_seconds",
        controls.transition_seconds,
        120,
        &mut repairs,
    );
    if controls.dwell_min_minutes > controls.dwell_max_minutes {
        std::mem::swap(
            &mut controls.dwell_min_minutes,
            &mut controls.dwell_max_minutes,
        );
        repairs.push("wander minimum/maximum".into());
    }
    if let Some(last) = root.get("last") {
        match last.as_str().and_then(|id| registry.resolve(id)) {
            Some(index) => controls.manual_target = index,
            None => repairs.push("last".into()),
        }
    }
    let situations = root.get("situations").and_then(Value::as_object);
    if root.contains_key("situations") && situations.is_none() {
        repairs.push("situations".into());
    }
    let mut offset = 0;
    for definition in registry.definitions() {
        let entry = situations.and_then(|situations| situations.get(definition.id));
        let knobs = entry.and_then(Value::as_object);
        if entry.is_some() && knobs.is_none() {
            repairs.push(format!("situations.{}", definition.id));
        }
        for knob in definition.knobs {
            let name = format!("situations.{}.{}", definition.id, knob.id);
            controls.knobs[offset] = Normalized::new(number(
                knobs.and_then(|knobs| knobs.get(knob.id)),
                f64::from(knob.initial.get()),
                0.0,
                1.0,
                &name,
                &mut repairs,
            ) as f32)
            .expect("clamped knob is normalized");
            offset += 1;
        }
    }
    let window = normalize_window(root.get("window"), &mut repairs);
    let notice = if repairs.is_empty() {
        None
    } else {
        Some(format!("Saved state repaired: {}", repairs.join(", ")))
    };
    (LoadedState { controls, window }, notice)
}

fn normalize_window(value: Option<&Value>, repairs: &mut Vec<String>) -> Option<WindowPlacement> {
    let value = value?;
    let Some(window) = value.as_object() else {
        repairs.push("window".into());
        return None;
    };
    let finite = |key| {
        window
            .get(key)
            .and_then(Value::as_f64)
            .filter(|number| number.is_finite())
    };
    let coordinates = (finite("x"), finite("y"), finite("width"), finite("height"));
    let (Some(x), Some(y), Some(width), Some(height)) = coordinates else {
        repairs.push("window rectangle".into());
        return None;
    };
    if width <= 0.0 || height <= 0.0 {
        repairs.push("window dimensions".into());
        return None;
    }
    let display_uuid = match window.get("display_uuid") {
        None | Some(Value::Null) => None,
        Some(Value::String(uuid)) => Some(uuid.clone()),
        Some(_) => {
            repairs.push("window.display_uuid".into());
            None
        }
    };
    Some(WindowPlacement {
        x,
        y,
        width,
        height,
        display_uuid,
    })
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("object was initialized")
}

impl StateDocument {
    fn merge(&mut self, registry: &Registry, snapshot: &SaveSnapshot) {
        let root = object(&mut self.raw);
        let controls = &snapshot.controls;
        root.insert("version".into(), Value::from(1));
        root.insert("volume".into(), Value::from(controls.volume.get()));
        root.insert("blur".into(), Value::from(controls.blur));
        root.insert("eightbit".into(), Value::from(controls.eightbit));
        root.insert(
            "background_image".into(),
            if controls.background_image {
                Value::from(crate::background::FILE_NAME)
            } else {
                Value::Null
            },
        );
        root.insert(
            "background_opacity".into(),
            Value::from(controls.background_opacity.get()),
        );
        root.insert(
            "background_blur".into(),
            Value::from(controls.background_blur.get()),
        );
        root.insert("veil".into(), Value::from(controls.veil.get()));
        root.insert("visuals".into(), Value::from(controls.visuals.name()));
        root.insert("mute".into(), Value::from(controls.mute));
        root.insert("wander".into(), Value::from(controls.wander));
        root.insert(
            "wander_min_minutes".into(),
            Value::from(controls.dwell_min_minutes),
        );
        root.insert(
            "wander_max_minutes".into(),
            Value::from(controls.dwell_max_minutes),
        );
        root.insert(
            "transition_seconds".into(),
            Value::from(controls.transition_seconds),
        );
        root.insert(
            "last".into(),
            Value::from(registry.definitions()[snapshot.resume_target.get()].id),
        );
        let situations = object(root.entry("situations").or_insert(Value::Null));
        let mut offset = 0;
        for definition in registry.definitions() {
            let knobs = object(situations.entry(definition.id).or_insert(Value::Null));
            for knob in definition.knobs {
                knobs.insert(knob.id.into(), Value::from(controls.knobs[offset].get()));
                offset += 1;
            }
        }
        if let Some(placement) = &snapshot.window {
            let window = object(root.entry("window").or_insert(Value::Null));
            window.insert("x".into(), Value::from(placement.x));
            window.insert("y".into(), Value::from(placement.y));
            window.insert("width".into(), Value::from(placement.width));
            window.insert("height".into(), Value::from(placement.height));
            window.insert(
                "display_uuid".into(),
                placement
                    .display_uuid
                    .as_ref()
                    .map_or(Value::Null, |uuid| Value::from(uuid.as_str())),
            );
        }
    }

    fn save(
        &mut self,
        path: &Path,
        registry: &Registry,
        policy: &WritePolicy,
        snapshot: &SaveSnapshot,
        hooks: &IoHooks,
    ) -> Result<(), SaveError> {
        if let WritePolicy::ReadOnly(reason) = policy {
            return Err(SaveError::new(SaveStage::ReadOnly, reason));
        }
        self.merge(registry, snapshot);
        let bytes = serde_json::to_vec(&self.raw)
            .map_err(|error| SaveError::new(SaveStage::Serialize, error))?;
        atomic_replace(path, &bytes, hooks)
    }
}

struct OwnedTemp {
    path: PathBuf,
    present: bool,
}

impl Drop for OwnedTemp {
    fn drop(&mut self) {
        // Only this create_new-owned name is cleaned, never the destination.
        if self.present {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn atomic_replace(path: &Path, bytes: &[u8], hooks: &IoHooks) -> Result<(), SaveError> {
    let parent = parent_directory(path);
    hooks.stage(SaveStage::CreateTemp)?;
    fs::create_dir_all(parent).map_err(|error| SaveError::new(SaveStage::CreateTemp, error))?;
    let (mut file, mut temp) = loop {
        let temp = parent.join(unique_name(".state-temp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => {
                break (
                    file,
                    OwnedTemp {
                        path: temp,
                        present: true,
                    },
                );
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(SaveError::new(SaveStage::CreateTemp, error)),
        }
    };
    hooks.stage(SaveStage::Write)?;
    file.write_all(bytes)
        .map_err(|error| SaveError::new(SaveStage::Write, error))?;
    hooks.stage(SaveStage::SyncFile)?;
    file.sync_all()
        .map_err(|error| SaveError::new(SaveStage::SyncFile, error))?;
    drop(file);
    hooks.stage(SaveStage::Replace)?;
    fs::rename(&temp.path, path).map_err(|error| SaveError::new(SaveStage::Replace, error))?;
    temp.present = false;
    hooks.stage(SaveStage::SyncDirectory)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| SaveError::new(SaveStage::SyncDirectory, error))
}

#[derive(Default)]
struct Completion {
    result: Mutex<Option<Result<(), SaveError>>>,
    changed: Condvar,
}

impl Completion {
    fn finish(&self, result: Result<(), SaveError>) {
        *self.result.lock() = Some(result);
        self.changed.notify_all();
    }

    fn wait(&self) -> Result<(), SaveError> {
        let mut result = self.result.lock();
        while result.is_none() {
            self.changed.wait(&mut result);
        }
        result
            .as_ref()
            .expect("completion became available")
            .clone()
    }
}

#[derive(Clone)]
struct Generation {
    revision: Revision,
    completion: Arc<Completion>,
}

struct SaveRequest {
    generation: Generation,
    snapshot: SaveSnapshot,
}

#[derive(Default)]
struct WorkerState {
    pending: Option<SaveRequest>,
    in_flight: Option<Generation>,
    latest: Option<Revision>,
    completed: Option<Generation>,
    receipt: Option<SaveReceipt>,
    stop: bool,
    terminal: Option<SaveError>,
    #[cfg(test)]
    flush_waiters: usize,
}

#[derive(Default)]
struct Shared {
    state: Mutex<WorkerState>,
    changed: Condvar,
}

pub(crate) struct SaveWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl SaveWorker {
    pub(crate) fn start(
        path: PathBuf,
        registry: Arc<Registry>,
    ) -> Result<
        (
            Self,
            async_channel::Receiver<SaveStartup>,
            async_channel::Receiver<()>,
        ),
        SaveError,
    > {
        Self::start_with_hooks(path, registry, IoHooks::default())
    }

    fn start_with_hooks(
        path: PathBuf,
        registry: Arc<Registry>,
        hooks: IoHooks,
    ) -> Result<
        (
            Self,
            async_channel::Receiver<SaveStartup>,
            async_channel::Receiver<()>,
        ),
        SaveError,
    > {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let (startup_sender, startup_receiver) = async_channel::bounded(1);
        let (notifications, notification_receiver) = async_channel::bounded(1);
        let handle = thread::Builder::new()
            .name("ambien-state".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_worker(
                        &path,
                        &registry,
                        &worker_shared,
                        startup_sender,
                        &notifications,
                        &hooks,
                    )
                }));
                let terminal = match result {
                    Ok(Ok(())) => SaveError::new(SaveStage::WorkerStopped, "save worker stopped"),
                    Ok(Err(error)) => error,
                    Err(_) => SaveError::new(SaveStage::WorkerPanicked, "save worker panicked"),
                };
                let mut state = worker_shared.state.lock();
                state.terminal = Some(terminal.clone());
                let in_flight = state.in_flight.take();
                let pending = state.pending.take();
                if let Some(generation) = &in_flight {
                    generation.completion.finish(Err(terminal.clone()));
                }
                if let Some(request) = &pending {
                    request.generation.completion.finish(Err(terminal.clone()));
                }
                if let Some(generation) = pending.map(|request| request.generation).or(in_flight) {
                    state.receipt = Some(SaveReceipt {
                        revision: generation.revision,
                        result: Err(terminal),
                    });
                    state.completed = Some(generation);
                    let _ = notifications.try_send(());
                }
                worker_shared.changed.notify_all();
                notifications.close();
            })
            .map_err(|error| SaveError::new(SaveStage::Spawn, error))?;
        Ok((
            Self {
                shared,
                thread: Some(handle),
            },
            startup_receiver,
            notification_receiver,
        ))
    }

    pub(crate) fn submit(
        &self,
        revision: Revision,
        snapshot: SaveSnapshot,
    ) -> Result<(), SaveError> {
        let mut state = self.shared.state.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.stop {
            return Err(SaveError::new(
                SaveStage::WorkerStopped,
                "save worker is stopping",
            ));
        }
        if state
            .latest
            .is_some_and(|latest| revision.get() <= latest.get())
        {
            return Err(SaveError::new(
                SaveStage::InvalidRevision,
                "save revisions must strictly increase",
            ));
        }
        state.latest = Some(revision);
        let request = SaveRequest {
            generation: Generation {
                revision,
                completion: Arc::new(Completion::default()),
            },
            snapshot,
        };
        if let Some(superseded) = state.pending.replace(request) {
            superseded.generation.completion.finish(Err(SaveError::new(
                SaveStage::UnknownRevision,
                "pending save generation was superseded",
            )));
        }
        self.shared.changed.notify_all();
        Ok(())
    }

    pub(crate) fn take_receipt(&self) -> Option<SaveReceipt> {
        self.shared.state.lock().receipt.take()
    }

    /// Only tracked exact generations can be waited on. A superseded pending
    /// generation, or an expired older receipt, is explicitly unknown.
    pub(crate) fn flush(&self, revision: Revision) -> Result<(), SaveError> {
        let state = self.shared.state.lock();
        #[cfg(test)]
        let mut state = state;
        let generation = state
            .pending
            .as_ref()
            .map(|request| &request.generation)
            .filter(|generation| generation.revision == revision)
            .or_else(|| {
                state
                    .in_flight
                    .as_ref()
                    .filter(|generation| generation.revision == revision)
            })
            .or_else(|| {
                state
                    .completed
                    .as_ref()
                    .filter(|generation| generation.revision == revision)
            })
            .cloned()
            .ok_or_else(|| {
                SaveError::new(
                    SaveStage::UnknownRevision,
                    format!(
                        "save generation {} is not pending, in flight, or retained",
                        revision.get()
                    ),
                )
            })?;
        #[cfg(test)]
        {
            state.flush_waiters += 1;
            self.shared.changed.notify_all();
        }
        drop(state);
        // A flush retains only its exact completion, not the original JSON or
        // a history queue. Newer completions cannot overwrite its result.
        let result = generation.completion.wait();
        #[cfg(test)]
        {
            self.shared.state.lock().flush_waiters -= 1;
        }
        result
    }

    #[cfg(test)]
    fn wait_until_flush_waits(&self) {
        let mut state = self.shared.state.lock();
        while state.flush_waiters == 0 {
            self.shared.changed.wait(&mut state);
        }
    }
}

impl Drop for SaveWorker {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock();
            state.stop = true;
            self.shared.changed.notify_all();
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_worker(
    path: &Path,
    registry: &Registry,
    shared: &Shared,
    startup_sender: async_channel::Sender<SaveStartup>,
    notifications: &async_channel::Sender<()>,
    hooks: &IoHooks,
) -> Result<(), SaveError> {
    #[cfg(test)]
    let outcome = load_with_hooks(path, registry, hooks);
    #[cfg(not(test))]
    let outcome = load_state(path, registry);
    let (settings, mut document, policy, notice) = match outcome {
        LoadOutcome::Loaded {
            settings,
            document,
            write_policy,
            notice,
        } => (settings, document, write_policy, notice),
        LoadOutcome::Defaults {
            reason,
            document,
            write_policy,
        } => {
            let notice = if matches!(reason, LoadReason::Missing) {
                None
            } else {
                Some(reason.to_string())
            };
            (
                LoadedState {
                    controls: ControlState::new(registry),
                    window: None,
                },
                document,
                write_policy,
                notice,
            )
        }
    };
    #[cfg(test)]
    if let Some(pause) = &hooks.startup_pause {
        pause.pause();
    }
    startup_sender
        .try_send(SaveStartup { settings, notice })
        .map_err(|error| SaveError::new(SaveStage::StartupChannel, error))?;
    drop(startup_sender);
    loop {
        let request = {
            let mut state = shared.state.lock();
            loop {
                if let Some(request) = state.pending.take() {
                    state.in_flight = Some(request.generation.clone());
                    break request;
                }
                if state.stop {
                    return Ok(());
                }
                shared.changed.wait(&mut state);
            }
        };
        hooks.before_save(request.generation.revision);
        let result = document.save(path, registry, &policy, &request.snapshot, hooks);
        let mut state = shared.state.lock();
        request.generation.completion.finish(result.clone());
        state.in_flight = None;
        state.receipt = Some(SaveReceipt {
            revision: request.generation.revision,
            result,
        });
        state.completed = Some(request.generation);
        // Full means an earlier wake is already queued. Closed receivers do
        // not affect I/O completion or the independent flush condition.
        let _ = notifications.try_send(());
        shared.changed.notify_all();
    }
}

#[cfg(test)]
#[derive(Default)]
struct IoPause {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

#[cfg(test)]
impl IoPause {
    fn pause(&self) {
        let mut state = self.state.lock();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            self.changed.wait(&mut state);
        }
    }

    fn wait_until_paused(&self) {
        let mut state = self.state.lock();
        while !state.0 {
            self.changed.wait(&mut state);
        }
    }

    fn release(&self) {
        let mut state = self.state.lock();
        state.1 = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::sync::mpsc;
    use std::time::Duration;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            loop {
                let path = std::env::temp_dir().join(unique_name("ambien-state-test"));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("private fixture: {error}"),
                }
            }
        }

        fn path(&self) -> PathBuf {
            self.0.join("state.json")
        }

        fn write(&self, value: &Value) {
            fs::write(self.path(), serde_json::to_vec(value).unwrap()).unwrap();
        }

        fn entries(&self) -> Vec<PathBuf> {
            fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn registry() -> Arc<Registry> {
        Arc::new(Registry::new().unwrap())
    }

    fn loaded(outcome: LoadOutcome) -> (LoadedState, StateDocument, WritePolicy, Option<String>) {
        match outcome {
            LoadOutcome::Loaded {
                settings,
                document,
                write_policy,
                notice,
            } => (settings, document, write_policy, notice),
            LoadOutcome::Defaults { reason, .. } => panic!("unexpected defaults: {reason:?}"),
        }
    }

    fn snapshot(registry: &Registry, volume: f32) -> SaveSnapshot {
        let mut controls = ControlState::new(registry);
        controls.volume = Normalized::new(volume).unwrap();
        SaveSnapshot {
            resume_target: registry.first_index(),
            controls,
            window: None,
        }
    }

    #[test]
    fn missing_is_normal_and_missing_fields_use_metadata_without_control_edits() {
        let fixture = Fixture::new();
        let registry = registry();
        assert!(matches!(
            load_state(&fixture.path(), &registry),
            LoadOutcome::Defaults {
                reason: LoadReason::Missing,
                write_policy: WritePolicy::Writable,
                ..
            }
        ));
        fixture.write(&json!({"version": 1}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls, ControlState::new(&registry));
        assert_eq!(state.window, None);
        assert_eq!(notice, None);
        assert_eq!(
            fs::metadata(&fixture.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn normalization_clamps_rounds_swaps_and_repairs_types() {
        let fixture = Fixture::new();
        let registry = registry();
        let definition = &registry.definitions()[0];
        let mut knobs = Map::new();
        knobs.insert(definition.knobs[0].id.into(), json!(2.0));
        let mut situations = Map::new();
        situations.insert(definition.id.into(), Value::Object(knobs));
        fixture.write(&json!({"version": 1, "volume": -7, "mute": "wrong",
            "wander": false, "wander_min_minutes": 20.4, "wander_max_minutes": 10.5,
            "transition_seconds": 900, "last": "not-registered", "situations": situations}));
        let (state, _, policy, notice) = loaded(load_state(&fixture.path(), &registry));
        let defaults = ControlState::new(&registry);
        assert_eq!(state.controls.volume, Normalized::ZERO);
        assert_eq!(state.controls.mute, defaults.mute);
        assert!(!state.controls.wander);
        assert_eq!(
            (
                state.controls.dwell_min_minutes,
                state.controls.dwell_max_minutes
            ),
            (11, 20)
        );
        assert_eq!(state.controls.transition_seconds, 120);
        assert_eq!(state.controls.knobs[0], Normalized::ONE);
        assert_eq!(state.controls.knobs[1..], defaults.knobs[1..]);
        assert_eq!(state.controls.manual_target, registry.first_index());
        assert_eq!(state.controls.manual_revision, Revision::INITIAL);
        assert_eq!(state.controls.settings_revision, Revision::INITIAL);
        assert_eq!(state.controls.wander_revision, Revision::INITIAL);
        assert!(notice.is_some());
        assert!(matches!(policy, WritePolicy::Writable));
        fixture.write(&json!({"version":1, "volume":2, "wander_min_minutes":-2,
            "wander_max_minutes":200, "transition_seconds":0.1}));
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::ONE);
        assert_eq!(
            (
                state.controls.dwell_min_minutes,
                state.controls.dwell_max_minutes
            ),
            (1, 180)
        );
        assert_eq!(state.controls.transition_seconds, 1);
    }

    #[test]
    fn merge_roundtrip_preserves_unknowns_and_none_window_preserves_existing_geometry() {
        let fixture = Fixture::new();
        let registry = registry();
        let definition = &registry.definitions()[0];
        let original = json!({"version":1, "extra":{"nested":[3,true]},
            "window":{"x":-20,"y":15,"width":640,"height":480,
                "display_uuid":"display", "future":{"a":1}},
            "situations":{(definition.id):{(definition.knobs[0].id):0.4,"removed": {"x":8}},
                "unregistered":{"future_knob":0.8}}});
        fixture.write(&original);
        let (worker, startup, notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        let initial = startup.recv_blocking().unwrap();
        let saved_window = initial.settings.window.unwrap();
        assert_eq!((saved_window.x, saved_window.y), (-20.0, 15.0));
        let revision = Revision::from_raw(1);
        let mut request = snapshot(&registry, 0.83);
        request.controls.knobs[0] = Normalized::new(0.91).unwrap();
        request.resume_target = SituationIndex::from_index(
            registry.definitions().len() - 1,
            registry.definitions().len(),
        )
        .unwrap();
        let target = request.resume_target;
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        notices.recv_blocking().unwrap();
        assert_eq!(worker.take_receipt().unwrap().revision, revision);
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["extra"], original["extra"]);
        assert_eq!(
            saved["situations"]["unregistered"],
            original["situations"]["unregistered"]
        );
        assert_eq!(
            saved["situations"][definition.id]["removed"],
            original["situations"][definition.id]["removed"]
        );
        assert_eq!(saved["window"], original["window"]);
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.83).unwrap());
        assert_eq!(state.controls.knobs[0], Normalized::new(0.91).unwrap());
        assert_eq!(state.controls.manual_target, target);
        let revision = Revision::from_raw(2);
        let mut request = snapshot(&registry, 0.6);
        request.window = Some(WindowPlacement {
            x: 2.0,
            y: 3.0,
            width: 800.0,
            height: 500.0,
            display_uuid: None,
        });
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["window"]["future"], original["window"]["future"]);
        assert_eq!(saved["window"]["x"], json!(2.0));
        assert_eq!(saved["window"]["display_uuid"], Value::Null);
    }

    #[test]
    fn autumn_knobs_default_clamp_round_trip_and_keep_knob_keys_this_version_does_not_know() {
        let fixture = Fixture::new();
        let registry = registry();
        let autumn = registry.resolve("autumn").unwrap();
        let range = registry.knob_range(autumn);
        let normalized = |values: [f32; 2]| values.map(|value| Normalized::new(value).unwrap());
        // Absent: the registered initial values, and the situation is a valid resume target.
        fixture.write(&json!({"version":1, "last":"autumn"}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.manual_target, autumn);
        assert_eq!(
            state.controls.knobs[range.clone()],
            normalized([0.55, 0.20])
        );
        assert_eq!(notice, None);
        // Out of range is clamped; a knob from some other version is left alone.
        fixture.write(&json!({"version":1, "situations":{"autumn":{
            "insects":1.7, "wind":"loud", "frogs":0.9}}}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.knobs[range.clone()], normalized([1.0, 0.20]));
        assert!(notice.is_some(), "the wrong type is reported as repaired");
        // A save writes the two knobs, keeps the unknown one and loads back the same values.
        let (worker, startup, notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let revision = Revision::from_raw(1);
        let mut request = snapshot(&registry, 0.5);
        request.controls.knobs[range.start] = Normalized::new(0.25).unwrap();
        request.controls.knobs[range.start + 1] = Normalized::new(0.75).unwrap();
        request.resume_target = autumn;
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        notices.recv_blocking().unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["situations"]["autumn"]["frogs"], json!(0.9));
        assert!(saved["situations"]["autumn"].get("insects").is_some());
        assert!(saved["situations"]["autumn"].get("wind").is_some());
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.knobs[range], normalized([0.25, 0.75]));
        assert_eq!(state.controls.manual_target, autumn);
    }

    #[test]
    fn visual_intensity_defaults_to_calm_repairs_unknown_names_and_round_trips() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(&json!({"version":1}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.visuals, VisualIntensity::Calm);
        assert!(notice.is_none());
        for bad in [json!("loud"), json!(3), json!(true), json!(null)] {
            fixture.write(&json!({"version":1, "visuals": bad}));
            let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
            assert_eq!(state.controls.visuals, VisualIntensity::Calm);
            assert!(notice.unwrap().contains("visuals"));
        }
        fixture.write(&json!({"version":1, "visuals": "standard"}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.visuals, VisualIntensity::Standard);
        assert!(notice.is_none());
        let (worker, startup, _notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let revision = Revision::from_raw(1);
        let mut request = snapshot(&registry, 0.5);
        request.controls.visuals = VisualIntensity::Vivid;
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["visuals"], json!("vivid"));
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.visuals, VisualIntensity::Vivid);
    }

    #[test]
    fn blur_and_veil_default_repair_and_roundtrip_with_unknown_keys_kept() {
        let fixture = Fixture::new();
        let registry = registry();
        // An older file without the keys keeps today's glass: blurred, no veil.
        fixture.write(&json!({"version":1, "volume":0.4}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(state.controls.blur);
        assert_eq!(state.controls.veil, Normalized::ZERO);
        assert!(notice.is_none());
        // Wrong types fall back to the defaults, out-of-range values clamp to the ends.
        fixture.write(&json!({"version":1, "blur":"yes", "veil":7}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(state.controls.blur);
        assert_eq!(state.controls.veil, Normalized::ONE);
        assert!(notice.unwrap().contains("blur"));
        // A saved choice survives a round trip next to keys this version does not know.
        fixture.write(&json!({"version":1, "future_display": {"mode": 2}}));
        let (worker, startup, _notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let revision = Revision::from_raw(1);
        let mut request = snapshot(&registry, 0.5);
        request.controls.blur = false;
        request.controls.veil = Normalized::new(0.35).unwrap();
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["future_display"], json!({"mode": 2}));
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert!(!state.controls.blur);
        assert_eq!(state.controls.veil, Normalized::new(0.35).unwrap());
    }

    #[test]
    fn eight_bit_defaults_off_repairs_a_wrong_type_and_round_trips() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(&json!({"version":1, "volume":0.4}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(!state.controls.eightbit);
        assert!(notice.is_none());
        // A wrong type falls back to off and is reported, like the other display settings.
        fixture.write(&json!({"version":1, "eightbit":"yes"}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(!state.controls.eightbit);
        assert!(notice.unwrap().contains("eightbit"));
        fixture.write(&json!({"version":1, "eightbit":true}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(state.controls.eightbit);
        assert!(notice.is_none());
        // A saved choice survives a restart.
        fixture.write(&json!({"version":1}));
        let (worker, startup, _notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let revision = Revision::from_raw(1);
        let mut request = snapshot(&registry, 0.5);
        request.controls.eightbit = true;
        worker.submit(revision, request).unwrap();
        worker.flush(revision).unwrap();
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert!(state.controls.eightbit);
    }

    #[test]
    fn background_settings_default_repair_clamp_and_round_trip() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(&json!({"version":1}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(!state.controls.background_image);
        assert_eq!(state.controls.background_opacity.get(), 0.7);
        assert_eq!(state.controls.background_blur.get(), 0.0);
        assert!(notice.is_none());
        // Only the saved file name or null is accepted; anything else returns to the default.
        for wrong in [
            json!("other.png"),
            json!("../background.png"),
            json!(true),
            json!(3),
        ] {
            fixture.write(&json!({"version":1, "background_image":wrong}));
            let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
            assert!(!state.controls.background_image);
            assert!(notice.unwrap().contains("background_image"));
        }
        fixture.write(&json!({"version":1, "background_image":null}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(!state.controls.background_image);
        assert!(notice.is_none());
        fixture.write(&json!({"version":1, "background_image":"background.png"}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(state.controls.background_image);
        assert!(notice.is_none());
        // Wrong types fall back to the defaults; out-of-range numbers go to the nearest end.
        fixture.write(&json!({"version":1, "background_opacity":"x", "background_blur":[1]}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.background_opacity.get(), 0.7);
        assert_eq!(state.controls.background_blur.get(), 0.0);
        let notice = notice.unwrap();
        assert!(notice.contains("background_opacity") && notice.contains("background_blur"));
        fixture.write(&json!({"version":1, "background_opacity":4.0, "background_blur":-2}));
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.background_opacity.get(), 1.0);
        assert_eq!(state.controls.background_blur.get(), 0.0);
        // The choices survive a restart, and clearing the image saves null.
        fixture.write(&json!({"version":1}));
        let (worker, startup, _notices) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let mut request = snapshot(&registry, 0.5);
        request.controls.background_image = true;
        request.controls.background_opacity = Normalized::new(0.25).unwrap();
        request.controls.background_blur = Normalized::new(0.5).unwrap();
        worker.submit(Revision::from_raw(1), request).unwrap();
        worker.flush(Revision::from_raw(1)).unwrap();
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(notice.is_none());
        assert!(state.controls.background_image);
        assert_eq!(state.controls.background_opacity.get(), 0.25);
        assert_eq!(state.controls.background_blur.get(), 0.5);
        let mut request = snapshot(&registry, 0.5);
        request.controls.background_image = false;
        worker.submit(Revision::from_raw(2), request).unwrap();
        worker.flush(Revision::from_raw(2)).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(saved["background_image"], Value::Null);
    }

    #[test]
    fn invalid_window_is_not_invented_and_invalid_objects_are_repaired_on_save() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(
            &json!({"version":1,"window":{"x":1,"y":2,"width":0,"height":5},
            "situations":false,"volume":null}),
        );
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert!(notice.is_some());
        assert!(state.window.is_none());
        assert_eq!(state.controls, ControlState::new(&registry));
        let (worker, startup, _) = SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.5))
            .unwrap();
        worker.flush(Revision::INITIAL).unwrap();
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.5).unwrap());
        assert_eq!(state.controls.knobs, ControlState::new(&registry).knobs);
    }

    #[test]
    fn future_version_never_backs_up_or_changes_bytes() {
        let fixture = Fixture::new();
        let registry = registry();
        let bytes = b"{\"version\":99,\"future\": [1, 2]}\n";
        fs::write(fixture.path(), bytes).unwrap();
        assert!(matches!(
            load_state(&fixture.path(), &registry),
            LoadOutcome::Defaults {
                reason: LoadReason::FutureVersion(99),
                write_policy: WritePolicy::ReadOnly(_),
                ..
            }
        ));
        let (worker, startup, _) = SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        let startup = startup.recv_blocking().unwrap();
        assert_eq!(startup.settings.controls, ControlState::new(&registry));
        assert!(startup.notice.is_some());
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.9))
            .unwrap();
        assert_eq!(
            worker.flush(Revision::INITIAL).unwrap_err().stage,
            SaveStage::ReadOnly
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), bytes);
        assert_eq!(fixture.entries(), vec![fixture.path()]);
    }

    #[test]
    fn corruption_uses_distinct_backups_and_only_success_allows_writes() {
        let fixture = Fixture::new();
        let registry = registry();
        let mut backups = Vec::new();
        for bytes in [
            b"not json".as_slice(),
            b"[]",
            b"{\"version\":0}",
            b"{\"version\":\"one\"}",
        ] {
            fs::write(fixture.path(), bytes).unwrap();
            match load_state(&fixture.path(), &registry) {
                LoadOutcome::Defaults {
                    reason:
                        LoadReason::Protected {
                            backup: BackupOutcome::Saved(path),
                            ..
                        },
                    write_policy: WritePolicy::Writable,
                    ..
                } => {
                    assert_eq!(fs::read(&path).unwrap(), bytes);
                    assert!(!backups.contains(&path));
                    backups.push(path);
                }
                _ => panic!("corrupt original was not protected"),
            }
            assert!(!fixture.path().exists());
        }
        assert_eq!(fixture.entries().len(), backups.len());
    }

    #[test]
    fn failed_backup_protects_original_and_reports_a_real_save_failure() {
        let fixture = Fixture::new();
        let registry = registry();
        fs::write(fixture.path(), b"broken").unwrap();
        let hooks = IoHooks {
            deny_backup: true,
            ..IoHooks::default()
        };
        let (worker, startup, _) =
            SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
        assert!(startup.recv_blocking().unwrap().notice.is_some());
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.6))
            .unwrap();
        assert_eq!(
            worker.flush(Revision::INITIAL).unwrap_err().stage,
            SaveStage::ReadOnly
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), b"broken");
        assert_eq!(fixture.entries(), vec![fixture.path()]);
    }

    #[test]
    fn nonregular_original_is_not_read_and_its_contents_are_backed_up() {
        let fixture = Fixture::new();
        let registry = registry();
        fs::create_dir(fixture.path()).unwrap();
        fs::write(fixture.path().join("owned"), b"original").unwrap();
        match load_state(&fixture.path(), &registry) {
            LoadOutcome::Defaults {
                reason:
                    LoadReason::Protected {
                        problem: LoadProblem::Unreadable(_),
                        backup: BackupOutcome::Saved(path),
                    },
                write_policy: WritePolicy::Writable,
                ..
            } => {
                assert_eq!(fs::read(path.join("owned")).unwrap(), b"original");
            }
            _ => panic!("nonregular state was not safely backed up"),
        }
    }

    #[test]
    fn pre_replace_failures_leave_old_bytes_and_clean_only_owned_temp() {
        for stage in [
            SaveStage::CreateTemp,
            SaveStage::Write,
            SaveStage::SyncFile,
            SaveStage::Replace,
        ] {
            let fixture = Fixture::new();
            let registry = registry();
            fixture.write(&json!({"version":1,"volume":0.2}));
            let old = fs::read(fixture.path()).unwrap();
            let unrelated = fixture.0.join("unrelated.tmp");
            fs::write(&unrelated, b"leave me").unwrap();
            let hooks = IoHooks {
                fail: Some(stage),
                ..IoHooks::default()
            };
            let (worker, startup, _) =
                SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
            startup.recv_blocking().unwrap();
            worker
                .submit(Revision::INITIAL, snapshot(&registry, 0.8))
                .unwrap();
            assert_eq!(worker.flush(Revision::INITIAL).unwrap_err().stage, stage);
            assert_eq!(
                worker.take_receipt().unwrap().result.unwrap_err().stage,
                stage
            );
            assert_eq!(fs::read(fixture.path()).unwrap(), old);
            assert_eq!(fs::read(unrelated).unwrap(), b"leave me");
            assert_eq!(fixture.entries().len(), 2);
        }
    }

    #[test]
    fn directory_sync_error_reports_replaced_but_not_durable() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(&json!({"version":1,"volume":0.2}));
        let hooks = IoHooks {
            fail: Some(SaveStage::SyncDirectory),
            ..IoHooks::default()
        };
        let (worker, startup, _) =
            SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
        startup.recv_blocking().unwrap();
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.8))
            .unwrap();
        assert_eq!(
            worker.flush(Revision::INITIAL).unwrap_err().stage,
            SaveStage::SyncDirectory
        );
        assert_eq!(
            worker.take_receipt().unwrap().result.unwrap_err().stage,
            SaveStage::SyncDirectory
        );
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.8).unwrap());
        assert_eq!(fixture.entries(), vec![fixture.path()]);
    }

    struct ReleaseOnDrop(Arc<IoPause>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    #[test]
    fn latest_pending_is_bounded_and_flush_waits_for_its_exact_io_generation() {
        let fixture = Fixture::new();
        let registry = registry();
        let r1 = Revision::from_raw(1);
        let r2 = Revision::from_raw(2);
        let r3 = Revision::from_raw(3);
        let first = Arc::new(IoPause::default());
        let latest = Arc::new(IoPause::default());
        let hooks = IoHooks {
            pauses: vec![(r1, first.clone()), (r3, latest.clone())],
            ..IoHooks::default()
        };
        let (worker, startup, notifications) =
            SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
        let worker = Arc::new(worker);
        let _release_first = ReleaseOnDrop(first.clone());
        let _release_latest = ReleaseOnDrop(latest.clone());
        startup.recv_blocking().unwrap();
        worker.submit(r1, snapshot(&registry, 0.1)).unwrap();
        first.wait_until_paused();
        worker.submit(r2, snapshot(&registry, 0.2)).unwrap();
        let (superseded_tx, superseded_rx) = mpsc::channel();
        let superseded_worker = worker.clone();
        let superseded_flush = thread::spawn(move || {
            superseded_tx.send(superseded_worker.flush(r2)).unwrap();
        });
        worker.wait_until_flush_waits();
        worker.submit(r3, snapshot(&registry, 0.9)).unwrap();
        assert_eq!(
            superseded_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err()
                .stage,
            SaveStage::UnknownRevision
        );
        superseded_flush.join().unwrap();
        assert_eq!(
            worker.flush(r2).unwrap_err().stage,
            SaveStage::UnknownRevision
        );
        assert_eq!(
            worker.flush(Revision::from_raw(42)).unwrap_err().stage,
            SaveStage::UnknownRevision
        );
        let (done_tx, done_rx) = mpsc::channel();
        let flushing = worker.clone();
        let flush_thread = thread::spawn(move || {
            done_tx.send(flushing.flush(r3)).unwrap();
        });
        worker.wait_until_flush_waits();
        first.release();
        latest.wait_until_paused();
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.1).unwrap());
        latest.release();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        flush_thread.join().unwrap();
        assert_eq!(notifications.len(), 1);
        let receipt = worker.take_receipt().unwrap();
        assert_eq!(receipt.revision, r3);
        receipt.result.unwrap();
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.9).unwrap());
    }

    #[test]
    fn worker_panic_and_closed_receivers_cannot_strand_flush_or_detach_thread() {
        let fixture = Fixture::new();
        let registry = registry();
        let hooks = IoHooks {
            panic_revision: Some(Revision::INITIAL),
            ..IoHooks::default()
        };
        let (worker, startup, notifications) =
            SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
        startup.recv_blocking().unwrap();
        drop(notifications);
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.8))
            .unwrap();
        assert_eq!(
            worker.flush(Revision::INITIAL).unwrap_err().stage,
            SaveStage::WorkerPanicked
        );
        assert_eq!(
            worker
                .submit(Revision::from_raw(1), snapshot(&registry, 0.5))
                .unwrap_err()
                .stage,
            SaveStage::WorkerPanicked
        );
        drop(worker);
        let (worker, startup, notifications) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        drop(notifications);
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.7))
            .unwrap();
        worker.flush(Revision::INITIAL).unwrap();
        drop(worker);
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.7).unwrap());
    }

    #[test]
    fn closed_startup_channel_fails_queued_flush_without_writing_original() {
        let fixture = Fixture::new();
        let registry = registry();
        fixture.write(&json!({"version":1,"volume":0.2}));
        let original = fs::read(fixture.path()).unwrap();
        let paused = Arc::new(IoPause::default());
        let hooks = IoHooks {
            startup_pause: Some(paused.clone()),
            ..IoHooks::default()
        };
        let (worker, startup, _) =
            SaveWorker::start_with_hooks(fixture.path(), registry.clone(), hooks).unwrap();
        let _release = ReleaseOnDrop(paused.clone());
        paused.wait_until_paused();
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.9))
            .unwrap();
        drop(startup);
        paused.release();
        assert_eq!(
            worker.flush(Revision::INITIAL).unwrap_err().stage,
            SaveStage::StartupChannel
        );
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
        assert_eq!(fixture.entries(), vec![fixture.path()]);
    }

    #[test]
    fn drop_joins_worker_and_finishes_accepted_pending_save() {
        let fixture = Fixture::new();
        let registry = registry();
        let (worker, startup, _) = SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        worker
            .submit(Revision::from_raw(7), snapshot(&registry, 0.7))
            .unwrap();
        assert_eq!(
            worker
                .submit(Revision::from_raw(7), snapshot(&registry, 0.3))
                .unwrap_err()
                .stage,
            SaveStage::InvalidRevision
        );
        drop(worker);
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.7).unwrap());
    }

    #[test]
    fn real_filesystem_create_and_replace_denials_do_not_remove_original() {
        let fixture = Fixture::new();
        let parent_file = fixture.0.join("not-a-directory");
        fs::write(&parent_file, b"protected").unwrap();
        let error = atomic_replace(&parent_file.join("state.json"), b"new", &IoHooks::default())
            .unwrap_err();
        assert_eq!(error.stage, SaveStage::CreateTemp);
        assert_eq!(fs::read(&parent_file).unwrap(), b"protected");
        fs::create_dir(fixture.path()).unwrap();
        fs::write(fixture.path().join("original"), b"protected").unwrap();
        let error = atomic_replace(&fixture.path(), b"new", &IoHooks::default()).unwrap_err();
        assert_eq!(error.stage, SaveStage::Replace);
        assert_eq!(
            fs::read(fixture.path().join("original")).unwrap(),
            b"protected"
        );
        assert_eq!(fixture.entries().len(), 2);
    }

    #[test]
    fn wrong_typed_known_fields_restore_defaults_and_window_identifier_is_repaired() {
        let fixture = Fixture::new();
        let registry = registry();
        let definition = &registry.definitions()[0];
        fixture.write(&json!({"version":1, "volume":true, "mute":9, "wander":null,
            "wander_min_minutes":"bad", "wander_max_minutes":[], "transition_seconds":{},
            "last":42, "situations":{(definition.id):{(definition.knobs[0].id):"bad"}},
            "window":{"x":1,"y":2,"width":3,"height":4,"display_uuid":22}}));
        let (state, _, _, notice) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls, ControlState::new(&registry));
        assert!(notice.is_some());
        let placement = state.window.unwrap();
        assert_eq!(
            (placement.x, placement.y, placement.width, placement.height),
            (1.0, 2.0, 3.0, 4.0)
        );
        assert!(placement.display_uuid.is_none());
    }

    #[test]
    fn a_latest_failure_replaces_older_success_without_an_unbounded_notice_queue() {
        let fixture = Fixture::new();
        let registry = registry();
        let (worker, startup, notifications) =
            SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        startup.recv_blocking().unwrap();
        let r1 = Revision::from_raw(1);
        let r2 = Revision::from_raw(2);
        worker.submit(r1, snapshot(&registry, 0.1)).unwrap();
        worker.flush(r1).unwrap();
        let old = fs::read(fixture.path()).unwrap();
        let preserved = fixture.0.join("preserved.json");
        fs::rename(fixture.path(), &preserved).unwrap();
        fs::create_dir(fixture.path()).unwrap();
        worker.submit(r2, snapshot(&registry, 0.9)).unwrap();
        assert_eq!(worker.flush(r2).unwrap_err().stage, SaveStage::Replace);
        assert_eq!(notifications.len(), 1);
        let latest = worker.take_receipt().unwrap();
        assert_eq!(latest.revision, r2);
        assert_eq!(latest.result.unwrap_err().stage, SaveStage::Replace);
        assert_eq!(fs::read(preserved).unwrap(), old);
        assert_eq!(fixture.entries().len(), 2);
    }

    #[test]
    fn corrupt_worker_startup_saves_defaults_only_after_real_backup_succeeds() {
        let fixture = Fixture::new();
        let registry = registry();
        fs::write(fixture.path(), b"corrupt original").unwrap();
        let (worker, startup, _) = SaveWorker::start(fixture.path(), registry.clone()).unwrap();
        let initial = startup.recv_blocking().unwrap();
        assert_eq!(initial.settings.controls, ControlState::new(&registry));
        assert!(initial.notice.is_some());
        let backup = fixture.entries().pop().unwrap().join("state.json");
        assert_eq!(fs::read(&backup).unwrap(), b"corrupt original");
        worker
            .submit(Revision::INITIAL, snapshot(&registry, 0.67))
            .unwrap();
        worker.flush(Revision::INITIAL).unwrap();
        let (state, _, _, _) = loaded(load_state(&fixture.path(), &registry));
        assert_eq!(state.controls.volume, Normalized::new(0.67).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"corrupt original");
        assert_eq!(fixture.entries().len(), 2);
    }
}
