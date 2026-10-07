use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use crate::controls::{ControlState, Normalized, Revision};
use crate::situations::{SituationIndex, VisualShape};

use super::timing::{MachNanos, StreamEpoch, WorldSeconds};

pub(crate) const EVENT_CAPACITY: usize = 2048;
pub(crate) const STATE_CAPACITY: usize = 128;
pub(crate) const VISIBLE_CAPACITY: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogicalPixels(f32);

impl LogicalPixels {
    pub(crate) fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ParticleVisual {
    pub(crate) x: Normalized,
    pub(crate) y: Normalized,
    pub(crate) strength: Normalized,
    pub(crate) radius: LogicalPixels,
    pub(crate) shape: VisualShape,
    pub(crate) lifetime: Duration,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VisualEvent {
    pub(crate) epoch: StreamEpoch,
    pub(crate) world: WorldSeconds,
    pub(crate) present: MachNanos,
    pub(crate) situation: SituationIndex,
    pub(crate) x: Normalized,
    pub(crate) y: Normalized,
    pub(crate) strength: Normalized,
    pub(crate) radius: LogicalPixels,
    pub(crate) shape: VisualShape,
    pub(crate) lifetime: Duration,
    pub(crate) serial: u64,
}

pub(crate) struct WorldVisualState<'a> {
    pub(crate) epoch: StreamEpoch,
    pub(crate) world: WorldSeconds,
    pub(crate) present: MachNanos,
    pub(crate) phase: u32,
    pub(crate) gains: &'a [f32],
    pub(crate) breath_intensity: &'a [f32],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorldVisualHeader {
    pub(crate) epoch: StreamEpoch,
    pub(crate) world: WorldSeconds,
    pub(crate) present: MachNanos,
    pub(crate) phase: u32,
}

pub(crate) struct VisualWriter<'a> {
    ring: &'a VisualRing,
    epoch: StreamEpoch,
    situation: SituationIndex,
    world: WorldSeconds,
    present: MachNanos,
    contributing: bool,
    failed: bool,
}

impl<'a> VisualWriter<'a> {
    pub(crate) fn new(
        ring: &'a VisualRing,
        epoch: StreamEpoch,
        situation: SituationIndex,
        world: WorldSeconds,
        present: MachNanos,
    ) -> Self {
        Self {
            ring,
            epoch,
            situation,
            world,
            present,
            contributing: true,
            failed: false,
        }
    }

    pub(crate) fn set_contributing(&mut self, active: bool) {
        self.contributing = active;
    }

    pub(crate) const fn has_failed(&self) -> bool {
        self.failed
    }

    pub(crate) fn emit(&mut self, particle: ParticleVisual) -> bool {
        if !self.contributing {
            return false;
        }
        let published = self.ring.push_event(VisualEvent {
            epoch: self.epoch,
            world: self.world,
            present: self.present,
            situation: self.situation,
            x: particle.x,
            y: particle.y,
            strength: particle.strength,
            radius: particle.radius,
            shape: particle.shape,
            lifetime: particle.lifetime,
            serial: 0,
        });
        self.failed |= !published;
        published
    }
}

// EngineLease supplies exactly one producer. A producer performs only bounded
// atomic stores, never waits for a reader, and overwrites old visual slots.
// Every payload word is atomic: an unsuccessful snapshot is a visual omission,
// not a data race. SeqCst puts sequence and payload accesses in one total order;
// identical even sequence reads therefore exclude an intervening publication.
// Sequence and serial exhaustion fail instead of permitting an ABA rollover.
// No unsafe code, callback allocation, destruction, notification or retry loop
// is used here. All boxes and registry-sized state arrays are prepared off RT.
pub(crate) struct VisualRing {
    events: Box<[EventSlot]>,
    states: Box<[StateSlot]>,
    registry_count: usize,
    event_serial: AtomicU64,
    state_serial: AtomicU64,
}

impl VisualRing {
    pub(crate) fn new(event_capacity: usize, registry_count: usize) -> Self {
        assert!(event_capacity > 0);
        assert!(registry_count > 0);
        assert!(u64::try_from(event_capacity).is_ok());
        assert!(u64::try_from(registry_count).is_ok());
        Self {
            events: (0..event_capacity).map(|_| EventSlot::new()).collect(),
            states: (0..STATE_CAPACITY)
                .map(|_| StateSlot::new(registry_count))
                .collect(),
            registry_count,
            event_serial: AtomicU64::new(0),
            state_serial: AtomicU64::new(0),
        }
    }

    // The ring, not the caller, assigns the serial. Full is never a failure:
    // false means invalid timing/packet data or exhausted checked counters.
    pub(crate) fn push_event(&self, event: VisualEvent) -> bool {
        let Ok(lifetime) = u64::try_from(event.lifetime.as_nanos()) else {
            return false;
        };
        if event.situation.get() >= self.registry_count
            || event.present.get().checked_add(lifetime).is_none()
        {
            return false;
        }
        let Some(serial) = self.event_serial.load(Ordering::SeqCst).checked_add(1) else {
            return false;
        };
        let index = ((serial - 1) % self.events.len() as u64) as usize;
        if !self.events[index].publish(event, lifetime, serial) {
            return false;
        }
        self.event_serial.store(serial, Ordering::SeqCst);
        true
    }

    pub(crate) fn push_state(&self, state: WorldVisualState<'_>) -> bool {
        if state.gains.len() != self.registry_count
            || state.breath_intensity.len() != self.registry_count
            || !state.gains.iter().copied().all(valid_unit)
            || !state.breath_intensity.iter().copied().all(valid_unit)
        {
            return false;
        }
        let Some(serial) = self.state_serial.load(Ordering::SeqCst).checked_add(1) else {
            return false;
        };
        let index = ((serial - 1) % self.states.len() as u64) as usize;
        if !self.states[index].publish(state, serial) {
            return false;
        }
        self.state_serial.store(serial, Ordering::SeqCst);
        true
    }

    pub(crate) fn read_live_into(
        &self,
        epoch: StreamEpoch,
        now: MachNanos,
        dst: &mut [Option<VisualEvent>],
    ) -> usize {
        assert!(dst.len() <= VISIBLE_CAPACITY);
        let published = self.event_serial.load(Ordering::SeqCst);
        let mut count = 0;
        // A previously accepted particle can finish its lifetime after its
        // transport slot was overwritten. Compact and expire this fixed cache
        // first; no queue drain or hidden-window cursor catch-up is required.
        for index in 0..dst.len() {
            if let Some(event) = dst[index]
                && event.serial > 0
                && event.serial <= published
                && event.situation.get() < self.registry_count
                && live(event, epoch, now)
                && !contains_serial(&dst[..count], event.serial)
            {
                dst[count] = Some(event);
                count += 1;
            }
        }
        dst[count..].fill(None);
        if dst.is_empty() || published == 0 {
            return count;
        }
        let oldest = published
            .saturating_sub(self.events.len() as u64 - 1)
            .max(1);
        for (index, slot) in self.events.iter().enumerate() {
            let Some(event) = slot.read(self.registry_count) else {
                continue;
            };
            if event.serial < oldest
                || event.serial > published
                || (event.serial - 1) % self.events.len() as u64 != index as u64
                || !live(event, epoch, now)
                || contains_serial(&dst[..count], event.serial)
            {
                continue;
            }
            if count < dst.len() {
                dst[count] = Some(event);
                count += 1;
            } else {
                let mut weakest = 0;
                for candidate in 1..count {
                    if stronger(dst[weakest].unwrap(), dst[candidate].unwrap()) {
                        weakest = candidate;
                    }
                }
                if stronger(event, dst[weakest].unwrap()) {
                    dst[weakest] = Some(event);
                }
            }
        }
        count
    }

    // Arrays are caller-preallocated scratch. Adopt them only on Some: a slot
    // overwritten during this single full read may leave scratch partly changed.
    pub(crate) fn read_state_into(
        &self,
        epoch: StreamEpoch,
        now: MachNanos,
        gains: &mut [f32],
        breath: &mut [f32],
    ) -> Option<WorldVisualHeader> {
        assert_eq!(gains.len(), self.registry_count);
        assert_eq!(breath.len(), self.registry_count);
        let published = self.state_serial.load(Ordering::SeqCst);
        if published == 0 {
            return None;
        }
        let oldest = published
            .saturating_sub(self.states.len() as u64 - 1)
            .max(1);
        let mut latest: Option<(usize, StateSnapshot)> = None;
        for (index, slot) in self.states.iter().enumerate() {
            let Some(snapshot) = slot.header() else {
                continue;
            };
            if snapshot.serial < oldest
                || snapshot.serial > published
                || (snapshot.serial - 1) % self.states.len() as u64 != index as u64
                || snapshot.header.epoch != epoch
                || snapshot.header.present > now
            {
                continue;
            }
            if latest.as_ref().is_none_or(|(_, previous)| {
                snapshot.header.present > previous.header.present
                    || (snapshot.header.present == previous.header.present
                        && snapshot.serial > previous.serial)
            }) {
                latest = Some((index, snapshot));
            }
        }
        let (index, snapshot) = latest?;
        self.states[index].read_arrays(snapshot, gains, breath)
    }
}

fn valid_unit(value: f32) -> bool {
    (0.0..=1.0).contains(&value)
}

fn live(event: VisualEvent, epoch: StreamEpoch, now: MachNanos) -> bool {
    let Ok(lifetime) = u64::try_from(event.lifetime.as_nanos()) else {
        return false;
    };
    event.epoch == epoch
        && event.present <= now
        && event
            .present
            .get()
            .checked_add(lifetime)
            .is_some_and(|end| now.get() < end)
}

fn contains_serial(events: &[Option<VisualEvent>], serial: u64) -> bool {
    events
        .iter()
        .any(|event| event.is_some_and(|event| event.serial == serial))
}

fn stronger(left: VisualEvent, right: VisualEvent) -> bool {
    left.strength.get() > right.strength.get()
        || (left.strength == right.strength && left.serial > right.serial)
}

fn begin_publication(sequence: &AtomicU64) -> Option<u64> {
    let previous = sequence.load(Ordering::SeqCst);
    if previous & 1 != 0 {
        return None;
    }
    let next = previous.checked_add(2)?;
    sequence.store(next - 1, Ordering::SeqCst);
    Some(next)
}

struct EventSlot {
    sequence: AtomicU64,
    serial: AtomicU64,
    epoch: AtomicU64,
    world: AtomicU64,
    present: AtomicU64,
    situation: AtomicU64,
    x: AtomicU32,
    y: AtomicU32,
    strength: AtomicU32,
    radius: AtomicU32,
    shape: AtomicU32,
    lifetime: AtomicU64,
}

impl EventSlot {
    fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            serial: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            world: AtomicU64::new(0),
            present: AtomicU64::new(0),
            situation: AtomicU64::new(0),
            x: AtomicU32::new(0),
            y: AtomicU32::new(0),
            strength: AtomicU32::new(0),
            radius: AtomicU32::new(0),
            shape: AtomicU32::new(0),
            lifetime: AtomicU64::new(0),
        }
    }

    fn publish(&self, event: VisualEvent, lifetime: u64, serial: u64) -> bool {
        let Some(sequence) = begin_publication(&self.sequence) else {
            return false;
        };
        self.serial.store(serial, Ordering::SeqCst);
        self.epoch.store(event.epoch.get(), Ordering::SeqCst);
        self.world
            .store(event.world.get().to_bits(), Ordering::SeqCst);
        self.present.store(event.present.get(), Ordering::SeqCst);
        self.situation
            .store(event.situation.get() as u64, Ordering::SeqCst);
        self.x.store(event.x.get().to_bits(), Ordering::SeqCst);
        self.y.store(event.y.get().to_bits(), Ordering::SeqCst);
        self.strength
            .store(event.strength.get().to_bits(), Ordering::SeqCst);
        self.radius
            .store(event.radius.get().to_bits(), Ordering::SeqCst);
        self.shape.store(event.shape as u32, Ordering::SeqCst);
        self.lifetime.store(lifetime, Ordering::SeqCst);
        self.sequence.store(sequence, Ordering::SeqCst);
        true
    }

    fn read(&self, registry_count: usize) -> Option<VisualEvent> {
        let before = self.sequence.load(Ordering::SeqCst);
        if before == 0 || before & 1 != 0 {
            return None;
        }
        let serial = self.serial.load(Ordering::SeqCst);
        if serial == 0 {
            return None;
        }
        let event = VisualEvent {
            serial,
            epoch: StreamEpoch::new(self.epoch.load(Ordering::SeqCst))?,
            world: WorldSeconds::new(f64::from_bits(self.world.load(Ordering::SeqCst)))?,
            present: MachNanos::new(self.present.load(Ordering::SeqCst)),
            situation: SituationIndex::from_index(
                usize::try_from(self.situation.load(Ordering::SeqCst)).ok()?,
                registry_count,
            )?,
            x: Normalized::new(f32::from_bits(self.x.load(Ordering::SeqCst)))?,
            y: Normalized::new(f32::from_bits(self.y.load(Ordering::SeqCst)))?,
            strength: Normalized::new(f32::from_bits(self.strength.load(Ordering::SeqCst)))?,
            radius: LogicalPixels::new(f32::from_bits(self.radius.load(Ordering::SeqCst)))?,
            shape: match self.shape.load(Ordering::SeqCst) {
                0 => VisualShape::Point,
                1 => VisualShape::Impact,
                2 => VisualShape::SlowGlow,
                _ => return None,
            },
            lifetime: Duration::from_nanos(self.lifetime.load(Ordering::SeqCst)),
        };
        (self.sequence.load(Ordering::SeqCst) == before).then_some(event)
    }
}

#[derive(Clone, Copy)]
struct StateSnapshot {
    sequence: u64,
    serial: u64,
    header: WorldVisualHeader,
}

struct StateSlot {
    sequence: AtomicU64,
    serial: AtomicU64,
    epoch: AtomicU64,
    world: AtomicU64,
    present: AtomicU64,
    phase: AtomicU32,
    gains: Box<[AtomicU32]>,
    breath: Box<[AtomicU32]>,
}

impl StateSlot {
    fn new(registry_count: usize) -> Self {
        Self {
            sequence: AtomicU64::new(0),
            serial: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            world: AtomicU64::new(0),
            present: AtomicU64::new(0),
            phase: AtomicU32::new(0),
            gains: (0..registry_count).map(|_| AtomicU32::new(0)).collect(),
            breath: (0..registry_count).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    fn publish(&self, state: WorldVisualState<'_>, serial: u64) -> bool {
        let Some(sequence) = begin_publication(&self.sequence) else {
            return false;
        };
        self.serial.store(serial, Ordering::SeqCst);
        self.epoch.store(state.epoch.get(), Ordering::SeqCst);
        self.world
            .store(state.world.get().to_bits(), Ordering::SeqCst);
        self.present.store(state.present.get(), Ordering::SeqCst);
        self.phase.store(state.phase, Ordering::SeqCst);
        for (dst, value) in self.gains.iter().zip(state.gains) {
            dst.store(value.to_bits(), Ordering::SeqCst);
        }
        for (dst, value) in self.breath.iter().zip(state.breath_intensity) {
            dst.store(value.to_bits(), Ordering::SeqCst);
        }
        self.sequence.store(sequence, Ordering::SeqCst);
        true
    }

    fn header(&self) -> Option<StateSnapshot> {
        let before = self.sequence.load(Ordering::SeqCst);
        if before == 0 || before & 1 != 0 {
            return None;
        }
        let serial = self.serial.load(Ordering::SeqCst);
        if serial == 0 {
            return None;
        }
        let snapshot = StateSnapshot {
            sequence: before,
            serial,
            header: WorldVisualHeader {
                epoch: StreamEpoch::new(self.epoch.load(Ordering::SeqCst))?,
                world: WorldSeconds::new(f64::from_bits(self.world.load(Ordering::SeqCst)))?,
                present: MachNanos::new(self.present.load(Ordering::SeqCst)),
                phase: self.phase.load(Ordering::SeqCst),
            },
        };
        (self.sequence.load(Ordering::SeqCst) == before).then_some(snapshot)
    }

    fn read_arrays(
        &self,
        snapshot: StateSnapshot,
        gains: &mut [f32],
        breath: &mut [f32],
    ) -> Option<WorldVisualHeader> {
        if self.sequence.load(Ordering::SeqCst) != snapshot.sequence {
            return None;
        }
        for (dst, value) in gains.iter_mut().zip(self.gains.iter()) {
            *dst = f32::from_bits(value.load(Ordering::SeqCst));
            if !valid_unit(*dst) {
                return None;
            }
        }
        for (dst, value) in breath.iter_mut().zip(self.breath.iter()) {
            *dst = f32::from_bits(value.load(Ordering::SeqCst));
            if !valid_unit(*dst) {
                return None;
            }
        }
        (self.sequence.load(Ordering::SeqCst) == snapshot.sequence).then_some(snapshot.header)
    }
}

// Main is the sole publisher; the callback does one snapshot attempt at the
// buffer boundary. Failed reads may change scratch, never committed controls.
pub(crate) struct ControlMailbox {
    sequence: AtomicU64,
    registry_count: usize,
    volume: AtomicU32,
    mute: AtomicU32,
    wander: AtomicU32,
    dwell_min_minutes: AtomicU32,
    dwell_max_minutes: AtomicU32,
    transition_seconds: AtomicU32,
    knobs: Box<[AtomicU32]>,
    manual_target: AtomicU64,
    manual_revision: AtomicU64,
    wander_revision: AtomicU64,
    settings_revision: AtomicU64,
}

impl ControlMailbox {
    pub(crate) fn new(initial: &ControlState) -> Self {
        assert!(initial.registry_count() > 0);
        assert!(initial.manual_target.get() < initial.registry_count());
        assert!(initial.dwell_min_minutes <= initial.dwell_max_minutes);
        Self {
            sequence: AtomicU64::new(0),
            registry_count: initial.registry_count(),
            volume: AtomicU32::new(initial.volume.get().to_bits()),
            mute: AtomicU32::new(u32::from(initial.mute)),
            wander: AtomicU32::new(u32::from(initial.wander)),
            dwell_min_minutes: AtomicU32::new(initial.dwell_min_minutes),
            dwell_max_minutes: AtomicU32::new(initial.dwell_max_minutes),
            transition_seconds: AtomicU32::new(initial.transition_seconds),
            knobs: initial
                .knobs
                .iter()
                .map(|value| AtomicU32::new(value.get().to_bits()))
                .collect(),
            manual_target: AtomicU64::new(initial.manual_target.get() as u64),
            manual_revision: AtomicU64::new(initial.manual_revision.get()),
            wander_revision: AtomicU64::new(initial.wander_revision.get()),
            settings_revision: AtomicU64::new(initial.settings_revision.get()),
        }
    }

    pub(crate) fn publish(&self, state: &ControlState) {
        assert_eq!(state.registry_count(), self.registry_count);
        assert_eq!(state.knobs.len(), self.knobs.len());
        assert!(state.manual_target.get() < self.registry_count);
        assert!(state.dwell_min_minutes <= state.dwell_max_minutes);
        let sequence = begin_publication(&self.sequence)
            .expect("control mailbox requires one publisher and nonexhausted sequence");
        self.volume
            .store(state.volume.get().to_bits(), Ordering::SeqCst);
        self.mute.store(u32::from(state.mute), Ordering::SeqCst);
        self.wander.store(u32::from(state.wander), Ordering::SeqCst);
        self.dwell_min_minutes
            .store(state.dwell_min_minutes, Ordering::SeqCst);
        self.dwell_max_minutes
            .store(state.dwell_max_minutes, Ordering::SeqCst);
        self.transition_seconds
            .store(state.transition_seconds, Ordering::SeqCst);
        for (dst, value) in self.knobs.iter().zip(state.knobs.iter()) {
            dst.store(value.get().to_bits(), Ordering::SeqCst);
        }
        self.manual_target
            .store(state.manual_target.get() as u64, Ordering::SeqCst);
        self.manual_revision
            .store(state.manual_revision.get(), Ordering::SeqCst);
        self.wander_revision
            .store(state.wander_revision.get(), Ordering::SeqCst);
        self.settings_revision
            .store(state.settings_revision.get(), Ordering::SeqCst);
        self.sequence.store(sequence, Ordering::SeqCst);
    }

    pub(crate) fn try_read_into(&self, dst: &mut ControlState) -> bool {
        assert_eq!(dst.registry_count(), self.registry_count);
        assert_eq!(dst.knobs.len(), self.knobs.len());
        let before = self.sequence.load(Ordering::SeqCst);
        if before & 1 != 0 {
            return false;
        }
        let Some(volume) = Normalized::new(f32::from_bits(self.volume.load(Ordering::SeqCst)))
        else {
            return false;
        };
        dst.volume = volume;
        let Some(mute) = read_bool(&self.mute) else {
            return false;
        };
        dst.mute = mute;
        let Some(wander) = read_bool(&self.wander) else {
            return false;
        };
        dst.wander = wander;
        dst.dwell_min_minutes = self.dwell_min_minutes.load(Ordering::SeqCst);
        dst.dwell_max_minutes = self.dwell_max_minutes.load(Ordering::SeqCst);
        dst.transition_seconds = self.transition_seconds.load(Ordering::SeqCst);
        if dst.dwell_min_minutes > dst.dwell_max_minutes {
            return false;
        }
        for (dst, value) in dst.knobs.iter_mut().zip(self.knobs.iter()) {
            let Some(value) = Normalized::new(f32::from_bits(value.load(Ordering::SeqCst))) else {
                return false;
            };
            *dst = value;
        }
        let Ok(target) = usize::try_from(self.manual_target.load(Ordering::SeqCst)) else {
            return false;
        };
        let Some(target) = SituationIndex::from_index(target, self.registry_count) else {
            return false;
        };
        dst.manual_target = target;
        dst.manual_revision = Revision::from_raw(self.manual_revision.load(Ordering::SeqCst));
        dst.wander_revision = Revision::from_raw(self.wander_revision.load(Ordering::SeqCst));
        dst.settings_revision = Revision::from_raw(self.settings_revision.load(Ordering::SeqCst));
        self.sequence.load(Ordering::SeqCst) == before
    }
}

fn read_bool(value: &AtomicU32) -> Option<bool> {
    match value.load(Ordering::SeqCst) {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub(crate) struct ErrorCode;

impl ErrorCode {
    pub(crate) const DEVICE_BUSY: u64 = 1 << 0;
    pub(crate) const DEVICE_CHANGED: u64 = 1 << 1;
    pub(crate) const DEVICE_NOT_AVAILABLE: u64 = 1 << 2;
    pub(crate) const HOST_UNAVAILABLE: u64 = 1 << 3;
    pub(crate) const INVALID_INPUT: u64 = 1 << 4;
    pub(crate) const PERMISSION_DENIED: u64 = 1 << 5;
    pub(crate) const REALTIME_DENIED: u64 = 1 << 6;
    pub(crate) const RESOURCE_EXHAUSTED: u64 = 1 << 7;
    pub(crate) const STREAM_INVALIDATED: u64 = 1 << 8;
    pub(crate) const UNSUPPORTED_CONFIG: u64 = 1 << 9;
    pub(crate) const UNSUPPORTED_OPERATION: u64 = 1 << 10;
    pub(crate) const XRUN: u64 = 1 << 11;
    pub(crate) const BACKEND_ERROR: u64 = 1 << 12;
    pub(crate) const OTHER: u64 = 1 << 13;
    pub(crate) const TIMING_FAULT: u64 = 1 << 14;
    pub(crate) const INVALID_SAMPLE: u64 = 1 << 15;

    // CPAL 0.18.2 classifies these as an active-stream notification rather than
    // device absence. The worker still invalidates epoch timing on route change.
    pub(crate) const NON_FATAL: u64 = Self::DEVICE_CHANGED | Self::REALTIME_DENIED | Self::XRUN;
    pub(crate) const FATAL: u64 = ((1 << 16) - 1) & !Self::NON_FATAL;
}

// A cell's epoch never changes. Old CPAL closures retain only their old cell,
// so late monitor errors cannot affect a newly adopted stream's error bitset.
pub(crate) struct ErrorCell {
    epoch: StreamEpoch,
    bits: AtomicU64,
}

impl ErrorCell {
    pub(crate) fn new(epoch: StreamEpoch) -> Self {
        Self {
            epoch,
            bits: AtomicU64::new(0),
        }
    }

    pub(crate) const fn epoch(&self) -> StreamEpoch {
        self.epoch
    }

    pub(crate) fn report(&self, kind: cpal::ErrorKind) {
        let bit = match kind {
            cpal::ErrorKind::DeviceBusy => ErrorCode::DEVICE_BUSY,
            cpal::ErrorKind::DeviceChanged => ErrorCode::DEVICE_CHANGED,
            cpal::ErrorKind::DeviceNotAvailable => ErrorCode::DEVICE_NOT_AVAILABLE,
            cpal::ErrorKind::HostUnavailable => ErrorCode::HOST_UNAVAILABLE,
            cpal::ErrorKind::InvalidInput => ErrorCode::INVALID_INPUT,
            cpal::ErrorKind::PermissionDenied => ErrorCode::PERMISSION_DENIED,
            cpal::ErrorKind::RealtimeDenied => ErrorCode::REALTIME_DENIED,
            cpal::ErrorKind::ResourceExhausted => ErrorCode::RESOURCE_EXHAUSTED,
            cpal::ErrorKind::StreamInvalidated => ErrorCode::STREAM_INVALIDATED,
            cpal::ErrorKind::UnsupportedConfig => ErrorCode::UNSUPPORTED_CONFIG,
            cpal::ErrorKind::UnsupportedOperation => ErrorCode::UNSUPPORTED_OPERATION,
            cpal::ErrorKind::Xrun => ErrorCode::XRUN,
            cpal::ErrorKind::BackendError => ErrorCode::BACKEND_ERROR,
            cpal::ErrorKind::Other => ErrorCode::OTHER,
            _ => ErrorCode::OTHER,
        };
        self.bits.fetch_or(bit, Ordering::SeqCst);
    }

    pub(crate) fn report_timing_fault(&self) {
        self.bits
            .fetch_or(ErrorCode::TIMING_FAULT, Ordering::SeqCst);
    }

    pub(crate) fn report_invalid_sample(&self) {
        self.bits
            .fetch_or(ErrorCode::INVALID_SAMPLE, Ordering::SeqCst);
    }

    pub(crate) fn take_bits(&self) -> u64 {
        self.bits.swap(0, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use crate::situations::Registry;

    fn epoch(value: u64) -> StreamEpoch {
        StreamEpoch::new(value).unwrap()
    }

    fn event(world: u64, present: u64, strength: f32) -> VisualEvent {
        VisualEvent {
            epoch: epoch(1),
            world: WorldSeconds::new(world as f64).unwrap(),
            present: MachNanos::new(present),
            situation: SituationIndex::from_index(0, 1).unwrap(),
            x: Normalized::new(if world.is_multiple_of(2) { 0.25 } else { 0.75 }).unwrap(),
            y: Normalized::new(0.5).unwrap(),
            strength: Normalized::new(strength).unwrap(),
            radius: LogicalPixels::new(2.0).unwrap(),
            shape: VisualShape::Point,
            lifetime: Duration::from_nanos(100),
            serial: 0,
        }
    }

    #[test]
    fn inactive_voice_world_updates_do_not_publish_particles_or_consume_serials() {
        let ring = VisualRing::new(1, 1);
        let mut writer = VisualWriter::new(
            &ring,
            epoch(1),
            SituationIndex::from_index(0, 1).unwrap(),
            WorldSeconds::new(3.0).unwrap(),
            MachNanos::new(10),
        );
        let particle = ParticleVisual {
            x: Normalized::new(0.75).unwrap(),
            y: Normalized::new(0.5).unwrap(),
            strength: Normalized::ONE,
            radius: LogicalPixels::new(2.0).unwrap(),
            shape: VisualShape::Point,
            lifetime: Duration::from_nanos(100),
        };
        writer.set_contributing(false);
        assert!(!writer.emit(particle));
        assert!(!writer.has_failed());
        assert_eq!(ring.event_serial.load(Ordering::SeqCst), 0);
        writer.set_contributing(true);
        assert!(writer.emit(particle));
        assert!(!writer.has_failed());
        let mut cache = [None; 1];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(10), &mut cache),
            1
        );
        let accepted = cache[0].unwrap();
        assert_eq!(accepted.serial, 1);
        assert_eq!(accepted.world.get(), 3.0);
        assert_eq!(accepted.present.get(), 10);
        assert_eq!(accepted.x, particle.x);
    }

    #[test]
    fn active_publication_failure_is_latched_for_the_audio_timing_fault_boundary() {
        let ring = VisualRing::new(1, 1);
        let mut writer = VisualWriter::new(
            &ring,
            epoch(1),
            SituationIndex::from_index(0, 1).unwrap(),
            WorldSeconds::new(0.0).unwrap(),
            MachNanos::new(0),
        );
        let mut particle = ParticleVisual {
            x: Normalized::ZERO,
            y: Normalized::ZERO,
            strength: Normalized::ONE,
            radius: LogicalPixels::new(2.0).unwrap(),
            shape: VisualShape::Point,
            lifetime: Duration::from_secs(u64::MAX),
        };
        assert!(!writer.emit(particle));
        assert!(writer.has_failed());
        assert_eq!(ring.event_serial.load(Ordering::SeqCst), 0);
        particle.lifetime = Duration::from_nanos(100);
        assert!(writer.emit(particle));
        assert!(writer.has_failed());
        assert_eq!(ring.event_serial.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lifetime_future_and_epoch_boundaries_do_not_replay_hidden_particles() {
        let ring = VisualRing::new(8, 1);
        assert!(ring.push_event(event(1, 10, 0.5)));
        assert!(ring.push_event(event(2, 20, 0.5)));
        let mut old = event(3, 10, 0.5);
        old.epoch = epoch(2);
        assert!(ring.push_event(old));
        let mut cache = [None; 8];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(9), &mut cache),
            0
        );
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(10), &mut cache),
            1
        );
        assert_eq!(cache[0].unwrap().world.get(), 1.0);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(20), &mut cache),
            2
        );
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(110), &mut cache),
            1
        );
        assert_eq!(cache[0].unwrap().world.get(), 2.0);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(120), &mut cache),
            0
        );
        assert_eq!(
            ring.read_live_into(epoch(2), MachNanos::new(10), &mut cache),
            1
        );
        assert_eq!(cache[0].unwrap().world.get(), 3.0);
    }

    #[test]
    fn overwritten_visual_slots_keep_accepting_and_cached_particles_are_not_duplicated() {
        let ring = VisualRing::new(2, 1);
        for world in 1..=4 {
            assert!(ring.push_event(event(world, 0, 0.5)));
        }
        let mut cache = [None; 4];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            2
        );
        assert!(cache[..2].iter().all(|item| item.unwrap().serial >= 3));
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(2), &mut cache),
            2
        );
        assert!(ring.push_event(event(5, 0, 0.5)));
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(3), &mut cache),
            3
        );
        assert!(cache[..3].iter().any(|item| item.unwrap().serial == 3));
        let mut unique = [0; 3];
        for (index, item) in cache[..3].iter().enumerate() {
            unique[index] = item.unwrap().serial;
        }
        unique.sort_unstable();
        assert_eq!(unique, [3, 4, 5]);
    }

    #[test]
    fn fixed_visible_cache_discards_weakest_then_oldest_particles() {
        let ring = VisualRing::new(EVENT_CAPACITY, 1);
        for world in 1..=VISIBLE_CAPACITY as u64 {
            assert!(ring.push_event(event(world, 0, 0.25)));
        }
        assert!(ring.push_event(event(VISIBLE_CAPACITY as u64 + 1, 0, 1.0)));
        let mut cache = [None; VISIBLE_CAPACITY];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            VISIBLE_CAPACITY
        );
        assert!(
            cache
                .iter()
                .any(|item| item.unwrap().strength == Normalized::ONE)
        );
        assert!(!cache.iter().any(|item| item.unwrap().serial == 1));
        assert!(ring.push_event(event(VISIBLE_CAPACITY as u64 + 2, 0, 0.0)));
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(2), &mut cache),
            VISIBLE_CAPACITY
        );
        assert!(cache.iter().all(|item| item.unwrap().strength.get() > 0.0));
    }

    #[test]
    fn serial_and_slot_sequence_exhaustion_are_explicit_publication_failures() {
        let ring = VisualRing::new(1, 1);
        ring.event_serial.store(u64::MAX - 1, Ordering::SeqCst);
        assert!(ring.push_event(event(1, 0, 0.5)));
        let mut cache = [None; 1];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            1
        );
        assert_eq!(cache[0].unwrap().serial, u64::MAX);
        assert!(!ring.push_event(event(2, 0, 0.5)));
        assert_eq!(ring.event_serial.load(Ordering::SeqCst), u64::MAX);
        let another = VisualRing::new(1, 1);
        another.events[0]
            .sequence
            .store(u64::MAX - 1, Ordering::SeqCst);
        assert!(!another.push_event(event(1, 0, 0.5)));
        assert_eq!(another.event_serial.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unrepresentable_event_timing_is_rejected_without_consuming_a_serial() {
        let ring = VisualRing::new(1, 1);
        let mut packet = event(1, u64::MAX, 0.5);
        assert!(!ring.push_event(packet));
        packet.present = MachNanos::new(0);
        packet.lifetime = Duration::from_secs(u64::MAX);
        assert!(!ring.push_event(packet));
        assert_eq!(ring.event_serial.load(Ordering::SeqCst), 0);
        assert!(ring.push_event(event(2, 0, 0.5)));
        let mut cache = [None; 1];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(0), &mut cache),
            1
        );
        assert_eq!(cache[0].unwrap().serial, 1);
    }

    #[test]
    fn odd_or_malformed_atomic_packets_are_skipped_without_invalid_typed_values() {
        let ring = VisualRing::new(1, 1);
        assert!(ring.push_event(event(1, 0, 0.5)));
        let slot = &ring.events[0];
        slot.sequence.store(3, Ordering::SeqCst);
        let mut cache = [None; 1];
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            0
        );
        slot.sequence.store(4, Ordering::SeqCst);
        slot.radius.store(f32::NAN.to_bits(), Ordering::SeqCst);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            0
        );
        slot.radius.store(2.0_f32.to_bits(), Ordering::SeqCst);
        slot.shape.store(99, Ordering::SeqCst);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            0
        );
        slot.shape
            .store(VisualShape::Point as u32, Ordering::SeqCst);
        slot.situation.store(1, Ordering::SeqCst);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache),
            0
        );
        slot.situation.store(0, Ordering::SeqCst);
        slot.present.store(u64::MAX, Ordering::SeqCst);
        assert_eq!(
            ring.read_live_into(epoch(1), MachNanos::new(u64::MAX), &mut cache),
            0
        );
    }

    #[test]
    fn concurrent_overwrites_never_accept_a_mixed_event_packet() {
        let ring = Arc::new(VisualRing::new(8, 1));
        let barrier = Arc::new(Barrier::new(2));
        let writer_ring = Arc::clone(&ring);
        let writer_barrier = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            for world in 1..=10_000 {
                assert!(writer_ring.push_event(event(world, 0, 0.5)));
            }
        });
        barrier.wait();
        let mut cache = [None; 8];
        for _ in 0..10_000 {
            let count = ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache);
            for item in &cache[..count] {
                let packet = item.unwrap();
                assert_eq!(packet.world.get(), packet.serial as f64);
                assert_eq!(
                    packet.x.get(),
                    if packet.serial % 2 == 0 { 0.25 } else { 0.75 }
                );
            }
        }
        writer.join().unwrap();
        assert!(ring.read_live_into(epoch(1), MachNanos::new(1), &mut cache) > 0);
    }

    fn state<'a>(
        world: u32,
        present: u64,
        gains: &'a [f32],
        breath: &'a [f32],
    ) -> WorldVisualState<'a> {
        WorldVisualState {
            epoch: epoch(1),
            world: WorldSeconds::new(world as f64).unwrap(),
            present: MachNanos::new(present),
            phase: world,
            gains,
            breath_intensity: breath,
        }
    }

    #[test]
    fn world_state_uses_latest_arrived_time_not_newest_future_publication() {
        let ring = VisualRing::new(1, 2);
        assert!(ring.push_state(state(1, 10, &[0.25, 0.75], &[0.1, 0.2])));
        assert!(ring.push_state(state(3, 30, &[0.75, 0.25], &[0.3, 0.4])));
        let mut gains = [0.0; 2];
        let mut breath = [0.0; 2];
        assert!(
            ring.read_state_into(epoch(1), MachNanos::new(9), &mut gains, &mut breath)
                .is_none()
        );
        let header = ring
            .read_state_into(epoch(1), MachNanos::new(20), &mut gains, &mut breath)
            .unwrap();
        assert_eq!(header.phase, 1);
        assert_eq!(gains, [0.25, 0.75]);
        assert_eq!(breath, [0.1, 0.2]);
        assert!(ring.push_state(state(2, 20, &[0.5, 0.5], &[0.2, 0.3])));
        assert_eq!(
            ring.read_state_into(epoch(1), MachNanos::new(20), &mut gains, &mut breath)
                .unwrap()
                .phase,
            2
        );
        assert!(
            ring.read_state_into(epoch(2), MachNanos::new(40), &mut gains, &mut breath)
                .is_none()
        );
        assert_eq!(
            ring.read_state_into(epoch(1), MachNanos::new(30), &mut gains, &mut breath)
                .unwrap()
                .phase,
            3
        );
    }

    #[test]
    fn state_history_overwrites_and_invalid_arrays_do_not_publish() {
        let ring = VisualRing::new(1, 2);
        for world in 1..=STATE_CAPACITY as u32 + 1 {
            assert!(ring.push_state(state(world, world as u64, &[0.5, 0.5], &[0.25, 0.75])));
        }
        let mut gains = [0.0; 2];
        let mut breath = [0.0; 2];
        assert!(
            ring.read_state_into(epoch(1), MachNanos::new(1), &mut gains, &mut breath)
                .is_none()
        );
        assert!(!ring.push_state(state(999, 999, &[f32::NAN, 0.5], &[0.25, 0.75])));
        assert!(!ring.push_state(state(999, 999, &[0.5], &[0.25, 0.75])));
        assert_eq!(
            ring.read_state_into(epoch(1), MachNanos::new(999), &mut gains, &mut breath)
                .unwrap()
                .phase,
            STATE_CAPACITY as u32 + 1
        );
    }

    #[test]
    fn state_exhaustion_and_malformed_atomic_arrays_do_not_return_a_header() {
        let ring = VisualRing::new(1, 2);
        ring.state_serial.store(u64::MAX - 1, Ordering::SeqCst);
        assert!(ring.push_state(state(1, 0, &[0.25, 0.75], &[0.5, 0.5])));
        assert!(!ring.push_state(state(2, 0, &[0.75, 0.25], &[0.5, 0.5])));
        let mut gains = [0.0; 2];
        let mut breath = [0.0; 2];
        assert_eq!(
            ring.read_state_into(epoch(1), MachNanos::new(0), &mut gains, &mut breath)
                .unwrap()
                .phase,
            1
        );
        let index = ((u64::MAX - 1) % STATE_CAPACITY as u64) as usize;
        ring.states[index].gains[1].store(f32::NAN.to_bits(), Ordering::SeqCst);
        assert!(
            ring.read_state_into(epoch(1), MachNanos::new(0), &mut gains, &mut breath)
                .is_none()
        );
        let another = VisualRing::new(1, 2);
        another.states[0]
            .sequence
            .store(u64::MAX - 1, Ordering::SeqCst);
        assert!(!another.push_state(state(1, 0, &[0.5, 0.5], &[0.5, 0.5])));
        assert_eq!(another.state_serial.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn concurrent_state_snapshots_never_mix_header_and_voice_arrays() {
        let ring = Arc::new(VisualRing::new(1, 2));
        let barrier = Arc::new(Barrier::new(2));
        let writer_ring = Arc::clone(&ring);
        let writer_barrier = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            for world in 1..=10_000 {
                let values = if world % 2 == 0 {
                    [0.25, 0.75]
                } else {
                    [0.75, 0.25]
                };
                assert!(writer_ring.push_state(state(world, 0, &values, &values)));
            }
        });
        barrier.wait();
        let mut gains = [0.0; 2];
        let mut breath = [0.0; 2];
        for _ in 0..10_000 {
            if let Some(header) =
                ring.read_state_into(epoch(1), MachNanos::new(0), &mut gains, &mut breath)
            {
                assert_eq!(header.world.get(), header.phase as f64);
                let expected = if header.phase % 2 == 0 {
                    [0.25, 0.75]
                } else {
                    [0.75, 0.25]
                };
                assert_eq!(gains, expected);
                assert_eq!(breath, expected);
            }
        }
        writer.join().unwrap();
        assert_eq!(
            ring.read_state_into(epoch(1), MachNanos::new(0), &mut gains, &mut breath)
                .unwrap()
                .phase,
            10_000
        );
    }

    fn controls(registry: &Registry, high: bool) -> ControlState {
        let mut value = ControlState::new(registry);
        value.volume = Normalized::new(if high { 0.75 } else { 0.25 }).unwrap();
        value.mute = high;
        value.wander = !high;
        value.dwell_min_minutes = if high { 7 } else { 3 };
        value.dwell_max_minutes = if high { 9 } else { 5 };
        value.transition_seconds = if high { 31 } else { 11 };
        value.knobs.fill(value.volume);
        let revision = if high {
            Revision::from_raw(1)
        } else {
            Revision::INITIAL
        };
        value.manual_revision = revision;
        value.wander_revision = revision;
        value.settings_revision = revision;
        value
    }

    fn assert_controls_coherent(value: &ControlState) {
        let high = value.mute;
        assert_eq!(value.volume.get(), if high { 0.75 } else { 0.25 });
        assert_eq!(value.wander, !high);
        assert_eq!(value.dwell_min_minutes, if high { 7 } else { 3 });
        assert_eq!(value.dwell_max_minutes, if high { 9 } else { 5 });
        assert_eq!(value.transition_seconds, if high { 31 } else { 11 });
        assert!(value.knobs.iter().all(|knob| *knob == value.volume));
        let revision = if high {
            Revision::from_raw(1)
        } else {
            Revision::INITIAL
        };
        assert_eq!(value.manual_revision.get(), revision.get());
        assert_eq!(value.wander_revision.get(), revision.get());
        assert_eq!(value.settings_revision.get(), revision.get());
        assert_eq!(value.manual_target.get(), 0);
    }

    #[test]
    fn control_snapshot_rejects_in_progress_or_malformed_data_without_adoption() {
        let registry = Registry::new().unwrap();
        let initial = controls(&registry, false);
        let mailbox = ControlMailbox::new(&initial);
        let mut scratch = ControlState::new(&registry);
        let committed = initial;
        let pointer = scratch.knobs.as_ptr();
        mailbox.sequence.store(1, Ordering::SeqCst);
        mailbox.volume.store(0.75_f32.to_bits(), Ordering::SeqCst);
        assert!(!mailbox.try_read_into(&mut scratch));
        assert_controls_coherent(&committed);
        mailbox.sequence.store(2, Ordering::SeqCst);
        mailbox.manual_target.store(1, Ordering::SeqCst);
        assert!(!mailbox.try_read_into(&mut scratch));
        assert_controls_coherent(&committed);
        assert_eq!(scratch.knobs.as_ptr(), pointer);
        mailbox.publish(&committed);
        assert!(mailbox.try_read_into(&mut scratch));
        assert_controls_coherent(&scratch);
    }

    #[test]
    fn actual_atomic_control_snapshots_are_coherent_under_concurrent_publication() {
        let registry = Registry::new().unwrap();
        let low = controls(&registry, false);
        let high = controls(&registry, true);
        let mailbox = Arc::new(ControlMailbox::new(&low));
        let barrier = Arc::new(Barrier::new(2));
        let writer_mailbox = Arc::clone(&mailbox);
        let writer_barrier = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            for _ in 0..10_000 {
                writer_mailbox.publish(&high);
                writer_mailbox.publish(&low);
            }
        });
        barrier.wait();
        let mut scratch = ControlState::new(&registry);
        let pointer = scratch.knobs.as_ptr();
        for _ in 0..10_000 {
            if mailbox.try_read_into(&mut scratch) {
                assert_controls_coherent(&scratch);
            }
            assert_eq!(scratch.knobs.as_ptr(), pointer);
        }
        writer.join().unwrap();
        assert!(mailbox.try_read_into(&mut scratch));
        assert_controls_coherent(&scratch);
    }

    #[test]
    fn late_old_epoch_errors_do_not_contaminate_new_epoch_cell() {
        let old = Arc::new(ErrorCell::new(epoch(1)));
        let current = ErrorCell::new(epoch(2));
        let barrier = Arc::new(Barrier::new(2));
        let delayed = Arc::clone(&old);
        let delayed_barrier = Arc::clone(&barrier);
        let monitor = thread::spawn(move || {
            delayed_barrier.wait();
            delayed.report(cpal::ErrorKind::DeviceChanged);
            delayed.report(cpal::ErrorKind::DeviceNotAvailable);
        });
        barrier.wait();
        monitor.join().unwrap();
        assert_eq!(current.epoch(), epoch(2));
        assert_eq!(current.take_bits(), 0);
        assert_eq!(
            old.take_bits(),
            ErrorCode::DEVICE_CHANGED | ErrorCode::DEVICE_NOT_AVAILABLE
        );
        assert_eq!(old.take_bits(), 0);
    }

    #[test]
    fn xrun_and_route_notifications_are_not_device_absence_and_typed_faults_are_fatal() {
        let cell = ErrorCell::new(epoch(1));
        cell.report(cpal::ErrorKind::Xrun);
        cell.report(cpal::ErrorKind::DeviceChanged);
        cell.report(cpal::ErrorKind::RealtimeDenied);
        let notices = cell.take_bits();
        assert_eq!(notices & ErrorCode::FATAL, 0);
        assert_eq!(notices & ErrorCode::DEVICE_NOT_AVAILABLE, 0);
        assert_ne!(notices & ErrorCode::XRUN, 0);
        cell.report_timing_fault();
        cell.report_invalid_sample();
        cell.report(cpal::ErrorKind::UnsupportedConfig);
        let faults = cell.take_bits();
        assert_eq!(
            faults,
            ErrorCode::TIMING_FAULT | ErrorCode::INVALID_SAMPLE | ErrorCode::UNSUPPORTED_CONFIG
        );
        assert_eq!(faults & ErrorCode::FATAL, faults);
        assert_eq!(cell.take_bits(), 0);
    }
}
