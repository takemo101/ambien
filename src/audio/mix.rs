use std::time::Duration;

use crate::situations::SituationIndex;

// Re-anchor the scalar rotation at a bounded interval, irrespective of callback
// size. Neither drift correction nor rate preparation changes current gains.
const CORRECTION_FRAMES: u16 = 256;

/// A prepared N-voice unit-vector arc. Retargeting and rendering never allocate.
pub(crate) struct GainRamp {
    current: Box<[f64]>,
    origin: Box<[f64]>,
    tangent: Box<[f64]>,
    target: usize,
    theta: f64,
    duration: f64,
    elapsed: f64,
    compensation: f64,
    cosine: f64,
    sine: f64,
    step_cosine: f64,
    step_sine: f64,
    rate: u32,
    step_seconds: f64,
    until_correction: u16,
}

impl GainRamp {
    pub(crate) fn new(count: usize, initial: SituationIndex) -> Self {
        assert!(initial.get() < count);
        let mut current = vec![0.0; count].into_boxed_slice();
        current[initial.get()] = 1.0;
        Self {
            current,
            origin: vec![0.0; count].into_boxed_slice(),
            tangent: vec![0.0; count].into_boxed_slice(),
            target: initial.get(),
            theta: 0.0,
            duration: 0.0,
            elapsed: 0.0,
            compensation: 0.0,
            cosine: 1.0,
            sine: 0.0,
            step_cosine: 1.0,
            step_sine: 0.0,
            rate: 0,
            step_seconds: 0.0,
            until_correction: CORRECTION_FRAMES,
        }
    }

    pub(crate) fn retarget(&mut self, target: SituationIndex, duration: Duration) {
        assert!(target.get() < self.current.len());
        self.target = target.get();
        let cosine = self.current[self.target];
        let sine = self
            .current
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != self.target)
            .fold(0.0_f64, |norm, (_, gain)| norm.hypot(*gain));
        // acos(1) would lose genuinely positive, arbitrarily small contributions.
        // Only an exactly empty non-target vector is an unnecessary crossing.
        if sine == 0.0 {
            self.duration = 0.0;
            self.elapsed = 0.0;
            return;
        }
        // Correct the origin's accumulated roundoff once per intent, while
        // retaining current exactly until an actual frame is generated.
        let norm = cosine.hypot(sine);
        let cosine = cosine / norm;
        let sine = sine / norm;
        for (origin, current) in self.origin.iter_mut().zip(self.current.iter()) {
            *origin = current / norm;
        }
        for (position, tangent) in self.tangent.iter_mut().enumerate() {
            *tangent = if position == self.target {
                sine
            } else {
                -cosine * (self.origin[position] / sine)
            };
        }
        self.theta = sine.atan2(cosine);
        self.duration = duration.as_secs_f64();
        self.elapsed = 0.0;
        self.compensation = 0.0;
        self.cosine = 1.0;
        self.sine = 0.0;
        self.rate = 0;
        self.until_correction = CORRECTION_FRAMES;
        if duration.is_zero() {
            self.finish();
        }
    }

    /// Once at each render interval, and whenever the actual rate changes.
    /// Elapsed time is in seconds, never an old frame count at a new rate.
    pub(crate) fn prepare_interval(&mut self, rate: u32) {
        assert!(rate > 0);
        if !self.is_crossing() {
            return;
        }
        if self.rate != rate {
            self.step_seconds = 1.0 / f64::from(rate);
            let step = self.theta / self.duration * self.step_seconds;
            (self.step_sine, self.step_cosine) = step.sin_cos();
            self.rate = rate;
        }
        self.correct_rotation();
    }

    pub(crate) fn advance_frames(&mut self, frames: usize, rate: u32) {
        // A zero-frame call is wholly inert, including its rate argument.
        if frames == 0 || !self.is_crossing() {
            return;
        }
        assert!(rate > 0);
        if self.rate != rate {
            self.prepare_interval(rate);
        }
        let step_seconds = self.step_seconds;
        for _ in 0..frames {
            let corrected = step_seconds - self.compensation;
            let elapsed = self.elapsed + corrected;
            self.compensation = (elapsed - self.elapsed) - corrected;
            self.elapsed = elapsed;
            if self.elapsed >= self.duration {
                self.finish();
                break;
            }
            let cosine = self.cosine * self.step_cosine - self.sine * self.step_sine;
            self.sine = self.sine * self.step_cosine + self.cosine * self.step_sine;
            self.cosine = cosine;
            self.until_correction -= 1;
            if self.until_correction == 0 {
                self.correct_rotation();
            }
            for ((gain, origin), tangent) in self
                .current
                .iter_mut()
                .zip(self.origin.iter())
                .zip(self.tangent.iter())
            {
                *gain = origin * self.cosine + tangent * self.sine;
                if *gain < 0.0 && *gain >= -64.0 * f64::EPSILON {
                    *gain = 0.0;
                }
                debug_assert!(*gain >= 0.0 && gain.is_finite());
            }
        }
    }

    pub(crate) fn gains(&self) -> &[f64] {
        &self.current
    }

    pub(crate) fn is_crossing(&self) -> bool {
        self.elapsed < self.duration
    }

    fn correct_rotation(&mut self) {
        let phase = self.theta * (self.elapsed / self.duration);
        (self.sine, self.cosine) = phase.sin_cos();
        self.until_correction = CORRECTION_FRAMES;
    }

    fn finish(&mut self) {
        self.current.fill(0.0);
        self.current[self.target] = 1.0;
        self.elapsed = self.duration;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn index(value: usize) -> SituationIndex {
        SituationIndex::from_index(value, 3).unwrap()
    }

    fn assert_unit(gains: &[f64]) {
        assert!(gains.iter().all(|gain| gain.is_finite() && *gain >= 0.0));
        let power: f64 = gains.iter().map(|gain| gain * gain).sum();
        assert!((power - 1.0).abs() < 1e-12, "power was {power}");
    }

    fn assert_endpoint(gains: &[f64], target: usize) {
        for (position, gain) in gains.iter().enumerate() {
            assert_eq!(*gain, if position == target { 1.0 } else { 0.0 });
        }
    }

    #[test]
    fn third_target_keeps_the_entire_current_mix_and_reaches_exact_endpoint() {
        let mut ramp = GainRamp::new(3, index(0));
        ramp.retarget(index(1), Duration::from_secs(2));
        ramp.advance_frames(8_000, 8_000);
        assert!((ramp.gains()[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((ramp.gains()[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        let current = ramp.gains().to_vec();
        ramp.retarget(index(2), Duration::from_secs(2));
        assert_eq!(ramp.gains(), current);
        ramp.advance_frames(1, 8_000);
        assert!(ramp.gains().iter().all(|gain| *gain > 0.0));
        assert_unit(ramp.gains());
        ramp.advance_frames(15_998, 8_000);
        assert!(ramp.gains()[0] > 0.0 && ramp.gains()[1] > 0.0);
        ramp.advance_frames(1, 8_000);
        assert_endpoint(ramp.gains(), 2);
    }

    #[test]
    fn a_thousand_rapid_retargets_preserve_continuity_and_nonnegative_unit_power() {
        let mut ramp = GainRamp::new(3, index(0));
        for turn in 0..1_000 {
            let current = ramp.gains().to_vec();
            ramp.retarget(index((turn + 1) % 3), Duration::from_secs(2));
            assert_eq!(ramp.gains(), current);
            for _ in 0..1 + turn % 13 {
                ramp.advance_frames(1, 8_000);
                assert_unit(ramp.gains());
            }
        }
        ramp.retarget(index(2), Duration::from_secs(2));
        ramp.advance_frames(16_000, 8_000);
        assert_endpoint(ramp.gains(), 2);
    }

    #[test]
    fn a_rounded_one_target_does_not_discard_a_tiny_positive_other_voice() {
        let mut ramp = GainRamp::new(3, index(0));
        ramp.retarget(index(1), Duration::from_secs(1_000_000_000_000_000));
        ramp.advance_frames(1, 100_000);
        let tiny = ramp.gains()[1];
        assert_eq!(ramp.gains()[0], 1.0);
        assert!(tiny > 1e-20 && tiny < 2e-20);
        ramp.retarget(index(0), Duration::from_secs(2));
        assert_eq!(ramp.gains()[1], tiny);
        ramp.advance_frames(100_000, 100_000);
        assert!(ramp.gains()[1] > 0.0 && ramp.gains()[1] < tiny);
        assert!((ramp.gains()[1] / tiny - 0.5).abs() < 1e-12);
        ramp.advance_frames(100_000, 100_000);
        assert_endpoint(ramp.gains(), 0);
    }

    #[test]
    fn exact_onehot_selection_is_an_unchanging_noop() {
        let mut ramp = GainRamp::new(3, index(1));
        ramp.retarget(index(1), Duration::from_secs(2));
        ramp.advance_frames(100_000, 48_000);
        assert_endpoint(ramp.gains(), 1);
    }

    #[test]
    fn rate_change_preserves_remaining_seconds_instead_of_old_frame_count() {
        let mut ramp = GainRamp::new(3, index(0));
        ramp.retarget(index(1), Duration::from_secs(2));
        ramp.advance_frames(24_000, 48_000);
        let before = ramp.gains().to_vec();
        ramp.advance_frames(0, 32_000);
        assert_eq!(ramp.gains(), before);
        ramp.advance_frames(47_999, 32_000);
        assert!(ramp.gains()[0] > 0.0);
        assert_unit(ramp.gains());
        ramp.advance_frames(1, 32_000);
        assert_endpoint(ramp.gains(), 1);
    }

    #[test]
    fn advancing_zero_frames_changes_neither_gains_nor_later_progress() {
        let mut ramp = GainRamp::new(3, index(0));
        let mut reference = GainRamp::new(3, index(0));
        ramp.retarget(index(1), Duration::from_secs(2));
        reference.retarget(index(1), Duration::from_secs(2));
        ramp.advance_frames(317, 48_000);
        reference.advance_frames(317, 48_000);
        let before = ramp.gains().to_vec();
        ramp.advance_frames(0, 0);
        ramp.advance_frames(0, 44_100);
        assert_eq!(ramp.gains(), before);
        ramp.advance_frames(701, 48_000);
        reference.advance_frames(701, 48_000);
        assert_eq!(ramp.gains(), reference.gains());
    }
}
