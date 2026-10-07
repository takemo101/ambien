use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::render::PreparedEngine;
use super::timing::StreamEpoch;

const OWNER_IDLE: u8 = 0;
const OWNER_CALLBACK: u8 = 1;
const OWNER_WORKER: u8 = 2;

/// This gate can only be enabled by constructing its fresh lease.
pub struct EpochGate {
    epoch: StreamEpoch,
    enabled: AtomicBool,
}

impl EpochGate {
    pub fn epoch(&self) -> StreamEpoch {
        self.epoch
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

/// One ownership cell per output epoch, never reused for another engine.
///
/// The worker retains its own lease Arc until `try_take` succeeds, even if
/// building/playing the stream fails. Thus the last callback Arc only drops
/// an empty cell, not DSP resources. Stream pause/drop is not a transfer ack.
pub struct EngineLease {
    owner: AtomicU8,
    engine: UnsafeCell<Option<PreparedEngine>>,
    gate: EpochGate,
}

// SAFETY: PreparedEngine is Send (its voices are Box<dyn Voice: Send>).
// The only accesses to `engine` are a CallbackGuard's exclusive mutable
// borrow or `try_take` after the mutually exclusive Idle→Worker CAS.
// A successful Idle→Callback CAS admits exactly one guard. Its private
// fields cannot be constructed/cloned by callers, and `engine(&mut self)`
// ties every returned reference to the guard's mutable borrow. Drop cannot
// run before those safe Rust borrows end and publishes Idle only afterward.
// Forgetting a guard strands ownership but cannot create aliasing.
// Worker ownership is permanent, so no empty cell is ever reborrowed.
// SeqCst ownership transitions publish prior engine writes before access
// by the next owner; the permanent gate never establishes a second owner.
unsafe impl Sync for EngineLease {}

impl EngineLease {
    pub fn new(engine: PreparedEngine, epoch: StreamEpoch) -> Self {
        // Install the engine under worker ownership before making the cell
        // idle, then enable only this newly constructed epoch's gate.
        let lease = Self {
            owner: AtomicU8::new(OWNER_WORKER),
            engine: UnsafeCell::new(Some(engine)),
            gate: EpochGate {
                epoch,
                enabled: AtomicBool::new(false),
            },
        };
        lease.owner.store(OWNER_IDLE, Ordering::SeqCst);
        lease.gate.enabled.store(true, Ordering::SeqCst);
        lease
    }

    pub fn gate(&self) -> &EpochGate {
        &self.gate
    }

    /// A bounded callback operation: two gate reads and one CAS, no retry.
    pub fn try_borrow(&self) -> Option<CallbackGuard<'_>> {
        if !self.gate.is_enabled() {
            return None;
        }
        self.owner
            .compare_exchange(
                OWNER_IDLE,
                OWNER_CALLBACK,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .ok()?;
        let guard = CallbackGuard {
            lease: self,
            callback_thread: PhantomData,
        };
        if !self.gate.is_enabled() {
            // Guard Drop only publishes the scalar Idle owner.
            return None;
        }
        Some(guard)
    }

    pub fn close(&self) {
        self.gate.enabled.store(false, Ordering::SeqCst);
    }

    /// Worker-only, non-RT transfer. A running callback means immediate None;
    /// any waiting/retry policy belongs to the worker, outside this method.
    pub fn try_take(&self) -> Option<PreparedEngine> {
        self.close();
        self.owner
            .compare_exchange(OWNER_IDLE, OWNER_WORKER, Ordering::SeqCst, Ordering::SeqCst)
            .ok()?;
        // SAFETY: the successful CAS establishes unique Worker ownership.
        // No CallbackGuard is alive, and the permanently closed gate/Worker
        // owner prevent future cell access. Nothing returns owner to Idle.
        unsafe { (&mut *self.engine.get()).take() }
    }
}

/// A callback-scoped exclusive borrow; neither clonable nor transferable.
pub struct CallbackGuard<'a> {
    lease: &'a EngineLease,
    callback_thread: PhantomData<*mut ()>,
}

impl CallbackGuard<'_> {
    pub fn engine(&mut self) -> &mut PreparedEngine {
        // SAFETY: only try_borrow constructs this guard after exclusive CAS.
        // Worker cannot take while owner is Callback. Its &mut self lifetime
        // bounds this reference, including across a later permanent close.
        unsafe { (&mut *self.lease.engine.get()).as_mut() }
            .expect("callback ownership always contains the prepared engine")
    }
}

impl Drop for CallbackGuard<'_> {
    fn drop(&mut self) {
        // All engine borrows end before safe Rust can drop their guard.
        // No allocation, DSP destruction, wake, wait, or retry occurs here.
        self.lease.owner.store(OWNER_IDLE, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier, mpsc};
    use std::thread;

    use super::*;
    use crate::audio::timing::MachNanos;
    use crate::audio::transport::{ErrorCell, VisualRing};
    use crate::controls::ControlState;
    use crate::situations::Registry;

    fn epoch(value: u64) -> StreamEpoch {
        StreamEpoch::new(value).unwrap()
    }

    fn engine() -> PreparedEngine {
        let registry = Arc::new(Registry::new().unwrap());
        let controls = ControlState::new(&registry);
        let visuals = Arc::new(VisualRing::new(2048, registry.definitions().len()));
        PreparedEngine::prepare(registry, controls, visuals, 48_000, 42).unwrap()
    }

    fn render(guard: &mut CallbackGuard<'_>, frames: &mut [f32], epoch: StreamEpoch) {
        guard.engine().render_f32(
            frames,
            2,
            MachNanos::new(1_000_000_000),
            &ErrorCell::new(epoch),
        );
    }

    #[test]
    fn closed_epoch_rejects_callbacks_and_preserves_world_in_new_cell() {
        let old = EngineLease::new(engine(), epoch(1));
        assert!(old.gate().is_enabled());
        assert_eq!(old.gate().epoch(), epoch(1));
        let preserved = {
            let mut callback = old.try_borrow().unwrap();
            render(&mut callback, &mut [0.0; 254], epoch(1));
            callback.engine().world()
        };
        old.close();
        assert!(old.try_borrow().is_none());
        let recovered = old.try_take().unwrap();
        assert_eq!(recovered.world(), preserved);
        assert!(old.try_take().is_none());
        assert_eq!(old.owner.load(Ordering::SeqCst), OWNER_WORKER);
        let new = EngineLease::new(recovered, epoch(2));
        let mut callback = new.try_borrow().unwrap();
        assert_eq!(callback.engine().world(), preserved);
        render(&mut callback, &mut [0.0; 1026], epoch(2));
        assert!(callback.engine().world().get() > preserved.get());
        drop(callback);
        drop(new.try_take().unwrap());
    }

    #[test]
    fn delayed_old_callback_cannot_contend_with_new_epoch_callback() {
        let old = EngineLease::new(engine(), epoch(1));
        let delayed_ready = Barrier::new(2);
        let new_ready = Barrier::new(2);
        thread::scope(|scope| {
            let delayed = scope.spawn(|| {
                assert!(old.gate().is_enabled());
                delayed_ready.wait();
                new_ready.wait();
                assert!(old.try_borrow().is_none());
            });
            delayed_ready.wait();
            let new = EngineLease::new(old.try_take().unwrap(), epoch(2));
            let mut callback = new.try_borrow().unwrap();
            let before = callback.engine().world();
            new_ready.wait();
            delayed.join().unwrap();
            assert_eq!(callback.engine().world(), before);
            render(&mut callback, &mut [0.0; 2], epoch(2));
            assert!(callback.engine().world().get() > before.get());
            drop(callback);
            drop(new.try_take().unwrap());
        });
    }

    #[test]
    fn two_callbacks_are_exclusive_and_worker_cannot_take_live_borrow() {
        let lease = EngineLease::new(engine(), epoch(1));
        let acquired = Barrier::new(2);
        let release = Barrier::new(2);
        thread::scope(|scope| {
            let first = scope.spawn(|| {
                let mut callback = lease.try_borrow().unwrap();
                let before = callback.engine().world();
                acquired.wait();
                release.wait();
                assert_eq!(callback.engine().world(), before);
                render(&mut callback, &mut [0.0; 2], epoch(1));
            });
            acquired.wait();
            assert!(lease.try_borrow().is_none());
            assert!(lease.try_take().is_none());
            assert!(!lease.gate().is_enabled());
            release.wait();
            first.join().unwrap();
        });
        assert!(lease.try_borrow().is_none());
        assert!(lease.try_take().unwrap().world().get() > 0.0);
    }

    #[test]
    fn worker_owned_callback_returns_before_worker_release_without_world_change() {
        let lease = EngineLease::new(engine(), epoch(1));
        let owned = Barrier::new(2);
        let release = Barrier::new(2);
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let recovered = lease.try_take().unwrap();
                let before = recovered.world();
                owned.wait();
                release.wait();
                assert_eq!(recovered.world(), before);
            });
            owned.wait();
            // A waiting callback would deadlock: worker release follows this return.
            assert!(lease.try_borrow().is_none());
            assert_eq!(lease.owner.load(Ordering::SeqCst), OWNER_WORKER);
            release.wait();
            worker.join().unwrap();
        });
    }

    #[test]
    fn guard_drop_is_scalar_and_engine_drops_only_on_recovering_owner_thread() {
        let observation = Arc::new(AtomicU8::new(0));
        let release = Barrier::new(2);
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let mut prepared = engine();
                prepared.observe_drop(thread::current().id(), Arc::clone(&observation));
                let lease = Arc::new(EngineLease::new(prepared, epoch(1)));
                sender.send(Arc::clone(&lease)).unwrap();
                release.wait();
                let recovered = lease.try_take().unwrap();
                drop(recovered);
            });
            let lease = receiver.recv().unwrap();
            {
                let mut callback = lease.try_borrow().unwrap();
                render(&mut callback, &mut [0.0; 2], epoch(1));
            }
            assert_eq!(observation.load(Ordering::SeqCst), 0);
            release.wait();
            worker.join().unwrap();
            assert_eq!(observation.load(Ordering::SeqCst), 1);
            // The retained callback's last Arc owns only the empty old cell.
            thread::spawn(move || drop(lease)).join().unwrap();
            assert_eq!(observation.load(Ordering::SeqCst), 1);
        });
    }
}
