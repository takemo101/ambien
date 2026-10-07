use crate::situations::{Registry, SituationIndex};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Normalized(f32);

impl Normalized {
    pub(crate) const ZERO: Self = Self(0.0);
    pub(crate) const ONE: Self = Self(1.0);

    pub(crate) const fn new(value: f32) -> Option<Self> {
        if value >= 0.0 && value <= 1.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Revision(u64);

impl Revision {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ControlState {
    pub volume: Normalized,
    pub mute: bool,
    pub wander: bool,
    pub dwell_min_minutes: u32,
    pub dwell_max_minutes: u32,
    pub transition_seconds: u32,
    pub knobs: Box<[Normalized]>,
    pub manual_target: SituationIndex,
    pub manual_revision: Revision,
    pub wander_revision: Revision,
    pub settings_revision: Revision,
    registry_count: usize,
}

impl ControlState {
    pub(crate) fn new(registry: &Registry) -> Self {
        let knobs = registry
            .definitions()
            .iter()
            .flat_map(|definition| definition.knobs.iter().map(|knob| knob.initial))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            volume: Normalized::new(0.3).expect("SPEC volume is normalized"),
            mute: false,
            wander: true,
            dwell_min_minutes: 12,
            dwell_max_minutes: 40,
            transition_seconds: 40,
            knobs,
            manual_target: registry.first_index(),
            manual_revision: Revision::INITIAL,
            wander_revision: Revision::INITIAL,
            settings_revision: Revision::INITIAL,
            registry_count: registry.definitions().len(),
        }
    }

    pub(crate) fn registry_count(&self) -> usize {
        self.registry_count
    }
}

#[cfg(test)]
mod tests {
    use super::Normalized;

    #[test]
    fn rejects_nonfinite_and_out_of_range_control_values() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.001, 1.001] {
            assert!(Normalized::new(value).is_none());
        }
        assert_eq!(Normalized::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Normalized::new(1.0).unwrap().get(), 1.0);
        assert_eq!(Normalized::new(0.3).unwrap().get(), 0.3);
    }
}
