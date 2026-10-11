use std::time::Duration;

use fundsp::{
    audionode::AudioNode,
    biquad::Resonator,
    filter::{DCBlock, Lowpole, Pinkpass},
    noise::Noise,
    prelude32::{U2, U3},
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
static KNOBS: [KnobDefinition; 3] = [
    KnobDefinition {
        id: "amount",
        label: "岩の量",
        initial: match Normalized::new(0.27) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
    KnobDefinition {
        id: "size",
        label: "大きさ",
        initial: match Normalized::new(0.61) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
    KnobDefinition {
        id: "hardness",
        label: "硬さ",
        initial: match Normalized::new(0.18) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
];

// Loudness trim, settled in P13 listening. The raw output is quiet; the soft ceiling keeps up
// to 24 simultaneous maximal collisions under 0.9, so the trim is not bounded by peaks.
const OUTPUT_TRIM: f32 = 6.0;
const MAX_COLLISIONS: usize = 24;
/// Collision strength from which an impact is a large one, drawn at every intensity.
const LARGE_FROM: f32 = 0.68;
const VISUAL: VisualStyle = VisualStyle {
    color: [0.65, 0.65, 0.65],
    shape: VisualShape::Impact,
};
// Bar-like inharmonic partials: the ring of struck stone/ore, not a tuned tone.
const MODE_RATIOS: [f32; 3] = [1.0, 2.76, 5.40];
const MODE_WEIGHTS: [f32; 3] = [0.16, 0.30, 0.54];

pub(super) fn definition() -> SituationDefinition {
    SituationDefinition {
        id: "rock",
        label: "岩",
        knobs: &KNOBS,
        visual: VISUAL,
        grain_capacity: MAX_COLLISIONS,
        prepare: |config| Ok(Box::new(RockVoice::new(config)?)),
    }
}

#[derive(Clone, Copy)]
struct CollisionTone {
    center: f32,
    lifetime: f64,
    attack: f64,
    high_mix: f32,
    radius: f32,
    level: f32,
}

impl CollisionTone {
    fn new(size: f32, hardness: f32) -> Self {
        Self {
            center: 360.0 * (115.0_f32 / 360.0).powf(size),
            lifetime: 0.30
                * (0.95_f64 / 0.30).powf(size as f64)
                * 1.25
                * (0.58_f64 / 1.25).powf(hardness as f64),
            attack: 0.004 * (0.0008_f64 / 0.004).powf(hardness as f64),
            high_mix: 0.08 + hardness * 0.30,
            radius: 2.0 + size * 4.0,
            // Faster attack/high-band excitation is compensated rather than
            // turning hardness into an output volume or surprise transient.
            level: 0.075 / (1.0 + hardness * 0.65),
        }
    }
}

struct Collision {
    noise: Noise,
    excitation_filter: Lowpole<f32, U2>,
    // f64 pole arithmetic avoids low-frequency coefficient cancellation at
    // the supported high sample rates. Tick input/output remain f32.
    resonators: [Resonator<f64, U3>; 3],
    age: f64,
    lifetime: f64,
    attack: f64,
    strength: f32,
    high_mix: f32,
    level: f32,
    centers: [f32; 3],
    bandwidths: [f32; 3],
    q: [f32; 3],
    pan: [f32; 2],
    y: f32,
    radius: f32,
    audio_eligible: bool,
    grit_pole: f32,
    grit_gain: f32,
    rough_pole: f32,
    rough_gain: f32,
    grit_state: f32,
    drive: f32,
    rough_state: f32,
    rng: u64,
}

// Grit layer of a collision: share of the lifetime it lasts, its level relative to the
// resonator body, and the depth and rate of its roughness jitter.
const GRIT_SPAN: f64 = 0.3;
const GRIT_LEVEL: f32 = 0.025;
// Share of collisions that are a big, low, dull stone.
const BOULDER_CHANCE: f64 = 0.06;
const ROUGH_DEPTH: f32 = 1.0;
// Strike drive: contact pulse amplitude into the resonators, and the noise share.
const PULSE_GAIN: f32 = 0.5;
const REF_CENTER: f32 = 160.0;
const REF_LIFETIME: f32 = 0.5;
const CRACKLE_LEVEL: f32 = 0.04;
const ROUGH_HZ: f32 = 110.0;

// Strike-only room: wet share, 60 dB decay time and delay-size stretch. A big, hard room so
// every strike hangs in the air for seconds.
const STRIKE_REVERB: f32 = 1.1;
const ROOM_DECAY_SECONDS: f64 = 3.6;
const ROOM_SIZE: f64 = 1.8;
// Collisions per second at full amount (it was 3.2 when strikes were too sparse).
const COLLISIONS_PER_SECOND: f64 = 8.0;

fn strike_room(rate: u32) -> Result<RestartableReverb, VoicePreparationError> {
    RestartableReverb::with_room(rate, ROOM_DECAY_SECONDS, ROOM_SIZE)
}

/// One-pole smoothing coefficient for a cutoff, clamped below Nyquist.
fn pole(cutoff: f32, rate: u32) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff.min(rate as f32 * 0.4) / rate as f32).exp()
}

/// Standard deviation of uniform white noise (-1..1) after this one-pole, as the reciprocal
/// normaliser.
fn unit_spread(pole: f32) -> f32 {
    (1.0_f32 / 3.0).sqrt() * (pole / (2.0 - pole)).sqrt()
}

impl Collision {
    fn new(seed: u64) -> Self {
        let mut noise = Noise::new();
        noise.set_hash(seed);
        Self {
            noise,
            excitation_filter: Lowpole::new(650.0),
            resonators: std::array::from_fn(|index| {
                Resonator::new(360.0 * MODE_RATIOS[index] as f64, 12.0)
            }),
            age: 0.0,
            lifetime: 0.0,
            attack: 0.022,
            strength: 0.0,
            high_mix: 0.08,
            level: 0.075,
            centers: MODE_RATIOS.map(|ratio| ratio * 360.0),
            bandwidths: [30.0; 3],
            q: MODE_RATIOS.map(|ratio| ratio * 12.0),
            grit_pole: 0.0,
            grit_gain: 0.0,
            rough_pole: 0.0,
            rough_gain: 0.0,
            grit_state: 0.0,
            drive: 0.0,
            rough_state: 0.0,
            rng: seed | 1,
            pan: [0.0; 2],
            y: 0.0,
            radius: 2.0,
            audio_eligible: false,
        }
    }

    fn alive(&self) -> bool {
        self.age < self.lifetime
    }

    fn start(&mut self, tone: CollisionTone, strength: f32, x: f32, y: f32, scale: f32, rate: u32) {
        self.age = 0.0;
        self.lifetime = tone.lifetime * scale as f64;
        self.attack = tone.attack;
        self.strength = strength;
        self.high_mix = tone.high_mix;
        self.level = tone.level;
        self.radius = tone.radius * scale;
        self.y = y;
        self.audio_eligible = true;
        let angle = x * std::f32::consts::FRAC_PI_2;
        self.pan = [angle.cos(), angle.sin()];
        for (index, ratio) in MODE_RATIOS.iter().copied().enumerate() {
            self.centers[index] = (tone.center / scale * ratio).min(rate as f32 * 0.40);
            self.bandwidths[index] =
                (3.0 + 2.0 / self.lifetime as f32) * (1.0 + index as f32 * 0.5);
            // FunDSP's second Resonator input is Q, NOT bandwidth Hz.
            self.q[index] = self.centers[index] / self.bandwidths[index];
            self.resonators[index].reset();
        }
        // Only fixed scalar histories are cleared at admission. Noise phase
        // and the independent world PRNG are never reset in the callback.
        self.excitation_filter.reset();
        // Hand-written scalar one-poles (no node preparation): the grit body sits just
        // above the strike pitch, its roughness jitters at about 110 Hz. Gains normalise the
        // filtered noise to a fixed spread at any sample rate.
        let grit_pole = pole(self.centers[0] * 1.2, rate);
        self.grit_pole = grit_pole;
        self.grit_gain = GRIT_LEVEL / unit_spread(grit_pole);
        self.rough_pole = pole(ROUGH_HZ, rate);
        self.rough_gain = ROUGH_DEPTH / unit_spread(self.rough_pole);
        self.grit_state = 0.0;
        self.rough_state = 0.0;
        // Equal contact momentum at every size: a lower body rings with a proportionally
        // smaller amplitude for the same momentum, so the pulse amplitude falls with pitch
        // and the first mode keeps its level. Hardness only changes the colour.
        // Long-ringing collisions overlap and add up, so the drive also falls with ring length.
        self.drive = PULSE_GAIN
            * (REF_CENTER / self.centers[0]).sqrt()
            * (REF_LIFETIME / self.lifetime as f32).powf(0.75);
    }

    fn advance(&mut self, delta: f64) {
        if self.alive() {
            self.age = (self.age + delta).min(self.lifetime);
        }
    }

    fn render(&mut self) -> [f32; 2] {
        if !self.alive() || !self.audio_eligible {
            return [0.0; 2];
        }
        let excitation_seconds = self.attack + 0.012;
        let grit_seconds = self.lifetime * GRIT_SPAN;
        let white = if self.age < excitation_seconds || self.age < grit_seconds {
            self.noise.tick(&Default::default())[0]
        } else {
            0.0
        };
        let excitation = if self.age < excitation_seconds {
            let soft = self.excitation_filter.tick(&[white, 650.0].into())[0];
            // A struck body is driven by a short, smooth contact pulse (half sine over the
            // contact time), not by a noise burst: that gives the thump of a hit instead of a
            // puff. A little filtered noise on top only roughens the contact.
            let contact = (self.age / self.attack).min(1.0) as f32;
            let pulse = (std::f32::consts::PI * contact).sin();
            let release = (1.0 - self.age / excitation_seconds) as f32;
            let crackle = (soft + (white - soft) * self.high_mix) * release * release;
            (pulse + crackle * CRACKLE_LEVEL * (1.0 - (contact - 0.5).abs() * 2.0)) * self.drive
        } else {
            0.0
        };
        let mut sample = 0.0;
        for (index, resonator) in self.resonators.iter_mut().enumerate() {
            sample += resonator.tick(&[excitation, self.centers[index], self.q[index]].into())[0]
                * MODE_WEIGHTS[index];
        }
        // A finite, slope-zero envelope makes the logical pool slot reusable
        // without discontinuously truncating an audible resonator.
        let release = (1.0 - self.age / self.lifetime) as f32;
        let envelope = release * release * (3.0 - 2.0 * release);
        sample *= envelope;
        // Grinding: low-passed noise whose loudness itself jitters at a few tens of hertz, so
        // the body of the impact scrapes and grates instead of only ringing. It ends well
        // before the resonators so the tail is the clean ring.
        if self.age < grit_seconds {
            self.grit_state += (white - self.grit_state) * self.grit_pole;
            self.rng ^= self.rng >> 12;
            self.rng ^= self.rng << 25;
            self.rng ^= self.rng >> 27;
            let bits = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d);
            let random = (bits >> 40) as f32 * (2.0 / 16_777_216.0) - 1.0;
            self.rough_state += (random - self.rough_state) * self.rough_pole;
            let rough = (self.rough_state * self.rough_gain).abs().min(1.0);
            let onset = (self.age / self.attack).min(1.0) as f32;
            let fade = 1.0 - (self.age / grit_seconds) as f32;
            sample += self.grit_state * self.grit_gain * rough * onset * fade * fade;
        }
        sample *= self.strength * self.level;
        [sample * self.pan[0], sample * self.pan[1]]
    }

    fn invalidate_history(&mut self) {
        self.audio_eligible = false;
        self.excitation_filter.reset();
        self.grit_state = 0.0;
        self.rough_state = 0.0;
        for resonator in &mut self.resonators {
            resonator.reset();
        }
    }
}

struct RollingChannel {
    noise: Noise,
    gate_noise: Noise,
    pink: Pinkpass<f32>,
    filter: Lowpole<f32, U2>,
    resonators: [Resonator<f64, U3>; 2],
}

impl RollingChannel {
    fn new(seed: u64) -> Self {
        let mut noise = Noise::new();
        noise.set_hash(seed);
        let mut gate_noise = Noise::new();
        gate_noise.set_hash(seed.wrapping_add(0xbb67ae8584caa73b));
        Self {
            noise,
            gate_noise,
            pink: Pinkpass::new(),
            filter: Lowpole::new(420.0),
            resonators: [Resonator::new(240.0, 3.0), Resonator::new(390.0, 3.0)],
        }
    }

    fn render(&mut self, density: f32, center: f32, cutoff: f32, level: f32) -> f32 {
        // Density is linear in amount; gate/noise are DSP-only independent
        // generators. At exact zero there is no new rolling excitation, but
        // already excited filter/modal tails continue to tick to silence.
        let input = if density > 0.0
            && (self.gate_noise.tick(&Default::default())[0] + 1.0) * 0.5 < density
        {
            self.noise.tick(&Default::default())[0]
        } else {
            0.0
        };
        let pink = self.pink.tick(&[input].into())[0];
        let colored = self
            .filter
            .tick(&[0.35 * input + 0.65 * pink, cutoff].into())[0];
        let first = self.resonators[0].tick(&[colored, center, 3.0].into())[0];
        let second = self.resonators[1].tick(&[colored, center * 1.63, 3.0].into())[0];
        (first * 0.75 + second * 0.25) * level
    }

    fn invalidate_history(&mut self) {
        self.pink.reset();
        self.filter.reset();
        for resonator in &mut self.resonators {
            resonator.reset();
        }
    }
}

struct RockVoice {
    sample_rate: u32,
    dry_delay: Option<VoiceLatencyFrames>,
    contributing: bool,
    random: WorldRandom,
    poisson: Poisson,
    modulation: Modulation,
    amount: KnobRamp,
    size: KnobRamp,
    hardness: KnobRamp,
    tone: CollisionTone,
    rolling_density: f32,
    rolling_center: f32,
    rolling_cutoff: f32,
    rolling_level: f32,
    breath_intensity: Normalized,
    rolling: [RollingChannel; 2],
    grains: Box<[Collision]>,
    dc: [DCBlock<f32>; 2],
    reverb: RestartableReverb,
}

impl RockVoice {
    fn new(config: VoicePreparation) -> Result<Self, VoicePreparationError> {
        if !(1..=MAX_COLLISIONS).contains(&config.grain_capacity) {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let reverb = strike_room(config.sample_rate)?;
        let mut random = WorldRandom::new(config.seed);
        let poisson = Poisson::new(&mut random);
        let tone = CollisionTone::new(KNOBS[1].initial.get(), KNOBS[2].initial.get());
        let mut voice = Self {
            sample_rate: config.sample_rate,
            dry_delay: None,
            contributing: true,
            random,
            poisson,
            modulation: Modulation::new(),
            amount: KnobRamp::new(KNOBS[0].initial),
            size: KnobRamp::new(KNOBS[1].initial),
            hardness: KnobRamp::new(KNOBS[2].initial),
            tone,
            rolling_density: 0.0,
            rolling_center: tone.center,
            rolling_cutoff: 420.0,
            rolling_level: 0.05,
            breath_intensity: Normalized::ZERO,
            rolling: std::array::from_fn(|index| {
                RollingChannel::new(config.seed.wrapping_add(0x6a09e667f3bcc909 + index as u64))
            }),
            grains: (0..config.grain_capacity)
                .map(|index| {
                    Collision::new(
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
        for channel in &mut self.rolling {
            channel.invalidate_history();
        }
        for grain in &mut self.grains {
            grain.invalidate_history();
        }
        for dc in &mut self.dc {
            dc.reset();
        }
        self.reverb.invalidate();
    }
}

impl Voice for RockVoice {
    fn advance_world(
        &mut self,
        frame: VoiceFrame,
        knobs: &[Normalized],
        events: &mut VisualWriter<'_>,
    ) {
        self.amount.advance(knobs[0], frame.delta_seconds);
        self.size.advance(knobs[1], frame.delta_seconds);
        self.hardness.advance(knobs[2], frame.delta_seconds);
        for grain in &mut self.grains {
            grain.advance(frame.delta_seconds);
        }
        let cadence = self
            .modulation
            .advance(frame.delta_seconds, &mut self.random);
        let breath = (self.modulation.breath() * 6.0).clamp(-1.0, 1.0);
        let weather = (self.modulation.weather() * 16.0).clamp(-1.0, 1.0);
        let amount = self.amount.value();
        // Weather cannot cross either endpoint, particularly the no-birth gate.
        let density = amount + amount * (1.0 - amount) * weather * 0.08;
        self.rolling_density = density * 0.18;
        self.rolling_level = 0.05 * (1.0 + breath * 0.12) / (1.0 + self.hardness.value() * 0.30);
        self.breath_intensity =
            Normalized::new(density * (0.45 + breath * 0.08)).expect("bounded breath intensity");
        if cadence {
            self.tone = CollisionTone::new(self.size.value(), self.hardness.value());
            self.rolling_center = self.tone.center;
            self.rolling_cutoff = 320.0 + self.hardness.value() * 560.0 + breath * 30.0;
        }
        // Zero contribution is not mute: retain logical ages/modulation and
        // Poisson hazard, but do not manufacture inaudible particle births.
        if !self.contributing {
            return;
        }
        let births = self.poisson.births(
            COLLISIONS_PER_SECOND * density as f64,
            frame.delta_seconds,
            &mut self.random,
        );
        for _ in 0..births {
            let Some(grain) = self.grains.iter_mut().find(|grain| !grain.alive()) else {
                break;
            };
            let x = self.random.unit() as f32;
            let y = self.random.unit() as f32;
            let strength = 0.30 + self.random.unit() as f32 * 0.60;
            let scale = 0.85 + self.random.unit() as f32 * 0.30;
            // Now and then a big stone: about 0.4x the pitch, 2.5x as long, struck hard. The
            // roll is always drawn so the world random sequence does not depend on it.
            let boulder = self.random.unit() < BOULDER_CHANCE;
            let (strength, scale) = if boulder {
                (0.92 + strength * 0.08, 2.0 + scale * 0.5)
            } else {
                (strength, scale)
            };
            grain.start(self.tone, strength, x, y, scale, frame.sample_rate);
            // Admission and every physical choice precede visual publication. Every collision is
            // published; stronger ones are marked large and share the actual audio pan, scale and
            // lifetime. Ring overwrite cannot change sound or the world random sequence.
            events.emit(ParticleVisual {
                x: Normalized::new(x).expect("bounded collision x"),
                y: Normalized::new(grain.y).expect("bounded collision y"),
                strength: Normalized::new(grain.strength).expect("bounded collision strength"),
                radius: LogicalPixels::new(grain.radius).expect("finite collision radius"),
                shape: VISUAL.shape,
                large: strength >= LARGE_FROM,
                lifetime: Duration::from_secs_f64(grain.lifetime),
            });
        }
    }

    fn render_sample(&mut self) -> [f32; 2] {
        if !self.contributing {
            return [0.0; 2];
        }
        let mut dry = self.rolling.each_mut().map(|channel| {
            channel.render(
                self.rolling_density,
                self.rolling_center,
                self.rolling_cutoff,
                self.rolling_level,
            )
        });
        // Only the strikes go into the room; the rolling bed stays dry.
        let mut strikes = [0.0_f32; 2];
        for grain in &mut self.grains {
            let sample = grain.render();
            strikes[0] += sample[0];
            strikes[1] += sample[1];
        }
        dry[0] += strikes[0];
        dry[1] += strikes[1];
        for (sample, dc) in dry.iter_mut().zip(&mut self.dc) {
            *sample = dc.tick(&[*sample].into())[0];
        }
        let wet = self.reverb.tick(strikes);
        [
            soft_ceiling((dry[0] + wet[0] * STRIKE_REVERB) * OUTPUT_TRIM),
            soft_ceiling((dry[1] + wet[1] * STRIKE_REVERB) * OUTPUT_TRIM),
        ]
    }

    fn set_contributing(&mut self, active: bool) {
        if self.contributing != active {
            // Permanently revoke current grains' audio eligibility; a short
            // leave/reenter cannot resume the rest of their excitation window.
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
            Some(strike_room(sample_rate)?)
        } else {
            None
        };
        // Worker-owned preparation only. Routing analysis allocates; callback
        // uses statically typed nodes and tick, never process/graph preparation.
        for dc in &mut self.dc {
            prepare_node(dc, sample_rate);
        }
        let dc_delay = dry_latency(&mut self.dc[0])?;
        if dry_latency(&mut self.dc[1])? != dc_delay {
            return Err(VoicePreparationError::UnresolvedCausalDelay);
        }
        let mut verified_delay = None;
        for channel in &mut self.rolling {
            prepare_node(&mut channel.noise, sample_rate);
            prepare_node(&mut channel.gate_noise, sample_rate);
            prepare_node(&mut channel.pink, sample_rate);
            channel.filter.set_cutoff(self.rolling_cutoff);
            prepare_node(&mut channel.filter, sample_rate);
            let pink_delay = dry_latency(&mut channel.pink)?;
            let filter_delay = dry_latency(&mut channel.filter)?;
            // White and pink run in parallel before the common lowpass.
            if pink_delay != 0.0 {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            for (index, resonator) in channel.resonators.iter_mut().enumerate() {
                let center = self.rolling_center * if index == 0 { 1.0 } else { 1.63 };
                resonator.set_center_q(center as f64, 3.0);
                prepare_node(resonator, sample_rate);
                let delay = filter_delay + dry_latency(resonator)? + dc_delay;
                if verified_delay.is_some_and(|previous| previous != delay) {
                    return Err(VoicePreparationError::UnresolvedCausalDelay);
                }
                verified_delay = Some(delay);
            }
        }
        for grain in &mut self.grains {
            prepare_node(&mut grain.noise, sample_rate);
            prepare_node(&mut grain.excitation_filter, sample_rate);
            // Soft and high-band excitation are parallel paths.
            if dry_latency(&mut grain.excitation_filter)? != 0.0 {
                return Err(VoicePreparationError::UnresolvedCausalDelay);
            }
            for index in 0..3 {
                let center = grain.centers[index];
                let bandwidth = grain.bandwidths[index];
                let q = center / bandwidth;
                if !center.is_finite()
                    || center <= 0.0
                    || center >= sample_rate as f32 * 0.5
                    || !bandwidth.is_finite()
                    || bandwidth <= 0.0
                    || !q.is_finite()
                    || q <= 0.0
                    || q > 128.0
                {
                    return Err(VoicePreparationError::InvalidParameters);
                }
                grain.q[index] = q;
                grain.resonators[index].set_center_q(center as f64, q as f64);
                prepare_node(&mut grain.resonators[index], sample_rate);
                let delay = dry_latency(&mut grain.resonators[index])? + dc_delay;
                if verified_delay != Some(delay) {
                    return Err(VoicePreparationError::UnresolvedCausalDelay);
                }
            }
        }
        self.sample_rate = sample_rate;
        if let Some(reverb) = reverb {
            self.reverb = reverb;
        } else {
            self.reverb.invalidate();
        }
        let delay = verified_delay
            .and_then(VoiceLatencyFrames::new)
            .ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
        self.dry_delay = Some(delay);
        Ok(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::{
            timing::{MachNanos, StreamEpoch, WorldSeconds},
            transport::VisualRing,
        },
        situations::SituationIndex,
    };

    fn voice(rate: u32, knobs: [Normalized; 3]) -> RockVoice {
        let mut voice = RockVoice::new(VoicePreparation {
            sample_rate: rate,
            seed: 19,
            grain_capacity: MAX_COLLISIONS,
        })
        .unwrap();
        voice.amount = KnobRamp::new(knobs[0]);
        voice.size = KnobRamp::new(knobs[1]);
        voice.hardness = KnobRamp::new(knobs[2]);
        voice
    }

    fn step(
        voice: &mut RockVoice,
        knobs: &[Normalized; 3],
        ring: &VisualRing,
        sample: usize,
    ) -> [f32; 2] {
        let seconds = sample as f64 / voice.sample_rate as f64;
        let world = WorldSeconds::new(seconds).unwrap();
        let mut events = VisualWriter::new(
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
            &mut events,
        );
        voice.render_sample()
    }

    #[test]
    fn amount_endpoint_stops_births_and_rolling_but_finishes_collision_and_room_tails() {
        let ring = VisualRing::new(16, 1);
        let on = [Normalized::ONE, Normalized::ONE, Normalized::ZERO];
        let off = [Normalized::ZERO, Normalized::ONE, Normalized::ZERO];
        let mut voice = voice(48_000, on);
        let mut newborn = None;
        for sample in 0..96_000 {
            step(&mut voice, &on, &ring, sample);
            if voice
                .grains
                .iter()
                .any(|grain| grain.age == 0.0 && grain.alive())
            {
                newborn = Some(sample);
                break;
            }
        }
        let start = newborn.expect("amount one admits collisions");
        for sample in start + 1..=start + 9_600 {
            step(&mut voice, &off, &ring, sample);
        }
        assert_eq!(voice.amount.value(), 0.0);
        assert_eq!(voice.rolling_density, 0.0);
        assert!(voice.grains.iter().any(Collision::alive));
        let mut early_energy = 0.0;
        let mut late_energy = 0.0;
        for offset in 0..384_000 {
            let ages: [f64; MAX_COLLISIONS] = std::array::from_fn(|index| voice.grains[index].age);
            let frame = step(&mut voice, &off, &ring, start + 9_601 + offset);
            assert_eq!(voice.rolling_density, 0.0);
            assert!(
                voice
                    .grains
                    .iter()
                    .zip(ages)
                    .all(|(grain, age)| grain.age >= age)
            );
            let energy = frame[0].abs() + frame[1].abs();
            if offset < 4_800 {
                early_energy += energy;
            }
            if offset >= 379_200 {
                late_energy += energy;
            }
        }
        assert!(early_energy > 0.0, "amount is not a tail-cutting mute");
        assert!(late_energy < early_energy * 1e-4);
        assert!(voice.grains.iter().all(|grain| !grain.alive()));
    }

    #[test]
    fn ordinary_loud_play_stays_below_the_soft_ceiling_knee() {
        // The soft ceiling is a guard for pile-ups, not a compressor in normal play: if the voice
        // gets louder than this, the sound is being squashed instead of kept clean.
        let ring = VisualRing::new(8, 1);
        let knobs = [Normalized::ONE, Normalized::ONE, Normalized::ZERO];
        let mut voice = voice(48_000, knobs);
        let mut peak = 0.0_f32;
        for sample in 0..96_000 {
            let frame = step(&mut voice, &knobs, &ring, sample);
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        assert!(peak > 0.0 && peak < 0.5, "peak {peak}");
    }

    #[test]
    fn smallest_soft_rock_is_audible_and_zero_amount_cannot_be_revived_by_weather() {
        let ring = VisualRing::new(8, 1);
        let knobs = [Normalized::ONE, Normalized::ZERO, Normalized::ZERO];
        let mut audible = voice(48_000, knobs);
        let mut silent = voice(48_000, [Normalized::ZERO; 3]);
        // Let the real colored weather/breath filters drift for four minutes
        // before checking the exact zero gate; no endpoint gets a fake weather.
        for _ in 0..12_000 {
            silent.modulation.advance(0.02, &mut silent.random);
        }
        assert_ne!(silent.modulation.weather(), 0.0);
        let mut energy = 0.0;
        for sample in 0..96_000 {
            let frame = step(&mut audible, &knobs, &ring, sample);
            energy += frame[0].abs() + frame[1].abs();
            assert_eq!(
                step(&mut silent, &[Normalized::ZERO; 3], &ring, sample),
                [0.0; 2]
            );
            assert_eq!(silent.rolling_density, 0.0);
            assert!(silent.grains.iter().all(|grain| !grain.alive()));
        }
        assert!(energy > 0.0);
    }

    #[test]
    fn amount_changes_collision_density_without_hardness_or_size_muting_births() {
        let ring = VisualRing::new(8, 1);
        let low_knobs = [
            Normalized::new(0.2).unwrap(),
            Normalized::ZERO,
            Normalized::ZERO,
        ];
        let high_knobs = [Normalized::ONE; 3];
        let mut low = voice(8_000, low_knobs);
        let mut high = voice(8_000, high_knobs);
        let mut births = [0; 2];
        for sample in 0..240_000 {
            step(&mut low, &low_knobs, &ring, sample);
            step(&mut high, &high_knobs, &ring, sample);
            births[0] += low
                .grains
                .iter()
                .filter(|grain| grain.age == 0.0 && grain.alive())
                .count();
            births[1] += high
                .grains
                .iter()
                .filter(|grain| grain.age == 0.0 && grain.alive())
                .count();
        }
        assert!(births[0] > 0, "soft smallest collisions still happen");
        assert!(
            births[1] > births[0] * 2,
            "amount controls occurrence, not only gain"
        );
    }

    #[test]
    fn reentry_invalidates_an_already_running_room_tail_without_clearing_world_lifetime() {
        let ring = VisualRing::new(8, 1);
        let knobs = [Normalized::ZERO, Normalized::ONE, Normalized::ZERO];
        let mut reentered = voice(48_000, knobs);
        let mut clean = voice(48_000, knobs);
        reentered.grains[0].start(CollisionTone::new(1.0, 0.0), 0.9, 0.25, 0.4, 1.0, 48_000);
        let mut tail_energy = 0.0;
        for sample in 0..8_000 {
            let frame = step(&mut reentered, &knobs, &ring, sample);
            assert_eq!(step(&mut clean, &knobs, &ring, sample), [0.0; 2]);
            if sample > 7_000 {
                tail_energy += frame[0].abs() + frame[1].abs();
            }
        }
        assert!(tail_energy > 0.0);
        assert_ne!(
            reentered.reverb.tick([0.0; 2]),
            [0.0; 2],
            "the old collision has populated the actual wet room, not only the dry resonators"
        );
        let age = reentered.grains[0].age;
        let random = reentered.random.state;
        reentered.set_contributing(false);
        assert_eq!(reentered.grains[0].age, age);
        assert_eq!(reentered.random.state, random);
        for sample in 8_000..8_256 {
            assert_eq!(step(&mut reentered, &knobs, &ring, sample), [0.0; 2]);
            assert_eq!(step(&mut clean, &knobs, &ring, sample), [0.0; 2]);
        }
        let inactive_age = reentered.grains[0].age;
        let inactive_random = reentered.random.state;
        assert!(inactive_age > age);
        reentered.set_contributing(true);
        assert_eq!(reentered.grains[0].age, inactive_age);
        assert_eq!(reentered.random.state, inactive_random);
        assert_eq!(reentered.random.state, clean.random.state);
        assert!(reentered.grains[0].alive());
        for sample in 8_256..20_000 {
            assert_eq!(step(&mut reentered, &knobs, &ring, sample), [0.0; 2]);
            assert_eq!(step(&mut clean, &knobs, &ring, sample), [0.0; 2]);
        }

        // Measure the real room's impulse predelay separately: it is not the
        // declared dry causal delay and must not postpone the fresh collision.
        let mut room = strike_room(48_000).unwrap();
        let wet_predelay = (0..48_000)
            .find(|&offset| room.tick(if offset == 0 { [1.0; 2] } else { [0.0; 2] }) != [0.0; 2])
            .expect("the actual room eventually returns the scheduled impulse");
        let declared_delay = reentered.prepared_delay().unwrap().get();
        assert_eq!(declared_delay, clean.prepared_delay().unwrap().get());

        // Slot one has the same untouched independent noise phase in both
        // worlds. Only reentered has an old collision/room history; the clean
        // reference has followed the same world clock and PRNG without it.
        // Keep the old slot's age/eligibility intact instead of resetting the
        // world or reseeding a generator to manufacture a matching reference.
        let old_age = reentered.grains[0].age;
        let random = reentered.random.state;
        for voice in [&mut reentered, &mut clean] {
            voice.grains[1].start(CollisionTone::new(1.0, 0.0), 0.9, 0.25, 0.4, 1.0, 48_000);
            // Admission is age zero: the collision's intentional slope-zero
            // attack is silent, independently of its dry routing latency.
            assert_eq!(voice.render_sample(), [0.0; 2]);
        }
        assert_eq!(reentered.grains[0].age, old_age);
        assert_eq!(reentered.random.state, random);
        let mut onset = None;
        for offset in 1..=12_000 {
            let frame = step(&mut reentered, &knobs, &ring, 20_000 + offset - 1);
            let reference = step(&mut clean, &knobs, &ring, 20_000 + offset - 1);
            assert_eq!(
                frame, reference,
                "fresh dry/room output must be time-aligned and free of old tail at offset {offset}"
            );
            if onset.is_none() && frame != [0.0; 2] {
                onset = Some(offset);
            }
        }
        let onset = onset.expect("the explicitly scheduled real collision has an onset");
        // One world tick moves the envelope off its intentional zero. Any
        // further offset must be exactly the prepared dry causal delay, not a
        // post-reentry buffer refill or the room's intentional wet predelay.
        assert_eq!(onset as f64, 1.0 + declared_delay);
        assert!(onset < wet_predelay);
    }

    #[test]
    fn size_and_hardness_change_independent_collision_properties() {
        let small_soft = CollisionTone::new(0.0, 0.0);
        let large_soft = CollisionTone::new(1.0, 0.0);
        let small_hard = CollisionTone::new(0.0, 1.0);
        assert!(large_soft.center < small_soft.center);
        assert!(large_soft.lifetime > small_soft.lifetime);
        assert!(large_soft.radius > small_soft.radius);
        assert_eq!(large_soft.attack, small_soft.attack);
        assert_eq!(large_soft.high_mix, small_soft.high_mix);
        assert_eq!(small_hard.center, small_soft.center);
        assert_eq!(small_hard.radius, small_soft.radius);
        assert!(small_hard.attack < small_soft.attack);
        assert!(small_hard.lifetime < small_soft.lifetime);
        assert!(small_hard.high_mix > small_soft.high_mix);
        assert!(
            small_hard.level < small_soft.level,
            "hard attacks are peak-compensated"
        );
        let halfway = CollisionTone::new(0.5, 0.0);
        assert!(
            (halfway.center * halfway.center - small_soft.center * large_soft.center).abs() < 0.1
        );
        assert!(
            (halfway.lifetime * halfway.lifetime - small_soft.lifetime * large_soft.lifetime).abs()
                < 1e-8
        );
    }

    #[test]
    fn full_pool_extremes_are_finite_peak_bounded_and_callback_heap_free_at_supported_rates() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000, 192_000, 384_000] {
            for size in [Normalized::ZERO, Normalized::ONE] {
                for hardness in [Normalized::ZERO, Normalized::ONE] {
                    let ring = VisualRing::new(1, 1);
                    let knobs = [Normalized::ONE, size, hardness];
                    let mut voice = voice(rate, knobs);
                    for grain in &mut voice.grains {
                        grain.start(
                            CollisionTone::new(size.get(), hardness.get()),
                            1.0,
                            0.5,
                            0.5,
                            1.0,
                            rate,
                        );
                    }
                    let pointer = voice.grains.as_ptr();
                    let original_lifetime = voice.grains[0].lifetime;
                    let (_, counts) = crate::rt_alloc::measure(|| {
                        for sample in 0..rate as usize / 4 {
                            let frame = step(&mut voice, &knobs, &ring, sample);
                            assert!(
                                frame
                                    .iter()
                                    .all(|value| value.is_finite() && value.abs() < 0.9)
                            );
                            assert!(
                                voice.grains.iter().filter(|grain| grain.alive()).count()
                                    <= MAX_COLLISIONS
                            );
                            if (sample + 1) as f64 / (rate as f64) < original_lifetime - 1e-9 {
                                let age = voice.grains[0].age;
                                assert!(
                                    voice
                                        .grains
                                        .iter()
                                        .all(|grain| grain.alive() && grain.age == age),
                                    "full admission must not steal or restart an existing collision"
                                );
                            }
                            for grain in &voice.grains {
                                assert!(
                                    grain
                                        .centers
                                        .iter()
                                        .all(|center| *center > 0.0 && *center < rate as f32 * 0.5)
                                );
                                assert!(
                                    grain
                                        .q
                                        .iter()
                                        .all(|q| q.is_finite() && *q > 0.0 && *q <= 128.0)
                                );
                            }
                        }
                        voice.set_contributing(false);
                        voice.set_contributing(true);
                    });
                    assert_eq!(counts, crate::rt_alloc::Counts::default());
                    assert_eq!(voice.grains.as_ptr(), pointer);
                }
            }
        }
    }

    #[test]
    fn impact_light_belongs_to_the_accepted_collision_not_visual_capacity() {
        let tiny_ring = VisualRing::new(1, 1);
        let roomy_ring = VisualRing::new(2048, 1);
        let knobs = [Normalized::ONE; 3];
        let mut tiny = voice(48_000, knobs);
        let mut roomy = voice(48_000, knobs);
        let mut visible = [None];
        let mut observed = false;
        for sample in 0..96_000 {
            assert_eq!(
                step(&mut tiny, &knobs, &tiny_ring, sample),
                step(&mut roomy, &knobs, &roomy_ring, sample)
            );
            let now = MachNanos::new((sample as f64 / 48_000.0 * 1e9) as u64);
            if tiny_ring.read_live_into(StreamEpoch::new(1).unwrap(), now, &mut visible) != 0 {
                let event = visible[0].unwrap();
                if event.present == now {
                    let grain = tiny
                        .grains
                        .iter()
                        .find(|grain| grain.age == 0.0 && grain.strength == event.strength.get())
                        .unwrap();
                    let angle = event.x.get() * std::f32::consts::FRAC_PI_2;
                    assert_eq!(grain.pan, [angle.cos(), angle.sin()]);
                    assert_eq!(grain.y, event.y.get());
                    assert_eq!(grain.radius, event.radius.get());
                    assert_eq!(Duration::from_secs_f64(grain.lifetime), event.lifetime);
                    assert_eq!(event.shape, VisualShape::Impact);
                    observed = true;
                }
            }
        }
        assert!(observed);
    }

    #[test]
    fn reentry_does_not_resurrect_young_collisions_but_output_mute_keeps_the_world_alive() {
        let ring = VisualRing::new(8, 1);
        let knobs = [Normalized::ZERO, Normalized::ONE, Normalized::ZERO];
        let mut leaving = voice(48_000, knobs);
        let mut muted = voice(48_000, knobs);
        for voice in [&mut leaving, &mut muted] {
            voice.grains[0].start(CollisionTone::new(1.0, 0.0), 0.9, 0.25, 0.4, 1.0, 48_000);
        }
        for sample in 0..96 {
            assert_eq!(
                step(&mut leaving, &knobs, &ring, sample),
                step(&mut muted, &knobs, &ring, sample)
            );
        }
        let age = leaving.grains[0].age;
        let random = leaving.random.state;
        let breath = leaving.modulation.breath();
        leaving.set_contributing(false);
        leaving.set_contributing(true);
        assert_eq!(leaving.grains[0].age, age);
        assert_eq!(leaving.random.state, random);
        assert_eq!(leaving.modulation.breath(), breath);
        let mut mute_world_energy = 0.0;
        for sample in 96..4_896 {
            assert_eq!(step(&mut leaving, &knobs, &ring, sample), [0.0; 2]);
            // Output mute is downstream: rendering continues, its samples are discarded.
            let frame = step(&mut muted, &knobs, &ring, sample);
            mute_world_energy += frame[0].abs() + frame[1].abs();
        }
        assert!(mute_world_energy > 0.0);
        leaving.set_contributing(false);
        for sample in 4_896..9_696 {
            assert_eq!(
                step(&mut leaving, &[Normalized::ONE; 3], &ring, sample),
                [0.0; 2]
            );
            assert!(leaving.grains.iter().skip(1).all(|grain| !grain.alive()));
        }
        assert!(leaving.grains[0].age > age);
        assert_ne!(leaving.random.state, random);
        leaving.set_contributing(true);
        let mut new_energy = 0.0;
        for sample in 9_696..57_696 {
            let frame = step(&mut leaving, &[Normalized::ONE; 3], &ring, sample);
            new_energy += frame[0].abs() + frame[1].abs();
        }
        assert!(
            new_energy > 0.0,
            "new excitation works after history invalidation"
        );
    }

    #[test]
    fn worker_rate_preparation_preserves_world_age_and_smoother_seconds() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(48_000, [Normalized::ONE; 3]);
        for sample in 0..4_800 {
            step(&mut voice, &[Normalized::ZERO; 3], &ring, sample);
        }
        let random = voice.random.state;
        let ages: Vec<_> = voice.grains.iter().map(|grain| grain.age).collect();
        let breath = voice.modulation.breath();
        let weather = voice.modulation.weather();
        let amount = voice.amount.value();
        assert!(
            amount > 0.0 && amount < 1.0,
            "the knob edit is a finite ramp, not an instant switch"
        );
        let delay = voice.prepare_rate(8_000).unwrap();
        assert_eq!(voice.prepared_delay().unwrap(), delay);
        assert_eq!(voice.random.state, random);
        assert_eq!(voice.amount.value(), amount);
        assert_eq!(voice.modulation.breath(), breath);
        assert_eq!(voice.modulation.weather(), weather);
        assert_eq!(
            ages,
            voice
                .grains
                .iter()
                .map(|grain| grain.age)
                .collect::<Vec<_>>()
        );
        for sample in 0..800 {
            assert!(
                step(&mut voice, &[Normalized::ZERO; 3], &ring, sample)
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
        assert_eq!(voice.amount.value(), 0.0);
        assert_eq!(voice.size.value(), 0.0);
        assert_eq!(voice.hardness.value(), 0.0);
        assert!(matches!(
            voice.prepare_rate(0),
            Err(VoicePreparationError::InvalidRate)
        ));
        assert!(matches!(
            voice.prepare_rate(384_001),
            Err(VoicePreparationError::InvalidRate)
        ));
        assert_eq!(voice.sample_rate, 8_000);
    }
}
