use std::{error::Error, fmt, sync::Arc};

use crate::situations::{KnobIndex, Registry, SituationIndex};

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

    fn next(self, name: &'static str) -> Result<Self, ControlEditError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(ControlEditError::RevisionExhausted(name))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ControlEdit {
    Volume(Normalized),
    Mute(bool),
    Select(SituationIndex),
    Knob(KnobIndex, Normalized),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlEditError {
    InvalidSituationIndex(SituationIndex),
    InvalidKnobIndex(KnobIndex),
    RevisionExhausted(&'static str),
}

impl fmt::Display for ControlEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSituationIndex(index) => {
                write!(formatter, "invalid situation index {}", index.get())
            }
            Self::InvalidKnobIndex(index) => {
                write!(formatter, "invalid knob index {}", index.get())
            }
            Self::RevisionExhausted(name) => write!(formatter, "{name} revision is exhausted"),
        }
    }
}

impl Error for ControlEditError {}

// This private setter input reuses the current common fields; it is not a new
// time/wander UI or a queued intent. Volume and mute use the same revision rule.
#[derive(Clone, Copy, PartialEq)]
struct CommonFields {
    volume: Normalized,
    mute: bool,
    wander: bool,
    dwell_min_minutes: u32,
    dwell_max_minutes: u32,
    transition_seconds: u32,
}

impl From<&ControlState> for CommonFields {
    fn from(state: &ControlState) -> Self {
        Self {
            volume: state.volume,
            mute: state.mute,
            wander: state.wander,
            dwell_min_minutes: state.dwell_min_minutes,
            dwell_max_minutes: state.dwell_max_minutes,
            transition_seconds: state.transition_seconds,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
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
    knob_owners: Arc<[SituationIndex]>,
}

impl ControlState {
    pub(crate) fn new(registry: &Registry) -> Self {
        let knobs = registry
            .definitions()
            .iter()
            .flat_map(|definition| definition.knobs.iter().map(|knob| knob.initial))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let knob_owners = registry
            .definitions()
            .iter()
            .enumerate()
            .flat_map(|(position, definition)| {
                let owner = SituationIndex::from_index(position, registry.definitions().len())
                    .expect("registered situation index is valid");
                definition.knobs.iter().map(move |_| owner)
            })
            .collect();
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
            knob_owners,
        }
    }

    pub(crate) fn registry_count(&self) -> usize {
        self.registry_count
    }

    pub(crate) fn apply(&mut self, edit: ControlEdit) -> Result<bool, ControlEditError> {
        match edit {
            ControlEdit::Volume(value) => {
                let mut common = CommonFields::from(&*self);
                common.volume = value;
                self.set_common(common)
            }
            ControlEdit::Mute(value) => {
                let mut common = CommonFields::from(&*self);
                common.mute = value;
                self.set_common(common)
            }
            ControlEdit::Select(target) => {
                if target.get() >= self.registry_count {
                    return Err(ControlEditError::InvalidSituationIndex(target));
                }
                self.issue_manual(target, None)
            }
            ControlEdit::Knob(index, value) => self.edit_knob(index, value),
        }
    }

    pub(crate) fn edit_knob(
        &mut self,
        index: KnobIndex,
        value: Normalized,
    ) -> Result<bool, ControlEditError> {
        let owner = self
            .knob_owners
            .get(index.get())
            .copied()
            .filter(|_| index.get() < self.knobs.len())
            .ok_or(ControlEditError::InvalidKnobIndex(index))?;
        self.issue_manual(owner, Some((index, value)))
    }

    fn issue_manual(
        &mut self,
        target: SituationIndex,
        knob: Option<(KnobIndex, Normalized)>,
    ) -> Result<bool, ControlEditError> {
        // Check every fallible increment before touching any endpoint.
        let manual_revision = self.manual_revision.next("manual")?;
        let settings_revision = self.settings_revision.next("settings")?;
        let wander_revision = if self.wander {
            self.wander_revision.next("wander")?
        } else {
            self.wander_revision
        };
        if let Some((index, value)) = knob {
            self.knobs[index.get()] = value;
        }
        self.manual_target = target;
        self.wander = false;
        self.manual_revision = manual_revision;
        self.wander_revision = wander_revision;
        self.settings_revision = settings_revision;
        Ok(true)
    }

    fn set_common(&mut self, common: CommonFields) -> Result<bool, ControlEditError> {
        if common == CommonFields::from(&*self) {
            return Ok(false);
        }
        let settings_revision = self.settings_revision.next("settings")?;
        let wander_revision = if self.wander != common.wander {
            self.wander_revision.next("wander")?
        } else {
            self.wander_revision
        };
        self.volume = common.volume;
        self.mute = common.mute;
        self.wander = common.wander;
        self.dwell_min_minutes = common.dwell_min_minutes;
        self.dwell_max_minutes = common.dwell_max_minutes;
        self.transition_seconds = common.transition_seconds;
        self.wander_revision = wander_revision;
        self.settings_revision = settings_revision;
        Ok(true)
    }

    /// Copies only endpoint values into an already-prepared identical layout.
    /// Reading/refreshing a view never issues an edit or allocates knob storage.
    pub(crate) fn copy_values_from(&mut self, source: &Self) {
        assert_eq!(self.registry_count, source.registry_count);
        assert_eq!(self.knob_owners, source.knob_owners);
        assert_eq!(self.knobs.len(), source.knobs.len());
        self.volume = source.volume;
        self.mute = source.mute;
        self.wander = source.wander;
        self.dwell_min_minutes = source.dwell_min_minutes;
        self.dwell_max_minutes = source.dwell_max_minutes;
        self.transition_seconds = source.transition_seconds;
        self.knobs.copy_from_slice(&source.knobs);
        self.manual_target = source.manual_target;
        self.manual_revision = source.manual_revision;
        self.wander_revision = source.wander_revision;
        self.settings_revision = source.settings_revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nonfinite_and_out_of_range_control_values() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.001, 1.001] {
            assert!(Normalized::new(value).is_none());
        }
        assert_eq!(Normalized::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Normalized::new(1.0).unwrap().get(), 1.0);
        assert_eq!(Normalized::new(0.3).unwrap().get(), 0.3);
    }

    #[test]
    fn volume_and_mute_are_independent_nonmanual_edits() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let target = state.manual_target;
        assert!(state.apply(ControlEdit::Mute(true)).unwrap());
        assert!(
            state
                .apply(ControlEdit::Volume(Normalized::new(0.8).unwrap()))
                .unwrap()
        );
        assert!(state.mute);
        assert_eq!(state.volume.get(), 0.8);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert!(state.wander);
        assert_eq!(state.settings_revision.get(), 2);
        assert!(!state.apply(ControlEdit::Mute(true)).unwrap());
        assert!(
            !state
                .apply(ControlEdit::Volume(Normalized::new(0.8).unwrap()))
                .unwrap()
        );
        assert_eq!(state.settings_revision.get(), 2);
    }

    #[test]
    fn every_registered_knob_issues_latest_intent_for_its_metadata_owner() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let mut revision = 0;
        for position in 0..registry.definitions().len() {
            let owner = SituationIndex::from_index(position, registry.definitions().len()).unwrap();
            for raw in registry.knob_range(owner) {
                let index = KnobIndex::from_index(raw, registry.knob_count()).unwrap();
                assert!(state.edit_knob(index, Normalized::ONE).unwrap());
                revision += 1;
                assert_eq!(state.knobs[raw], Normalized::ONE);
                assert_eq!(state.manual_target, owner);
                assert_eq!(state.manual_revision.get(), revision);
                assert_eq!(state.settings_revision.get(), revision);
                assert_eq!(state.wander_revision.get(), 1);
                assert!(!state.wander);
                assert_eq!(state.volume.get(), 0.3);
                assert!(!state.mute);
            }
        }
    }

    #[test]
    fn explicit_selection_and_same_value_knob_are_new_manual_intents() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let target = state.manual_target;
        let index = KnobIndex::from_index(0, registry.knob_count()).unwrap();
        let initial = state.knobs[0];
        assert!(state.apply(ControlEdit::Select(target)).unwrap());
        assert!(state.apply(ControlEdit::Knob(index, initial)).unwrap());
        assert_eq!(state.manual_revision.get(), 2);
        assert_eq!(state.settings_revision.get(), 2);
        assert_eq!(state.wander_revision.get(), 1);
        assert!(!state.wander);
    }

    #[test]
    fn invalid_indices_and_exhausted_revisions_leave_every_field_unchanged() {
        let registry = Registry::new().unwrap();
        let invalid_knob =
            KnobIndex::from_index(registry.knob_count(), registry.knob_count() + 1).unwrap();
        let invalid_target = SituationIndex::from_index(
            registry.definitions().len(),
            registry.definitions().len() + 1,
        )
        .unwrap();
        let mut state = ControlState::new(&registry);
        let before = state.clone();
        for edit in [
            ControlEdit::Knob(invalid_knob, Normalized::ONE),
            ControlEdit::Select(invalid_target),
        ] {
            assert!(state.apply(edit).is_err());
            assert_eq!(state, before);
        }
        for exhausted in 0..3 {
            let mut state = before.clone();
            let maximum = Revision::from_raw(u64::MAX);
            match exhausted {
                0 => state.manual_revision = maximum,
                1 => state.wander_revision = maximum,
                _ => state.settings_revision = maximum,
            }
            let original = state.clone();
            let index = KnobIndex::from_index(0, registry.knob_count()).unwrap();
            assert!(state.edit_knob(index, Normalized::ONE).is_err());
            assert_eq!(state, original);
            assert!(
                state
                    .apply(ControlEdit::Select(state.manual_target))
                    .is_err()
            );
            assert_eq!(state, original);
        }
        state.settings_revision = Revision::from_raw(u64::MAX);
        let original = state.clone();
        assert!(state.apply(ControlEdit::Mute(true)).is_err());
        assert_eq!(state, original);
        assert!(state.apply(ControlEdit::Volume(Normalized::ONE)).is_err());
        assert_eq!(state, original);
        assert!(!state.apply(ControlEdit::Mute(false)).unwrap());
        assert_eq!(state, original);
    }

    #[test]
    fn common_time_and_wander_fields_never_become_manual_intent() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let target = state.manual_target;
        let mut common = CommonFields::from(&state);
        common.dwell_min_minutes = 8;
        common.dwell_max_minutes = 24;
        common.transition_seconds = 17;
        assert!(state.set_common(common).unwrap());
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert_eq!(state.settings_revision.get(), 1);
        common.wander = false;
        assert!(state.set_common(common).unwrap());
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.wander_revision.get(), 1);
        assert_eq!(state.settings_revision.get(), 2);
        assert_eq!(state.dwell_min_minutes, 8);
        assert_eq!(state.dwell_max_minutes, 24);
        assert_eq!(state.transition_seconds, 17);
        assert!(!state.set_common(common).unwrap());
    }

    #[test]
    fn common_edits_ignore_manual_exhaustion_and_check_wander_before_mutation() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        state.manual_revision = Revision::from_raw(u64::MAX);
        state.wander_revision = Revision::from_raw(u64::MAX);
        assert!(state.apply(ControlEdit::Mute(true)).unwrap());
        assert!(state.apply(ControlEdit::Volume(Normalized::ONE)).unwrap());
        assert_eq!(state.manual_revision.get(), u64::MAX);
        assert_eq!(state.wander_revision.get(), u64::MAX);
        let before = state.clone();
        let mut common = CommonFields::from(&state);
        common.wander = false;
        common.dwell_min_minutes = 8;
        common.dwell_max_minutes = 24;
        assert_eq!(
            state.set_common(common),
            Err(ControlEditError::RevisionExhausted("wander"))
        );
        assert_eq!(state, before);
    }
}
