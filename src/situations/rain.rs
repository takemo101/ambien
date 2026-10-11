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
    },
};

// Coefficients and initial values below were settled by user listening (P13); see
// docs/verification.md for what was and was not observed.
static KNOBS: [KnobDefinition; 2] = [
    KnobDefinition {
        id: "amount",
        label: "雨の量",
        initial: match Normalized::new(0.34) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
    KnobDefinition {
        id: "wind",
        label: "風の量",
        initial: match Normalized::new(0.12) {
            Some(value) => value,
            None => panic!("initial out of range"),
        },
    },
];

// Loudness trim, settled in P13 listening: the raw rain measured -40.8 dBFS RMS at 100% app
// volume; this balances it with the other voices, keeping peak headroom.
const OUTPUT_TRIM: f32 = 2.0;
/// Grain strength from which a drop is a large one, drawn at every intensity.
const LARGE_FROM: f32 = 0.63;
const VISUAL: VisualStyle = VisualStyle {
    color: [0.50, 0.72, 0.92],
    shape: VisualShape::Point,
};

pub(super) fn definition() -> SituationDefinition {
    SituationDefinition {
        id: "rain",
        label: "雨",
        knobs: &KNOBS,
        visual: VISUAL,
        grain_capacity: 64,
        prepare: |config| Ok(Box::new(RainVoice::new(config)?)),
    }
}

struct Grain {
    noise: Noise,
    resonator: Resonator<f32, U3>,
    age: f64,
    lifetime: f64,
    audio_eligible: bool,
    attack: f64,
    strength: f32,
    center: f32,
    bandwidth: f32,
    q: f32,
    pan: [f32; 2],
}

impl Grain {
    fn new(seed: u64) -> Self {
        let mut noise = Noise::new();
        noise.set_hash(seed);
        Self {
            noise,
            resonator: Resonator::new(900.0, 10.0),
            age: 0.0,
            lifetime: 0.0,
            attack: 0.002,
            audio_eligible: false,
            strength: 0.0,
            center: 900.0,
            bandwidth: 90.0,
            q: 10.0,
            pan: [0.0; 2],
        }
    }

    fn alive(&self) -> bool {
        self.age < self.lifetime
    }

    fn start(
        &mut self,
        attack: f64,
        lifetime: f64,
        center: f32,
        bandwidth: f32,
        x: f32,
        rate: u32,
    ) {
        self.age = 0.0;
        self.attack = attack;
        self.lifetime = lifetime;
        self.audio_eligible = true;
        self.center = center.min(rate as f32 * 0.42);
        self.bandwidth = bandwidth;
        // FunDSP 0.23 Resonator::new/tick use Q despite prelude32's bandwidth
        // argument name. Model bandwidth is Hz; store and explicitly convert.
        // BiquadCoefs::resonator has pole radius exp(-pi*bandwidth/fs):
        // positive finite bandwidth makes it <1; center is below Nyquist.
        self.q = self.center / self.bandwidth;
        let angle = x * std::f32::consts::FRAC_PI_2;
        self.pan = [angle.cos(), angle.sin()];
        // Resonator::reset only resets Biquad's four scalar history values.
        // No generator reseed, graph reset, buffer clear or allocation here.
        self.resonator.reset();
    }

    fn advance(&mut self, delta: f64) {
        if self.alive() {
            self.age = (self.age + delta).min(self.lifetime);
        }
    }

    fn render(&mut self) -> [f32; 2] {
        if !self.audio_eligible || !self.alive() {
            return [0.0; 2];
        }
        let attack = (self.age / self.attack).min(1.0) as f32;
        let release = ((self.lifetime - self.age) / (self.lifetime - self.attack)).min(1.0) as f32;
        let envelope = attack * attack * (3.0 - 2.0 * attack) * release * release;
        let excitation = if self.age < 0.008 {
            self.noise.tick(&Default::default())[0] * (1.0 - self.age as f32 / 0.008)
        } else {
            0.0
        };
        let sample = self
            .resonator
            .tick(&[excitation, self.center, self.q].into())[0]
            * envelope
            * self.strength
            * 0.22;
        [sample * self.pan[0], sample * self.pan[1]]
    }
}

// Drop impacts per second at full rain amount. Above a few thousand per second the impacts
// fuse into noise; at light rain they stay individually audible.
const SHOT_RATE_MAX: f32 = 8_000.0;
// Impact amplitudes are uniform in 0.3..1 with random sign: E[a^2] = 0.463.
const SHOT_POWER: f32 = 0.463;

/// Per-impact amplitude giving the same RMS at full amount as the unit-variance-1/3 noise
/// (uniform -1..1) this bed replaced, at any sample rate.
fn shot_amplitude(sample_rate: u32) -> f32 {
    (sample_rate as f32 / (SHOT_RATE_MAX * SHOT_POWER)).sqrt() * (1.0_f32 / 3.0).sqrt()
}

fn splitmix(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    (value ^ (value >> 31)) | 1
}

/// One audio-side sample of the impact train: xorshift64* per channel, separate from the
/// world PRNG. Zero most of the time; at a hit a random-sign amplitude in 0.3..1.
fn next_shot(state: &mut u64, probability: f32) -> f32 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    let bits = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
    let chance = (bits >> 40) as f32 * (1.0 / 16_777_216.0);
    if chance >= probability {
        return 0.0;
    }
    let size = ((bits >> 16) & 0x00ff_ffff) as f32 * (1.0 / 16_777_216.0);
    let sign = if bits & 1 == 0 { 1.0 } else { -1.0 };
    sign * (0.3 + 0.7 * size)
}

struct RainVoice {
    sample_rate: u32,
    dry_delay: Option<VoiceLatencyFrames>,
    contributing: bool,
    random: WorldRandom,
    poisson: Poisson,
    modulation: Modulation,
    amount: KnobRamp,
    wind: KnobRamp,
    breath_intensity: Normalized,
    rain_level: f32,
    wind_level: f32,
    rain_cutoff: f32,
    wind_cutoff: f32,
    wet_level: f32,
    wind_pan: [f32; 2],
    shot_state: [u64; 2],
    shot_probability: f32,
    shot_amplitude: f32,
    rain_noise: [Noise; 2],
    shot_gain: f32,
    noise_gain: f32,
    rain_pink: [Pinkpass<f32>; 2],
    rain_filters: [[Lowpole<f32, U2>; 2]; 2],
    wind_noise: Noise,
    wind_pink: Pinkpass<f32>,
    wind_filters: [Lowpole<f32, U2>; 2],
    dc: [DCBlock<f32>; 2],
    grains: Box<[Grain]>,
    reverb: RestartableReverb,
}

impl RainVoice {
    fn new(config: VoicePreparation) -> Result<Self, VoicePreparationError> {
        if !(1..=64).contains(&config.grain_capacity) {
            return Err(VoicePreparationError::InvalidParameters);
        }
        let reverb = RestartableReverb::new(config.sample_rate)?;
        let mut random = WorldRandom::new(config.seed);
        let poisson = Poisson::new(&mut random);
        let mut wind_noise = Noise::new();
        wind_noise.set_hash(config.seed.wrapping_add(0xa54ff53a5f1d36f1));
        let mut voice = Self {
            sample_rate: config.sample_rate,
            contributing: true,
            random,
            poisson,
            dry_delay: None,
            modulation: Modulation::new(),
            amount: KnobRamp::new(KNOBS[0].initial),
            wind: KnobRamp::new(KNOBS[1].initial),
            rain_level: 0.0,
            wind_level: 0.0,
            breath_intensity: Normalized::ZERO,
            rain_cutoff: 2_500.0_f32.min(config.sample_rate as f32 * 0.4),
            wind_cutoff: 220.0,
            wet_level: 0.12,
            wind_pan: [std::f32::consts::FRAC_1_SQRT_2; 2],
            shot_state: std::array::from_fn(|index| {
                splitmix(config.seed.wrapping_add(0x3c6ef372fe94f82a + index as u64))
            }),
            shot_probability: 0.0,
            shot_amplitude: shot_amplitude(config.sample_rate),
            rain_noise: std::array::from_fn(|index| {
                let mut noise = Noise::new();
                noise.set_hash(
                    config
                        .seed
                        .wrapping_add(0x3c6ef372fe94f82a)
                        .wrapping_add(index as u64),
                );
                noise
            }),
            shot_gain: 1.0,
            noise_gain: 0.0,
            rain_pink: std::array::from_fn(|_| Pinkpass::new()),
            rain_filters: std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    Lowpole::new(2_500.0_f32.min(config.sample_rate as f32 * 0.4))
                })
            }),
            wind_noise,
            wind_pink: Pinkpass::new(),
            wind_filters: std::array::from_fn(|_| Lowpole::new(220.0)),
            dc: std::array::from_fn(|_| DCBlock::new(8.0)),
            grains: (0..config.grain_capacity)
                .map(|index| {
                    Grain::new(
                        config
                            .seed
                            .wrapping_add(0x510e527fade682d1)
                            .wrapping_add(index as u64 * 7919),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            reverb,
        };
        voice.prepare_rate(config.sample_rate)?;
        Ok(voice)
    }

    fn invalidate_history(&mut self) {
        // The fixed 64-slot upper bound is independent of delay lengths.
        // All selected reset implementations touch only scalar histories:
        // filter.rs Lowpole/DCBlock/Pinkpass and biquad.rs Resonator/Biquad.
        // Noise generators and world modulation/PRNG/ages are untouched.
        for pink in &mut self.rain_pink {
            pink.reset();
        }
        for channel in &mut self.rain_filters {
            for filter in channel {
                filter.reset();
            }
        }
        self.wind_pink.reset();
        for filter in &mut self.wind_filters {
            filter.reset();
        }
        for dc in &mut self.dc {
            dc.reset();
        }
        for grain in &mut self.grains {
            grain.audio_eligible = false;
            grain.resonator.reset();
        }
        self.reverb.invalidate();
    }
}

impl Voice for RainVoice {
    fn advance_world(
        &mut self,
        frame: VoiceFrame,
        knobs: &[Normalized],
        events: &mut VisualWriter<'_>,
    ) {
        // Engine validates knob counts and advances once per sample at actual
        // rate; callback contains no strings, allocation, IO or waits.
        self.amount.advance(knobs[0], frame.delta_seconds);
        self.wind.advance(knobs[1], frame.delta_seconds);
        for grain in &mut self.grains {
            grain.advance(frame.delta_seconds);
        }
        let cadence = self
            .modulation
            .advance(frame.delta_seconds, &mut self.random);
        let breath = (self.modulation.breath() * 6.0).clamp(-1.0, 1.0);
        let weather = (self.modulation.weather() * 16.0).clamp(-1.0, 1.0);
        let amount = self.amount.value();
        let wind = self.wind.value();
        // x*(1-x) constrains weather to the knob's range and preserves exact
        // endpoint gates. Independent wind never depends on rain amount.
        let rain_amount = amount + amount * (1.0 - amount) * weather * 0.10;
        let wind_amount = wind + wind * (1.0 - wind) * weather * 0.08;
        // P13: the bed is a Poisson train of tiny drop impacts, not a Gaussian noise generator.
        // Few impacts per second are heard as separate fine drops; many overlap into the
        // familiar rain noise. Amplitude per impact is fixed, so level follows sqrt(rate).
        // The bed gate is exact at amount 0; the amount sets how many drops fall per second.
        self.rain_level = if rain_amount > 0.0 {
            0.065 * (1.0 + breath * 0.14)
        } else {
            0.0
        };
        self.shot_probability = (SHOT_RATE_MAX * rain_amount * rain_amount * rain_amount.sqrt()
            / frame.sample_rate as f32)
            .min(1.0);
        // Equal-power cross from the drop impacts (light rain) into white noise (heavy rain).
        // The noise weight is the cube of the amount, so it is inaudible until the rain is
        // heavy; both beds are calibrated to the same level at full amount.
        let noise_weight = rain_amount * rain_amount * rain_amount;
        self.shot_gain = (1.0 - noise_weight).sqrt();
        self.noise_gain = noise_weight.sqrt() * rain_amount.powf(1.25);
        self.wind_level = 0.085 * wind_amount * (1.0 + breath * 0.18);
        self.breath_intensity =
            Normalized::new(rain_amount.sqrt().max(wind_amount) * (0.5 + breath * 0.10))
                .expect("bounded world breath intensity");
        if cadence {
            self.rain_cutoff =
                (1_400.0 + 3_200.0 * amount + breath * 180.0).min(frame.sample_rate as f32 * 0.40);
            self.wind_cutoff =
                (100.0 + 460.0 * wind + breath * wind * 100.0).min(frame.sample_rate as f32 * 0.40);
            self.wet_level = 0.10 + amount * 0.10;
            let angle = (0.5 + breath * wind * 0.12) * std::f32::consts::FRAC_PI_2;
            self.wind_pan = [angle.cos(), angle.sin()];
        }
        // Inactive voices keep ages, controls and world modulation moving, but
        // admit no new audio grains and do not consume the Poisson clock.
        if !self.contributing {
            return;
        }
        let births = self.poisson.births(
            46.0 * rain_amount as f64,
            frame.delta_seconds,
            &mut self.random,
        );
        for _ in 0..births {
            let Some(grain) = self.grains.iter_mut().find(|grain| !grain.alive()) else {
                break;
            };
            let x = self.random.unit() as f32;
            let y = self.random.unit() as f32;
            let strength = 0.32 + self.random.unit() as f32 * 0.68;
            let center = 600.0 + self.random.unit() as f32 * 3_200.0;
            let bandwidth = 100.0 + self.random.unit() as f32 * 180.0;
            let lifetime = 0.11 + self.random.unit() * 0.17;
            grain.start(0.0015, lifetime, center, bandwidth, x, frame.sample_rate);
            grain.strength = strength;
            // Every accepted grain is published; the large/near ones are marked so the scene can
            // draw only those at the calm level. Ring overwrite/failure cannot alter admission
            // or the audio PRNG.
            if self.contributing {
                events.emit(ParticleVisual {
                    x: Normalized::new(x).expect("bounded world position"),
                    y: Normalized::new(y).expect("bounded world position"),
                    strength: Normalized::new(strength).expect("bounded grain strength"),
                    radius: LogicalPixels::new(1.3 + strength * 2.4).expect("finite radius"),
                    shape: VISUAL.shape,
                    large: strength >= LARGE_FROM,
                    lifetime: Duration::from_secs_f64(lifetime),
                });
            }
        }
    }

    fn render_sample(&mut self) -> [f32; 2] {
        if !self.contributing {
            return [0.0; 2];
        }
        let mut dry = [0.0; 2];
        if self.rain_level != 0.0 {
            for (index, output) in dry.iter_mut().enumerate() {
                let white = self.rain_noise[index].tick(&Default::default())[0] * self.noise_gain;
                let impulse = next_shot(&mut self.shot_state[index], self.shot_probability)
                    * self.shot_amplitude
                    * self.shot_gain;
                let source = impulse + white;
                let pink = self.rain_pink[index].tick(&[source].into())[0];
                let first = self.rain_filters[index][0]
                    .tick(&[0.72 * source + 0.28 * pink, self.rain_cutoff].into())[0];
                *output = self.rain_filters[index][1].tick(&[first, self.rain_cutoff].into())[0]
                    * self.rain_level;
            }
        }
        if self.wind_level != 0.0 {
            let white = self.wind_noise.tick(&Default::default())[0];
            let pink = self.wind_pink.tick(&[white].into())[0];
            let first = self.wind_filters[0].tick(&[pink, self.wind_cutoff].into())[0];
            let wind =
                self.wind_filters[1].tick(&[first, self.wind_cutoff].into())[0] * self.wind_level;
            for (output, pan) in dry.iter_mut().zip(self.wind_pan) {
                *output += wind * pan;
            }
        }
        for grain in &mut self.grains {
            let sample = grain.render();
            dry[0] += sample[0];
            dry[1] += sample[1];
        }
        for (output, dc) in dry.iter_mut().zip(&mut self.dc) {
            *output = dc.tick(&[*output].into())[0];
        }
        let wet = self.reverb.tick(dry);
        [
            (dry[0] + wet[0] * self.wet_level) * OUTPUT_TRIM,
            (dry[1] + wet[1] * self.wet_level) * OUTPUT_TRIM,
        ]
    }

    fn set_contributing(&mut self, active: bool) {
        if self.contributing == active {
            return;
        }
        self.invalidate_history();
        self.contributing = active;
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
        let reverb = if sample_rate == self.sample_rate {
            None
        } else {
            Some(RestartableReverb::new(sample_rate)?)
        };
        // Build/routing/allocate/reset below is worker-only. No composite
        // process, BufferArray, Slot/Net/Sequencer or dynamically boxed nodes.
        let mut rain_delay = 0.0_f64;
        for index in 0..2 {
            prepare_node(&mut self.rain_noise[index], sample_rate);
            prepare_node(&mut self.rain_pink[index], sample_rate);
            let mut delay = dry_latency(&mut self.rain_pink[index])?;
            for filter in &mut self.rain_filters[index] {
                filter.set_cutoff(self.rain_cutoff.min(sample_rate as f32 * 0.40));
                prepare_node(filter, sample_rate);
                delay += dry_latency(filter)?;
            }
            prepare_node(&mut self.dc[index], sample_rate);
            delay += dry_latency(&mut self.dc[index])?;
            rain_delay = rain_delay.max(delay);
        }
        prepare_node(&mut self.wind_noise, sample_rate);
        prepare_node(&mut self.wind_pink, sample_rate);
        let mut wind_delay = dry_latency(&mut self.wind_pink)?;
        for filter in &mut self.wind_filters {
            filter.set_cutoff(self.wind_cutoff.min(sample_rate as f32 * 0.40));
            prepare_node(filter, sample_rate);
            wind_delay += dry_latency(filter)?;
        }
        wind_delay += dry_latency(&mut self.dc[0])?;
        let mut grain_delay = 0.0_f64;
        for grain in &mut self.grains {
            grain.center = grain.center.min(sample_rate as f32 * 0.42);
            grain.q = grain.center / grain.bandwidth;
            if !grain.center.is_finite()
                || grain.center <= 0.0
                || !grain.bandwidth.is_finite()
                || grain.bandwidth <= 0.0
                || !grain.q.is_finite()
                || grain.q <= 0.0
                || grain.center >= sample_rate as f32 * 0.5
            {
                return Err(VoicePreparationError::InvalidParameters);
            }
            grain.resonator.set_center_q(grain.center, grain.q);
            prepare_node(&mut grain.noise, sample_rate);
            prepare_node(&mut grain.resonator, sample_rate);
            grain_delay =
                grain_delay.max(dry_latency(&mut grain.resonator)? + dry_latency(&mut self.dc[0])?);
        }
        // All three dry source routes are immediate-noise -> scalar filters;
        // no lookahead or intentional delay. Their actual routing must agree.
        if rain_delay != wind_delay || rain_delay != grain_delay {
            return Err(VoicePreparationError::UnresolvedCausalDelay);
        }
        self.rain_cutoff = self.rain_cutoff.min(sample_rate as f32 * 0.40);
        self.wind_cutoff = self.wind_cutoff.min(sample_rate as f32 * 0.40);
        self.sample_rate = sample_rate;
        self.shot_amplitude = shot_amplitude(sample_rate);
        if let Some(reverb) = reverb {
            self.reverb = reverb;
        } else {
            self.reverb.invalidate();
        }
        let delay = VoiceLatencyFrames::new(rain_delay)
            .ok_or(VoicePreparationError::UnresolvedCausalDelay)?;
        self.dry_delay = Some(delay);
        Ok(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{
        timing::{MachNanos, StreamEpoch, WorldSeconds},
        transport::VisualRing,
    };
    use crate::situations::SituationIndex;

    fn voice(amount: Normalized, wind: Normalized) -> RainVoice {
        let mut voice = RainVoice::new(VoicePreparation {
            sample_rate: 48_000,
            seed: 19,
            grain_capacity: 64,
        })
        .unwrap();
        voice.amount = KnobRamp::new(amount);
        voice.wind = KnobRamp::new(wind);
        voice
    }

    fn step(
        voice: &mut RainVoice,
        knobs: &[Normalized],
        ring: &VisualRing,
        sample: usize,
    ) -> [f32; 2] {
        let mut events = VisualWriter::new(
            ring,
            StreamEpoch::new(1).unwrap(),
            SituationIndex::from_index(0, 1).unwrap(),
            WorldSeconds::new(sample as f64 / 48_000.0).unwrap(),
            MachNanos::new(sample as u64),
        );
        voice.advance_world(
            VoiceFrame {
                world: WorldSeconds::new(sample as f64 / 48_000.0).unwrap(),
                delta_seconds: 1.0 / 48_000.0,
                sample_rate: 48_000,
            },
            knobs,
            &mut events,
        );
        voice.render_sample()
    }

    #[test]
    fn every_drop_is_published_and_only_the_strong_ones_are_marked_large() {
        let knobs = [Normalized::ONE, Normalized::ZERO];
        let ring = VisualRing::new(crate::audio::transport::EVENT_CAPACITY, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ZERO);
        for sample in 0..96_000 {
            step(&mut voice, &knobs, &ring, sample);
        }
        let mut visible = [None; crate::audio::transport::VISIBLE_CAPACITY];
        let count = ring.read_live_into(
            StreamEpoch::new(1).unwrap(),
            MachNanos::new(96_000),
            &mut visible,
        );
        assert!(count > 0);
        let events = || visible.iter().take(count).flatten();
        assert!(events().any(|event| event.large), "strong drops are large");
        assert!(
            events().any(|event| !event.large),
            "weak drops are published too"
        );
        for event in events() {
            assert_eq!(event.large, event.strength.get() >= LARGE_FROM);
        }
    }

    #[test]
    fn amount_zero_leaves_wind_but_never_rain_or_rain_particles() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(Normalized::ZERO, Normalized::ONE);
        let mut energy = 0.0;
        for sample in 0..48_000 {
            let frame = step(
                &mut voice,
                &[Normalized::ZERO, Normalized::ONE],
                &ring,
                sample,
            );
            assert_eq!(voice.rain_level, 0.0);
            assert!(voice.grains.iter().all(|grain| !grain.alive()));
            assert!(frame.iter().all(|value| value.is_finite()));
            energy += frame[0].abs() + frame[1].abs();
        }
        assert!(energy > 0.0);
    }

    #[test]
    fn wind_zero_keeps_rain_and_accepted_grains() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ZERO);
        let mut energy = 0.0;
        let mut saw_grain = false;
        for sample in 0..48_000 {
            let frame = step(
                &mut voice,
                &[Normalized::ONE, Normalized::ZERO],
                &ring,
                sample,
            );
            assert_eq!(voice.wind_level, 0.0);
            saw_grain |= voice.grains.iter().any(Grain::alive);
            energy += frame[0].abs() + frame[1].abs();
        }
        assert!(energy > 0.0 && saw_grain);
    }

    #[test]
    fn endpoints_stop_new_excitation_without_cutting_existing_grain_and_wet_tail() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ONE);
        for sample in 0..24_000 {
            step(&mut voice, &[Normalized::ONE; 2], &ring, sample);
        }
        for sample in 24_000..33_600 {
            if sample == 33_000 {
                voice.grains[0].start(0.0015, 0.2, 900.0, 150.0, 0.7, 48_000);
                voice.grains[0].strength = 0.8;
            }
            step(&mut voice, &[Normalized::ZERO; 2], &ring, sample);
        }
        assert_eq!(voice.amount.value(), 0.0);
        assert_eq!(voice.wind.value(), 0.0);
        assert_eq!(voice.rain_level, 0.0);
        assert_eq!(voice.wind_level, 0.0);
        assert!(voice.grains[0].alive());
        let age = voice.grains[0].age;
        assert!(age > 0.0 && age < voice.grains[0].lifetime);
        assert_eq!(voice.breath_intensity().get(), 0.0);
        let mut energy = 0.0;
        for sample in 33_600..34_600 {
            let frame = step(&mut voice, &[Normalized::ZERO; 2], &ring, sample);
            energy += frame[0].abs() + frame[1].abs();
        }
        assert!(energy > 0.0);
        for sample in 34_600..400_000 {
            step(&mut voice, &[Normalized::ZERO; 2], &ring, sample);
        }
        assert!(voice.grains.iter().all(|grain| !grain.alive()));
        assert!(
            voice
                .render_sample()
                .iter()
                .all(|sample| sample.abs() < 1e-5)
        );
    }

    #[test]
    fn full_sixty_four_pool_and_visual_overwrite_do_not_change_sound_admission() {
        let small_ring = VisualRing::new(1, 1);
        let large_ring = VisualRing::new(2048, 1);
        let mut small = voice(Normalized::ONE, Normalized::ZERO);
        let mut large = voice(Normalized::ONE, Normalized::ZERO);
        for index in 0..64 {
            small.grains[index].start(0.01, 0.8, 900.0, 90.0, 0.2, 48_000);
            small.grains[index].strength = 0.8;
            large.grains[index].start(0.01, 0.8, 900.0, 90.0, 0.2, 48_000);
            large.grains[index].strength = 0.8;
        }
        let small_pointer = small.grains.as_ptr();
        let (_, heap) = crate::rt_alloc::measure(|| {
            for sample in 0..96_000 {
                let a = step(
                    &mut small,
                    &[Normalized::ONE, Normalized::ZERO],
                    &small_ring,
                    sample,
                );
                let b = step(
                    &mut large,
                    &[Normalized::ONE, Normalized::ZERO],
                    &large_ring,
                    sample,
                );
                assert_eq!(a, b);
                assert!(small.grains.iter().filter(|grain| grain.alive()).count() <= 64);
                if sample < 30_000 {
                    assert_eq!(
                        small.grains.iter().filter(|grain| grain.alive()).count(),
                        64
                    );
                    let age = small.grains[0].age;
                    assert!(small.grains.iter().all(|grain| grain.age == age));
                }
            }
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        assert_eq!(small_pointer, small.grains.as_ptr());
    }

    #[test]
    fn accepted_particle_position_is_the_actual_grains_equal_power_pan() {
        let ring = VisualRing::new(1, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ZERO);
        let epoch = StreamEpoch::new(1).unwrap();
        let mut visible = [None];
        let mut observed = false;
        for sample in 0..48_000 {
            step(
                &mut voice,
                &[Normalized::ONE, Normalized::ZERO],
                &ring,
                sample,
            );
            if ring.read_live_into(epoch, MachNanos::new(sample as u64), &mut visible) != 0 {
                let event = visible[0].unwrap();
                let grain = voice
                    .grains
                    .iter()
                    .find(|grain| grain.age == 0.0 && grain.strength == event.strength.get())
                    .expect("light belongs to the newly accepted audible grain");
                let angle = event.x.get() * std::f32::consts::FRAC_PI_2;
                assert_eq!(grain.pan, [angle.cos(), angle.sin()]);
                assert!(
                    (grain.pan[0] * grain.pan[0] + grain.pan[1] * grain.pan[1] - 1.0).abs() < 1e-6
                );
                assert_eq!(grain.q, grain.center / grain.bandwidth);
                assert!(grain.center < voice.sample_rate as f32 * 0.5);
                observed = true;
                break;
            }
        }
        assert!(observed);
    }

    #[test]
    fn worker_rate_change_preserves_world_and_prepares_finite_lower_rate_nodes() {
        let ring = VisualRing::new(4, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ONE);
        for sample in 0..12_000 {
            step(&mut voice, &[Normalized::ONE; 2], &ring, sample);
        }
        let random = voice.random.state;
        let amount = voice.amount.value();
        let wind = voice.wind.value();
        let ages: Vec<_> = voice.grains.iter().map(|grain| grain.age).collect();
        let breath = voice.modulation.breath();
        assert_eq!(voice.prepare_rate(8_000).unwrap().get(), 0.0);
        assert_eq!(voice.random.state, random);
        assert_eq!(voice.amount.value(), amount);
        assert_eq!(voice.wind.value(), wind);
        assert_eq!(voice.modulation.breath(), breath);
        assert_eq!(
            ages,
            voice
                .grains
                .iter()
                .map(|grain| grain.age)
                .collect::<Vec<_>>()
        );
        assert!(
            voice
                .grains
                .iter()
                .all(|grain| grain.center < 4_000.0 && grain.q.is_finite() && grain.q > 0.0)
        );
        for _ in 0..1_000 {
            assert!(
                voice
                    .render_sample()
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
    }

    #[test]
    fn short_reentry_cannot_reexcite_a_still_alive_old_grain() {
        let mut voice = voice(Normalized::ZERO, Normalized::ZERO);
        voice.grains[0].start(0.0015, 0.2, 900.0, 150.0, 0.7, 48_000);
        voice.grains[0].strength = 0.8;
        voice.grains[0].advance(0.001);
        assert!(voice.render_sample().iter().any(|sample| *sample != 0.0));
        let random = voice.random.state;
        let age = voice.grains[0].age;
        let lifetime = voice.grains[0].lifetime;
        let mut noise = voice.grains[0].noise.clone();
        let next_noise = noise.tick(&Default::default())[0];
        let (_, heap) = crate::rt_alloc::measure(|| {
            voice.set_contributing(false);
            voice.set_contributing(true);
            assert_eq!(voice.render_sample(), [0.0; 2]);
        });
        assert_eq!(heap, crate::rt_alloc::Counts::default());
        assert_eq!(voice.random.state, random);
        assert_eq!(voice.grains[0].age, age);
        assert_eq!(voice.grains[0].lifetime, lifetime);
        assert_eq!(
            voice.grains[0].noise.tick(&Default::default())[0],
            next_noise
        );
        voice.grains[0].start(0.0015, 0.2, 900.0, 150.0, 0.7, 48_000);
        voice.grains[0].advance(0.001);
        assert!(voice.render_sample().iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn inactive_voice_ages_existing_grains_but_admits_no_new_grains() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ZERO);
        voice.grains[0].start(0.0015, 1.0, 900.0, 150.0, 0.7, 48_000);
        voice.grains[0].strength = 0.8;
        voice.set_contributing(false);
        let initial_random = voice.random.state;
        for sample in 0..4_800 {
            assert_eq!(
                step(
                    &mut voice,
                    &[Normalized::ONE, Normalized::ZERO],
                    &ring,
                    sample
                ),
                [0.0; 2]
            );
        }
        assert!(voice.grains[0].age > 0.09 && voice.grains[0].alive());
        assert!(voice.grains[1..].iter().all(|grain| grain.lifetime == 0.0));
        assert_ne!(voice.random.state, initial_random);
        let ages: Vec<_> = voice.grains.iter().map(|grain| grain.age).collect();
        voice.rain_level = 0.0;
        voice.wind_level = 0.0;
        voice.set_contributing(true);
        for grain in &mut voice.grains {
            grain.advance(0.001);
        }
        assert_eq!(voice.render_sample(), [0.0; 2]);
        assert!(
            voice
                .grains
                .iter()
                .zip(ages)
                .all(|(grain, age)| { grain.age == (age + 0.001).min(grain.lifetime) })
        );
    }

    #[test]
    fn reentry_invalidates_old_tail_without_reseeding_or_resetting_grain_age() {
        let ring = VisualRing::new(8, 1);
        let mut voice = voice(Normalized::ONE, Normalized::ZERO);
        for sample in 0..48_000 {
            step(
                &mut voice,
                &[Normalized::ONE, Normalized::ZERO],
                &ring,
                sample,
            );
        }
        let random = voice.random.state;
        let ages: Vec<_> = voice.grains.iter().map(|grain| grain.age).collect();
        let breath = voice.modulation.breath();
        voice.set_contributing(false);
        voice.set_contributing(true);
        assert_eq!(random, voice.random.state);
        assert_eq!(
            ages,
            voice
                .grains
                .iter()
                .map(|grain| grain.age)
                .collect::<Vec<_>>()
        );
        assert_eq!(breath, voice.modulation.breath());
        voice.set_contributing(false);
        for sample in 48_000..192_000 {
            let mut events = VisualWriter::new(
                &ring,
                StreamEpoch::new(1).unwrap(),
                SituationIndex::from_index(0, 1).unwrap(),
                WorldSeconds::new(sample as f64 / 48_000.0).unwrap(),
                MachNanos::new(sample as u64),
            );
            voice.advance_world(
                VoiceFrame {
                    world: WorldSeconds::new(sample as f64 / 48_000.0).unwrap(),
                    delta_seconds: 1.0 / 48_000.0,
                    sample_rate: 48_000,
                },
                &[Normalized::ZERO; 2],
                &mut events,
            );
        }
        voice.set_contributing(true);
        assert_eq!(voice.render_sample(), [0.0; 2]);
        assert_ne!(voice.random.state, random);
    }
}
