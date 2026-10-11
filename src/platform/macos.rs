use std::{
    fmt,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::ThreadId,
};

use block2::RcBlock;
use gpui::{Bounds, DisplayId, Pixels, point, px, size};
use objc2::{
    MainThreadMarker,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Imp, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSApplicationTerminateReply,
    NSScreen, NSView, NSWindow, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
};
use objc2_core_audio::{
    AudioObjectAddPropertyListenerBlock, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListenerBlock, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreams, kAudioHardwareBadObjectError,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioObjectUnknown, kAudioStreamPropertyPhysicalFormat, kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSUInteger, ns_string,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::audio::RecoverySignals;

#[derive(Debug)]
pub(crate) enum AudioMonitorError {
    MainThreadRequired,
    OutputWorkerRequired,
    WrongOutputThread,
    Native {
        operation: &'static str,
        object: u32,
        selector: u32,
        status: i32,
    },
    PropertySize {
        object: u32,
        selector: u32,
        expected: u32,
        actual: u32,
    },
    PropertyValue {
        object: u32,
        selector: u32,
        detail: &'static str,
    },
    Shutdown(Vec<AudioMonitorError>),
}

impl fmt::Display for AudioMonitorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MainThreadRequired => {
                f.write_str("power monitor requires the AppKit main thread")
            }
            Self::OutputWorkerRequired => {
                f.write_str("output monitor must be created on its output worker")
            }
            Self::WrongOutputThread => {
                f.write_str("output monitor called outside its owning worker")
            }
            Self::Native {
                operation,
                object,
                selector,
                status,
            } => write!(
                f,
                "{operation} failed for audio object {object}, property {selector:#010x}: OSStatus {status}"
            ),
            Self::PropertySize {
                object,
                selector,
                expected,
                actual,
            } => write!(
                f,
                "audio object {object}, property {selector:#010x}: expected {expected} bytes, received {actual}"
            ),
            Self::PropertyValue {
                object,
                selector,
                detail,
            } => write!(
                f,
                "audio object {object}, property {selector:#010x}: {detail}"
            ),
            Self::Shutdown(errors) => {
                f.write_str("output listener shutdown failed")?;
                for error in errors {
                    write!(f, "; {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AudioMonitorError {}

/// Main-owned workspace tokens. Blocks retain only the atomic recovery signal.
pub(crate) struct PowerMonitor {
    _main: MainThreadMarker,
    center: Retained<NSNotificationCenter>,
    sleep: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    wake: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl PowerMonitor {
    pub(crate) fn new(signals: Arc<RecoverySignals>) -> Result<Self, AudioMonitorError> {
        let main = MainThreadMarker::new().ok_or(AudioMonitorError::MainThreadRequired)?;
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let sleeping_signals = Arc::clone(&signals);
        let sleep_block = RcBlock::new(move |_: NonNull<NSNotification>| {
            sleeping_signals.set_sleeping(true);
        });
        let wake_block = RcBlock::new(move |_: NonNull<NSNotification>| {
            signals.set_sleeping(false);
        });
        // Workspace's own center copies the blocks. queue=None permits delivery on
        // the posting thread; both closures are Send+Sync and touch only atomics.
        let sleep = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillSleepNotification),
                None,
                None,
                &sleep_block,
            )
        };
        let wake = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidWakeNotification),
                None,
                None,
                &wake_block,
            )
        };
        Ok(Self {
            _main: main,
            center,
            sleep: Some(sleep),
            wake: Some(wake),
        })
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), AudioMonitorError> {
        MainThreadMarker::new().ok_or(AudioMonitorError::MainThreadRequired)?;
        // removeObserver has no native failure result. These are exactly the
        // retained registration tokens from this center, not selector observers.
        if let Some(token) = self.sleep.take() {
            unsafe {
                self.center.removeObserver((*token).as_ref());
            }
        }
        if let Some(token) = self.wake.take() {
            unsafe {
                self.center.removeObserver((*token).as_ref());
            }
        }
        Ok(())
    }
}

impl Drop for PowerMonitor {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Ambien power monitor shutdown: {error}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MainThreadRequired;

impl fmt::Display for MainThreadRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppKit requires the main thread")
    }
}

impl std::error::Error for MainThreadRequired {}

/// The OS display options Ambien follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisplayOptions {
    pub(crate) reduce_motion: bool,
    pub(crate) reduce_transparency: bool,
}

pub(crate) fn read_display_options() -> Result<DisplayOptions, MainThreadRequired> {
    MainThreadMarker::new().ok_or(MainThreadRequired)?;
    let workspace = NSWorkspace::sharedWorkspace();
    Ok(DisplayOptions {
        reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
        reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
    })
}

/// Every screen's frame and visible frame, in AppKit order. Two equal layouts mean no
/// screen was added, removed, moved or resized, and no Dock/menu-bar workarea changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScreenLayout(Vec<[f64; 8]>);

pub(crate) fn read_screen_layout() -> Result<ScreenLayout, MainThreadRequired> {
    let mtm = MainThreadMarker::new().ok_or(MainThreadRequired)?;
    let screens = NSScreen::screens(mtm);
    let mut layout = Vec::with_capacity(screens.count());
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let (frame, visible) = (screen.frame(), screen.visibleFrame());
        layout.push([
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            visible.origin.x,
            visible.origin.y,
            visible.size.width,
            visible.size.height,
        ]);
    }
    Ok(ScreenLayout(layout))
}

/// One observer token, paired with the exact center that issued it.
struct EnvironmentRegistration {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

/// Main-owned observers for display accessibility and screen-parameter changes. The
/// blocks capture only the `Send + Sync` callback, which must merely signal; the newest
/// state is read afterwards on the main thread.
pub(crate) struct EnvironmentMonitor {
    _main: MainThreadMarker,
    registrations: Vec<EnvironmentRegistration>,
}

impl EnvironmentMonitor {
    pub(crate) fn new(
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, MainThreadRequired> {
        let main = MainThreadMarker::new().ok_or(MainThreadRequired)?;
        let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(on_change);
        let mut registrations = Vec::with_capacity(2);
        let centers = [
            // Reduce motion/transparency are posted on the workspace's own center.
            (
                NSWorkspace::sharedWorkspace().notificationCenter(),
                unsafe { NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification },
            ),
            // Screen add/remove, resolution, scale and workarea changes.
            (NSNotificationCenter::defaultCenter(), unsafe {
                NSApplicationDidChangeScreenParametersNotification
            }),
        ];
        for (center, name) in centers {
            let callback = Arc::clone(&on_change);
            let block = RcBlock::new(move |_: NonNull<NSNotification>| callback());
            // The center copies the block; queue=None permits delivery on the posting thread.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            registrations.push(EnvironmentRegistration { center, token });
        }
        Ok(Self {
            _main: main,
            registrations,
        })
    }

    /// Removes every observer from its own center. Dropping a token alone does not.
    pub(crate) fn shutdown(&mut self) -> Result<(), MainThreadRequired> {
        MainThreadMarker::new().ok_or(MainThreadRequired)?;
        for EnvironmentRegistration { center, token } in self.registrations.drain(..) {
            // removeObserver has no failure result; this is exactly the issued token.
            unsafe { center.removeObserver((*token).as_ref()) };
        }
        Ok(())
    }
}

impl Drop for EnvironmentMonitor {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Ambien environment monitor shutdown: {error}");
        }
    }
}

/// The OS asked to terminate and the app could not (yet) commit. AppKit's own
/// termination stays in charge: the app only decides when it may proceed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TerminationGateError {
    MainThreadRequired,
    DelegateClassMissing,
    /// GPUI (or a later version of it) now implements the selector itself. Replacing or
    /// wrapping its implementation is forbidden, so this is an integration failure.
    SelectorAlreadyImplemented,
    AlreadyInstalled,
    AddMethodFailed,
}

impl fmt::Display for TerminationGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MainThreadRequired => "the termination gate must be installed on the main thread",
            Self::DelegateClassMissing => "the GPUI application delegate class was not found",
            Self::SelectorAlreadyImplemented => {
                "applicationShouldTerminate: is already implemented by the delegate"
            }
            Self::AlreadyInstalled => "the termination gate is already installed",
            Self::AddMethodFailed => "the runtime refused to add applicationShouldTerminate:",
        })
    }
}

impl std::error::Error for TerminationGateError {}

struct TerminationGate {
    // Only signals: the AppKit callback never re-enters GPUI.
    on_request: Box<dyn Fn() + Send + Sync>,
    permitted: AtomicBool,
    pending_reply: AtomicBool,
}

static TERMINATION_GATE: OnceLock<TerminationGate> = OnceLock::new();

const TERMINATE_NOW: NSUInteger = NSApplicationTerminateReply::TerminateNow.0;
const TERMINATE_LATER: NSUInteger = NSApplicationTerminateReply::TerminateLater.0;

extern "C-unwind" fn application_should_terminate(
    _this: *mut AnyObject,
    _command: Sel,
    _sender: *mut AnyObject,
) -> NSUInteger {
    let Some(gate) = TERMINATION_GATE.get() else {
        return TERMINATE_NOW;
    };
    if gate.permitted.load(Ordering::SeqCst) {
        return TERMINATE_NOW;
    }
    // Not committed: AppKit waits for replyToApplicationShouldTerminate:. The request
    // is handed to the main app owner, which merges it into the active attempt.
    gate.pending_reply.store(true, Ordering::SeqCst);
    (gate.on_request)();
    TERMINATE_LATER
}

/// Adds `applicationShouldTerminate:` to GPUI's existing application delegate class so
/// ⌘Q, Dock Quit and OS shutdown all wait for the same termination attempt. Only an
/// absent selector is added; an existing implementation is never replaced or wrapped.
pub(crate) fn install_termination_gate(
    on_request: impl Fn() + Send + Sync + 'static,
) -> Result<(), TerminationGateError> {
    MainThreadMarker::new().ok_or(TerminationGateError::MainThreadRequired)?;
    // SAFETY: a NUL-terminated class name; the runtime returns null when absent.
    let class = unsafe { objc2::ffi::objc_lookUpClass(c"GPUIApplicationDelegate".as_ptr()) };
    install_on_class(class, on_request)
}

fn install_on_class(
    class: *const AnyClass,
    on_request: impl Fn() + Send + Sync + 'static,
) -> Result<(), TerminationGateError> {
    if class.is_null() {
        return Err(TerminationGateError::DelegateClassMissing);
    }
    let selector = sel!(applicationShouldTerminate:);
    // SAFETY: `class` is a live class pointer from the runtime.
    if !unsafe { objc2::ffi::class_getInstanceMethod(class, selector) }.is_null() {
        return Err(TerminationGateError::SelectorAlreadyImplemented);
    }
    TERMINATION_GATE
        .set(TerminationGate {
            on_request: Box::new(on_request),
            permitted: AtomicBool::new(false),
            pending_reply: AtomicBool::new(false),
        })
        .map_err(|_| TerminationGateError::AlreadyInstalled)?;
    // SAFETY: the implementation has exactly the signature the type encoding declares
    // (NSUInteger return, receiver, selector, one object argument).
    let added = unsafe {
        let implementation: Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> NSUInteger,
            Imp,
        >(application_should_terminate);
        objc2::ffi::class_addMethod(class.cast_mut(), selector, implementation, c"Q@:@".as_ptr())
    };
    if added.as_bool() {
        Ok(())
    } else {
        Err(TerminationGateError::AddMethodFailed)
    }
}

/// Main-thread: end the attempt's hold on the OS request. `proceed` lets termination
/// continue (it is allowed from now on); otherwise it is cancelled. Returns whether AppKit
/// was waiting on a reply, in which case the reply was sent here and the caller must not
/// quit again. Without a gate (installation failed) nothing is pending.
pub(crate) fn finish_termination(proceed: bool) -> Result<bool, MainThreadRequired> {
    let mtm = MainThreadMarker::new().ok_or(MainThreadRequired)?;
    let Some(gate) = TERMINATION_GATE.get() else {
        return Ok(false);
    };
    gate.permitted.store(proceed, Ordering::SeqCst);
    if gate.pending_reply.swap(false, Ordering::SeqCst) {
        NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(proceed);
        return Ok(true);
    }
    Ok(false)
}

type OutputChangeBlock = RcBlock<dyn Fn(u32, NonNull<AudioObjectPropertyAddress>)>;

struct OutputListener {
    object: u32,
    address: AudioObjectPropertyAddress,
    block: OutputChangeBlock,
}

#[derive(Debug, PartialEq, Eq)]
enum ListenerRetirement {
    Unregistered,
    ObjectInvalidated,
}

impl OutputListener {
    fn new(
        object: u32,
        selector: u32,
        scope: u32,
        signals: Arc<RecoverySignals>,
    ) -> Result<Self, AudioMonitorError> {
        let address = property_address(selector, scope);
        let block: OutputChangeBlock =
            RcBlock::new(move |_: u32, _: NonNull<AudioObjectPropertyAddress>| {
                signals.output_changed()
            });
        // The installed binding's exact block ABI is (u32, NonNull<Address>).
        // CoreAudio Block_copy retains this Arc-only closure until removal.
        let status = unsafe {
            AudioObjectAddPropertyListenerBlock(
                object,
                NonNull::from(&address),
                None,
                RcBlock::as_ptr(&block),
            )
        };
        native_status("register listener", object, selector, status)?;
        Ok(Self {
            object,
            address,
            block,
        })
    }

    fn remove(&self) -> Result<ListenerRetirement, AudioMonitorError> {
        let status = unsafe {
            AudioObjectRemovePropertyListenerBlock(
                self.object,
                NonNull::from(&self.address),
                None,
                RcBlock::as_ptr(&self.block),
            )
        };
        if status == kAudioHardwareBadObjectError {
            // HAL says this ID no longer names an object, so retaining its guard
            // can never make a later removal succeed. Retire our reference, not
            // the new output. HAL-owned/queued Block copies may still survive;
            // their Arc-only callback never borrows this guard or the engine.
            Ok(ListenerRetirement::ObjectInvalidated)
        } else {
            native_status(
                "remove listener",
                self.object,
                self.address.mSelector,
                status,
            )
            .map(|()| ListenerRetirement::Unregistered)
        }
    }
}

/// Current native timing/layout identity, not just CPAL's reduced sample config.
#[derive(Debug, PartialEq)]
pub(crate) struct OutputDeviceSnapshot {
    pub device_id: u32,
    nominal_rate: f64,
    output_layout: Vec<u32>,
    streams: Vec<OutputStreamSnapshot>,
}

#[derive(Debug, PartialEq)]
struct OutputStreamSnapshot {
    id: u32,
    virtual_format: AudioStreamBasicDescription,
    physical_format: AudioStreamBasicDescription,
}

/// Worker-local listeners outlive every stream epoch, including no-output time.
pub(crate) struct OutputDeviceMonitor {
    thread: ThreadId,
    _worker_local: PhantomData<Rc<()>>,
    signals: Arc<RecoverySignals>,
    global: Option<OutputListener>,
    device: Option<u32>,
    device_listeners: Vec<OutputListener>,
}

impl OutputDeviceMonitor {
    pub(crate) fn new(signals: Arc<RecoverySignals>) -> Result<Self, AudioMonitorError> {
        if MainThreadMarker::new().is_some() {
            return Err(AudioMonitorError::OutputWorkerRequired);
        }
        let global = OutputListener::new(
            kAudioObjectSystemObject as u32,
            kAudioHardwarePropertyDefaultOutputDevice,
            kAudioObjectPropertyScopeGlobal,
            Arc::clone(&signals),
        )?;
        let monitor = Self {
            thread: std::thread::current().id(),
            _worker_local: PhantomData,
            signals,
            global: Some(global),
            device: None,
            device_listeners: Vec::with_capacity(4),
        };
        Ok(monitor)
    }

    fn check_thread(&self) -> Result<(), AudioMonitorError> {
        if std::thread::current().id() == self.thread {
            Ok(())
        } else {
            Err(AudioMonitorError::WrongOutputThread)
        }
    }

    pub(crate) fn refresh(&mut self) -> Result<Option<OutputDeviceSnapshot>, AudioMonitorError> {
        self.check_thread()?;
        let default = read_u32(
            kAudioObjectSystemObject as u32,
            kAudioHardwarePropertyDefaultOutputDevice,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let device = (default != kAudioObjectUnknown).then_some(default);
        if self.device != device {
            self.remove_device_listeners()?;
            self.device = device;
        }
        if let Some(device) = device {
            // A failed partial registration remains owned and is retried without
            // duplicates. The alive listener remains attached when alive=false.
            for (selector, scope) in [
                (
                    kAudioDevicePropertyDeviceIsAlive,
                    kAudioObjectPropertyScopeGlobal,
                ),
                (
                    kAudioDevicePropertyNominalSampleRate,
                    kAudioObjectPropertyScopeGlobal,
                ),
                (
                    kAudioDevicePropertyStreamConfiguration,
                    kAudioObjectPropertyScopeOutput,
                ),
                (kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
            ] {
                if !self
                    .device_listeners
                    .iter()
                    .any(|entry| entry.object == device && entry.address.mSelector == selector)
                {
                    self.device_listeners.push(OutputListener::new(
                        device,
                        selector,
                        scope,
                        Arc::clone(&self.signals),
                    )?);
                }
            }
            let alive = read_u32(
                device,
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            )?;
            if !device_alive(alive).map_err(|detail| AudioMonitorError::PropertyValue {
                object: device,
                selector: kAudioDevicePropertyDeviceIsAlive,
                detail,
            })? {
                return Ok(None);
            }
            let streams = output_stream_ids(device)?;
            self.refresh_stream_listeners(device, &streams)?;
            let rate = read_f64(device, kAudioDevicePropertyNominalSampleRate)?;
            validate_nominal_rate(rate).map_err(|detail| AudioMonitorError::PropertyValue {
                object: device,
                selector: kAudioDevicePropertyNominalSampleRate,
                detail,
            })?;
            let output_layout = read_output_configuration(device)?;
            let mut native_streams = Vec::with_capacity(streams.len());
            for stream in streams {
                native_streams.push(OutputStreamSnapshot {
                    id: stream,
                    virtual_format: read_stream_format(stream, kAudioStreamPropertyVirtualFormat)?,
                    physical_format: read_stream_format(
                        stream,
                        kAudioStreamPropertyPhysicalFormat,
                    )?,
                });
            }
            return Ok(Some(OutputDeviceSnapshot {
                device_id: device,
                nominal_rate: rate,
                output_layout,
                streams: native_streams,
            }));
        }
        Ok(None)
    }

    fn refresh_stream_listeners(
        &mut self,
        device: u32,
        streams: &[u32],
    ) -> Result<(), AudioMonitorError> {
        let mut errors = Vec::new();
        self.device_listeners.retain(|entry| {
            if entry.object == device || streams.contains(&entry.object) {
                return true;
            }
            match entry.remove() {
                Ok(ListenerRetirement::Unregistered | ListenerRetirement::ObjectInvalidated) => {
                    false
                }
                Err(error) => {
                    errors.push(error);
                    true
                }
            }
        });
        if !errors.is_empty() {
            return Err(AudioMonitorError::Shutdown(errors));
        }
        // Channel configuration and stream-list notifications alone do not cover
        // a format-only change. Keep each output stream's actual format listeners.
        for &stream in streams {
            for selector in [
                kAudioStreamPropertyVirtualFormat,
                kAudioStreamPropertyPhysicalFormat,
            ] {
                if !self
                    .device_listeners
                    .iter()
                    .any(|entry| entry.object == stream && entry.address.mSelector == selector)
                {
                    self.device_listeners.push(OutputListener::new(
                        stream,
                        selector,
                        kAudioObjectPropertyScopeGlobal,
                        Arc::clone(&self.signals),
                    )?);
                }
            }
        }
        Ok(())
    }

    fn remove_device_listeners(&mut self) -> Result<(), AudioMonitorError> {
        let mut errors = Vec::new();
        self.device_listeners.retain(|entry| match entry.remove() {
            Ok(ListenerRetirement::Unregistered | ListenerRetirement::ObjectInvalidated) => false,
            Err(error) => {
                errors.push(error);
                true
            }
        });
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AudioMonitorError::Shutdown(errors))
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), AudioMonitorError> {
        self.check_thread()?;
        let mut errors = Vec::new();
        if let Err(error) = self.remove_device_listeners() {
            errors.push(error);
        }
        if let Some(global) = &self.global {
            match global.remove() {
                Ok(ListenerRetirement::Unregistered | ListenerRetirement::ObjectInvalidated) => {
                    self.global = None
                }
                Err(error) => errors.push(error),
            }
        }
        if self.device_listeners.is_empty() {
            self.device = None;
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AudioMonitorError::Shutdown(errors))
        }
    }
}

impl Drop for OutputDeviceMonitor {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Ambien output monitor shutdown: {error}");
        }
        // On removal failure CoreAudio still owns its Block_copy. Releasing our
        // RcBlock is safe: no raw callback context, monitor pointer, or DSP is
        // captured. The remaining Arc only reaches closed atomic signals.
    }
}

fn property_address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn native_status(
    operation: &'static str,
    object: u32,
    selector: u32,
    status: i32,
) -> Result<(), AudioMonitorError> {
    if status == 0 {
        Ok(())
    } else {
        Err(AudioMonitorError::Native {
            operation,
            object,
            selector,
            status,
        })
    }
}

fn property_size(
    object: u32,
    address: &AudioObjectPropertyAddress,
) -> Result<u32, AudioMonitorError> {
    let mut size = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    native_status("query property size", object, address.mSelector, status)?;
    Ok(size)
}

fn read_property(
    object: u32,
    address: &AudioObjectPropertyAddress,
    capacity: u32,
    output: NonNull<std::ffi::c_void>,
) -> Result<u32, AudioMonitorError> {
    let mut size = capacity;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            output,
        )
    };
    native_status("read property", object, address.mSelector, status)?;
    Ok(size)
}

fn expect_size(
    object: u32,
    selector: u32,
    actual: u32,
    expected: u32,
) -> Result<(), AudioMonitorError> {
    if actual == expected {
        Ok(())
    } else {
        Err(AudioMonitorError::PropertySize {
            object,
            selector,
            expected,
            actual,
        })
    }
}

fn read_u32(object: u32, selector: u32, scope: u32) -> Result<u32, AudioMonitorError> {
    let address = property_address(selector, scope);
    let expected = std::mem::size_of::<u32>() as u32;
    expect_size(object, selector, property_size(object, &address)?, expected)?;
    let mut value = 0_u32;
    let actual = read_property(object, &address, expected, NonNull::from(&mut value).cast())?;
    expect_size(object, selector, actual, expected)?;
    Ok(value)
}

fn read_f64(object: u32, selector: u32) -> Result<f64, AudioMonitorError> {
    let address = property_address(selector, kAudioObjectPropertyScopeGlobal);
    let expected = std::mem::size_of::<f64>() as u32;
    expect_size(object, selector, property_size(object, &address)?, expected)?;
    let mut value = 0.0_f64;
    let actual = read_property(object, &address, expected, NonNull::from(&mut value).cast())?;
    expect_size(object, selector, actual, expected)?;
    Ok(value)
}

fn read_stream_format(
    object: u32,
    selector: u32,
) -> Result<AudioStreamBasicDescription, AudioMonitorError> {
    let address = property_address(selector, kAudioObjectPropertyScopeGlobal);
    let expected = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    expect_size(object, selector, property_size(object, &address)?, expected)?;
    let mut format = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let actual = read_property(
        object,
        &address,
        expected,
        NonNull::from(&mut format).cast(),
    )?;
    expect_size(object, selector, actual, expected)?;
    validate_nominal_rate(format.mSampleRate).map_err(|detail| {
        AudioMonitorError::PropertyValue {
            object,
            selector,
            detail,
        }
    })?;
    Ok(format)
}

fn device_alive(value: u32) -> Result<bool, &'static str> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("device alive is not 0 or 1"),
    }
}

fn validate_nominal_rate(rate: f64) -> Result<(), &'static str> {
    if rate.is_finite() && rate > 0.0 {
        Ok(())
    } else {
        Err("nominal sample rate is not finite and positive")
    }
}

fn output_buffer_channels(bytes: &[u8]) -> Result<Vec<u32>, &'static str> {
    let offset = std::mem::offset_of!(AudioBufferList, mBuffers);
    let stride = std::mem::size_of::<AudioBuffer>();
    if bytes.len() < offset {
        return Err("output configuration header is truncated");
    }
    let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
    let required = count
        .checked_mul(stride)
        .and_then(|size| offset.checked_add(size))
        .ok_or("output configuration size overflow")?;
    if required != bytes.len() {
        return Err("output configuration buffer count does not match its size");
    }
    let mut channels = 0_u32;
    let mut layout = Vec::with_capacity(count);
    for buffer in bytes[offset..].chunks_exact(stride) {
        let start = std::mem::offset_of!(AudioBuffer, mNumberChannels);
        let count = u32::from_ne_bytes(buffer[start..start + 4].try_into().unwrap());
        channels = channels
            .checked_add(count)
            .ok_or("output channel count overflow")?;
        layout.push(count);
    }
    if channels == 0 {
        Err("device has no output channels")
    } else {
        Ok(layout)
    }
}

fn read_output_configuration(object: u32) -> Result<Vec<u32>, AudioMonitorError> {
    let selector = kAudioDevicePropertyStreamConfiguration;
    let address = property_address(selector, kAudioObjectPropertyScopeOutput);
    let size = property_size(object, &address)?;
    if size < std::mem::offset_of!(AudioBufferList, mBuffers) as u32 {
        return Err(AudioMonitorError::PropertyValue {
            object,
            selector,
            detail: "output configuration header is truncated",
        });
    }
    // u64 storage provides the alignment of the installed AudioBufferList ABI,
    // including its native pointers, and is allocated only on the output worker.
    let mut storage = vec![0_u64; (size as usize).div_ceil(std::mem::size_of::<u64>())];
    let actual = read_property(
        object,
        &address,
        size,
        NonNull::new(storage.as_mut_ptr()).unwrap().cast(),
    )?;
    expect_size(object, selector, actual, size)?;
    // All storage began initialized; only the exact bytes returned are decoded.
    let bytes = unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), size as usize) };
    output_buffer_channels(bytes).map_err(|detail| AudioMonitorError::PropertyValue {
        object,
        selector,
        detail,
    })
}

fn output_stream_ids(object: u32) -> Result<Vec<u32>, AudioMonitorError> {
    let selector = kAudioDevicePropertyStreams;
    let address = property_address(selector, kAudioObjectPropertyScopeOutput);
    let size = property_size(object, &address)?;
    let stride = std::mem::size_of::<u32>() as u32;
    if size == 0 || !size.is_multiple_of(stride) {
        return Err(AudioMonitorError::PropertyValue {
            object,
            selector,
            detail: "output stream list is empty or not an array of AudioStreamID",
        });
    }
    let mut streams = vec![0_u32; (size / stride) as usize];
    let actual = read_property(
        object,
        &address,
        size,
        NonNull::new(streams.as_mut_ptr()).unwrap().cast(),
    )?;
    expect_size(object, selector, actual, size)?;
    if streams.contains(&kAudioObjectUnknown) {
        return Err(AudioMonitorError::PropertyValue {
            object,
            selector,
            detail: "output stream list contains an unknown stream ID",
        });
    }
    Ok(streams)
}

#[derive(Clone, Copy)]
pub(crate) enum WindowRole {
    Scene,
    Settings,
}

impl WindowRole {
    fn content_size(self) -> NSSize {
        match self {
            Self::Scene => NSSize::new(960.0, 600.0),
            Self::Settings => NSSize::new(480.0, 360.0),
        }
    }

    fn style(self) -> NSWindowStyleMask {
        let common = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        match self {
            Self::Scene => common | NSWindowStyleMask::FullSizeContentView,
            Self::Settings => common,
        }
    }
}

pub(crate) struct PreparedWindow {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) display_id: DisplayId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowPlacement {
    // Owning-screen-relative outer top-left, followed by logical content dimensions.
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) display_uuid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowGeometryError {
    MainThreadRequired,
    DisplayUnavailable,
    InvalidGeometry,
    NativeHandleUnavailable,
}

impl fmt::Display for WindowGeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MainThreadRequired => "AppKit window geometry requires the main thread",
            Self::DisplayUnavailable => "the requested display has no available AppKit screen",
            Self::InvalidGeometry => {
                "window or screen geometry is not finite, positive, and usable"
            }
            Self::NativeHandleUnavailable => "the GPUI window has no available AppKit NSWindow",
        })
    }
}

impl std::error::Error for WindowGeometryError {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    top_left: NSPoint,
    content_size: NSSize,
}

#[derive(Clone, Copy)]
struct FrameMetrics {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl FrameMetrics {
    fn measured(frame: NSRect, content: NSRect) -> Self {
        Self {
            left: content.origin.x - frame.origin.x,
            bottom: content.origin.y - frame.origin.y,
            right: max_x(frame) - max_x(content),
            top: max_y(frame) - max_y(content),
        }
    }

    fn valid(self) -> bool {
        [self.left, self.bottom, self.right, self.top]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
            && (self.left + self.right).is_finite()
            && (self.bottom + self.top).is_finite()
    }

    fn frame_for_content(self, content: NSRect) -> NSRect {
        NSRect::new(
            NSPoint::new(content.origin.x - self.left, content.origin.y - self.bottom),
            NSSize::new(
                content.size.width + self.left + self.right,
                content.size.height + self.bottom + self.top,
            ),
        )
    }

    fn content_for_frame(self, frame: NSRect) -> NSRect {
        NSRect::new(
            NSPoint::new(frame.origin.x + self.left, frame.origin.y + self.bottom),
            NSSize::new(
                frame.size.width - self.left - self.right,
                frame.size.height - self.bottom - self.top,
            ),
        )
    }
}

fn max_x(rect: NSRect) -> f64 {
    rect.origin.x + rect.size.width
}

fn max_y(rect: NSRect) -> f64 {
    rect.origin.y + rect.size.height
}

fn valid_size(size: NSSize) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

fn valid_rect(rect: NSRect) -> bool {
    valid_size(rect.size)
        && rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && max_x(rect).is_finite()
        && max_y(rect).is_finite()
}

fn relative_top_left(screen: NSRect, outer: NSRect) -> Result<NSPoint, WindowGeometryError> {
    if !valid_rect(screen) || !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let result = NSPoint::new(
        outer.origin.x - screen.origin.x,
        max_y(screen) - max_y(outer),
    );
    if !result.x.is_finite() || !result.y.is_finite() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(result)
}

fn outer_from_top_left(
    screen: NSRect,
    top_left: NSPoint,
    outer_size: NSSize,
) -> Result<NSRect, WindowGeometryError> {
    if !valid_rect(screen) || !top_left.x.is_finite() || !top_left.y.is_finite() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let outer = NSRect::new(
        NSPoint::new(
            screen.origin.x + top_left.x,
            max_y(screen) - top_left.y - outer_size.height,
        ),
        outer_size,
    );
    if !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(outer)
}

fn fit_geometry(
    screen: NSRect,
    visible: Option<NSRect>,
    baseline: NSSize,
    metrics: FrameMetrics,
    saved: Option<&WindowPlacement>,
) -> Result<Placement, WindowGeometryError> {
    let visible = visible.ok_or(WindowGeometryError::DisplayUnavailable)?;
    if !valid_rect(screen) || !valid_rect(visible) || !valid_size(baseline) || !metrics.valid() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let available = metrics.content_for_frame(visible);
    if !valid_rect(available) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let content_size = NSSize::new(
        baseline.width.min(available.size.width),
        baseline.height.min(available.size.height),
    );
    let outer_size = metrics
        .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), content_size))
        .size;
    let top_left = if let Some(saved) = saved {
        if !saved.x.is_finite()
            || !saved.y.is_finite()
            || !valid_size(NSSize::new(saved.width, saved.height))
        {
            return Err(WindowGeometryError::InvalidGeometry);
        }
        let minimum = relative_top_left(screen, visible)?;
        let maximum = NSPoint::new(
            minimum.x + visible.size.width - outer_size.width,
            minimum.y + visible.size.height - outer_size.height,
        );
        if !maximum.x.is_finite()
            || !maximum.y.is_finite()
            || minimum.x > maximum.x
            || minimum.y > maximum.y
        {
            return Err(WindowGeometryError::InvalidGeometry);
        }
        NSPoint::new(
            saved.x.clamp(minimum.x, maximum.x),
            saved.y.clamp(minimum.y, maximum.y),
        )
    } else {
        relative_top_left(screen, centered_outer(visible, outer_size)?)?
    };
    // Exercise the same inverse used by the native adapter, rejecting overflow before opening.
    outer_from_top_left(screen, top_left, outer_size)?;
    Ok(Placement {
        top_left,
        content_size,
    })
}

fn centered_outer(visible: NSRect, outer_size: NSSize) -> Result<NSRect, WindowGeometryError> {
    if !valid_rect(visible)
        || !valid_size(outer_size)
        || outer_size.width > visible.size.width
        || outer_size.height > visible.size.height
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let outer = NSRect::new(
        NSPoint::new(
            visible.origin.x + (visible.size.width - outer_size.width) / 2.0,
            visible.origin.y + (visible.size.height - outer_size.height) / 2.0,
        ),
        outer_size,
    );
    if !valid_rect(outer) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(outer)
}

fn content_pixels(value: f64) -> Result<Pixels, WindowGeometryError> {
    let mut represented = value as f32;
    // GPUI stores logical dimensions as f32. Do not round fitted content up beyond the workarea.
    if f64::from(represented) > value {
        represented = represented.next_down();
    }
    if !represented.is_finite() || represented <= 0.0 {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(px(represented))
}

fn coordinate_pixels(
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<Pixels, WindowGeometryError> {
    if !value.is_finite() || !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let mut represented = value.clamp(minimum, maximum) as f32;
    if f64::from(represented) < minimum {
        represented = represented.next_up();
    }
    if f64::from(represented) > maximum {
        represented = represented.next_down();
    }
    if !represented.is_finite()
        || f64::from(represented) < minimum
        || f64::from(represented) > maximum
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    Ok(px(represented))
}

fn placement_from_geometry(
    screen: NSRect,
    outer: NSRect,
    content: NSRect,
    display_uuid: Option<String>,
) -> Result<WindowPlacement, WindowGeometryError> {
    if !valid_rect(content) || !valid_rect(outer) || !FrameMetrics::measured(outer, content).valid()
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let top_left = relative_top_left(screen, outer)?;
    Ok(WindowPlacement {
        x: top_left.x,
        y: top_left.y,
        width: content.size.width,
        height: content.size.height,
        display_uuid,
    })
}

pub(crate) fn prepare_window(
    role: WindowRole,
    display_id: DisplayId,
    saved: Option<&WindowPlacement>,
) -> Result<PreparedWindow, WindowGeometryError> {
    let mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let screens = NSScreen::screens(mtm);
    let mut geometry = None;
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let description = screen.deviceDescription();
        let Some(number) = description.objectForKey(ns_string!("NSScreenNumber")) else {
            continue;
        };
        let Some(number) = number.downcast_ref::<NSNumber>() else {
            continue;
        };
        if DisplayId::new(u64::from(number.unsignedIntValue())) == display_id {
            geometry = Some((screen.frame(), screen.visibleFrame()));
            break;
        }
    }
    let (screen, visible) = geometry.ok_or(WindowGeometryError::DisplayUnavailable)?;
    if !valid_rect(screen) || !valid_rect(visible) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let style = role.style();
    // These are AppKit's real conversions for the exact style GPUI will create.
    let available_content = NSWindow::contentRectForFrameRect_styleMask(visible, style, mtm);
    if !valid_rect(available_content) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let metrics = FrameMetrics::measured(visible, available_content);
    let placement = fit_geometry(screen, Some(visible), role.content_size(), metrics, saved)?;
    let content_size = size(
        content_pixels(placement.content_size.width)?,
        content_pixels(placement.content_size.height)?,
    );
    let native_content_size = NSSize::new(
        f64::from(content_size.width),
        f64::from(content_size.height),
    );
    let actual_frame = NSWindow::frameRectForContentRect_styleMask(
        NSRect::new(NSPoint::new(0.0, 0.0), native_content_size),
        style,
        mtm,
    );
    if !valid_rect(actual_frame) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let top_left = placement.top_left;
    let origin = point(
        coordinate_pixels(
            top_left.x,
            visible.origin.x - screen.origin.x,
            max_x(visible) - screen.origin.x - actual_frame.size.width,
        )?,
        coordinate_pixels(
            top_left.y,
            max_y(screen) - max_y(visible),
            max_y(screen) - visible.origin.y - actual_frame.size.height,
        )?,
    );
    outer_from_top_left(
        screen,
        NSPoint::new(f64::from(origin.x), f64::from(origin.y)),
        actual_frame.size,
    )?;
    Ok(PreparedWindow {
        bounds: Bounds::new(origin, content_size),
        display_id,
    })
}

pub(crate) fn capture_placement(
    window: &gpui::Window,
) -> Result<(WindowPlacement, DisplayId), WindowGeometryError> {
    let _mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| WindowGeometryError::NativeHandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(WindowGeometryError::NativeHandleUnavailable);
    };
    // SAFETY: WindowHandle guarantees this NSView for the borrowed GPUI Window.
    // AppKit access is on the main thread and neither view nor getter-owned native
    // objects escape this synchronous scope. Capture performs no native mutations.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = view
        .window()
        .ok_or(WindowGeometryError::NativeHandleUnavailable)?;
    let screen = native
        .screen()
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let description = screen.deviceDescription();
    let number = description
        .objectForKey(ns_string!("NSScreenNumber"))
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let number = number
        .downcast_ref::<NSNumber>()
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let display_id = DisplayId::new(u64::from(number.unsignedIntValue()));
    let outer = native.frame();
    let placement = placement_from_geometry(
        screen.frame(),
        outer,
        native.contentRectForFrameRect(outer),
        None,
    )?;
    Ok((placement, display_id))
}

pub(crate) fn constrain_window(
    window: &gpui::Window,
    prepared: &PreparedWindow,
) -> Result<(), WindowGeometryError> {
    let _mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let content_size = NSSize::new(
        f64::from(prepared.bounds.size.width),
        f64::from(prepared.bounds.size.height),
    );
    if !valid_size(content_size)
        || !f64::from(prepared.bounds.origin.x).is_finite()
        || !f64::from(prepared.bounds.origin.y).is_finite()
    {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| WindowGeometryError::NativeHandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(WindowGeometryError::NativeHandleUnavailable);
    };
    // SAFETY: WindowHandle guarantees a valid NSView for the borrowed GPUI Window.
    // MainThreadMarker was checked above. This reference never escapes this synchronous
    // scope, and the raw pointer is neither retained nor released.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = view
        .window()
        .ok_or(WindowGeometryError::NativeHandleUnavailable)?;
    let frame = native.frame();
    if !valid_rect(frame) || !valid_rect(native.contentRectForFrameRect(frame)) {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    // All fallible work precedes mutations: an error leaves existing finite placement
    // and constraints untouched. NSView::window's getter-owned Retained ends here.
    let mut behavior = native.collectionBehavior();
    behavior.remove(
        NSWindowCollectionBehavior::FullScreenPrimary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::FullScreenAllowsTiling,
    );
    behavior.insert(
        NSWindowCollectionBehavior::FullScreenNone
            | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
    );
    native.setCollectionBehavior(behavior);
    if let Some(zoom) = native.standardWindowButton(NSWindowButton::ZoomButton) {
        zoom.setEnabled(false);
    }
    native.setContentMinSize(content_size);
    native.setContentMaxSize(content_size);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RefitTarget {
    outer: NSRect,
    content: NSSize,
}

/// Recomputes a live window's fixed size and position for its current screen. The
/// baseline is the role's fixed size, never the window's previous (possibly shrunk)
/// size. `None` means the window already matches within half a point.
fn refit_target(
    screen: NSRect,
    visible: NSRect,
    baseline: NSSize,
    metrics: FrameMetrics,
    current_outer: NSRect,
) -> Result<Option<RefitTarget>, WindowGeometryError> {
    if !valid_rect(current_outer) || !metrics.valid() {
        return Err(WindowGeometryError::InvalidGeometry);
    }
    let current_content = metrics.content_for_frame(current_outer);
    let top_left = relative_top_left(screen, current_outer)?;
    let saved = WindowPlacement {
        x: top_left.x,
        y: top_left.y,
        width: current_content.size.width,
        height: current_content.size.height,
        display_uuid: None,
    };
    let fitted = fit_geometry(screen, Some(visible), baseline, metrics, Some(&saved))?;
    let outer_size = metrics
        .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), fitted.content_size))
        .size;
    let outer = outer_from_top_left(screen, fitted.top_left, outer_size)?;
    let same = |a: f64, b: f64| (a - b).abs() < 0.5;
    if same(outer.origin.x, current_outer.origin.x)
        && same(outer.origin.y, current_outer.origin.y)
        && same(outer.size.width, current_outer.size.width)
        && same(outer.size.height, current_outer.size.height)
    {
        return Ok(None);
    }
    Ok(Some(RefitTarget {
        outer,
        content: fitted.content_size,
    }))
}

/// Re-fits a live window after its screen or workarea changed. All fallible work
/// precedes native mutation, so an unavailable screen keeps the last finite size.
/// Returns whether the native frame changed. AppKit's resize/move callbacks then run
/// synchronously; while GPUI holds the App borrow they cannot update the window, so
/// the caller must follow a `true` with `Window::bounds_changed`.
pub(crate) fn refit_window(
    window: &gpui::Window,
    role: WindowRole,
) -> Result<bool, WindowGeometryError> {
    let _mtm = MainThreadMarker::new().ok_or(WindowGeometryError::MainThreadRequired)?;
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| WindowGeometryError::NativeHandleUnavailable)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(WindowGeometryError::NativeHandleUnavailable);
    };
    // SAFETY: WindowHandle guarantees a valid NSView for the borrowed GPUI Window.
    // The main thread was checked above; this reference and the getter-owned native
    // objects end with this synchronous scope and the raw pointer is never retained.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = view
        .window()
        .ok_or(WindowGeometryError::NativeHandleUnavailable)?;
    let screen = native
        .screen()
        .ok_or(WindowGeometryError::DisplayUnavailable)?;
    let frame = native.frame();
    let metrics = FrameMetrics::measured(frame, native.contentRectForFrameRect(frame));
    let Some(target) = refit_target(
        screen.frame(),
        screen.visibleFrame(),
        role.content_size(),
        metrics,
        frame,
    )?
    else {
        return Ok(false);
    };
    native.setContentMinSize(target.content);
    native.setContentMaxSize(target.content);
    native.setFrame_display(target.outer, true);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    #[test]
    fn invalidated_audio_objects_do_not_block_device_listener_retirement() {
        let signals = Arc::new(RecoverySignals::new());
        let mut monitor = OutputDeviceMonitor {
            thread: std::thread::current().id(),
            _worker_local: PhantomData,
            signals: Arc::clone(&signals),
            global: None,
            device: Some(kAudioObjectUnknown),
            device_listeners: Vec::new(),
        };
        // Unknown is guaranteed not to name a live HAL object. Calling the real
        // removal API recreates the destroyed-device/stream status without
        // changing the default output or disconnecting physical hardware.
        for (selector, scope) in [
            (
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            ),
            (
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            ),
            (
                kAudioDevicePropertyStreamConfiguration,
                kAudioObjectPropertyScopeOutput,
            ),
            (kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
            (
                kAudioStreamPropertyVirtualFormat,
                kAudioObjectPropertyScopeGlobal,
            ),
            (
                kAudioStreamPropertyPhysicalFormat,
                kAudioObjectPropertyScopeGlobal,
            ),
        ] {
            let captured = Arc::clone(&signals);
            monitor.device_listeners.push(OutputListener {
                object: kAudioObjectUnknown,
                address: property_address(selector, scope),
                block: RcBlock::new(move |_: u32, _: NonNull<AudioObjectPropertyAddress>| {
                    captured.output_changed();
                }),
            });
        }
        let retired = monitor.remove_device_listeners();
        assert!(
            retired.is_ok(),
            "Invalidated objects blocked retirement: {retired:?}"
        );
        assert!(
            monitor.device_listeners.is_empty(),
            "Dead handles still prevent replacement"
        );
    }

    fn output_configuration(channels: &[u32]) -> Vec<u8> {
        let offset = std::mem::offset_of!(AudioBufferList, mBuffers);
        let stride = std::mem::size_of::<AudioBuffer>();
        let mut bytes = vec![0; offset + channels.len() * stride];
        bytes[..4].copy_from_slice(&(channels.len() as u32).to_ne_bytes());
        for (index, channels) in channels.iter().enumerate() {
            let start = offset + index * stride;
            bytes[start..start + 4].copy_from_slice(&channels.to_ne_bytes());
        }
        bytes
    }

    #[test]
    fn native_output_configuration_rejects_truncated_and_oversized_buffer_counts() {
        let valid = output_configuration(&[1, 1]);
        assert_eq!(output_buffer_channels(&valid), Ok(vec![1, 1]));
        assert!(output_buffer_channels(&valid[..valid.len() - 1]).is_err());
        let mut excessive = valid;
        excessive[..4].copy_from_slice(&u32::MAX.to_ne_bytes());
        assert!(output_buffer_channels(&excessive).is_err());
        assert!(output_buffer_channels(&[0; 3]).is_err());
    }

    #[test]
    fn native_output_configuration_rejects_absent_channels_and_overflow() {
        assert!(output_buffer_channels(&output_configuration(&[])).is_err());
        assert!(output_buffer_channels(&output_configuration(&[0, 0])).is_err());
        assert!(output_buffer_channels(&output_configuration(&[u32::MAX, 1])).is_err());
    }

    #[test]
    fn native_device_values_reject_non_boolean_alive_and_invalid_rate() {
        assert_eq!(device_alive(0), Ok(false));
        assert_eq!(device_alive(1), Ok(true));
        assert!(device_alive(2).is_err());
        assert!(validate_nominal_rate(0.0).is_err());
        assert!(validate_nominal_rate(f64::NAN).is_err());
        assert!(validate_nominal_rate(f64::INFINITY).is_err());
        assert_eq!(validate_nominal_rate(44_100.0), Ok(()));
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    fn metrics() -> FrameMetrics {
        // Measured frame/content deltas are input data, not a titlebar constant.
        FrameMetrics {
            left: 3.0,
            bottom: 2.0,
            right: 3.0,
            top: 29.0,
        }
    }

    fn saved(x: f64, y: f64, width: f64, height: f64) -> WindowPlacement {
        WindowPlacement {
            x,
            y,
            width,
            height,
            display_uuid: None,
        }
    }

    #[test]
    fn preparing_off_main_thread_fails_before_native_access() {
        let rejected = std::thread::spawn(|| {
            matches!(
                prepare_window(WindowRole::Scene, DisplayId::new(0), None),
                Err(WindowGeometryError::MainThreadRequired),
            )
        })
        .join()
        .unwrap();
        assert!(rejected);
    }

    #[test]
    fn capture_preserves_actual_offscreen_position_then_restore_uses_current_viewport() {
        let old_screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-2000.0, 400.0, 414.0, 259.0);
        let outer = metrics().frame_for_content(content);
        let captured = placement_from_geometry(old_screen, outer, content, None).unwrap();
        assert_eq!((captured.x, captured.y), (-83.0, -308.0));
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        let visible = rect(10.0, 20.0, 1420.0, 850.0);
        let restored = fit_geometry(
            screen,
            Some(visible),
            WindowRole::Scene.content_size(),
            metrics(),
            Some(&captured),
        )
        .unwrap();
        assert_eq!(restored.top_left, NSPoint::new(10.0, 30.0));
        assert_eq!(restored.content_size, WindowRole::Scene.content_size());
        assert_ne!(
            restored.content_size,
            NSSize::new(captured.width, captured.height),
        );
    }

    #[test]
    fn saved_outer_top_left_restores_on_a_negative_screen() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, -680.0, 1900.0, 1030.0);
        let saved = saved(220.0, 140.0, 720.0, 480.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            WindowRole::Settings.content_size(),
            metrics(),
            Some(&saved),
        )
        .unwrap();
        assert_eq!(placement.top_left, NSPoint::new(saved.x, saved.y));
        assert_eq!(placement.content_size, WindowRole::Settings.content_size());
        let frame = metrics()
            .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), placement.content_size));
        let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
        assert_eq!(outer.origin, NSPoint::new(-1700.0, -151.0));
    }

    #[test]
    fn offscreen_saved_positions_clamp_to_current_workarea_edges() {
        let screen = rect(-1920.0, 200.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, 220.0, 1900.0, 1030.0);
        for (x, y, expected) in [
            (-f64::MAX, -f64::MAX, NSPoint::new(10.0, 30.0)),
            (f64::MAX, f64::MAX, NSPoint::new(1424.0, 669.0)),
        ] {
            let saved = saved(x, y, f64::MAX, f64::MAX);
            let placement = fit_geometry(
                screen,
                Some(visible),
                WindowRole::Settings.content_size(),
                metrics(),
                Some(&saved),
            )
            .unwrap();
            assert_eq!(placement.top_left, expected);
            let frame = metrics()
                .frame_for_content(NSRect::new(NSPoint::new(0.0, 0.0), placement.content_size));
            let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
            assert!(outer.origin.x >= visible.origin.x);
            assert!(outer.origin.y >= visible.origin.y);
            assert!(max_x(outer) <= max_x(visible));
            assert!(max_y(outer) <= max_y(visible));
        }
    }

    #[test]
    fn restored_size_uses_current_fixed_role_and_available_content() {
        let screen = rect(-1300.0, -400.0, 800.0, 600.0);
        let visible = rect(-1290.0, -380.0, 420.0, 290.0);
        for saved in [
            saved(100.0, 100.0, 1.0, 1.0),
            saved(100.0, 100.0, 8000.0, 6000.0),
        ] {
            for role in [WindowRole::Scene, WindowRole::Settings] {
                let placement = fit_geometry(
                    screen,
                    Some(visible),
                    role.content_size(),
                    metrics(),
                    Some(&saved),
                )
                .unwrap();
                assert_eq!(placement.content_size, NSSize::new(414.0, 259.0));
                assert_eq!(placement.top_left, NSPoint::new(10.0, 290.0));
            }
        }
    }

    #[test]
    fn captured_geometry_keeps_outer_coordinates_and_logical_content_size() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-1700.0, -300.0, 480.25, 360.5);
        let outer = metrics().frame_for_content(content);
        let uuid = Some("owning-display".to_owned());
        let placement = placement_from_geometry(screen, outer, content, uuid.clone()).unwrap();
        let expected = relative_top_left(screen, outer).unwrap();
        assert_eq!(placement.x, expected.x);
        assert_eq!(placement.y, expected.y);
        assert_eq!(placement.width, content.size.width);
        assert_eq!(placement.height, content.size.height);
        assert_ne!(placement.height, outer.size.height);
        assert_eq!(placement.display_uuid, uuid);
        assert_eq!(
            outer_from_top_left(screen, NSPoint::new(placement.x, placement.y), outer.size,)
                .unwrap(),
            outer,
        );
        assert!(
            placement_from_geometry(screen, outer, content, None)
                .unwrap()
                .display_uuid
                .is_none()
        );
    }

    #[test]
    fn nonfinite_saved_fields_and_nonpositive_dimensions_fail_explicitly() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        for saved in [
            saved(f64::NAN, 20.0, 480.0, 360.0),
            saved(20.0, f64::INFINITY, 480.0, 360.0),
            saved(20.0, 20.0, 0.0, 360.0),
            saved(20.0, 20.0, 480.0, -1.0),
            saved(20.0, 20.0, f64::INFINITY, 360.0),
            saved(20.0, 20.0, 480.0, f64::NAN),
        ] {
            assert_eq!(
                fit_geometry(
                    screen,
                    Some(screen),
                    WindowRole::Settings.content_size(),
                    metrics(),
                    Some(&saved),
                ),
                Err(WindowGeometryError::InvalidGeometry),
            );
        }
    }

    #[test]
    fn capture_rejects_invalid_native_content_and_coordinate_overflow() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        let outer = rect(20.0, 20.0, 486.0, 391.0);
        for content in [
            rect(23.0, 22.0, 480.0, 0.0),
            rect(23.0, 22.0, f64::NAN, 360.0),
            rect(23.0, 22.0, 480.0, f64::INFINITY),
            rect(19.0, 22.0, 480.0, 360.0),
        ] {
            assert_eq!(
                placement_from_geometry(screen, outer, content, None),
                Err(WindowGeometryError::InvalidGeometry),
            );
        }
        assert_eq!(
            placement_from_geometry(
                rect(-1e308, 0.0, 1e300, 900.0),
                rect(1e308, 0.0, 480.0, 360.0),
                rect(1e308, 0.0, 480.0, 360.0),
                None,
            ),
            Err(WindowGeometryError::InvalidGeometry),
        );
    }

    #[test]
    fn narrow_negative_screen_fits_the_entire_outer_frame() {
        let screen = rect(-1300.0, -400.0, 800.0, 600.0);
        let visible = rect(-1290.0, -380.0, 420.0, 290.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            NSSize::new(960.0, 600.0),
            metrics(),
            None,
        )
        .unwrap();
        assert_eq!(placement.content_size, NSSize::new(414.0, 259.0));
        let outer = metrics().frame_for_content(rect(
            0.0,
            0.0,
            placement.content_size.width,
            placement.content_size.height,
        ));
        let outer = outer_from_top_left(screen, placement.top_left, outer.size).unwrap();
        assert_eq!(outer, visible);
        assert_eq!(placement.top_left, NSPoint::new(10.0, 290.0));
    }

    #[test]
    fn roomy_workarea_keeps_baseline_content_and_centers_outer_frame() {
        let screen = rect(-1920.0, 200.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, 220.0, 1900.0, 1030.0);
        let placement = fit_geometry(
            screen,
            Some(visible),
            NSSize::new(480.0, 360.0),
            metrics(),
            None,
        )
        .unwrap();
        assert_eq!(placement.content_size, NSSize::new(480.0, 360.0));
        let frame = metrics().frame_for_content(rect(0.0, 0.0, 480.0, 360.0));
        let outer = outer_from_top_left(screen, placement.top_left, frame.size).unwrap();
        assert_eq!(outer.origin, NSPoint::new(-1203.0, 539.5));
        assert_eq!(placement.top_left, NSPoint::new(717.0, 349.5));
    }

    #[test]
    fn frame_content_and_relative_top_left_round_trip() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let content = rect(-1700.0, -300.0, 480.0, 360.0);
        let frame = metrics().frame_for_content(content);
        assert_eq!(metrics().content_for_frame(frame), content);
        let top_left = relative_top_left(screen, frame).unwrap();
        assert_eq!(
            outer_from_top_left(screen, top_left, frame.size).unwrap(),
            frame
        );
    }

    #[test]
    fn fractional_frame_content_round_trip_preserves_top_left_within_tolerance() {
        let screen = rect(-1600.3, -900.7, 1600.3, 900.7);
        let content = rect(-1400.2, -700.1, 480.2, 360.3);
        let measured = FrameMetrics {
            left: 0.3,
            bottom: 0.4,
            right: 0.7,
            top: 28.6,
        };
        let frame = measured.frame_for_content(content);
        let restored = measured.frame_for_content(measured.content_for_frame(frame));
        let expected = relative_top_left(screen, frame).unwrap();
        let actual = relative_top_left(screen, restored).unwrap();
        assert!((actual.x - expected.x).abs() < 1e-9);
        assert!((actual.y - expected.y).abs() < 1e-9);
    }

    #[test]
    fn invalid_and_unavailable_workareas_return_typed_failure() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        for visible in [
            rect(0.0, 0.0, 0.0, 875.0),
            rect(0.0, 0.0, 1440.0, -1.0),
            rect(f64::NAN, 0.0, 100.0, 100.0),
            rect(0.0, 0.0, f64::INFINITY, 100.0),
            rect(0.0, 0.0, 4.0, 20.0),
        ] {
            let update = fit_geometry(
                screen,
                Some(visible),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            );
            assert_eq!(update, Err(WindowGeometryError::InvalidGeometry));
        }
        let unavailable = fit_geometry(screen, None, NSSize::new(480.0, 360.0), metrics(), None);
        assert_eq!(unavailable, Err(WindowGeometryError::DisplayUnavailable));
    }

    #[test]
    fn invalid_screen_content_and_metrics_are_rejected() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        assert!(
            fit_geometry(
                rect(0.0, 0.0, 0.0, 900.0),
                Some(screen),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            )
            .is_err()
        );
        assert!(
            fit_geometry(
                rect(f64::MAX, 0.0, f64::MAX, 900.0),
                Some(screen),
                NSSize::new(480.0, 360.0),
                metrics(),
                None,
            )
            .is_err()
        );
        for size in [
            NSSize::new(0.0, 360.0),
            NSSize::new(-1.0, 360.0),
            NSSize::new(480.0, f64::NAN),
            NSSize::new(f64::INFINITY, 360.0),
        ] {
            assert!(fit_geometry(screen, Some(screen), size, metrics(), None).is_err());
        }
        let invalid = FrameMetrics {
            top: f64::NAN,
            ..metrics()
        };
        assert!(
            fit_geometry(
                screen,
                Some(screen),
                NSSize::new(480.0, 360.0),
                invalid,
                None
            )
            .is_err()
        );
        let negative = FrameMetrics {
            left: -1.0,
            ..metrics()
        };
        assert!(
            fit_geometry(
                screen,
                Some(screen),
                NSSize::new(480.0, 360.0),
                negative,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn fitted_content_rounds_down_at_the_gpui_boundary_and_rejects_underflow() {
        let lower = 480.0_f32;
        let upper = lower.next_up();
        let available = f64::from(lower) + (f64::from(upper) - f64::from(lower)) * 0.75;
        assert_eq!(content_pixels(available), Ok(px(lower)));
        assert_eq!(
            content_pixels(f64::from(f32::from_bits(1)) * 0.25),
            Err(WindowGeometryError::InvalidGeometry)
        );
    }

    extern "C-unwind" fn existing_should_terminate(
        _this: &AnyObject,
        _command: Sel,
        _sender: *mut AnyObject,
    ) -> NSUInteger {
        TERMINATE_NOW
    }

    #[test]
    fn the_termination_gate_is_additive_defers_until_permitted_and_never_replaces() {
        use objc2::{ClassType, msg_send, runtime::ClassBuilder};
        use objc2_foundation::NSObject;
        use std::sync::atomic::AtomicUsize;

        assert_eq!(
            install_on_class(std::ptr::null(), || {}),
            Err(TerminationGateError::DelegateClassMissing)
        );

        // A class that already implements the selector is an integration failure and
        // its implementation keeps answering.
        let mut builder = ClassBuilder::new(c"AmbienGateFixtureImplemented", NSObject::class())
            .expect("fixture class name is unused");
        // SAFETY: the method signature matches applicationShouldTerminate:.
        unsafe {
            builder.add_method(
                sel!(applicationShouldTerminate:),
                existing_should_terminate as extern "C-unwind" fn(_, _, _) -> _,
            );
        }
        let implemented = builder.register();
        assert_eq!(
            install_on_class(implemented, || {}),
            Err(TerminationGateError::SelectorAlreadyImplemented)
        );
        assert!(
            TERMINATION_GATE.get().is_none(),
            "a refused install leaves no gate"
        );

        // A class without it gets the gate, and only the gate.
        let absent = ClassBuilder::new(c"AmbienGateFixtureAbsent", NSObject::class())
            .expect("fixture class name is unused")
            .register();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);
        install_on_class(absent, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        // SAFETY: a plain NSObject subclass instance and the declared selector.
        let object: Retained<AnyObject> = unsafe { msg_send![absent, new] };
        let ask = || -> NSUInteger {
            unsafe {
                msg_send![&*object, applicationShouldTerminate: std::ptr::null_mut::<AnyObject>()]
            }
        };
        let gate = TERMINATION_GATE.get().unwrap();
        assert_eq!(ask(), TERMINATE_LATER, "not committed: AppKit must wait");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(gate.pending_reply.load(Ordering::SeqCst));
        assert_eq!(ask(), TERMINATE_LATER);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            2,
            "each OS request is signalled"
        );
        gate.permitted.store(true, Ordering::SeqCst);
        assert_eq!(ask(), TERMINATE_NOW, "once committed the quit goes through");
        assert_eq!(requests.load(Ordering::SeqCst), 2);

        // The gate cannot be installed twice, not even on another class.
        let another = ClassBuilder::new(c"AmbienGateFixtureAnother", NSObject::class())
            .expect("fixture class name is unused")
            .register();
        assert_eq!(
            install_on_class(another, || {}),
            Err(TerminationGateError::AlreadyInstalled)
        );
    }

    #[test]
    fn coordinates_stay_inside_representable_closed_intervals() {
        let lower = 24.0_f32;
        let upper = lower.next_up();
        let left = f64::from(lower);
        let right = f64::from(upper);
        let gap = right - left;
        assert_eq!(
            coordinate_pixels(left + gap * 0.3, left + gap * 0.25, right),
            Ok(px(upper))
        );
        assert_eq!(
            coordinate_pixels(left + gap * 0.7, left, left + gap * 0.75),
            Ok(px(lower))
        );
        assert_eq!(
            coordinate_pixels(left + gap * 0.5, left + gap * 0.25, left + gap * 0.75),
            Err(WindowGeometryError::InvalidGeometry)
        );
    }

    fn contains(visible: NSRect, outer: NSRect) -> bool {
        outer.origin.x >= visible.origin.x
            && outer.origin.y >= visible.origin.y
            && max_x(outer) <= max_x(visible)
            && max_y(outer) <= max_y(visible)
    }

    #[test]
    fn a_window_that_already_fits_its_screen_is_not_touched() {
        let screen = rect(0.0, 0.0, 1920.0, 1080.0);
        let visible = rect(0.0, 25.0, 1920.0, 1030.0);
        let outer = rect(100.0, 200.0, 966.0, 631.0);
        assert_eq!(
            refit_target(screen, visible, NSSize::new(960.0, 600.0), metrics(), outer),
            Ok(None)
        );
    }

    #[test]
    fn a_smaller_workarea_shrinks_the_window_and_a_roomy_one_restores_the_baseline() {
        let baseline = NSSize::new(960.0, 600.0);
        let narrow_screen = rect(0.0, 0.0, 800.0, 500.0);
        let narrow = rect(0.0, 0.0, 800.0, 500.0);
        let current = rect(0.0, 0.0, 966.0, 631.0);
        let shrunk = refit_target(narrow_screen, narrow, baseline, metrics(), current)
            .unwrap()
            .unwrap();
        assert_eq!(shrunk.content, NSSize::new(794.0, 469.0));
        assert!(contains(narrow, shrunk.outer));
        // The earlier shrink is not kept as a free size: the roomy screen starts from baseline again.
        let roomy_screen = rect(0.0, 0.0, 2560.0, 1440.0);
        let roomy = rect(0.0, 0.0, 2560.0, 1415.0);
        let restored = refit_target(roomy_screen, roomy, baseline, metrics(), shrunk.outer)
            .unwrap()
            .unwrap();
        assert_eq!(restored.content, baseline);
        assert!(contains(roomy, restored.outer));
    }

    #[test]
    fn a_window_left_off_a_removed_or_moved_display_is_pulled_inside_on_a_negative_origin_screen() {
        let screen = rect(-1920.0, -700.0, 1920.0, 1080.0);
        let visible = rect(-1910.0, -680.0, 1900.0, 1030.0);
        let stranded = rect(-5000.0, 4000.0, 966.0, 631.0);
        let target = refit_target(
            screen,
            visible,
            NSSize::new(960.0, 600.0),
            metrics(),
            stranded,
        )
        .unwrap()
        .unwrap();
        assert_eq!(target.content, NSSize::new(960.0, 600.0));
        assert_eq!(target.outer.size, NSSize::new(966.0, 631.0));
        assert!(contains(visible, target.outer));
    }

    #[test]
    fn refit_reports_an_unavailable_or_invalid_workarea_without_inventing_one() {
        let screen = rect(0.0, 0.0, 1440.0, 900.0);
        let outer = rect(10.0, 10.0, 966.0, 631.0);
        let baseline = NSSize::new(960.0, 600.0);
        for visible in [rect(0.0, 0.0, 0.0, 875.0), rect(f64::NAN, 0.0, 10.0, 10.0)] {
            assert_eq!(
                refit_target(screen, visible, baseline, metrics(), outer),
                Err(WindowGeometryError::InvalidGeometry)
            );
        }
        assert_eq!(
            refit_target(
                screen,
                screen,
                baseline,
                metrics(),
                rect(0.0, 0.0, f64::NAN, 5.0)
            ),
            Err(WindowGeometryError::InvalidGeometry)
        );
    }
}
