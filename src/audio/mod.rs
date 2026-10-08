pub(crate) mod lease;
mod mix;
mod render;
pub(crate) mod timing;
pub(crate) mod transport;
mod world;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
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

#[derive(Clone, PartialEq)]
pub(crate) enum OutputStatus {
    Starting,
    Running {
        sample_rate: u32,
        channels: u16,
        format: cpal::SampleFormat,
        notices: u64,
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

    pub(crate) fn text(&self) -> String {
        match self {
            Self::Starting => "標準出力を準備しています。".into(),
            Self::Running {
                sample_rate,
                channels,
                format,
                notices,
            } => {
                let mut text =
                    format!("標準出力で再生中: {sample_rate} Hz / {channels} channel / {format:?}");
                if notices & ErrorCode::XRUN != 0 {
                    text.push_str("。出力の遅延を検出しました。");
                }
                if notices & ErrorCode::REALTIME_DENIED != 0 {
                    text.push_str("。OS によるリアルタイム優先度の許可は得られていません。");
                }
                text
            }
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
    worker: Option<JoinHandle<()>>,
}

impl AudioController {
    pub(crate) fn start(registry: Arc<Registry>, controls: ControlState) -> (Self, Receiver<()>) {
        let shared = Arc::new(AudioShared {
            state: Mutex::new((OutputStatus::Starting, None, None)),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (notify, updates) = async_channel::bounded(1);
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let failure_notify = notify.clone();
        let worker = match thread::Builder::new()
            .name("ambien-output".into())
            .spawn(move || {
                run_worker(worker_shared, worker_stop, notify, registry, controls);
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

struct PreparedOutput {
    stream: cpal::Stream,
    lease: Arc<EngineLease>,
    errors: Arc<ErrorCell>,
    source: Arc<AudioSource>,
    sample_rate: u32,
    channels: u16,
    format: cpal::SampleFormat,
}

impl PreparedOutput {
    fn take_engine(&self) -> PreparedEngine {
        self.lease.close();
        loop {
            if let Some(engine) = self.lease.try_take() {
                return engine;
            }
            // Only the non-RT worker retries. An admitted callback remains bounded.
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn prepare_output(
    registry: Arc<Registry>,
    controls: ControlState,
) -> Result<PreparedOutput, String> {
    // This endpoint is CPAL's DefaultOutput; do not enumerate it as a HalOutput device.
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
    let channels = config.channels;
    let sample_rate = config.sample_rate;
    let visuals = Arc::new(VisualRing::new(
        EVENT_CAPACITY,
        registry.definitions().len(),
    ));
    let clock = MachClock::new().map_err(|error| error.to_string())?;
    let seed = clock.now().map_err(|error| error.to_string())?.get();
    let engine = PreparedEngine::prepare(
        registry.clone(),
        controls,
        visuals.clone(),
        sample_rate,
        seed,
    )
    .map_err(|error| error.to_string())?;
    let mailbox = engine.control_mailbox().clone();
    let receipts = engine.status_mailbox().clone();
    let epoch = StreamEpoch::new(1).expect("initial stream generation is nonzero");
    let lease = Arc::new(EngineLease::new(engine, epoch));
    let epoch = lease.gate().epoch();
    let errors = Arc::new(ErrorCell::new(epoch));
    let callback_lease = lease.clone();
    let callback_errors = errors.clone();
    let monitor_errors = errors.clone();
    let stream_result = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
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
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
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
        _ => unreachable!("sample format was checked before engine preparation"),
    };
    let stream = match stream_result {
        Ok(stream) => stream,
        Err(error) => {
            lease.close();
            // Build failure cannot destroy DSP through a callback-owned last Arc.
            while lease.try_take().is_none() {
                thread::sleep(Duration::from_millis(1));
            }
            return Err(error.to_string());
        }
    };
    Ok(PreparedOutput {
        source: Arc::new(AudioSource {
            registry,
            visuals,
            controls: mailbox,
            receipts,
            epoch,
            clock,
            errors: errors.clone(),
            freeze_time: AtomicU64::new(0),
        }),
        stream,
        lease,
        errors,
        sample_rate,
        channels,
        format,
    })
}

fn run_worker(
    shared: Arc<AudioShared>,
    stop: Arc<AtomicBool>,
    notify: Sender<()>,
    registry: Arc<Registry>,
    controls: ControlState,
) {
    let output = match prepare_output(registry, controls) {
        Ok(output) => output,
        Err(reason) => {
            shared.set_status(
                OutputStatus::Failed {
                    reason,
                    world: None,
                },
                &notify,
            );
            return;
        }
    };
    if let Err(error) = output.stream.play() {
        let engine = output.take_engine();
        shared.set_status(
            OutputStatus::Failed {
                reason: format!("標準出力を開始できません: {error}"),
                world: Some(engine.world()),
            },
            &notify,
        );
        return;
    }
    let mut status = OutputStatus::Running {
        sample_rate: output.sample_rate,
        channels: output.channels,
        format: output.format,
        notices: 0,
    };
    *shared.state.lock() = (status.clone(), Some(output.source.clone()), None);
    let _ = notify.try_send(());
    let mut frozen_engine = None;
    while !stop.load(Ordering::SeqCst) {
        shared.accept_receipt(&output.source, &notify);
        let bits = output.errors.take_bits();
        if bits & (ErrorCode::FATAL | ErrorCode::DEVICE_CHANGED) != 0 {
            let engine = output.take_engine();
            let pause = output.stream.pause().err();
            let reason = if bits & ErrorCode::DEVICE_CHANGED != 0 {
                "出力先が変わり、旧 stream の時刻対応を無効にしました".to_owned()
            } else {
                format!("標準出力の失敗 (error bits 0x{bits:x})")
            };
            let reason = match pause {
                Some(error) => format!("{reason}。stream 停止も失敗しました: {error}"),
                None => reason,
            };
            let reason = match output.source.clock.now() {
                Ok(now) => {
                    output.source.freeze_time.store(now.get(), Ordering::SeqCst);
                    reason
                }
                Err(error) => format!("{reason}。停止時刻の取得も失敗しました: {error}"),
            };
            shared.set_status(
                OutputStatus::Failed {
                    reason,
                    world: Some(engine.world()),
                },
                &notify,
            );
            frozen_engine = Some(engine);
            break;
        }
        if bits != 0 {
            if let OutputStatus::Running { notices, .. } = &mut status {
                *notices |= bits;
            }
            shared.set_status(status.clone(), &notify);
        }
        thread::sleep(Duration::from_millis(20));
    }
    if frozen_engine.is_none() {
        frozen_engine = Some(output.take_engine());
    }
    // Keep the frozen world/DSP owned by this worker, even while both windows are closed.
    while !stop.load(Ordering::SeqCst) {
        if let Some(engine) = &mut frozen_engine {
            if !engine.sync_frozen_controls(output.source.epoch) {
                shared.set_status(
                    OutputStatus::Failed {
                        reason: "制御受領の公開に失敗しました。世界時刻を保持しています".into(),
                        world: Some(engine.world()),
                    },
                    &notify,
                );
            }
            shared.accept_receipt(&output.source, &notify);
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(output);
    drop(frozen_engine);
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
}
