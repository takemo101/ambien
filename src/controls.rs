use std::{error::Error, fmt, sync::Arc};

use crate::situations::{KnobIndex, Registry, SituationIndex};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Normalized(f32);

impl Normalized {
    pub(crate) const ZERO: Self = Self(0.0);
    #[cfg(test)]
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

/// The scene's level of visual effect; never changes sound or world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisualIntensity {
    #[default]
    Calm,
    Standard,
    Vivid,
}

impl VisualIntensity {
    /// The saved name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Standard => "standard",
            Self::Vivid => "vivid",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "calm" => Some(Self::Calm),
            "standard" => Some(Self::Standard),
            "vivid" => Some(Self::Vivid),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ControlEdit {
    Volume(Normalized),
    Blur(bool),
    EightBit(bool),
    BackgroundImage(bool),
    BackgroundOpacity(Normalized),
    BackgroundBlur(Normalized),
    Veil(Normalized),
    Visuals(VisualIntensity),
    Mute(bool),
    Wander(bool),
    DwellMinMinutes(u32),
    DwellMaxMinutes(u32),
    TransitionSeconds(u32),
    Select(SituationIndex),
    Knob(KnobIndex, Normalized),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlEditError {
    InvalidSituationIndex(SituationIndex),
    InvalidKnobIndex(KnobIndex),
    InvalidDwellMinMinutes(u32),
    InvalidDwellMaxMinutes(u32),
    InvalidTransitionSeconds(u32),
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
            Self::InvalidDwellMinMinutes(value) => {
                write!(
                    formatter,
                    "minimum dwell time {value} is outside 1..=180 minutes"
                )
            }
            Self::InvalidDwellMaxMinutes(value) => {
                write!(
                    formatter,
                    "maximum dwell time {value} is outside 1..=180 minutes"
                )
            }
            Self::InvalidTransitionSeconds(value) => {
                write!(
                    formatter,
                    "automatic transition time {value} is outside 1..=120 seconds"
                )
            }
            Self::RevisionExhausted(name) => write!(formatter, "{name} revision is exhausted"),
        }
    }
}

impl Error for ControlEditError {}

#[derive(Clone, Copy, PartialEq)]
struct CommonFields {
    volume: Normalized,
    blur: bool,
    eightbit: bool,
    background_image: bool,
    background_opacity: Normalized,
    background_blur: Normalized,
    veil: Normalized,
    visuals: VisualIntensity,
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
            blur: state.blur,
            eightbit: state.eightbit,
            background_image: state.background_image,
            background_opacity: state.background_opacity,
            background_blur: state.background_blur,
            veil: state.veil,
            visuals: state.visuals,
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
    /// Scene background: blurred glass when true, plain transparency when false. Display only.
    pub blur: bool,
    /// Draw the scene as Famicom-style dots (a pixelating, palette-reducing post-process). Display only.
    pub eightbit: bool,
    /// A background image copy (`background.png`) is set. Display only.
    pub background_image: bool,
    /// How strongly the background image shows, 0..=1. Display only.
    pub background_opacity: Normalized,
    /// How blurred the background image is, 0..=1. Display only.
    pub background_blur: Normalized,
    /// Amount of colour film over the scene, 0..=1. Display only.
    pub veil: Normalized,
    /// How much the scene shows: calm (large particles only), standard or vivid. Display only.
    pub visuals: VisualIntensity,
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
            blur: true,
            eightbit: false,
            background_image: false,
            background_opacity: Normalized::new(0.7)
                .expect("SPEC background opacity is normalized"),
            background_blur: Normalized::ZERO,
            veil: Normalized::ZERO,
            visuals: VisualIntensity::Calm,
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

    /// Pending manual intent takes precedence until the engine acknowledges it.
    pub(crate) fn display_target(
        &self,
        ack_manual_revision: Revision,
        display_dominant: SituationIndex,
    ) -> SituationIndex {
        if ack_manual_revision == self.manual_revision {
            display_dominant
        } else {
            self.manual_target
        }
    }

    pub(crate) fn apply(&mut self, edit: ControlEdit) -> Result<bool, ControlEditError> {
        match edit {
            ControlEdit::Volume(value) => {
                let mut common = CommonFields::from(&*self);
                common.volume = value;
                self.set_common(common)
            }
            ControlEdit::Blur(value) => {
                let mut common = CommonFields::from(&*self);
                common.blur = value;
                self.set_common(common)
            }
            ControlEdit::EightBit(value) => {
                let mut common = CommonFields::from(&*self);
                common.eightbit = value;
                self.set_common(common)
            }
            ControlEdit::BackgroundImage(value) => {
                let mut common = CommonFields::from(&*self);
                common.background_image = value;
                self.set_common(common)
            }
            ControlEdit::BackgroundOpacity(value) => {
                let mut common = CommonFields::from(&*self);
                common.background_opacity = value;
                self.set_common(common)
            }
            ControlEdit::BackgroundBlur(value) => {
                let mut common = CommonFields::from(&*self);
                common.background_blur = value;
                self.set_common(common)
            }
            ControlEdit::Veil(value) => {
                let mut common = CommonFields::from(&*self);
                common.veil = value;
                self.set_common(common)
            }
            ControlEdit::Visuals(value) => {
                let mut common = CommonFields::from(&*self);
                common.visuals = value;
                self.set_common(common)
            }
            ControlEdit::Mute(value) => {
                let mut common = CommonFields::from(&*self);
                common.mute = value;
                self.set_common(common)
            }
            ControlEdit::Wander(value) => {
                let mut common = CommonFields::from(&*self);
                common.wander = value;
                self.set_common(common)
            }
            ControlEdit::DwellMinMinutes(value) => {
                if !(1..=180).contains(&value) {
                    return Err(ControlEditError::InvalidDwellMinMinutes(value));
                }
                let mut common = CommonFields::from(&*self);
                common.dwell_min_minutes = value;
                common.dwell_max_minutes = common.dwell_max_minutes.max(value);
                self.set_common(common)
            }
            ControlEdit::DwellMaxMinutes(value) => {
                if !(1..=180).contains(&value) {
                    return Err(ControlEditError::InvalidDwellMaxMinutes(value));
                }
                let mut common = CommonFields::from(&*self);
                common.dwell_max_minutes = value;
                common.dwell_min_minutes = common.dwell_min_minutes.min(value);
                self.set_common(common)
            }
            ControlEdit::TransitionSeconds(value) => {
                if !(1..=120).contains(&value) {
                    return Err(ControlEditError::InvalidTransitionSeconds(value));
                }
                let mut common = CommonFields::from(&*self);
                common.transition_seconds = value;
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
        self.blur = common.blur;
        self.eightbit = common.eightbit;
        self.background_image = common.background_image;
        self.background_opacity = common.background_opacity;
        self.background_blur = common.background_blur;
        self.veil = common.veil;
        self.visuals = common.visuals;
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
        self.blur = source.blur;
        self.eightbit = source.eightbit;
        self.background_image = source.background_image;
        self.background_opacity = source.background_opacity;
        self.background_blur = source.background_blur;
        self.veil = source.veil;
        self.visuals = source.visuals;
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
    fn time_edits_correct_the_pair_without_issuing_manual_or_wander_intent() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        state.dwell_min_minutes = 10;
        state.dwell_max_minutes = 20;
        let target = state.manual_target;
        assert!(state.apply(ControlEdit::DwellMinMinutes(21)).unwrap());
        assert_eq!((state.dwell_min_minutes, state.dwell_max_minutes), (21, 21));
        assert!(state.apply(ControlEdit::DwellMaxMinutes(7)).unwrap());
        assert_eq!((state.dwell_min_minutes, state.dwell_max_minutes), (7, 7));
        assert!(state.apply(ControlEdit::TransitionSeconds(1)).unwrap());
        assert!(state.wander);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert_eq!(state.settings_revision.get(), 3);
        assert!(!state.apply(ControlEdit::DwellMaxMinutes(7)).unwrap());
        assert_eq!(state.settings_revision.get(), 3);
        for edit in [
            ControlEdit::DwellMinMinutes(0),
            ControlEdit::DwellMinMinutes(181),
            ControlEdit::DwellMaxMinutes(0),
            ControlEdit::DwellMaxMinutes(181),
            ControlEdit::TransitionSeconds(0),
            ControlEdit::TransitionSeconds(121),
        ] {
            let previous = state.clone();
            assert!(state.apply(edit).is_err());
            assert_eq!(state, previous);
        }
        state.settings_revision = Revision::from_raw(u64::MAX);
        let previous = state.clone();
        assert!(state.apply(ControlEdit::DwellMinMinutes(8)).is_err());
        assert_eq!(state, previous);
    }

    #[test]
    fn wander_toggle_does_not_issue_selection_and_manual_edit_turns_it_off() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let target = state.manual_target;
        assert!(state.apply(ControlEdit::Wander(false)).unwrap());
        assert!(!state.apply(ControlEdit::Wander(false)).unwrap());
        assert!(state.apply(ControlEdit::Wander(true)).unwrap());
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.wander_revision.get(), 2);
        assert!(state.apply(ControlEdit::Select(target)).unwrap());
        assert!(!state.wander);
        assert_eq!(state.wander_revision.get(), 3);
    }

    #[test]
    fn displayed_selection_uses_automatic_dominant_only_after_manual_ack() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        let automatic = SituationIndex::from_index(1, registry.definitions().len()).unwrap();
        assert_eq!(
            state.display_target(Revision::INITIAL, automatic),
            automatic
        );
        let manual = state.manual_target;
        state.apply(ControlEdit::Select(manual)).unwrap();
        assert_eq!(state.display_target(Revision::INITIAL, automatic), manual);
        assert_eq!(
            state.display_target(state.manual_revision, automatic),
            automatic
        );
    }

    #[test]
    fn wander_revision_exhaustion_rejects_toggle_without_mutation() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        state.wander_revision = Revision::from_raw(u64::MAX);
        let before = state.clone();
        assert_eq!(
            state.apply(ControlEdit::Wander(false)),
            Err(ControlEditError::RevisionExhausted("wander"))
        );
        assert_eq!(state, before);
        assert!(!state.apply(ControlEdit::Wander(true)).unwrap());
        assert_eq!(state, before);
    }

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
    fn blur_and_veil_default_to_the_existing_glass_and_never_become_manual_intent() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        assert!(state.blur);
        assert_eq!(state.veil, Normalized::ZERO);
        let target = state.manual_target;
        assert!(state.apply(ControlEdit::Blur(false)).unwrap());
        assert!(
            state
                .apply(ControlEdit::Veil(Normalized::new(0.4).unwrap()))
                .unwrap()
        );
        assert!(!state.blur);
        assert_eq!(state.veil.get(), 0.4);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert!(state.wander);
        assert_eq!(state.settings_revision.get(), 2);
        // Re-applying the same value is not an edit.
        assert!(!state.apply(ControlEdit::Blur(false)).unwrap());
        assert!(
            !state
                .apply(ControlEdit::Veil(Normalized::new(0.4).unwrap()))
                .unwrap()
        );
        assert_eq!(state.settings_revision.get(), 2);
    }

    #[test]
    fn the_eight_bit_mode_starts_off_and_is_a_display_only_edit_that_never_changes_the_world() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        assert!(!state.eightbit);
        let target = state.manual_target;
        assert!(state.apply(ControlEdit::EightBit(true)).unwrap());
        assert!(state.eightbit);
        // It is a settings change only: not a manual selection, and the movement settings are untouched.
        assert_eq!(state.manual_target, target);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert!(state.wander);
        assert_eq!(state.settings_revision.get(), 1);
        // Setting the same value again is not an edit, and a view copying the values sees the choice.
        assert!(!state.apply(ControlEdit::EightBit(true)).unwrap());
        assert_eq!(state.settings_revision.get(), 1);
        let mut shown = ControlState::new(&registry);
        shown.copy_values_from(&state);
        assert!(shown.eightbit);
        assert!(state.apply(ControlEdit::EightBit(false)).unwrap());
        assert!(!state.eightbit);
    }

    #[test]
    fn background_image_edits_are_display_only_and_never_change_the_world() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        assert!(!state.background_image);
        assert_eq!(state.background_opacity.get(), 0.7);
        assert_eq!(state.background_blur, Normalized::ZERO);
        let before = state.clone();
        let half = Normalized::new(0.5).unwrap();
        assert!(state.apply(ControlEdit::BackgroundImage(true)).unwrap());
        assert!(state.apply(ControlEdit::BackgroundOpacity(half)).unwrap());
        assert!(
            state
                .apply(ControlEdit::BackgroundBlur(Normalized::ONE))
                .unwrap()
        );
        assert!(state.background_image);
        assert_eq!(state.background_opacity, half);
        assert_eq!(state.background_blur, Normalized::ONE);
        // Settings changed, but no manual selection, movement or sound field did.
        assert_eq!(state.settings_revision.get(), 3);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert_eq!(state.wander_revision, Revision::INITIAL);
        assert_eq!(state.manual_target, before.manual_target);
        assert_eq!(state.knobs, before.knobs);
        assert_eq!(state.volume, before.volume);
        assert_eq!(state.mute, before.mute);
        assert_eq!(state.wander, before.wander);
        // The same value again is not an edit, and a view copying the values sees the choice.
        assert!(!state.apply(ControlEdit::BackgroundOpacity(half)).unwrap());
        assert_eq!(state.settings_revision.get(), 3);
        let mut shown = ControlState::new(&registry);
        shown.copy_values_from(&state);
        assert!(shown.background_image);
        assert_eq!(shown.background_opacity, half);
        assert_eq!(shown.background_blur, Normalized::ONE);
        assert!(state.apply(ControlEdit::BackgroundImage(false)).unwrap());
        assert!(!state.background_image);
    }

    #[test]
    fn visual_intensity_defaults_to_calm_round_trips_its_name_and_is_a_display_only_edit() {
        let registry = Registry::new().unwrap();
        let mut state = ControlState::new(&registry);
        assert_eq!(state.visuals, VisualIntensity::Calm);
        for (intensity, name) in [
            (VisualIntensity::Calm, "calm"),
            (VisualIntensity::Standard, "standard"),
            (VisualIntensity::Vivid, "vivid"),
        ] {
            assert_eq!(intensity.name(), name);
            assert_eq!(VisualIntensity::from_name(name), Some(intensity));
        }
        assert_eq!(VisualIntensity::from_name("loud"), None);
        assert_eq!(VisualIntensity::from_name(""), None);
        let target = state.manual_target;
        assert!(
            state
                .apply(ControlEdit::Visuals(VisualIntensity::Vivid))
                .unwrap()
        );
        assert_eq!(state.visuals, VisualIntensity::Vivid);
        assert_eq!(state.manual_target, target);
        assert_eq!(state.manual_revision, Revision::INITIAL);
        assert!(state.wander);
        assert_eq!(state.settings_revision.get(), 1);
        assert!(
            !state
                .apply(ControlEdit::Visuals(VisualIntensity::Vivid))
                .unwrap()
        );
        assert_eq!(state.settings_revision.get(), 1);
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
