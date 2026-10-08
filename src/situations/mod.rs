mod deep;
mod rain;
mod rock;
pub(crate) mod synthesis;

use std::{error::Error, fmt, ops::Range};

use crate::{
    audio::{timing::WorldSeconds, transport::VisualWriter},
    controls::Normalized,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SituationIndex(usize);

impl SituationIndex {
    pub const fn from_index(value: usize, count: usize) -> Option<Self> {
        if value < count {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnobIndex(usize);

impl KnobIndex {
    pub const fn from_index(value: usize, count: usize) -> Option<Self> {
        if value < count {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VisualShape {
    Point = 0,
    Impact = 1,
    SlowGlow = 2,
}

#[derive(Clone, Copy, Debug)]
pub struct VisualStyle {
    pub color: [f32; 3],
    pub shape: VisualShape,
}

pub struct KnobDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub initial: Normalized,
}

pub struct SituationDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub knobs: &'static [KnobDefinition],
    pub visual: VisualStyle,
    pub grain_capacity: usize,
    prepare: fn(VoicePreparation) -> Result<Box<dyn Voice>, VoicePreparationError>,
}

#[derive(Clone, Copy, Debug)]
pub struct VoicePreparation {
    pub sample_rate: u32,
    pub seed: u64,
    pub grain_capacity: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct VoiceFrame {
    pub world: WorldSeconds,
    pub delta_seconds: f64,
    pub sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceLatencyFrames(f64);

impl VoiceLatencyFrames {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoicePreparationError {
    InvalidRate,
    InvalidDefinition,
    InvalidParameters,
    UnresolvedCausalDelay,
}

impl fmt::Display for VoicePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRate => "invalid voice sample rate",
            Self::InvalidDefinition => "invalid situation registration",
            Self::InvalidParameters => "invalid voice preparation parameters",
            Self::UnresolvedCausalDelay => "dry causal delay could not be verified",
        })
    }
}

impl Error for VoicePreparationError {}

pub trait Voice: Send {
    fn advance_world(
        &mut self,
        frame: VoiceFrame,
        knobs: &[Normalized],
        events: &mut VisualWriter<'_>,
    );
    fn render_sample(&mut self) -> [f32; 2];
    fn set_contributing(&mut self, active: bool);
    fn breath_intensity(&self) -> Normalized;
    fn prepared_delay(&self) -> Result<VoiceLatencyFrames, VoicePreparationError>;
    /// Worker-exclusive: may allocate, replace buffers and inspect node routing.
    fn prepare_rate(
        &mut self,
        sample_rate: u32,
    ) -> Result<VoiceLatencyFrames, VoicePreparationError>;
}

pub struct Registry {
    definitions: Box<[SituationDefinition]>,
    knob_ranges: Box<[Range<usize>]>,
    knob_count: usize,
}

impl Registry {
    pub fn new() -> Result<Self, VoicePreparationError> {
        Self::validate(vec![
            rain::definition(),
            rock::definition(),
            deep::definition(),
        ])
    }

    fn validate(definitions: Vec<SituationDefinition>) -> Result<Self, VoicePreparationError> {
        if definitions.is_empty() {
            return Err(VoicePreparationError::InvalidDefinition);
        }
        let mut ranges = Vec::with_capacity(definitions.len());
        let mut count: usize = 0;
        for (index, definition) in definitions.iter().enumerate() {
            if definition.id.is_empty()
                || definition.label.is_empty()
                || definition.grain_capacity == 0
                || definitions[..index]
                    .iter()
                    .any(|other| other.id == definition.id)
                || definition
                    .visual
                    .color
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err(VoicePreparationError::InvalidDefinition);
            }
            for (knob_index, knob) in definition.knobs.iter().enumerate() {
                if knob.id.is_empty()
                    || knob.label.is_empty()
                    || definition.knobs[..knob_index]
                        .iter()
                        .any(|other| other.id == knob.id)
                {
                    return Err(VoicePreparationError::InvalidDefinition);
                }
            }
            let end = count
                .checked_add(definition.knobs.len())
                .ok_or(VoicePreparationError::InvalidDefinition)?;
            ranges.push(count..end);
            count = end;
        }
        Ok(Self {
            definitions: definitions.into_boxed_slice(),
            knob_ranges: ranges.into_boxed_slice(),
            knob_count: count,
        })
    }

    pub fn definitions(&self) -> &[SituationDefinition] {
        &self.definitions
    }

    pub fn resolve(&self, id: &str) -> Option<SituationIndex> {
        self.definitions
            .iter()
            .position(|definition| definition.id == id)
            .map(SituationIndex)
    }

    pub fn first_index(&self) -> SituationIndex {
        SituationIndex(0)
    }

    pub fn knob_range(&self, index: SituationIndex) -> Range<usize> {
        self.knob_ranges[index.get()].clone()
    }

    pub fn knob_count(&self) -> usize {
        self.knob_count
    }

    pub fn prepare_voice(
        &self,
        index: SituationIndex,
        config: VoicePreparation,
    ) -> Result<Box<dyn Voice>, VoicePreparationError> {
        let definition = self
            .definitions
            .get(index.get())
            .ok_or(VoicePreparationError::InvalidDefinition)?;
        (definition.prepare)(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_ambiguous_ids_and_knobs_and_empty_definitions() {
        assert!(matches!(
            Registry::validate(Vec::new()),
            Err(VoicePreparationError::InvalidDefinition)
        ));
        assert!(matches!(
            Registry::validate(vec![rain::definition(), rain::definition()]),
            Err(VoicePreparationError::InvalidDefinition)
        ));
        static DUPLICATE_KNOBS: [KnobDefinition; 2] = [
            KnobDefinition {
                id: "amount",
                label: "量",
                initial: Normalized::ZERO,
            },
            KnobDefinition {
                id: "amount",
                label: "量",
                initial: Normalized::ONE,
            },
        ];
        let mut definition = rain::definition();
        definition.knobs = &DUPLICATE_KNOBS;
        assert!(matches!(
            Registry::validate(vec![definition]),
            Err(VoicePreparationError::InvalidDefinition)
        ));
        let mut definition = rain::definition();
        definition.grain_capacity = 0;
        assert!(matches!(
            Registry::validate(vec![definition]),
            Err(VoicePreparationError::InvalidDefinition)
        ));
    }

    #[test]
    fn registry_prepares_real_voice_and_reports_invalid_boundaries() {
        let registry = Registry::new().unwrap();
        assert!(registry.resolve("missing").is_none());
        assert!(SituationIndex::from_index(1, 1).is_none());
        assert!(KnobIndex::from_index(2, 2).is_none());
        assert!(VoiceLatencyFrames::new(f64::NAN).is_none());
        assert!(VoiceLatencyFrames::new(-1.0).is_none());
        let mut voice = registry
            .prepare_voice(
                registry.first_index(),
                VoicePreparation {
                    sample_rate: 48_000,
                    seed: 3,
                    grain_capacity: 64,
                },
            )
            .unwrap();
        assert_eq!(voice.prepare_rate(44_100).unwrap().get(), 0.0);
        assert!(matches!(
            voice.prepare_rate(0),
            Err(VoicePreparationError::InvalidRate)
        ));
    }

    #[test]
    fn p04_smallest_soft_rock_still_produces_finite_audio_from_its_real_voice() {
        use crate::audio::{
            timing::{MachNanos, StreamEpoch, WorldSeconds},
            transport::VisualRing,
        };

        let registry = Registry::new().unwrap();
        let index = registry.resolve("rock").expect("rock must be registered");
        let definition = &registry.definitions()[index.get()];
        let mut voice = registry
            .prepare_voice(
                index,
                VoicePreparation {
                    sample_rate: 8_000,
                    seed: 137,
                    grain_capacity: definition.grain_capacity,
                },
            )
            .unwrap();
        voice.set_contributing(true);
        let ring = VisualRing::new(64, registry.definitions().len());
        let knobs = [Normalized::ONE, Normalized::ZERO, Normalized::ZERO];
        let mut energy = 0.0_f64;
        for sample in 0..16_000 {
            let world = WorldSeconds::new(sample as f64 / 8_000.0).unwrap();
            let mut events = VisualWriter::new(
                &ring,
                StreamEpoch::new(1).unwrap(),
                index,
                world,
                MachNanos::new(sample * 125_000),
            );
            voice.advance_world(
                VoiceFrame {
                    world,
                    delta_seconds: 1.0 / 8_000.0,
                    sample_rate: 8_000,
                },
                &knobs,
                &mut events,
            );
            for value in voice.render_sample() {
                assert!(value.is_finite());
                assert!(value.abs() < 1.0);
                energy += f64::from(value).powi(2);
            }
        }
        assert!((energy / 32_000.0).sqrt() > 1e-5, "size=0 is not mute");
        assert_eq!(voice.prepared_delay().unwrap().get(), 0.0);
    }
}
