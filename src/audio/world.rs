use crate::situations::VoicePreparationError;

use super::timing::WorldSeconds;

/// The world's sole clock source is generated samples. Moving this clock
/// with an engine preserves elapsed time; a stream epoch never resets it.
pub struct WorldClock {
    sample_rate: u32,
    step: f64,
    elapsed: WorldSeconds,
    compensation: f64,
}

impl WorldClock {
    pub fn new(sample_rate: u32) -> Result<Self, VoicePreparationError> {
        if sample_rate == 0 {
            return Err(VoicePreparationError::InvalidRate);
        }
        Ok(Self {
            sample_rate,
            step: 1.0 / f64::from(sample_rate),
            elapsed: WorldSeconds::new(0.0).expect("zero world seconds are valid"),
            compensation: 0.0,
        })
    }

    pub fn now(&self) -> WorldSeconds {
        self.elapsed
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn advance_sample(&mut self) -> WorldSeconds {
        let corrected = self.step - self.compensation;
        let sum = self.elapsed.get() + corrected;
        self.compensation = (sum - self.elapsed.get()) - corrected;
        // Starting at zero and adding finite positive 1/rate sample steps
        // keeps elapsed time finite and nonnegative for realizable playback.
        self.elapsed = WorldSeconds::new(sum).expect("prepared sample clock stays finite");
        self.elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_advances_only_when_a_sample_is_generated() {
        let mut world = WorldClock::new(48_000).unwrap();
        assert_eq!(world.now().get(), 0.0);
        assert_eq!(world.sample_rate(), 48_000);
        for _ in 0..20 {
            assert_eq!(world.now().get(), 0.0);
        }
        assert_eq!(world.advance_sample().get(), 1.0 / 48_000.0);
        let frozen = world.now();
        assert_eq!(world.now(), frozen);
    }

    #[test]
    fn compensated_sample_clock_does_not_accumulate_naive_rounding_drift() {
        let mut world = WorldClock::new(44_100).unwrap();
        for _ in 0..441_000 {
            world.advance_sample();
        }
        assert_eq!(world.now().get(), 10.0);
    }

    #[test]
    fn invalid_rate_is_rejected_before_callback() {
        assert!(matches!(
            WorldClock::new(0),
            Err(VoicePreparationError::InvalidRate)
        ));
        let mut highest_rate = WorldClock::new(u32::MAX).unwrap();
        assert!(highest_rate.advance_sample().get() > 0.0);
    }
}
