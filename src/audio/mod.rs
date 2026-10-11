pub(crate) mod lease;
mod mix;
mod render;
pub(crate) mod timing;
pub(crate) mod transport;
mod world;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use async_channel::{Receiver, Sender};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::{controls::ControlState, situations::Registry};
use lease::EngineLease;
use render::PreparedEngine;
use timing::{MachClock, StreamEpoch, WorldSeconds};
use transport::{
    ControlMailbox, ControlReceipt, EVENT_CAPACITY, ErrorCell, ErrorCode, StatusMailbox, VisualRing,
};

/// OS callbacks publish fixed signals, never borrow DSP or call GPUI.
/// Workspace power state is latest-wins; retaining both edges still forces
/// a freeze if sleep/wake arrives before the worker's next bounded read.
pub(crate) struct RecoverySignals {
    events: AtomicU8,
    sleeping: AtomicBool,
    closed: AtomicBool,
}

impl RecoverySignals {
    const SLEEP: u8 = 1;
    const WAKE: u8 = 2;
    const DEVICE: u8 = 4;

    pub(crate) fn new() -> Self {
        Self {
            events: AtomicU8::new(0),
            sleeping: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn set_sleeping(&self, sleeping: bool) {
        if !self.closed.load(Ordering::SeqCst) {
            self.sleeping.store(sleeping, Ordering::SeqCst);
            self.events.fetch_or(
                if sleeping { Self::SLEEP } else { Self::WAKE },
                Ordering::SeqCst,
            );
        }
    }

    pub(crate) fn output_changed(&self) {
        if !self.closed.load(Ordering::SeqCst) {
            self.events.fetch_or(Self::DEVICE, Ordering::SeqCst);
        }
    }

    fn take_events(&self) -> u8 {
        if self.closed.load(Ordering::SeqCst) {
            0
        } else {
            self.events.swap(0, Ordering::SeqCst)
        }
    }

    fn is_sleeping(&self) -> bool {
        self.sleeping.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.events.store(0, Ordering::SeqCst);
    }
}
#[derive(Clone, PartialEq)]
pub(crate) enum OutputStatus {
    Starting,
    Running {
        epoch: StreamEpoch,
        sample_rate: u32,
        channels: u16,
        format: cpal::SampleFormat,
        notices: u64,
    },
    Sleeping {
        world: Option<WorldSeconds>,
    },
    Preparing {
        world: Option<WorldSeconds>,
    },
    Failed {
        reason: String,
        world: Option<WorldSeconds>,
    },
}

impl OutputStatus {
    pub(crate) fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    /// Running with nothing to report: no xrun and no denied real-time priority.
    pub(crate) fn is_normal(&self) -> bool {
        matches!(self, Self::Running { notices, .. } if *notices == 0)
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Starting => "標準出力を準備しています。".into(),
            Self::Running {
                epoch,
                sample_rate,
                channels,
                format,
                notices,
            } => {
                let mut text = format!(
                    "標準出力で再生中: {sample_rate} Hz / {channels} channel / {format:?}。出力世代 {}。",
                    epoch.get()
                );
                if notices & ErrorCode::XRUN != 0 {
                    text.push_str("。出力の遅延を検出しました。");
                }
                if notices & ErrorCode::REALTIME_DENIED != 0 {
                    text.push_str("。OS によるリアルタイム優先度の許可は得られていません。");
                }
                text
            }
            Self::Sleeping { world } => match world {
                Some(world) => format!(
                    "Mac のスリープで音声出力を停止しています。世界時刻 {:.3} 秒を保持しています。",
                    world.get()
                ),
                None => "Mac のスリープで音声出力を停止しています。".into(),
            },
            Self::Preparing { world } => match world {
                Some(world) => format!(
                    "標準出力を再準備しています。世界時刻 {:.3} 秒を保持しています。",
                    world.get()
                ),
                None => "標準出力を再準備しています。".into(),
            },
            Self::Failed { reason, world } => {
                let mut text = format!("音声出力を停止しています: {reason}");
                if let Some(world) = world {
                    text.push_str(&format!(
                        "。世界時刻 {:.3} 秒を保持しています。",
                        world.get()
                    ));
                }
                text
            }
        }
    }
}

/// Immutable transport handles prepared by the worker; views never own DSP.
pub(crate) struct AudioSource {
    pub(crate) registry: Arc<Registry>,
    pub(crate) visuals: Arc<VisualRing>,
    pub(crate) controls: Arc<ControlMailbox>,
    pub(crate) receipts: Arc<StatusMailbox>,
    pub(crate) epoch: StreamEpoch,
    pub(crate) clock: MachClock,
    pub(crate) errors: Arc<ErrorCell>,
    pub(crate) freeze_time: AtomicU64,
}

pub(crate) struct AudioShared {
    state: Mutex<(
        OutputStatus,
        Option<Arc<AudioSource>>,
        Option<ControlReceipt>,
    )>,
}

impl AudioShared {
    pub(crate) fn snapshot(&self) -> (OutputStatus, Option<Arc<AudioSource>>) {
        let state = self.state.lock();
        (state.0.clone(), state.1.clone())
    }

    pub(crate) fn control_receipt(&self) -> Option<ControlReceipt> {
        self.state.lock().2
    }

    fn accept_receipt(&self, source: &AudioSource, notify: &Sender<()>) {
        let Some(receipt) = source.receipts.try_read() else {
            return;
        };
        if receipt.epoch != source.epoch {
            return;
        }
        let mut state = self.state.lock();
        if state
            .1
            .as_ref()
            .is_none_or(|current| current.epoch != receipt.epoch)
        {
            return;
        }
        let discrete_changed = state.2.is_none_or(|old| {
            old.epoch != receipt.epoch
                || old.ack_revision != receipt.ack_revision
                || old.ack_manual_revision != receipt.ack_manual_revision
                || old.display_dominant != receipt.display_dominant
                || old.resume_target != receipt.resume_target
        });
        // Retain the freshest coherent world value, but never wake main merely
        // because samples, gains, particles, or breath progressed.
        state.2 = Some(receipt);
        drop(state);
        if discrete_changed {
            let _ = notify.try_send(());
        }
    }

    fn set_status(&self, status: OutputStatus, notify: &Sender<()>) {
        let mut state = self.state.lock();
        if state.0 == status {
            return;
        }
        state.0 = status;
        drop(state);
        // Non-RT worker only. Capacity-one wakeups coalesce; latest state is authoritative.
        let _ = notify.try_send(());
    }
}

/// The application root, not either window, owns the output worker lifetime.
pub(crate) struct AudioController {
    shared: Arc<AudioShared>,
    stop: Arc<AtomicBool>,
    signals: Arc<RecoverySignals>,
    worker: Option<JoinHandle<()>>,
}

impl AudioController {
    pub(crate) fn start(
        registry: Arc<Registry>,
        controls: ControlState,
        signals: Arc<RecoverySignals>,
        power_ready: Result<(), String>,
    ) -> (Self, Receiver<()>) {
        let epoch = StreamEpoch::new(1).expect("initial output generation is nonzero");
        // Stable transports make controls available even before an output exists.
        // The first real engine consumes these exact mailboxes; no edit is lost
        // when a prepared source replaces its previous output generation.
        let source = MachClock::new().map(|clock| {
            Arc::new(AudioSource {
                visuals: Arc::new(VisualRing::new(
                    EVENT_CAPACITY,
                    registry.definitions().len(),
                )),
                controls: Arc::new(ControlMailbox::new(&controls)),
                receipts: Arc::new(StatusMailbox::new(registry.definitions().len())),
                registry,
                epoch,
                clock,
                errors: Arc::new(ErrorCell::new(epoch)),
                freeze_time: AtomicU64::new(0),
            })
        });
        let shared = Arc::new(AudioShared {
            state: Mutex::new((OutputStatus::Starting, source.as_ref().ok().cloned(), None)),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (notify, updates) = async_channel::bounded(1);
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let worker_signals = signals.clone();
        let failure_notify = notify.clone();
        let worker = match thread::Builder::new()
            .name("ambien-output".into())
            .spawn(move || match source {
                Ok(source) => run_worker(
                    worker_shared,
                    worker_stop,
                    notify,
                    source,
                    controls,
                    worker_signals,
                    power_ready,
                ),
                Err(error) => worker_shared.set_status(
                    OutputStatus::Failed {
                        reason: error.to_string(),
                        world: None,
                    },
                    &notify,
                ),
            }) {
            Ok(worker) => Some(worker),
            Err(error) => {
                shared.set_status(
                    OutputStatus::Failed {
                        reason: format!("音声 worker を起動できません: {error}"),
                        world: None,
                    },
                    &failure_notify,
                );
                None
            }
        };
        (
            Self {
                shared,
                stop,
                signals,
                worker,
            },
            updates,
        )
    }

    pub(crate) fn shared(&self) -> Arc<AudioShared> {
        self.shared.clone()
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.signals.close();
        if let Some(worker) = self.worker.take() {
            // Main is non-RT. The worker closes the gate and takes DSP before destruction.
            if worker.join().is_err() {
                eprintln!("Ambien: 音声 worker が予期せず終了しました");
            }
        }
    }
}

impl Drop for AudioController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct OutputConfiguration {
    device: cpal::Device,
    native: crate::platform::macos::OutputDeviceSnapshot,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
}

fn query_output(
    monitor: &mut crate::platform::macos::OutputDeviceMonitor,
) -> Result<OutputConfiguration, String> {
    let native = monitor
        .refresh()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "使用可能な標準出力 device がありません".to_owned())?;
    // CPAL's DefaultOutput endpoint reroutes; never enumerate a HalOutput instead.
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "標準出力 device がありません".to_owned())?;
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let format = supported.sample_format();
    if !matches!(format, cpal::SampleFormat::F32 | cpal::SampleFormat::I16) {
        return Err(format!("未対応の標準出力 sample format: {format:?}"));
    }
    let mut config = supported.config();
    config.buffer_size = cpal::BufferSize::Default;
    if config.channels == 0 || config.sample_rate == 0 {
        return Err("標準出力の channel 数または sample rate が無効です".into());
    }
    Ok(OutputConfiguration {
        device,
        native,
        config,
        format,
    })
}

struct PreparedOutput {
    stream: cpal::Stream,
    lease: Arc<EngineLease>,
    source: Arc<AudioSource>,
    native: crate::platform::macos::OutputDeviceSnapshot,
    sample_rate: u32,
    channels: u16,
    format: cpal::SampleFormat,
}

impl PreparedOutput {
    fn matches(&self, config: &OutputConfiguration) -> bool {
        self.native == config.native
            && self.sample_rate == config.config.sample_rate
            && self.channels == config.config.channels
            && self.format == config.format
    }

    fn status(&self, notices: u64) -> OutputStatus {
        OutputStatus::Running {
            epoch: self.lease.gate().epoch(),
            sample_rate: self.sample_rate,
            channels: self.channels,
            format: self.format,
            notices,
        }
    }

    fn freeze(self) -> (PreparedEngine, Option<String>) {
        let engine = take_closed_engine(&self.lease);
        // Pause/drop are cleanup, never the ownership transfer acknowledgment.
        let mut failure = self
            .stream
            .pause()
            .err()
            .map(|error| format!("旧 stream 停止に失敗しました: {error}"));
        match self.source.clock.now() {
            Ok(now) => self.source.freeze_time.store(now.get(), Ordering::SeqCst),
            Err(error) => {
                let reason = format!("停止時刻の取得に失敗しました: {error}");
                failure = Some(match failure {
                    Some(old) => format!("{old}。{reason}"),
                    None => reason,
                });
            }
        }
        drop(self);
        (engine, failure)
    }
}

fn take_closed_engine(lease: &EngineLease) -> PreparedEngine {
    lease.close();
    loop {
        if let Some(engine) = lease.try_take() {
            return engine;
        }
        // Only the non-RT worker waits for the admitted callback guard to return.
        thread::sleep(Duration::from_millis(1));
    }
}

fn source_for_epoch(previous: &Arc<AudioSource>, epoch: StreamEpoch) -> Arc<AudioSource> {
    if previous.epoch == epoch {
        return previous.clone();
    }
    Arc::new(AudioSource {
        registry: previous.registry.clone(),
        visuals: previous.visuals.clone(),
        controls: previous.controls.clone(),
        receipts: previous.receipts.clone(),
        epoch,
        clock: previous.clock,
        errors: Arc::new(ErrorCell::new(epoch)),
        freeze_time: AtomicU64::new(0),
    })
}

fn prepare_output(
    configuration: OutputConfiguration,
    source: Arc<AudioSource>,
    frozen: &mut Option<PreparedEngine>,
    latest: &ControlState,
) -> Result<PreparedOutput, String> {
    let channels = configuration.config.channels;
    let sample_rate = configuration.config.sample_rate;
    if frozen.is_none() {
        let seed = source.clock.now().map_err(|error| error.to_string())?.get();
        *frozen = Some(
            PreparedEngine::prepare(
                source.registry.clone(),
                latest.clone(),
                source.visuals.clone(),
                source.controls.clone(),
                source.receipts.clone(),
                sample_rate,
                seed,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let engine = frozen
        .as_mut()
        .expect("initial preparation installed the engine");
    engine
        .prepare_output_epoch(sample_rate)
        .map_err(|error| error.to_string())?;
    if !engine.sync_frozen_controls(source.epoch) {
        return Err("新しい出力世代の制御受領を公開できません".into());
    }
    let lease = Arc::new(EngineLease::new(
        frozen
            .take()
            .expect("worker exclusively owns the prepared engine"),
        source.epoch,
    ));
    let callback_lease = lease.clone();
    let callback_errors = source.errors.clone();
    let monitor_errors = source.errors.clone();
    let stream_result = match configuration.format {
        cpal::SampleFormat::F32 => configuration.device.build_output_stream(
            configuration.config,
            move |data: &mut [f32], info| {
                render::output_f32(
                    &callback_lease,
                    data,
                    usize::from(channels),
                    info,
                    &callback_errors,
                );
            },
            move |error| monitor_errors.report(error.kind()),
            None,
        ),
        cpal::SampleFormat::I16 => configuration.device.build_output_stream(
            configuration.config,
            move |data: &mut [i16], info| {
                render::output_i16(
                    &callback_lease,
                    data,
                    usize::from(channels),
                    info,
                    &callback_errors,
                );
            },
            move |error| monitor_errors.report(error.kind()),
            None,
        ),
        _ => unreachable!("format was validated before preparing the engine"),
    };
    let stream = match stream_result {
        Ok(stream) => stream,
        Err(error) => {
            *frozen = Some(take_closed_engine(&lease));
            return Err(format!("標準出力 stream を構築できません: {error}"));
        }
    };
    if let Err(error) = stream.play() {
        *frozen = Some(take_closed_engine(&lease));
        drop(stream);
        return Err(format!("標準出力を開始できません: {error}"));
    }
    Ok(PreparedOutput {
        stream,
        lease,
        source,
        native: configuration.native,
        sample_rate,
        channels,
        format: configuration.format,
    })
}

fn run_worker(
    shared: Arc<AudioShared>,
    stop: Arc<AtomicBool>,
    notify: Sender<()>,
    mut source: Arc<AudioSource>,
    latest: ControlState,
    signals: Arc<RecoverySignals>,
    power_ready: Result<(), String>,
) {
    if let Err(reason) = power_ready {
        shared.set_status(
            OutputStatus::Failed {
                reason,
                world: None,
            },
            &notify,
        );
        return;
    }
    let mut monitor = match crate::platform::macos::OutputDeviceMonitor::new(signals.clone()) {
        Ok(monitor) => monitor,
        Err(error) => {
            shared.set_status(
                OutputStatus::Failed {
                    reason: format!("標準出力の復旧通知を登録できません: {error}"),
                    world: None,
                },
                &notify,
            );
            return;
        }
    };
    let mut output: Option<PreparedOutput> = None;
    let mut frozen: Option<PreparedEngine> = None;
    let mut generation = 0_u64;
    let mut first = true;
    let mut notices = 0;
    let mut cleanup_blocked = false;
    while !stop.load(Ordering::SeqCst) {
        let events = signals.take_events();
        let sleeping = signals.is_sleeping();
        // Only the active epoch's error cell is consumed. Retired callbacks and
        // CPAL monitors can never request another freeze of the new output.
        let error_epoch = output.as_ref().map(|active| active.source.epoch);
        let errors = output
            .as_ref()
            .map_or(0, |active| active.source.errors.take_bits());
        let stream_changed =
            errors & (ErrorCode::DEVICE_CHANGED | ErrorCode::STREAM_INVALIDATED) != 0;
        let stream_failed = errors & ErrorCode::FATAL != 0;
        let sleep_edge = events & RecoverySignals::SLEEP != 0;
        let waking = events & RecoverySignals::WAKE != 0;
        let mut configuration = None;
        let mut device_changed = false;
        if events & RecoverySignals::DEVICE != 0 && !sleeping {
            let queried = query_output(&mut monitor);
            device_changed = match (&output, &queried) {
                (Some(active), Ok(current)) => !active.matches(current),
                _ => true,
            };
            configuration = Some(queried);
        }
        if !sleeping && (waking || device_changed || stream_changed) {
            cleanup_blocked = false;
        }
        if (sleeping || sleep_edge || stream_changed || stream_failed || device_changed)
            && let Some(active) = output.take()
        {
            let (engine, cleanup_failure) = active.freeze();
            frozen = Some(engine);
            notices = 0;
            if let Some(reason) = cleanup_failure {
                cleanup_blocked = true;
                shared.set_status(
                    OutputStatus::Failed {
                        reason,
                        world: frozen.as_ref().map(PreparedEngine::world),
                    },
                    &notify,
                );
            } else if sleeping {
                shared.set_status(
                    OutputStatus::Sleeping {
                        world: frozen.as_ref().map(PreparedEngine::world),
                    },
                    &notify,
                );
            } else if stream_failed && !stream_changed {
                shared.set_status(
                    OutputStatus::Failed {
                        reason: format!("標準出力の失敗 (error bits 0x{errors:x})"),
                        world: frozen.as_ref().map(PreparedEngine::world),
                    },
                    &notify,
                );
            } else {
                shared.set_status(
                    OutputStatus::Preparing {
                        world: frozen.as_ref().map(PreparedEngine::world),
                    },
                    &notify,
                );
            }
        }
        if sleeping && !cleanup_blocked {
            shared.set_status(
                OutputStatus::Sleeping {
                    world: frozen.as_ref().map(PreparedEngine::world),
                },
                &notify,
            );
        } else if !sleeping
            && !cleanup_blocked
            && output.is_none()
            && (first || waking || device_changed || stream_changed)
        {
            // One event permits one attempt. No timer retries, unknown-rate
            // substitute, default DSP reconstruction, or user-unmute fallback.
            first = false;
            let attempt = generation
                .checked_add(1)
                .and_then(StreamEpoch::new)
                .ok_or_else(|| "音声出力の世代が上限に達しました".to_owned());
            let prepared = match attempt {
                Ok(epoch) => {
                    generation = epoch.get();
                    let candidate = source_for_epoch(&source, epoch);
                    shared.set_status(
                        if frozen.is_some() {
                            OutputStatus::Preparing {
                                world: frozen.as_ref().map(PreparedEngine::world),
                            }
                        } else {
                            OutputStatus::Starting
                        },
                        &notify,
                    );
                    configuration
                        .unwrap_or_else(|| query_output(&mut monitor))
                        .and_then(|configuration| {
                            prepare_output(configuration, candidate, &mut frozen, &latest)
                        })
                }
                Err(reason) => Err(reason),
            };
            match prepared {
                Ok(active) => {
                    source = active.source.clone();
                    let status = active.status(0);
                    *shared.state.lock() = (status, Some(source.clone()), None);
                    let _ = notify.try_send(());
                    output = Some(active);
                }
                Err(reason) => shared.set_status(
                    OutputStatus::Failed {
                        reason,
                        world: frozen.as_ref().map(PreparedEngine::world),
                    },
                    &notify,
                ),
            }
        }
        if let Some(engine) = &mut frozen
            && !engine.sync_frozen_controls(source.epoch)
        {
            shared.set_status(
                OutputStatus::Failed {
                    reason: "制御受領の公開に失敗しました。世界時刻を保持しています".into(),
                    world: Some(engine.world()),
                },
                &notify,
            );
        }
        shared.accept_receipt(&source, &notify);
        if let Some(active) = &output
            && errors & ErrorCode::NON_FATAL != 0
            && Some(active.source.epoch) == error_epoch
        {
            notices |= errors & (ErrorCode::XRUN | ErrorCode::REALTIME_DENIED);
            shared.set_status(active.status(notices), &notify);
        }
        thread::sleep(Duration::from_millis(20));
    }
    if let Some(active) = output.take() {
        let (engine, failure) = active.freeze();
        frozen = Some(engine);
        if let Some(reason) = failure {
            eprintln!("Ambien: 終了時の音声停止に失敗しました: {reason}");
        }
    }
    if let Err(error) = monitor.shutdown() {
        eprintln!("Ambien: 出力復旧通知の解除に失敗しました: {error}");
    }
    drop(frozen); // DSP destruction stays on this worker, regardless of old closures.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{controls::Revision, situations::SituationIndex};

    fn source(epoch: StreamEpoch) -> Arc<AudioSource> {
        let registry = Arc::new(Registry::new().unwrap());
        let controls = ControlState::new(&registry);
        Arc::new(AudioSource {
            visuals: Arc::new(VisualRing::new(
                EVENT_CAPACITY,
                registry.definitions().len(),
            )),
            controls: Arc::new(ControlMailbox::new(&controls)),
            receipts: Arc::new(StatusMailbox::new(registry.definitions().len())),
            registry,
            epoch,
            clock: MachClock::new().unwrap(),
            errors: Arc::new(ErrorCell::new(epoch)),
            freeze_time: AtomicU64::new(0),
        })
    }

    fn receipt(epoch: StreamEpoch, revision: u64, world: f64) -> ControlReceipt {
        ControlReceipt {
            epoch,
            ack_revision: Revision::from_raw(revision),
            ack_manual_revision: Revision::INITIAL,
            display_dominant: SituationIndex::from_index(0, 1).unwrap(),
            resume_target: SituationIndex::from_index(0, 1).unwrap(),
            world: WorldSeconds::new(world).unwrap(),
        }
    }

    #[test]
    fn worker_receipts_notify_only_discrete_changes_and_never_invent_a_receipt() {
        let epoch = StreamEpoch::new(21).unwrap();
        let source = source(epoch);
        let shared = AudioShared {
            state: Mutex::new((OutputStatus::Starting, Some(source.clone()), None)),
        };
        let (notify, updates) = async_channel::bounded(1);
        shared.accept_receipt(&source, &notify);
        assert_eq!(shared.control_receipt(), None);
        assert!(updates.try_recv().is_err());
        let first = receipt(epoch, 0, 0.25);
        assert!(source.receipts.publish(first));
        shared.accept_receipt(&source, &notify);
        assert_eq!(updates.try_recv(), Ok(()));
        assert_eq!(shared.control_receipt(), Some(first));
        let progressing = receipt(epoch, 0, 0.5);
        assert!(source.receipts.publish(progressing));
        shared.accept_receipt(&source, &notify);
        assert!(updates.try_recv().is_err());
        assert_eq!(shared.control_receipt(), Some(progressing));
        let edited = receipt(epoch, 1, 0.75);
        assert!(source.receipts.publish(edited));
        shared.accept_receipt(&source, &notify);
        assert_eq!(updates.try_recv(), Ok(()));
        let mut manual = edited;
        manual.ack_manual_revision = Revision::from_raw(1);
        assert!(source.receipts.publish(manual));
        shared.accept_receipt(&source, &notify);
        assert_eq!(updates.try_recv(), Ok(()));
        shared.accept_receipt(&source, &notify);
        assert!(updates.try_recv().is_err());
    }

    #[test]
    fn worker_rejects_old_source_epoch_without_overwriting_current_receipt() {
        let old_epoch = StreamEpoch::new(22).unwrap();
        let current_epoch = StreamEpoch::new(23).unwrap();
        let old = source(old_epoch);
        let current = source(current_epoch);
        let latest = receipt(current_epoch, 3, 2.0);
        let shared = AudioShared {
            state: Mutex::new((OutputStatus::Starting, Some(current.clone()), Some(latest))),
        };
        let (notify, updates) = async_channel::bounded(1);
        assert!(old.receipts.publish(receipt(old_epoch, 99, 9.0)));
        shared.accept_receipt(&old, &notify);
        assert_eq!(shared.control_receipt(), Some(latest));
        assert!(updates.try_recv().is_err());
        assert!(current.receipts.publish(receipt(old_epoch, 99, 9.0)));
        shared.accept_receipt(&current, &notify);
        assert_eq!(shared.control_receipt(), Some(latest));
        assert!(updates.try_recv().is_err());
    }

    #[test]
    fn worker_output_notifications_coalesce_and_ignore_identical_status() {
        let shared = AudioShared {
            state: Mutex::new((OutputStatus::Starting, None, None)),
        };
        let (notify, updates) = async_channel::bounded(1);
        shared.set_status(OutputStatus::Starting, &notify);
        assert!(updates.try_recv().is_err());
        let running = OutputStatus::Running {
            epoch: StreamEpoch::new(1).unwrap(),
            sample_rate: 48_000,
            channels: 2,
            format: cpal::SampleFormat::F32,
            notices: 0,
        };
        shared.set_status(running.clone(), &notify);
        assert_eq!(updates.try_recv(), Ok(()));
        shared.set_status(running, &notify);
        assert!(updates.try_recv().is_err());
        shared.set_status(
            OutputStatus::Failed {
                reason: "actual DSP fault".into(),
                world: None,
            },
            &notify,
        );
        shared.set_status(
            OutputStatus::Failed {
                reason: "output paused".into(),
                world: None,
            },
            &notify,
        );
        assert_eq!(updates.try_recv(), Ok(()));
        assert!(updates.try_recv().is_err());
        assert!(
            matches!(shared.snapshot().0, OutputStatus::Failed { reason, .. } if reason == "output paused")
        );
    }

    #[test]
    fn only_a_running_output_with_no_warning_counts_as_normal() {
        let running = |notices| OutputStatus::Running {
            epoch: StreamEpoch::new(1).unwrap(),
            sample_rate: 48_000,
            channels: 2,
            format: cpal::SampleFormat::F32,
            notices,
        };
        assert!(running(0).is_normal());
        // A delayed or non-real-time output is still "running" but must not look normal: the
        // settings window shows it as a notice.
        assert!(!running(ErrorCode::XRUN).is_normal());
        assert!(!running(ErrorCode::REALTIME_DENIED).is_normal());
        assert!(!OutputStatus::Starting.is_normal());
    }
}
