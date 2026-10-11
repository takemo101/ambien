use fundsp::{audionode::AudioNode, filter::Lowpole, prelude32::U1};

use super::{VoiceLatencyFrames, VoicePreparationError};
use crate::controls::Normalized;

pub(super) const MAX_BIRTHS_PER_SAMPLE: usize = 4;
const CONTROL_SECONDS: f64 = 1.0 / 50.0;

pub(super) struct KnobRamp {
    start: f32,
    target: f32,
    value: f32,
    elapsed: f64,
}

impl KnobRamp {
    pub(super) fn new(initial: Normalized) -> Self {
        Self {
            start: initial.get(),
            target: initial.get(),
            value: initial.get(),
            elapsed: 0.2,
        }
    }

    pub(super) fn advance(&mut self, target: Normalized, delta: f64) {
        if target.get() != self.target {
            self.start = self.value;
            self.target = target.get();
            self.elapsed = 0.0;
        }
        self.elapsed = (self.elapsed + delta).min(0.2);
        // Round-off in summing 1/fs must not turn a finite endpoint into a tiny
        // nonzero gate for another sample.
        if self.elapsed >= 0.2 - 1e-12 {
            self.value = self.target;
            self.elapsed = 0.2;
        } else {
            let t = (self.elapsed / 0.2) as f32;
            self.value = self.start + (self.target - self.start) * t * t * (3.0 - 2.0 * t);
        }
    }

    pub(super) fn value(&self) -> f32 {
        self.value
    }
}

/// World-only SplitMix64; audio noise nodes have separate seeds and phase.
pub(crate) struct WorldRandom {
    pub(super) state: u64,
}

impl WorldRandom {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^= value >> 31;
        value
    }

    pub(super) fn unit(&mut self) -> f64 {
        let value = self.next_u64();
        // Midpoints exclude 0 and 1: exponential thresholds are finite > 0.
        ((value >> 12) as f64 + 0.5) * (1.0 / 4_503_599_627_370_496.0)
    }
}

pub(super) struct Poisson {
    accumulated: f64,
    threshold: f64,
}

impl Poisson {
    pub(super) fn new(random: &mut WorldRandom) -> Self {
        Self {
            accumulated: 0.0,
            threshold: -random.unit().ln(),
        }
    }

    pub(super) fn births(&mut self, rate: f64, delta: f64, random: &mut WorldRandom) -> usize {
        if rate == 0.0 {
            return 0;
        }
        self.accumulated += rate * delta;
        let mut births = 0;
        for _ in 0..MAX_BIRTHS_PER_SAMPLE {
            if self.accumulated < self.threshold {
                break;
            }
            self.accumulated -= self.threshold;
            self.threshold = -random.unit().ln();
            births += 1;
        }
        births
    }
}

/// Bounded low-passed stochastic targets at 50 Hz, interpolated in world time.
/// A cascade colors white noise twice; no periodic LFO and no sample-rate
/// dependent update cost. World filters are never reset at a contribution edge.
pub(super) struct Modulation {
    breath_filters: [Lowpole<f32, U1>; 2],
    weather_filters: [Lowpole<f32, U1>; 2],
    elapsed: f64,
    breath_from: f32,
    breath_to: f32,
    weather_from: f32,
    weather_to: f32,
}

impl Modulation {
    pub(super) fn new() -> Self {
        let mut breath_filters = [Lowpole::new(0.12), Lowpole::new(0.12)];
        let mut weather_filters = [Lowpole::new(0.004), Lowpole::new(0.004)];
        for node in breath_filters.iter_mut().chain(weather_filters.iter_mut()) {
            node.set_sample_rate(50.0);
            node.allocate();
            node.reset();
        }
        Self {
            breath_filters,
            weather_filters,
            elapsed: CONTROL_SECONDS,
            breath_from: 0.0,
            breath_to: 0.0,
            weather_from: 0.0,
            weather_to: 0.0,
        }
    }

    pub(super) fn advance(&mut self, delta: f64, random: &mut WorldRandom) -> bool {
        self.elapsed += delta;
        if self.elapsed < CONTROL_SECONDS {
            return false;
        }
        self.elapsed -= CONTROL_SECONDS;
        self.breath_from = self.breath_to;
        self.weather_from = self.weather_to;
        let breath_white = (random.unit() * 2.0 - 1.0) as f32;
        let weather_white = (random.unit() * 2.0 - 1.0) as f32;
        let first = self.breath_filters[0].tick(&[breath_white].into())[0];
        self.breath_to = self.breath_filters[1].tick(&[first].into())[0];
        let first = self.weather_filters[0].tick(&[weather_white].into())[0];
        self.weather_to = self.weather_filters[1].tick(&[first].into())[0];
        true
    }

    pub(super) fn breath(&self) -> f32 {
        self.breath_from
            + (self.breath_to - self.breath_from) * (self.elapsed / CONTROL_SECONDS).min(1.0) as f32
    }

    pub(super) fn weather(&self) -> f32 {
        self.weather_from
            + (self.weather_to - self.weather_from)
                * (self.elapsed / CONTROL_SECONDS).min(1.0) as f32
    }
}

/// Latency queries allocate SignalFrames, so this is worker-only. Unknown is an
/// error, not guessed zero. Filters explicitly route input latency with +0;
/// immediate noise/excitation sources have no lookahead or intentional delay.
pub(super) fn prepare_node<N: AudioNode>(node: &mut N, rate: u32) {
    node.set_sample_rate(rate as f64);
    node.allocate();
    node.reset();
}

pub(super) fn dry_latency<N: AudioNode>(node: &mut N) -> Result<f64, VoicePreparationError> {
    node.latency()
        .and_then(VoiceLatencyFrames::new)
        .map(VoiceLatencyFrames::get)
        .ok_or(VoicePreparationError::UnresolvedCausalDelay)
}

/// Delay storage stays allocated through RT invalidation. While not full every
/// read is zero; a complete new write traversal replaces all inaccessible old
/// cells before any can be read. No cell tags, clear or generation rollover.
struct LazyDelay {
    samples: Box<[f32]>,
    cursor: usize,
    filled: usize,
}

impl LazyDelay {
    fn new(length: usize) -> Self {
        assert!(length > 0);
        Self {
            samples: vec![0.0; length].into_boxed_slice(),
            cursor: 0,
            filled: 0,
        }
    }

    fn tick(&mut self, input: f32) -> f32 {
        let output = if self.filled == self.samples.len() {
            self.samples[self.cursor]
        } else {
            0.0
        };
        self.samples[self.cursor] = input;
        self.cursor += 1;
        if self.cursor == self.samples.len() {
            self.cursor = 0;
        }
        self.filled = (self.filled + 1).min(self.samples.len());
        output
    }

    fn read(&self) -> f32 {
        if self.filled == self.samples.len() {
            self.samples[self.cursor]
        } else {
            0.0
        }
    }

    fn invalidate(&mut self) {
        self.cursor = 0;
        self.filled = 0;
    }
}

struct Comb {
    delay: LazyDelay,
    feedback: f32,
    damping: f32,
    low: f32,
}

impl Comb {
    fn tick(&mut self, input: f32) -> f32 {
        let delayed = self.delay.read();
        self.low = delayed * (1.0 - self.damping) + self.low * self.damping;
        self.delay.tick(input + self.low * self.feedback);
        delayed
    }
}

struct Allpass {
    delay: LazyDelay,
    coefficient: f32,
}

impl Allpass {
    fn tick(&mut self, input: f32) -> f32 {
        let output = self.delay.read() - self.coefficient * input;
        self.delay.tick(input + self.coefficient * output);
        output
    }
}

struct ReverbChannel {
    combs: [Comb; 4],
    allpasses: [Allpass; 2],
}

/// Private Schroeder implementation, not a FunDSP FDN reset wrapper. All buffer
/// lengths and feedback powers are worker-prepared; RT tick and invalidation
/// visit exactly 2*(4+2) fixed nodes regardless of delay-storage lengths.
pub(super) struct RestartableReverb {
    channels: [ReverbChannel; 2],
}

impl RestartableReverb {
    pub(super) fn new(rate: u32) -> Result<Self, VoicePreparationError> {
        Self::with_room(rate, 1.2, 1.0)
    }

    /// `decay_seconds` is the time for the tail to fall 60 dB; `size` stretches the delay
    /// lengths (a bigger room has later, sparser reflections).
    pub(super) fn with_room(
        rate: u32,
        decay_seconds: f64,
        size: f64,
    ) -> Result<Self, VoicePreparationError> {
        if !(8_000..=384_000).contains(&rate) {
            return Err(VoicePreparationError::InvalidRate);
        }
        // Room coefficients are chosen per voice by listening (P13).
        let comb_seconds = [0.0297, 0.0371, 0.0411, 0.0437].map(|seconds| seconds * size);
        let allpass_seconds = [0.005, 0.0017];
        let channels: [ReverbChannel; 2] = std::array::from_fn(|channel| {
            let offset = channel as f64 * 0.00073;
            ReverbChannel {
                combs: std::array::from_fn(|index| {
                    let length = ((comb_seconds[index] + offset) * rate as f64).round() as usize;
                    let feedback =
                        10.0_f64.powf(-3.0 * length as f64 / rate as f64 / decay_seconds) as f32;
                    Comb {
                        delay: LazyDelay::new(length.max(1)),
                        feedback,
                        damping: 0.25,
                        low: 0.0,
                    }
                }),
                allpasses: std::array::from_fn(|index| Allpass {
                    delay: LazyDelay::new(
                        ((allpass_seconds[index] + offset) * rate as f64)
                            .round()
                            .max(1.0) as usize,
                    ),
                    coefficient: 0.5,
                }),
            }
        });
        if channels.iter().any(|channel| {
            channel
                .combs
                .iter()
                .any(|comb| !comb.feedback.is_finite() || comb.feedback.abs() >= 1.0)
                || channel.allpasses.iter().any(|allpass| {
                    !allpass.coefficient.is_finite() || allpass.coefficient.abs() >= 1.0
                })
        }) {
            return Err(VoicePreparationError::InvalidParameters);
        }
        Ok(Self { channels })
    }

    pub(super) fn tick(&mut self, input: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|index| {
            let channel = &mut self.channels[index];
            let mut sample = channel
                .combs
                .iter_mut()
                .map(|comb| comb.tick(input[index]))
                .sum::<f32>()
                * 0.25;
            for allpass in &mut channel.allpasses {
                sample = allpass.tick(sample);
            }
            sample
        })
    }

    pub(super) fn invalidate(&mut self) {
        for channel in &mut self.channels {
            for comb in &mut channel.combs {
                comb.delay.invalidate();
                comb.low = 0.0;
            }
            for allpass in &mut channel.allpasses {
                allpass.delay.invalidate();
            }
        }
    }
}

/// Linear up to a knee, then a smooth bend towards a fixed ceiling: strikes may pile up (up to
/// 24 at once) without ever leaving the output headroom or clipping. A non-finite input is
/// returned unchanged so the engine's output guard still sees a runaway voice.
pub(super) fn soft_ceiling(x: f32) -> f32 {
    const KNEE: f32 = 0.5;
    const CEILING: f32 = 0.85;
    if !x.is_finite() {
        return x;
    }
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (CEILING - KNEE) * ((a - KNEE) / (CEILING - KNEE)).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_dry_routing_is_an_error_instead_of_a_zero_delay() {
        let mut sink = fundsp::prelude32::sink();
        assert_eq!(
            dry_latency(&mut sink.0),
            Err(VoicePreparationError::UnresolvedCausalDelay)
        );
        let mut lowpole = Lowpole::<f32, U1>::new(300.0);
        prepare_node(&mut lowpole, 48_000);
        assert_eq!(dry_latency(&mut lowpole).unwrap(), 0.0);
    }

    #[test]
    fn knob_retarget_reaches_both_exact_endpoints_in_point_two_seconds() {
        let mut ramp = KnobRamp::new(Normalized::ONE);
        for _ in 0..9_600 {
            ramp.advance(Normalized::ZERO, 1.0 / 48_000.0);
        }
        assert_eq!(ramp.value(), 0.0);
        for _ in 0..4_410 {
            ramp.advance(Normalized::ONE, 1.0 / 22_050.0);
        }
        assert_eq!(ramp.value(), 1.0);
        ramp.advance(Normalized::ZERO, 0.1);
        let middle = ramp.value();
        ramp.advance(Normalized::ONE, 0.0);
        assert_eq!(ramp.value(), middle);
    }

    #[test]
    fn lazy_delay_invalidation_never_reads_old_history() {
        let mut delay = LazyDelay::new(3);
        for _ in 0..6 {
            assert!(delay.tick(1.0).is_finite());
        }
        delay.invalidate();
        for _ in 0..9 {
            assert_eq!(delay.tick(0.0), 0.0);
        }
        assert_eq!(delay.tick(2.0), 0.0);
        assert_eq!(delay.tick(0.0), 0.0);
        assert_eq!(delay.tick(0.0), 0.0);
        assert_eq!(delay.tick(0.0), 2.0);
    }

    #[test]
    fn actual_reverb_preserves_natural_tail_then_discards_it_on_exit() {
        let mut reverb = RestartableReverb::new(48_000).unwrap();
        reverb.tick([1.0, 1.0]);
        let mut energy = 0.0;
        for _ in 0..48_000 {
            let frame = reverb.tick([0.0; 2]);
            energy += frame[0].abs() + frame[1].abs();
        }
        assert!(energy > 0.0);
        reverb.invalidate();
        for _ in 0..96_000 {
            assert_eq!(reverb.tick([0.0; 2]), [0.0; 2]);
        }
    }

    #[test]
    fn saturated_poisson_still_has_a_fixed_sample_budget_and_zero_gate() {
        let mut random = WorldRandom::new(7);
        let mut poisson = Poisson::new(&mut random);
        assert_eq!(poisson.births(1e9, 1.0, &mut random), MAX_BIRTHS_PER_SAMPLE);
        assert_eq!(poisson.births(0.0, 1.0, &mut random), 0);
    }

    #[test]
    fn soft_ceiling_leaves_quiet_signals_alone_and_never_reaches_the_headroom_limit() {
        for x in [-0.5_f32, -0.2, 0.0, 0.3, 0.5] {
            assert_eq!(soft_ceiling(x), x);
        }
        assert!(!soft_ceiling(f32::INFINITY).is_finite());
        assert!(soft_ceiling(f32::NAN).is_nan());
        let mut previous = 0.5;
        for step in 1..=3000 {
            let x = 0.5 + step as f32 * 0.01;
            let y = soft_ceiling(x);
            assert!(y >= previous, "monotonic through the knee at {x}");
            assert!(y <= 0.85, "bounded at {x}");
            assert_eq!(soft_ceiling(-x), -y, "odd symmetric at {x}");
            previous = y;
        }
    }
}
