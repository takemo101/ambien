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
        soft_ceiling,
    },
};

// Coefficients and initial values below were settled by user listening (P13); see
// docs/verification.md for what was and was not observed.
static KNOBS: [KnobDefinition; 2] = [
    KnobDefinition {
        id: "depth",
        label: "深さ",
        initial: match Normalized::new(0.68) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
    KnobDefinition {
        id: "air",
        label: "空気の量",
        initial: match Normalized::new(0.16) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
];

// Loudness trim, settled in P13 listening: raw deep measured -46.6 dBFS RMS at 100% app
// volume; this brings it level with the other voices.
const OUTPUT_TRIM: f32 = 4.0;
const MAX_BUBBLES: usize = 24;
/// Bubble strength (before depth dimming) from which a bubble is a large one, drawn at every
/// intensity.
const LARGE_FROM: f32 = 0.68;
const PITCH_SECONDS: f64 = 1.0 / 200.0;
// Bubbles per second at full air (it was 3.6), and the shares of birth events that start a
// cluster of quick bubbles or one big air bubble.
const BUBBLES_PER_SECOND: f64 = 6.0;
const CLUSTER_CHANCE: f64 = 0.30;
const BIG_BUBBLE_CHANCE: f64 = 0.07;
// Bubble voicing: ring bandwidth (Hz, at unit size), upward pitch gain over its life, the
// formation pop, its noise share, and the per-bubble level.
const RING_BANDWIDTH: f32 = 14.0;
const CHIRP: f32 = 0.9;
const POP_SECONDS: f64 = 0.003;
const POP_NOISE: f32 = 0.15;
const BUBBLE_LEVEL: f32 = 0.05;
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
        self.bandwidth = RING_BANDWIDTH / scale;
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
            // Logical pitch advances even with DSP paused. A real bubble's pitch rises as it
            // forms and climbs; bounded, with no per-sample exp/cos coefficient recalculation.
            self.center =
                self.start_center * (1.0 + CHIRP * (self.age / self.lifetime).sqrt() as f32);
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
        let attack = (self.age / 0.004).min(1.0) as f32;
        let onset = attack * attack * (3.0 - 2.0 * attack);
        // A bubble forms with a short smooth pop (half sine over its first milliseconds) and
        // then rings on its own: a clean "bloop", with only a little noise for texture.
        let excitation = if self.age < POP_SECONDS {
            let pop = (std::f64::consts::PI * self.age / POP_SECONDS).sin() as f32;
            let white = self.noise.tick(&Default::default())[0];
            pop + white * pop * POP_NOISE
        } else {
            0.0
        };
        let release = (1.0 - self.age / self.lifetime) as f32;
        let envelope = onset * release * release * (3.0 - 2.0 * release);
        let sample =
            self.resonator.tick(&[excitation].into())[0] * envelope * self.strength * BUBBLE_LEVEL;
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
    // Bubbles still to come in the cluster in progress, and the time to the next one.
    burst_left: u32,
    burst_timer: f64,
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
            burst_left: 0,
            burst_timer: 0.0,
        };
        voice.prepare_rate(config.sample_rate)?;
        Ok(voice)
    }

    /// Starts one bubble in the first free pool slot, publishing the strong ones to the scene.
    /// Returns false when the pool is full.
    fn admit(
        &mut self,
        x: f32,
        y: f32,
        strength: f32,
        scale: f32,
        events: &mut VisualWriter<'_>,
    ) -> bool {
        let Some(grain) = self.grains.iter_mut().find(|grain| !grain.alive()) else {
            return false;
        };
        grain.start(self.tone, strength, x, y, scale, self.sample_rate);
        // Every admitted bubble is published; the large ones are chosen BEFORE depth dimming and
        // marked, so depth never changes which bubbles show at the calm level. The same physical
        // pan, dimmed strength, radius and slowed lifetime drive sound and SlowGlow; visual
        // capacity never feeds back into audio.
        events.emit(ParticleVisual {
            x: Normalized::new(x).expect("bounded bubble x"),
            y: Normalized::new(grain.y).expect("bounded bubble y"),
            strength: Normalized::new(grain.strength).expect("bounded bubble strength"),
            radius: LogicalPixels::new(grain.radius).expect("finite bubble radius"),
            shape: VISUAL.shape,
            large: strength >= LARGE_FROM,
            lifetime: Duration::from_secs_f64(grain.lifetime),
        });
        true
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
        // A cluster in progress: a few quick bubbles one after another, like air being let go.
        // At air zero no new bubble may start, so a pending cluster is dropped, not finished.
        if self.bubble_density <= 0.0 {
            self.burst_left = 0;
        } else if self.burst_left > 0 {
            self.burst_timer -= frame.delta_seconds;
            if self.burst_timer <= 0.0 {
                let x = self.random.unit() as f32;
                let y = self.random.unit() as f32;
                let strength = 0.45 + self.random.unit() as f32 * 0.40;
                let scale = 0.70 + self.random.unit() as f32 * 0.50;
                self.admit(x, y, strength, scale, events);
                self.burst_left -= 1;
                self.burst_timer = 0.05 + self.random.unit() * 0.09;
            }
        }
        let births = self.poisson.births(
            BUBBLES_PER_SECOND * self.bubble_density as f64,
            frame.delta_seconds,
            &mut self.random,
        );
        for _ in 0..births {
            let x = self.random.unit() as f32;
            let y = self.random.unit() as f32;
            let strength = 0.35 + self.random.unit() as f32 * 0.55;
            let scale = 0.85 + self.random.unit() as f32 * 0.30;
            let roll = self.random.unit();
            let more = self.random.unit();
            // Now and then a big air bubble: about a third of the pitch, thrice as long and
            // struck hard, followed by a string of smaller ones rising after it.
            let (strength, scale, followers) = if roll < BIG_BUBBLE_CHANCE {
                (0.92 + strength * 0.08, scale * 3.0, 3 + (more * 4.0) as u32)
            } else if roll < BIG_BUBBLE_CHANCE + CLUSTER_CHANCE {
                (strength, scale, 1 + (more * 3.0) as u32)
            } else {
                (strength, scale, 0)
            };
            if !self.admit(x, y, strength, scale, events) {
                break;
            }
            if followers > 0 && self.burst_left == 0 {
                self.burst_left = followers;
                self.burst_timer = 0.06 + self.random.unit() * 0.08;
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
        [
            soft_ceiling((dry[0] + wet[0] * 0.12) * OUTPUT_TRIM),
            soft_ceiling((dry[1] + wet[1] * 0.12) * OUTPUT_TRIM),
        ]
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
    fn a_pending_bubble_cluster_is_dropped_when_air_is_zero() {
        let ring = VisualRing::new(32, 1);
        let off = [Normalized::ZERO; 2];
        let mut voice = voice(8_000, off);
        for sample in 0..4_000 {
            step(&mut voice, &off, &ring, sample);
        }
        assert_eq!(voice.bubble_density, 0.0);
        voice.burst_left = 4;
        voice.burst_timer = 0.0;
        for sample in 4_000..4_800 {
            step(&mut voice, &off, &ring, sample);
            assert!(
                voice
                    .grains
                    .iter()
                    .all(|grain| !(grain.alive() && grain.age == 0.0)),
                "no bubble may start at air zero"
            );
        }
        assert_eq!(voice.burst_left, 0);
    }

    #[test]
    fn ordinary_loud_play_stays_below_the_soft_ceiling_knee() {
        // The soft ceiling is a guard for pile-ups, not a compressor in normal play.
        let ring = VisualRing::new(32, 1);
        let knobs = [Normalized::ZERO, Normalized::ONE];
        let mut voice = voice(48_000, knobs);
        let mut peak = 0.0_f32;
        for sample in 0..96_000 {
            let frame = step(&mut voice, &knobs, &ring, sample);
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        assert!(peak > 0.0 && peak < 0.5, "peak {peak}");
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
    fn bubble_pitch_rises_at_bounded_cadence_and_finite_envelope_reaches_zero() {
        let mut voice = voice(48_000, [Normalized::ZERO; 2]);
        let grain = &mut voice.grains[0];
        grain.start(DepthTone::new(0.0), 0.9, 0.3, 0.4, 1.0, 48_000);
        let initial = grain.center;
        let mut previous = initial;
        let mut energy = 0.0;
        for _ in 0..48_000 {
            grain.advance(1.0 / 48_000.0);
            assert!(grain.center >= previous);
            previous = grain.center;
            energy += grain.render().into_iter().map(f32::abs).sum::<f32>();
        }
        assert!(previous > initial && energy > 0.0);
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
        assert!(leaving.contributing);
        assert!(
            leaving
                .grains
                .iter()
                .filter(|grain| grain.alive())
                .all(|grain| !grain.audio_eligible)
        );
    }

    #[test]
    fn fresh_bubble_after_reentry_matches_dry_onset_without_low_bed_or_old_tail() {
        let ring = VisualRing::new(8, 1);
        let off = [Normalized::ZERO; 2];
        // Exercise both fixed-storage reentry and worker-prepared rate change.
        for prepared_rate in [None, Some(8_000)] {
            // Dirty bubble/baseline and clean-history bubble/baseline share the
            // exact world and continuous low bed. Only the first pair has heard
            // an old bubble; subtracting their baselines alone could hide its
            // stale tail, so complete outputs must also match the clean pair.
            let mut voices: [DeepVoice; 4] = std::array::from_fn(|_| voice(48_000, off));
            for voice in &mut voices[..2] {
                voice.grains[0].start(DepthTone::new(0.0), 0.9, 0.3, 0.4, 1.0, 48_000);
            }
            let mut old_bubble_peak = 0.0_f32;
            for sample in 0..8_000 {
                let output = voices
                    .each_mut()
                    .map(|voice| step(voice, &off, &ring, sample));
                for (dirty, clean) in output[0].iter().zip(output[2]) {
                    old_bubble_peak = old_bubble_peak.max((dirty - clean).abs());
                }
            }
            assert!(
                old_bubble_peak > 1e-6,
                "fixture must actually dirty audio history"
            );
            let old_age = voices[0].grains[0].age;
            for voice in &mut voices {
                voice.set_contributing(false);
            }
            for sample in 8_000..12_800 {
                for voice in &mut voices {
                    assert_eq!(step(voice, &off, &ring, sample), [0.0; 2]);
                }
            }
            assert!(voices[0].grains[0].age > old_age);
            assert!(voices[0].grains[0].alive());
            for voice in &mut voices {
                if let Some(rate) = prepared_rate {
                    voice.prepare_rate(rate).unwrap();
                }
                voice.set_contributing(true);
            }
            let rate = voices[0].sample_rate;
            let delay = voices[0].prepared_delay().unwrap().get();
            assert_eq!(delay, 0.0);
            let random = voices[0].random.state;
            let tone = voices[0].tone;
            for voice in &voices {
                assert_eq!(voice.random.state, random);
                assert_eq!(voice.modulation.breath(), voices[0].modulation.breath());
                assert_eq!(voice.modulation.weather(), voices[0].modulation.weather());
            }
            assert!(!voices[0].grains[0].audio_eligible);

            // Slot 1 has not consumed excitation noise. A separately prepared
            // real Bubble provides its dry source waveform, including the
            // intentional 4 ms attack, 3 ms pop and rising resonator, without any
            // voice history or low sound that could satisfy the onset check.
            let mut source = voice(rate, off);
            for index in [0, 2] {
                voices[index].grains[1].start(tone, 0.9, 0.3, 0.4, 1.0, rate);
            }
            source.grains[1].start(tone, 0.9, 0.3, 0.4, 1.0, rate);
            let mut dry_dc: [DCBlock<f64>; 2] = std::array::from_fn(|_| DCBlock::new(8.0));
            for dc in &mut dry_dc {
                prepare_node(dc, rate);
            }
            let mut wet_reference = RestartableReverb::new(rate).unwrap();
            assert_eq!(wet_reference.tick([0.0; 2]), [0.0; 2]);
            assert_eq!(
                source.grains[1].render(),
                [0.0; 2],
                "age-zero attack is silent"
            );
            let at_birth = voices.each_mut().map(DeepVoice::render_sample);
            assert_eq!(at_birth[0], at_birth[1]);
            assert_eq!(at_birth[0], at_birth[2]);
            assert_eq!(at_birth[1], at_birth[3]);

            // Very early attack samples can be below f32 low-bed resolution.
            // Match the independent source's resolvable dry-onset frame before
            // wet arrival, then require exact dirty/clean consumer waveforms.
            // Do not compare DC(bubble) to DC(bed+bubble)-DC(bed) amplitudes:
            // the voice rounds bed+bubble to f32 before filtering, so those
            // paths have different quantized inputs despite ideal linearity.
            let onset_threshold = 1e-7_f32;
            let mut source_onset = None;
            let mut raw_dry_onset = None;
            let mut expected_dry_onset = None;
            let mut audible_onset = None;
            let mut wet_onset = None;
            for sample in 1..=rate as usize / 5 {
                source.grains[1].advance(1.0 / f64::from(rate));
                let excitation = source.grains[1].render();
                if excitation.iter().any(|value| *value != 0.0) {
                    source_onset.get_or_insert(sample);
                }
                let expected_dry: [f32; 2] = std::array::from_fn(|channel| {
                    dry_dc[channel].tick(&[excitation[channel]].into())[0]
                });
                if expected_dry.iter().any(|value| *value != 0.0) {
                    raw_dry_onset.get_or_insert(sample);
                }
                let expected_wet = wet_reference.tick(expected_dry);
                if expected_wet.iter().any(|value| *value != 0.0) {
                    wet_onset.get_or_insert(sample);
                }
                let output = voices
                    .each_mut()
                    .map(|voice| step(voice, &off, &ring, 12_800 + sample));
                assert_eq!(
                    output[0], output[2],
                    "old history polluted fresh output at {sample}"
                );
                assert_eq!(
                    output[1], output[3],
                    "old history polluted low bed at {sample}"
                );
                assert_eq!(voices[0].random.state, voices[2].random.state);
                // The onset search is about timing; compare at the voice's untrimmed scale.
                let bubble: [f64; 2] = std::array::from_fn(|channel| {
                    (f64::from(output[0][channel]) - f64::from(output[1][channel]))
                        / f64::from(OUTPUT_TRIM)
                });
                if wet_onset.is_none() {
                    if expected_dry
                        .iter()
                        .any(|value| value.abs() > onset_threshold)
                    {
                        expected_dry_onset.get_or_insert(sample);
                    }
                    if bubble
                        .iter()
                        .any(|value| value.abs() > f64::from(onset_threshold))
                    {
                        audible_onset.get_or_insert(sample);
                    }
                }
            }
            assert_eq!(
                source_onset,
                Some(1),
                "only age-zero envelope silence precedes excitation"
            );
            assert_eq!(raw_dry_onset, source_onset);
            let expected_dry_onset =
                expected_dry_onset.expect("real bubble must clear the low-bed resolution");
            assert_eq!(
                audible_onset,
                Some(expected_dry_onset),
                "fresh dry onset acquired extra delay; declared {delay} frames"
            );
            assert!(
                wet_onset.expect("real reverb must receive the new bubble") > expected_dry_onset,
                "wet predelay must not be mistaken for dry causal delay"
            );
            assert!(source.grains[1].center > source.grains[1].start_center);
            assert!(voices[0].grains.iter().skip(2).all(|grain| !grain.alive()));
        }
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
