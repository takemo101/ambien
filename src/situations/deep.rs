use std::time::Duration;

use fundsp::{
    audionode::AudioNode,
    biquad::Resonator,
    filter::{DCBlock, Lowpole, Pinkpass},
    noise::Noise,
    prelude32::U1,
};

use crate::{
    audio::transport::{LogicalPixels, ParticleVisual, VisualWriter},
    controls::Normalized,
};

use super::{
    KnobDefinition, SituationDefinition, VisualShape, VisualStyle, Voice, VoiceFrame,
    VoiceLatencyFrames, VoicePreparation, VoicePreparationError,
    synthesis::{
        KnobRamp, Modulation, Poisson, RestartableReverb, WorldRandom, dry_latency, prepare_node,
    },
};

// Quiet implementation candidates, not listening-approved P13 defaults.
// All frequency, level, color and lifetime coefficients share this status.
static KNOBS: [KnobDefinition; 2] = [
    KnobDefinition {
        id: "depth",
        label: "深さ",
        initial: match Normalized::new(0.68) {
            Some(value) => value,
            None => panic!("candidate out of range"),
        },
    },
    KnobDefinition {
        id: "air",
        label: "空気の量",
        initial: match Normalized::new(0.16) {
            Some(value) => value,
            None => panic!("candidate out of range"),
        },
    },
];

const MAX_BUBBLES: usize = 24;
const PITCH_SECONDS: f64 = 1.0 / 200.0;
const VISUAL: VisualStyle = VisualStyle {
    color: [0.12, 0.28, 0.48],
    shape: VisualShape::SlowGlow,
};

pub(super) fn definition() -> SituationDefinition {
    SituationDefinition {
        id: "deep",
        label: "深海",
        knobs: &KNOBS,
        visual: VISUAL,
        grain_capacity: MAX_BUBBLES,
        prepare: |config| Ok(Box::new(DeepVoice::new(config)?)),
    }
}

#[derive(Clone, Copy)]
struct DepthTone {
    cutoff: f32,
    low_mix: f32,
    darkness: f32,
    speed: f32,
}

impl DepthTone {
    fn new(depth: f32) -> Self {
        Self {
            cutoff: 1_400.0 * (140.0_f32 / 1_400.0).powf(depth),
            low_mix: 0.30 + depth * 0.45,
            // Strength and current breath both reach the existing renderer;
            // depth cannot become an unused color/motion field.
            darkness: 1.0 - depth * 0.55,
            speed: 0.40_f32.powf(depth),
        }
    }
}

struct Bubble {
    noise: Noise,
    resonator: Resonator<f64, U1>,
    age: f64,
    lifetime: f64,
    pitch_elapsed: f64,
    start_center: f32,
    center: f32,
    bandwidth: f32,
    q: f32,
    strength: f32,
    pan: [f32; 2],
    y: f32,
    radius: f32,
    audio_eligible: bool,
}

impl Bubble {
    fn new(seed: u64) -> Self {
        let mut noise = Noise::new();
        noise.set_hash(seed);
        Self {
            noise,
            resonator: Resonator::new(720.0, 12.0),
            age: 0.0,
            lifetime: 0.0,
            pitch_elapsed: 0.0,
            start_center: 720.0,
            center: 720.0,
            bandwidth: 60.0,
            q: 12.0,
            strength: 0.0,
            pan: [0.0; 2],
            y: 0.0,
            radius: 3.0,
            audio_eligible: false,
        }
    }

    fn alive(&self) -> bool {
        self.age < self.lifetime
    }

    fn start(&mut self, tone: DepthTone, strength: f32, x: f32, y: f32, scale: f32, rate: u32) {
        self.age = 0.0;
        self.lifetime = 0.42 * scale as f64 / tone.speed as f64;
        self.pitch_elapsed = 0.0;
        self.start_center = (720.0 / scale).min(rate as f32 * 0.40);
        self.center = self.start_center;
        self.bandwidth = 60.0;
        // Pinned FunDSP 0.23 uses Q=center/bandwidth, not bandwidth Hz.
        // r=exp(-pi*bandwidth/fs) is strictly below 1 for these bounds.
        self.q = self.center / self.bandwidth;
        self.strength = strength * tone.darkness;
        let angle = x * std::f32::consts::FRAC_PI_2;
        self.pan = [angle.cos(), angle.sin()];
        self.y = y;
        self.radius = (2.5 + strength * 2.0) * scale;
        self.audio_eligible = true;
        self.resonator.reset();
        self.resonator
            .set_center_q(self.center as f64, self.q as f64);
    }

    fn advance(&mut self, delta: f64) {
        if !self.alive() {
            return;
        }
        self.age = (self.age + delta).min(self.lifetime);
        self.pitch_elapsed += delta;
        if self.pitch_elapsed >= PITCH_SECONDS {
            self.pitch_elapsed -= PITCH_SECONDS;
            // Bounded downward pitch, without per-sample exp/cos coefficient
            // recalculation. Logical pitch advances even with DSP paused.
            self.center = self.start_center * (1.0 - 0.45 * (self.age / self.lifetime) as f32);
            self.q = self.center / self.bandwidth;
            if self.audio_eligible {
                self.resonator
                    .set_center_q(self.center as f64, self.q as f64);
            }
        }
    }

    fn render(&mut self) -> [f32; 2] {
        if !self.alive() || !self.audio_eligible {
            return [0.0; 2];
        }
        let attack = (self.age / 0.009).min(1.0) as f32;
        let onset = attack * attack * (3.0 - 2.0 * attack);
        let excitation = if self.age < 0.022 {
            let release = (1.0 - self.age / 0.022) as f32;
            self.noise.tick(&Default::default())[0] * onset * release * release
        } else {
            0.0
        };
        let release = (1.0 - self.age / self.lifetime) as f32;
        let envelope = onset * release * release * (3.0 - 2.0 * release);
        let sample =
            self.resonator.tick(&[excitation].into())[0] * envelope * self.strength * 0.028;
        [sample * self.pan[0], sample * self.pan[1]]
    }

    fn invalidate_history(&mut self) {
        self.audio_eligible = false;
        self.resonator.reset();
    }
}

struct LowChannel {
    noise: Noise,
    pink: Pinkpass<f64>,
    brown: Lowpole<f64, U1>,
    air_low: Lowpole<f64, U1>,
    low_modes: [Resonator<f64, U1>; 2],
    filter: Lowpole<f64, U1>,
}

impl LowChannel {
    fn new(seed: u64, cutoff: f32) -> Self {
        let mut noise = Noise::new();
        noise.set_hash(seed);
        Self {
            noise,
            pink: Pinkpass::new(),
            // Leaky integration gives brown-like low-band noise without
            // an unbounded random walk or f32 pole cancellation at high rates.
            brown: Lowpole::new(60.0),
            air_low: Lowpole::new(800.0),
            low_modes: [Resonator::new(86.0, 2.2), Resonator::new(143.0, 2.2)],
            filter: Lowpole::new(cutoff as f64),
        }
    }

    fn render(&mut self, low_mix: f32, low_level: f32, air_level: f32) -> f32 {
        let white = self.noise.tick(&Default::default())[0];
        let pink = self.pink.tick(&[white].into())[0];
        let brown = self.brown.tick(&[pink].into())[0];
        let first = self.low_modes[0].tick(&[brown].into())[0];
        let second = self.low_modes[1].tick(&[brown].into())[0];
        let low = pink * (1.0 - low_mix) + brown * low_mix + (first * 0.7 + second * 0.3) * 0.20;
        let air = white - self.air_low.tick(&[white].into())[0];
        self.filter
            .tick(&[low * low_level + air * air_level].into())[0]
    }

    fn invalidate_history(&mut self) {
        self.pink.reset();
        self.brown.reset();
        self.air_low.reset();
        for mode in &mut self.low_modes {
            mode.reset();
        }
        self.filter.reset();
    }
}

struct DeepVoice {
    sample_rate: u32,
    dry_delay: Option<VoiceLatencyFrames>,
    contributing: bool,
    random: WorldRandom,
    poisson: Poisson,
    modulation: Modulation,
    depth: KnobRamp,
    air: KnobRamp,
    tone: DepthTone,
    bubble_density: f32,
    low_level: f32,
    air_level: f32,
    breath_intensity: Normalized,
    low: [LowChannel; 2],
    grains: Box<[Bubble]>,
    dc: [DCBlock<f64>; 2],
    reverb: RestartableReverb,
}

impl DeepVoice {
    fn new(config: VoicePreparation) -> Result<Self, VoicePreparationError> {
        if !(8_000..=384_000).contains(&config.sample_rate) {
            return Err(VoicePreparationError::InvalidRate);
        }
        if !(1..=MAX_BUBBLES).contains(&config.grain_capacity) {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let reverb = RestartableReverb::new(config.sample_rate)?;
        let mut random = WorldRandom::new(config.seed);
        let poisson = Poisson::new(&mut random);
        let tone = DepthTone::new(KNOBS[0].initial.get());
        let mut voice = Self {
            sample_rate: config.sample_rate,
            dry_delay: None,
            contributing: true,
            random,
            poisson,
            modulation: Modulation::new(),
            depth: KnobRamp::new(KNOBS[0].initial),
            air: KnobRamp::new(KNOBS[1].initial),
            tone,
            bubble_density: KNOBS[1].initial.get(),
            low_level: 0.035,
            air_level: 0.008 * KNOBS[1].initial.get(),
            breath_intensity: Normalized::ZERO,
            low: std::array::from_fn(|index| {
                LowChannel::new(
                    config.seed.wrapping_add(0x6a09e667f3bcc909 + index as u64),
                    tone.cutoff,
                )
            }),
            grains: (0..config.grain_capacity)
                .map(|index| {
                    Bubble::new(
                        config
                            .seed
                            .wrapping_add(0x510e527fade682d1 + index as u64 * 7919),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            dc: std::array::from_fn(|_| DCBlock::new(8.0)),
            reverb,
        };
        voice.prepare_rate(config.sample_rate)?;
        Ok(voice)
    }

    fn invalidate_history(&mut self) {
        for channel in &mut self.low {
            channel.invalidate_history();
        }
        for grain in &mut self.grains {
            grain.invalidate_history();
        }
        for dc in &mut self.dc {
            dc.reset();
        }
        // Fixed-work invalidation, never clearing sample-rate-sized storage.
        self.reverb.invalidate();
    }
}

impl Voice for DeepVoice {
    fn advance_world(
        &mut self,
        frame: VoiceFrame,
        knobs: &[Normalized],
        events: &mut VisualWriter<'_>,
    ) {
        self.depth.advance(knobs[0], frame.delta_seconds);
        self.air.advance(knobs[1], frame.delta_seconds);
        for grain in &mut self.grains {
            grain.advance(frame.delta_seconds);
        }
        let cadence = self
            .modulation
            .advance(frame.delta_seconds, &mut self.random);
        let breath = (self.modulation.breath() * 6.0).clamp(-1.0, 1.0);
        let weather = (self.modulation.weather() * 16.0).clamp(-1.0, 1.0);
        let depth = self.depth.value();
        let air = self.air.value();
        self.bubble_density = air + air * (1.0 - air) * weather * 0.08;
        self.air_level = air * 0.008;
        self.low_level = 0.035 * (1.0 + breath * 0.12);
        self.breath_intensity =
            Normalized::new((1.0 - depth * 0.55) * (0.45 + breath * 0.08) * (0.85 + air * 0.15))
                .expect("bounded deep breath intensity");
        if cadence {
            // Exp/pow and filter coefficient changes at 50 Hz, not each sample.
            let drift_depth = depth + depth * (1.0 - depth) * weather * 0.04;
            self.tone = DepthTone::new(drift_depth);
            for channel in &mut self.low {
                channel.filter.set_cutoff(self.tone.cutoff as f64);
            }
        }
        if !self.contributing {
            // World ages, ramps, breath/weather and world PRNG continue;
            // Poisson admissions pause, audio noise/DSP do not run.
            return;
        }
        let births = self.poisson.births(
            3.6 * self.bubble_density as f64,
            frame.delta_seconds,
            &mut self.random,
        );
        for _ in 0..births {
            let Some(grain) = self.grains.iter_mut().find(|grain| !grain.alive()) else {
                break;
            };
            let x = self.random.unit() as f32;
            let y = self.random.unit() as f32;
            let strength = 0.35 + self.random.unit() as f32 * 0.55;
            let scale = 0.85 + self.random.unit() as f32 * 0.30;
            grain.start(self.tone, strength, x, y, scale, self.sample_rate);
            // Select stronger admitted bubbles BEFORE depth dimming. The same
            // physical pan, dimmed strength, radius and slowed lifetime drive
            // sound and SlowGlow; visual capacity never feeds back into audio.
            if strength >= 0.68 {
                events.emit(ParticleVisual {
                    x: Normalized::new(x).expect("bounded bubble x"),
                    y: Normalized::new(grain.y).expect("bounded bubble y"),
                    strength: Normalized::new(grain.strength).expect("bounded bubble strength"),
                    radius: LogicalPixels::new(grain.radius).expect("finite bubble radius"),
                    shape: VISUAL.shape,
                    lifetime: Duration::from_secs_f64(grain.lifetime),
                });
            }
        }
    }

    fn render_sample(&mut self) -> [f32; 2] {
        if !self.contributing {
            return [0.0; 2];
        }
        let mut dry = self
            .low
            .each_mut()
            .map(|channel| channel.render(self.tone.low_mix, self.low_level, self.air_level));
        for grain in &mut self.grains {
            let sample = grain.render();
            dry[0] += sample[0];
            dry[1] += sample[1];
        }
        for (sample, dc) in dry.iter_mut().zip(&mut self.dc) {
            *sample = dc.tick(&[*sample].into())[0];
        }
        let wet = self.reverb.tick(dry);
        [dry[0] + wet[0] * 0.12, dry[1] + wet[1] * 0.12]
    }

    fn set_contributing(&mut self, active: bool) {
        if self.contributing != active {
            self.invalidate_history();
            self.contributing = active;
        }
    }

    fn breath_intensity(&self) -> Normalized {
        self.breath_intensity
    }

    fn prepared_delay(&self) -> Result<VoiceLatencyFrames, VoicePreparationError> {
        self.dry_delay
            .ok_or(VoicePreparationError::UnresolvedCausalDelay)
    }

    fn prepare_rate(
        &mut self,
        sample_rate: u32,
    ) -> Result<VoiceLatencyFrames, VoicePreparationError> {
        if !(8_000..=384_000).contains(&sample_rate) {
            return Err(VoicePreparationError::InvalidRate);
        }
        let reverb = if sample_rate != self.sample_rate {
            Some(RestartableReverb::new(sample_rate)?)
        } else {
            None
        };
        // Pinned scalar filters route input causal latency with +0. Verify
        // every serial/parallel dry branch instead of guessing from group delay.
        // Noise has no lookahead, and querying its source-only route is not an
        // input-latency proof. Routing analysis/allocate/reset are worker-only.
        fn prepare_immediate<N: AudioNode>(
            node: &mut N,
            rate: u32,
        ) -> Result<(), VoicePreparationError> {
            prepare_node(node, rate);
            if dry_latency(node)? != 0.0 {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            Ok(())
        }
        for channel in &mut self.low {
            prepare_node(&mut channel.noise, sample_rate);
            prepare_immediate(&mut channel.pink, sample_rate)?;
            prepare_immediate(&mut channel.brown, sample_rate)?;
            prepare_immediate(&mut channel.air_low, sample_rate)?;
            for mode in &mut channel.low_modes {
                prepare_immediate(mode, sample_rate)?;
            }
            channel.filter.set_cutoff(self.tone.cutoff as f64);
            prepare_immediate(&mut channel.filter, sample_rate)?;
        }
        for grain in &mut self.grains {
            if !grain.center.is_finite()
                || grain.center <= 0.0
                || grain.center >= sample_rate as f32 * 0.5
                || !grain.bandwidth.is_finite()
                || grain.bandwidth <= 0.0
                || !grain.q.is_finite()
                || grain.q <= 0.0
                || grain.q > 128.0
            {
                return Err(VoicePreparationError::InvalidParameters);
            }
            prepare_node(&mut grain.noise, sample_rate);
            grain
                .resonator
                .set_center_q(grain.center as f64, grain.q as f64);
            prepare_immediate(&mut grain.resonator, sample_rate)?;
        }
        for dc in &mut self.dc {
            prepare_immediate(dc, sample_rate)?;
        }
        self.sample_rate = sample_rate;
        if let Some(reverb) = reverb {
            self.reverb = reverb;
        } else {
            self.reverb.invalidate();
        }
        let delay =
            VoiceLatencyFrames::new(0.0).ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
        self.dry_delay = Some(delay);
        Ok(delay)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        audio::{
            timing::{MachNanos, StreamEpoch, WorldSeconds},
            transport::{VisualRing, VisualWriter},
        },
        controls::Normalized,
        situations::{Registry, VoiceFrame, VoicePreparation},
    };

    #[test]
    fn shallow_deep_air_zero_retains_the_low_sound_and_stops_new_visual_bubbles() {
        let registry = Registry::new().unwrap();
        let index = registry
            .resolve("deep")
            .expect("deep voice must be available");
        let definition = &registry.definitions()[index.get()];
        let rate = 48_000;
        let mut voice = registry
            .prepare_voice(
                index,
                VoicePreparation {
                    sample_rate: rate,
                    seed: 137,
                    grain_capacity: definition.grain_capacity,
                },
            )
            .unwrap();
        voice.set_contributing(true);
        let ring = VisualRing::new(64, registry.definitions().len());
        let epoch = StreamEpoch::new(1).unwrap();
        let knobs = [Normalized::ZERO, Normalized::ZERO];
        let mut live = [None; 64];
        let mut late_energy = 0.0_f64;
        for sample in 0..u64::from(rate) * 6 {
            let seconds = sample as f64 / f64::from(rate);
            let world = WorldSeconds::new(seconds).unwrap();
            let present = MachNanos::new((seconds * 1e9) as u64);
            let mut writer = VisualWriter::new(&ring, epoch, index, world, present);
            voice.advance_world(
                VoiceFrame {
                    world,
                    delta_seconds: 1.0 / f64::from(rate),
                    sample_rate: rate,
                },
                &knobs,
                &mut writer,
            );
            for value in voice.render_sample() {
                assert!(value.is_finite() && value.abs() < 1.0);
                if sample >= u64::from(rate) * 4 {
                    late_energy += f64::from(value).powi(2);
                }
            }
            if sample % 480 == 0 {
                ring.read_live_into(epoch, present, &mut live);
                for event in live.iter().flatten() {
                    // Existing tails may finish; a settled zero must never
                    // create another bubble despite continuing world drift.
                    assert!(event.world.get() <= 0.25);
                }
            }
        }
        let rms = (late_energy / (f64::from(rate) * 2.0 * 2.0)).sqrt();
        assert!(rms > 1e-5, "depth=0 and air=0 must retain low sound: {rms}");
    }
}

#[cfg(test)]
mod behavioral_tests {
    use super::*;
    use crate::{
        audio::{
            timing::{MachNanos, StreamEpoch, WorldSeconds},
            transport::VisualRing,
        },
        situations::SituationIndex,
    };

    fn voice(rate: u32, knobs: [Normalized; 2]) -> DeepVoice {
        let mut voice = DeepVoice::new(VoicePreparation {
            sample_rate: rate,
            seed: 19,
            grain_capacity: MAX_BUBBLES,
        })
        .unwrap();
        voice.depth = KnobRamp::new(knobs[0]);
        voice.air = KnobRamp::new(knobs[1]);
        voice
    }

    fn step(
        voice: &mut DeepVoice,
        knobs: &[Normalized; 2],
        ring: &VisualRing,
        sample: usize,
    ) -> [f32; 2] {
        let seconds = sample as f64 / voice.sample_rate as f64;
        let world = WorldSeconds::new(seconds).unwrap();
        let mut writer = VisualWriter::new(
            ring,
            StreamEpoch::new(1).unwrap(),
            SituationIndex::from_index(0, 1).unwrap(),
            world,
            MachNanos::new((seconds * 1e9) as u64),
        );
        voice.advance_world(
            VoiceFrame {
                world,
                delta_seconds: 1.0 / voice.sample_rate as f64,
                sample_rate: voice.sample_rate,
            },
            knobs,
            &mut writer,
        );
        voice.render_sample()
    }

    #[test]
    fn air_endpoint_retires_bubbles_without_muting_the_independent_low_sound() {
        let ring = VisualRing::new(32, 1);
        let on = [Normalized::ZERO, Normalized::ONE];
        let off = [Normalized::ZERO; 2];
        let mut voice = voice(8_000, on);
        let mut births = 0;
        for sample in 0..16_000 {
            step(&mut voice, &on, &ring, sample);
            births += voice
                .grains
                .iter()
                .filter(|grain| grain.alive() && grain.age == 0.0)
                .count();
        }
        assert!(births > 0);
        // Put a known young real slot through the zero ramp so tail survival
        // does not depend on where this seed's last Poisson arrival happened.
        voice.grains[0].start(DepthTone::new(0.0), 0.9, 0.3, 0.4, 1.0, 8_000);
        for sample in 16_000..17_600 {
            step(&mut voice, &off, &ring, sample);
        }
        assert_eq!(voice.air.value(), 0.0);
        assert_eq!(voice.air_level, 0.0);
        assert!(
            voice.grains.iter().any(Bubble::alive),
            "zero is not a tail cut"
        );
        let mut energy = 0.0_f64;
        let mut live = [None; 32];
        for sample in 17_600..49_600 {
            let ages: [f64; MAX_BUBBLES] = std::array::from_fn(|i| voice.grains[i].age);
            let output = step(&mut voice, &off, &ring, sample);
            assert_eq!(voice.air_level, 0.0);
            assert_eq!(voice.bubble_density, 0.0);
            assert!(
                voice
                    .grains
                    .iter()
                    .zip(ages)
                    .all(|(grain, age)| grain.age >= age)
            );
            if sample > 41_600 {
                energy += output
                    .into_iter()
                    .map(|x| f64::from(x).powi(2))
                    .sum::<f64>();
            }
            if sample % 80 == 0 {
                let present = MachNanos::new((sample as f64 / 8_000.0 * 1e9) as u64);
                ring.read_live_into(StreamEpoch::new(1).unwrap(), present, &mut live);
                assert!(live.iter().flatten().all(|event| event.world.get() < 2.2));
            }
        }
        assert!(
            energy > 1e-6,
            "shallow air-zero retains real low-band output"
        );
        assert!(voice.grains.iter().all(|grain| !grain.alive()));
    }

    #[test]
    fn depth_curves_change_tone_and_actual_light_without_changing_air_frequency() {
        let shallow = DepthTone::new(0.0);
        let halfway = DepthTone::new(0.5);
        let deep = DepthTone::new(1.0);
        assert!(deep.cutoff < shallow.cutoff);
        assert!(deep.low_mix > shallow.low_mix);
        assert!(deep.darkness < shallow.darkness);
        assert!(deep.speed < shallow.speed && deep.speed > 0.0);
        assert!((halfway.cutoff * halfway.cutoff - shallow.cutoff * deep.cutoff).abs() < 0.1);
        assert!((halfway.speed * halfway.speed - shallow.speed * deep.speed).abs() < 1e-6);
        let ring = VisualRing::new(8, 1);
        let shallow_knobs = [Normalized::ZERO, Normalized::ONE];
        let deep_knobs = [Normalized::ONE; 2];
        let mut a = voice(8_000, shallow_knobs);
        let mut b = voice(8_000, deep_knobs);
        let mut births = 0;
        for sample in 0..80_000 {
            step(&mut a, &shallow_knobs, &ring, sample);
            step(&mut b, &deep_knobs, &ring, sample);
            assert_eq!(a.bubble_density, b.bubble_density);
            assert_eq!(a.air_level, b.air_level);
            assert!(a.breath_intensity().get() > b.breath_intensity().get());
            let ga = a
                .grains
                .iter()
                .find(|grain| grain.alive() && grain.age == 0.0);
            let gb = b
                .grains
                .iter()
                .find(|grain| grain.alive() && grain.age == 0.0);
            if let (Some(ga), Some(gb)) = (ga, gb) {
                assert!(ga.strength > gb.strength);
                assert!(ga.lifetime < gb.lifetime);
                assert_eq!(ga.pan, gb.pan);
                births += 1;
            }
        }
        assert!(births > 0);
    }

    #[test]
    fn air_is_linear_and_colored_weather_never_crosses_long_zero_or_one() {
        let ring = VisualRing::new(8, 1);
        let low_knobs = [Normalized::ZERO, Normalized::new(0.25).unwrap()];
        let high_knobs = [Normalized::ZERO, Normalized::ONE];
        let mut low = voice(8_000, low_knobs);
        let mut high = voice(8_000, high_knobs);
        let mut zero = voice(8_000, [Normalized::ZERO; 2]);
        for _ in 0..15_000 {
            zero.modulation.advance(0.02, &mut zero.random);
        }
        assert_ne!(zero.modulation.weather(), 0.0);
        let mut births = [0; 2];
        for sample in 0..160_000 {
            step(&mut low, &low_knobs, &ring, sample);
            step(&mut high, &high_knobs, &ring, sample);
            step(&mut zero, &[Normalized::ZERO; 2], &ring, sample);
            assert_eq!(high.bubble_density, 1.0);
            assert_eq!(zero.bubble_density, 0.0);
            assert_eq!(zero.air_level, 0.0);
            assert!(zero.grains.iter().all(|grain| !grain.alive()));
            assert!((low.air_level * 4.0 - high.air_level).abs() < 1e-8);
            births[0] += low
                .grains
                .iter()
                .filter(|grain| grain.alive() && grain.age == 0.0)
                .count();
            births[1] += high
                .grains
                .iter()
                .filter(|grain| grain.alive() && grain.age == 0.0)
                .count();
        }
        assert!(births[0] > 0 && births[1] > births[0] * 2);
    }

    #[test]
    fn strong_light_describes_the_admitted_bubble_and_ring_size_cannot_change_sound() {
        let tiny_ring = VisualRing::new(1, 1);
        let roomy_ring = VisualRing::new(2048, 1);
        let knobs = [Normalized::ONE; 2];
        let mut tiny = voice(8_000, knobs);
        let mut roomy = voice(8_000, knobs);
        let mut live = [None];
        let mut observed = false;
        let (_, counts) = crate::rt_alloc::measure(|| {
            for sample in 0..80_000 {
                assert_eq!(
                    step(&mut tiny, &knobs, &tiny_ring, sample),
                    step(&mut roomy, &knobs, &roomy_ring, sample)
                );
                assert_eq!(tiny.random.state, roomy.random.state);
                let now = MachNanos::new((sample as f64 / 8_000.0 * 1e9) as u64);
                tiny_ring.read_live_into(StreamEpoch::new(1).unwrap(), now, &mut live);
                if let Some(event) = live[0].filter(|event| event.present == now) {
                    let grain = tiny
                        .grains
                        .iter()
                        .find(|grain| {
                            grain.age == 0.0
                                && grain.alive()
                                && grain.strength == event.strength.get()
                        })
                        .unwrap();
                    let angle = event.x.get() * std::f32::consts::FRAC_PI_2;
                    assert_eq!(grain.pan, [angle.cos(), angle.sin()]);
                    assert_eq!(grain.y, event.y.get());
                    assert_eq!(grain.radius, event.radius.get());
                    assert_eq!(Duration::from_secs_f64(grain.lifetime), event.lifetime);
                    assert_eq!(event.shape, VisualShape::SlowGlow);
                    observed = true;
                }
            }
        });
        assert_eq!(counts, crate::rt_alloc::Counts::default());
        assert!(observed);
    }

    #[test]
    fn full_pool_pitch_extremes_and_large_retargets_are_finite_and_heap_free() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000, 192_000, 384_000] {
            for depth in [Normalized::ZERO, Normalized::ONE] {
                let ring = VisualRing::new(1, 1);
                let knobs = [depth, Normalized::ONE];
                let mut voice = voice(rate, knobs);
                let tone = DepthTone::new(depth.get());
                for grain in &mut voice.grains {
                    grain.start(tone, 1.0, 0.5, 0.5, 1.0, rate);
                }
                let pointer = voice.grains.as_ptr();
                let lifetime = voice.grains[0].lifetime;
                let mut peak = 0.0_f32;
                let mut energy = 0.0_f64;
                let (_, counts) = crate::rt_alloc::measure(|| {
                    for sample in 0..rate as usize / 2 {
                        let target = if sample < rate as usize / 4 {
                            knobs
                        } else {
                            [Normalized::ONE, Normalized::ZERO]
                        };
                        let output = step(&mut voice, &target, &ring, sample);
                        for value in output {
                            assert!(value.is_finite() && value.abs() < 0.9);
                            peak = peak.max(value.abs());
                            energy += f64::from(value).powi(2);
                        }
                        if (sample + 1) as f64 / (rate as f64) < lifetime - 1e-9 {
                            let age = voice.grains[0].age;
                            assert!(
                                voice
                                    .grains
                                    .iter()
                                    .all(|grain| grain.alive() && grain.age == age)
                            );
                        }
                        for grain in &voice.grains {
                            assert!(grain.center > 0.0 && grain.center < rate as f32 * 0.5);
                            assert!(grain.q.is_finite() && grain.q > 0.0 && grain.q <= 128.0);
                        }
                    }
                    voice.set_contributing(false);
                    voice.set_contributing(true);
                });
                assert_eq!(counts, crate::rt_alloc::Counts::default());
                assert_eq!(pointer, voice.grains.as_ptr());
                assert!(peak > 0.0 && energy > 0.0);
            }
        }
    }

    #[test]
    fn bubble_pitch_descends_at_bounded_cadence_and_finite_envelope_reaches_zero() {
        let mut voice = voice(48_000, [Normalized::ZERO; 2]);
        let grain = &mut voice.grains[0];
        grain.start(DepthTone::new(0.0), 0.9, 0.3, 0.4, 1.0, 48_000);
        let initial = grain.center;
        let mut previous = initial;
        let mut energy = 0.0;
        for _ in 0..48_000 {
            grain.advance(1.0 / 48_000.0);
            assert!(grain.center <= previous);
            previous = grain.center;
            energy += grain.render().into_iter().map(f32::abs).sum::<f32>();
        }
        assert!(previous < initial && energy > 0.0);
        assert!(!grain.alive());
        assert_eq!(grain.render(), [0.0; 2]);
    }

    #[test]
    fn inactive_ages_and_rate_preparation_keep_world_but_never_revive_audio_history() {
        let ring = VisualRing::new(8, 1);
        let off = [Normalized::ZERO; 2];
        let mut leaving = voice(48_000, off);
        let mut fresh = voice(48_000, off);
        leaving.grains[0].start(DepthTone::new(0.0), 0.9, 0.3, 0.4, 1.0, 48_000);
        for sample in 0..8_000 {
            step(&mut leaving, &off, &ring, sample);
            step(&mut fresh, &off, &ring, sample);
        }
        let age = leaving.grains[0].age;
        let random = leaving.random.state;
        let breath = leaving.modulation.breath();
        leaving.set_contributing(false);
        fresh.set_contributing(false);
        leaving.set_contributing(true);
        fresh.set_contributing(true);
        assert_eq!(leaving.grains[0].age, age);
        assert!(leaving.grains[0].alive() && !leaving.grains[0].audio_eligible);
        assert_eq!(leaving.random.state, random);
        assert_eq!(leaving.modulation.breath(), breath);
        for sample in 8_000..10_000 {
            assert_eq!(
                step(&mut leaving, &off, &ring, sample),
                step(&mut fresh, &off, &ring, sample)
            );
        }
        leaving.set_contributing(false);
        for sample in 10_000..14_800 {
            assert_eq!(
                step(&mut leaving, &[Normalized::ONE; 2], &ring, sample),
                [0.0; 2]
            );
            assert!(leaving.grains.iter().skip(1).all(|grain| !grain.alive()));
        }
        assert!(leaving.grains[0].age > age);
        assert_ne!(leaving.random.state, random);
        assert!(leaving.depth.value() > 0.0 && leaving.depth.value() < 1.0);
        let age = leaving.grains[0].age;
        let random = leaving.random.state;
        let depth = leaving.depth.value();
        let air = leaving.air.value();
        let breath = leaving.modulation.breath();
        let weather = leaving.modulation.weather();
        let delay = leaving.prepare_rate(8_000).unwrap();
        assert_eq!(delay, leaving.prepared_delay().unwrap());
        assert_eq!(leaving.grains[0].age, age);
        assert_eq!(leaving.random.state, random);
        assert_eq!(leaving.depth.value(), depth);
        assert_eq!(leaving.air.value(), air);
        assert_eq!(leaving.modulation.breath(), breath);
        assert_eq!(leaving.modulation.weather(), weather);
        for sample in 0..800 {
            step(&mut leaving, &[Normalized::ONE; 2], &ring, sample);
        }
        assert_eq!(leaving.depth.value(), 1.0);
        assert_eq!(leaving.air.value(), 1.0);
        leaving.set_contributing(true);
        let mut energy = 0.0;
        let mut births = 0;
        for sample in 800..16_800 {
            energy += step(&mut leaving, &[Normalized::ONE; 2], &ring, sample)
                .into_iter()
                .map(f32::abs)
                .sum::<f32>();
            births += leaving
                .grains
                .iter()
                .filter(|grain| grain.alive() && grain.age == 0.0)
                .count();
        }
        assert!(energy > 0.0 && births > 0);
    }

    #[test]
    fn worker_rejects_unsupported_rates_and_capacity_without_mutating_a_prepared_voice() {
        for rate in [0, 7_999, 384_001, u32::MAX] {
            assert!(matches!(
                DeepVoice::new(VoicePreparation {
                    sample_rate: rate,
                    seed: 0,
                    grain_capacity: MAX_BUBBLES
                }),
                Err(VoicePreparationError::InvalidRate)
            ));
        }
        for capacity in [0, MAX_BUBBLES + 1, usize::MAX] {
            assert!(matches!(
                DeepVoice::new(VoicePreparation {
                    sample_rate: 48_000,
                    seed: 0,
                    grain_capacity: capacity
                }),
                Err(VoicePreparationError::InvalidParameters)
            ));
        }
        let mut voice = voice(48_000, [Normalized::ZERO; 2]);
        let delay = voice.prepared_delay().unwrap();
        for rate in [0, 7_999, 384_001] {
            assert_eq!(
                voice.prepare_rate(rate),
                Err(VoicePreparationError::InvalidRate)
            );
            assert_eq!(voice.sample_rate, 48_000);
            assert_eq!(voice.prepared_delay().unwrap(), delay);
        }
    }
}
