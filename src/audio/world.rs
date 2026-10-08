use crate::situations::{SituationIndex, VoicePreparationError};

use super::timing::WorldSeconds;

/// GainRamp owns the actual departure mixture and crossfade duration.
/// Automatic display and resume selection have distinct tie rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Movement {
    Staying {
        current: SituationIndex,
    },
    ManualCrossing {
        target: SituationIndex,
    },
    AutomaticCrossing {
        departure: SituationIndex,
        target: SituationIndex,
        display: SituationIndex,
    },
}

impl Movement {
    pub(crate) fn select_manual(&mut self, target: SituationIndex, crossing: bool) {
        *self = if crossing {
            Self::ManualCrossing { target }
        } else {
            Self::Staying { current: target }
        };
    }

    pub(crate) fn select_automatic(&mut self, target: SituationIndex) {
        let Self::Staying { current: departure } = *self else {
            unreachable!("automatic crossing starts only after arrival");
        };
        *self = Self::AutomaticCrossing {
            departure,
            target,
            display: departure,
        };
    }

    pub(crate) fn observe_gains(&mut self, gains: &[f64]) {
        if let Self::AutomaticCrossing {
            departure,
            target,
            display,
        } = self
        {
            let from = gains[departure.get()].powi(2);
            let to = gains[target.get()].powi(2);
            if from > to {
                *display = *departure;
            } else if to > from {
                *display = *target;
            }
        }
    }

    pub(crate) fn display_target(self) -> SituationIndex {
        match self {
            Self::AutomaticCrossing { display, .. } => display,
            _ => self.target(),
        }
    }

    pub(crate) fn resume_target(self, gains: &[f64]) -> SituationIndex {
        match self {
            Self::AutomaticCrossing {
                departure, target, ..
            } => {
                if gains[target.get()].powi(2) > gains[departure.get()].powi(2) {
                    target
                } else {
                    departure
                }
            }
            _ => self.target(),
        }
    }

    pub(crate) fn target(self) -> SituationIndex {
        match self {
            Self::Staying { current } => current,
            Self::ManualCrossing { target } | Self::AutomaticCrossing { target, .. } => target,
        }
    }

    pub(crate) fn arrive(&mut self) {
        *self = Self::Staying {
            current: self.target(),
        };
    }

    pub(crate) fn phase(self) -> u32 {
        match self {
            Self::Staying { .. } => 0,
            Self::ManualCrossing { .. } => 1,
            Self::AutomaticCrossing { .. } => 2,
        }
    }
}

/// One budget is created at the callback boundary, never per sample or chunk.
pub(crate) struct DrawBudget {
    remaining: u8,
}

impl DrawBudget {
    pub(crate) fn new() -> Self {
        Self { remaining: 8 }
    }

    fn draw(&mut self, next: &mut impl FnMut() -> u64) -> Option<u64> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(next())
    }
}

/// Pending draws are private Staying state; no biased destination fallback.
/// A queued dwell captures its start time, but endpoints apply at the draw.
pub(crate) struct WanderTimer {
    enabled: bool,
    dwell_from: Option<WorldSeconds>,
    deadline: Option<WorldSeconds>,
}

impl WanderTimer {
    pub(crate) fn new(enabled: bool, now: WorldSeconds) -> Self {
        Self {
            enabled,
            dwell_from: enabled.then_some(now),
            deadline: None,
        }
    }

    pub(crate) fn update(
        &mut self,
        enabled: bool,
        wander_changed: bool,
        manual_changed: bool,
        movement: Movement,
        now: WorldSeconds,
    ) {
        self.enabled = enabled;
        if wander_changed || manual_changed {
            self.deadline = None;
            self.dwell_from =
                (enabled && matches!(movement, Movement::Staying { .. })).then_some(now);
        }
    }

    pub(crate) fn arrived(&mut self, now: WorldSeconds) {
        self.deadline = None;
        self.dwell_from = self.enabled.then_some(now);
    }

    pub(crate) fn poll(
        &mut self,
        now: WorldSeconds,
        movement: Movement,
        count: usize,
        dwell_minutes: (u32, u32),
        budget: &mut DrawBudget,
        next: &mut impl FnMut() -> u64,
    ) -> Option<SituationIndex> {
        let Movement::Staying { current } = movement else {
            return None;
        };
        if !self.enabled || count == 1 {
            return None;
        }
        if let Some(start) = self.dwell_from {
            let (min, max) = dwell_minutes;
            let minimum = f64::from(min) * 60.0;
            let maximum = f64::from(max) * 60.0;
            let seconds = if min == max {
                minimum
            } else {
                let unit = (budget.draw(next)? >> 11) as f64 / 9_007_199_254_740_992.0;
                // Addition can round the top lattice point up to the endpoint.
                (minimum + (maximum - minimum) * unit).min(maximum.next_down())
            };
            self.deadline = Some(
                WorldSeconds::new(start.get() + seconds)
                    .expect("bounded dwell and generated world time are finite"),
            );
            self.dwell_from = None;
        }
        if now.get() < self.deadline?.get() {
            return None;
        }
        let other_count = (count - 1) as u64;
        let threshold = other_count.wrapping_neg() % other_count;
        while let Some(value) = budget.draw(next) {
            if value < threshold {
                continue;
            }
            let other = (value % other_count) as usize;
            let target = other + usize::from(other >= current.get());
            self.deadline = None;
            return SituationIndex::from_index(target, count);
        }
        None
    }
}

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

    fn at(seconds: f64) -> WorldSeconds {
        WorldSeconds::new(seconds).unwrap()
    }

    fn index(value: usize, count: usize) -> SituationIndex {
        SituationIndex::from_index(value, count).unwrap()
    }

    #[test]
    fn destination_rejection_budget_is_shared_until_the_next_callback() {
        let mut timer = WanderTimer::new(true, at(0.0));
        let movement = Movement::Staying {
            current: index(1, 4),
        };
        let mut budget = DrawBudget::new();
        assert_eq!(
            timer.poll(at(0.0), movement, 4, (1, 1), &mut budget, &mut || panic!(
                "fixed dwell"
            )),
            None
        );
        let mut draws = 0;
        let mut reject = || {
            draws += 1;
            0
        };
        for _ in 0..1_000 {
            assert_eq!(
                timer.poll(at(60.0), movement, 4, (1, 1), &mut budget, &mut reject),
                None
            );
        }
        assert_eq!(draws, 8);
        assert_eq!(timer.deadline, Some(at(60.0)));
        assert_eq!(
            timer.poll(
                at(60.0),
                movement,
                4,
                (1, 1),
                &mut DrawBudget::new(),
                &mut || 1
            ),
            Some(index(2, 4))
        );
        for raw in [0, u64::MAX] {
            let mut timer = WanderTimer::new(true, at(0.0));
            let movement = Movement::Staying {
                current: index(1, 3),
            };
            let mut budget = DrawBudget::new();
            timer.poll(at(0.0), movement, 3, (1, 1), &mut budget, &mut || raw);
            let target = timer
                .poll(at(60.0), movement, 3, (1, 1), &mut budget, &mut || raw)
                .unwrap();
            assert_eq!(target, index(if raw == 0 { 0 } else { 2 }, 3));
        }
        let mut timer = WanderTimer::new(true, at(0.0));
        let only = Movement::Staying {
            current: index(0, 1),
        };
        assert_eq!(
            timer.poll(
                at(10_000.0),
                only,
                1,
                (1, 1),
                &mut DrawBudget::new(),
                &mut || panic!("single situation")
            ),
            None
        );
    }

    #[test]
    fn dwell_is_arrival_based_and_existing_wait_survives_time_edits() {
        let stay = Movement::Staying {
            current: index(0, 3),
        };
        let cross = Movement::ManualCrossing {
            target: index(1, 3),
        };
        let mut timer = WanderTimer::new(false, at(0.0));
        timer.update(true, true, false, cross, at(100.0));
        assert_eq!(
            timer.poll(
                at(110.0),
                cross,
                3,
                (7, 7),
                &mut DrawBudget::new(),
                &mut || panic!("crossing")
            ),
            None
        );
        timer.arrived(at(120.0));
        timer.poll(
            at(120.0),
            stay,
            3,
            (7, 7),
            &mut DrawBudget::new(),
            &mut || panic!("fixed dwell"),
        );
        assert_eq!(timer.deadline, Some(at(540.0)));
        timer.poll(
            at(523.0),
            stay,
            3,
            (1, 1),
            &mut DrawBudget::new(),
            &mut || panic!("old wait"),
        );
        assert_eq!(timer.deadline, Some(at(540.0)));
        assert_eq!(
            timer.poll(
                at(540.0),
                stay,
                3,
                (1, 1),
                &mut DrawBudget::new(),
                &mut || 0
            ),
            Some(index(1, 3))
        );
        for manual in [false, true] {
            let mut pending = WanderTimer::new(true, at(0.0));
            pending.poll(
                at(0.0),
                Movement::Staying {
                    current: index(1, 4),
                },
                4,
                (1, 1),
                &mut DrawBudget::new(),
                &mut || 0,
            );
            pending.poll(
                at(60.0),
                Movement::Staying {
                    current: index(1, 4),
                },
                4,
                (1, 1),
                &mut DrawBudget::new(),
                &mut || 0,
            );
            pending.update(false, !manual, manual, stay, at(60.0));
            assert_eq!(
                pending.poll(
                    at(61.0),
                    stay,
                    4,
                    (1, 1),
                    &mut DrawBudget::new(),
                    &mut || panic!("cancelled draw")
                ),
                None
            );
            assert_eq!(pending.deadline, None);
        }
        timer.update(false, true, false, cross, at(545.0));
        timer.arrived(at(600.0));
        assert_eq!(timer.deadline, None);
        timer.update(true, true, false, stay, at(700.0));
        timer.poll(
            at(700.0),
            stay,
            3,
            (7, 7),
            &mut DrawBudget::new(),
            &mut || panic!("fixed dwell"),
        );
        assert_eq!(timer.deadline, Some(at(1120.0)));
    }

    #[test]
    fn random_dwell_includes_minimum_and_excludes_maximum() {
        let stay = Movement::Staying {
            current: index(0, 3),
        };
        for raw in [0, u64::MAX] {
            let mut timer = WanderTimer::new(true, at(0.0));
            timer.poll(
                at(0.0),
                stay,
                3,
                (1, 180),
                &mut DrawBudget::new(),
                &mut || raw,
            );
            let deadline = timer.deadline.unwrap().get();
            if raw == 0 {
                assert_eq!(deadline, 60.0);
            }
            assert!((60.0..10_800.0).contains(&deadline));
        }
    }

    #[test]
    fn automatic_display_ties_keep_previous_but_resume_ties_use_departure() {
        let departure = index(0, 3);
        let target = index(2, 3);
        let mut movement = Movement::Staying { current: departure };
        movement.select_automatic(target);
        movement.observe_gains(&[0.8, 0.0, 0.6]);
        assert_eq!(movement.display_target(), departure);
        movement.observe_gains(&[0.6, 0.0, 0.8]);
        assert_eq!(movement.display_target(), target);
        movement.observe_gains(&[0.5_f64.sqrt(), 0.0, 0.5_f64.sqrt()]);
        assert_eq!(movement.display_target(), target);
        assert_eq!(
            movement.resume_target(&[0.5_f64.sqrt(), 0.0, 0.5_f64.sqrt()]),
            departure
        );
        movement.select_manual(index(1, 3), true);
        movement.observe_gains(&[0.9, 0.1, 0.0]);
        assert_eq!(movement.display_target(), index(1, 3));
        assert_eq!(movement.resume_target(&[0.9, 0.1, 0.0]), index(1, 3));
    }

    #[test]
    fn manual_target_is_displayed_and_retained_for_resume_during_interrupted_crossings() {
        let first = SituationIndex::from_index(0, 3).unwrap();
        let second = SituationIndex::from_index(1, 3).unwrap();
        let third = SituationIndex::from_index(2, 3).unwrap();
        let mut movement = Movement::Staying { current: first };
        movement.select_manual(second, true);
        assert_eq!(movement.target(), second);
        assert_eq!(movement.phase(), 1);
        movement.select_manual(third, true);
        assert_eq!(movement.target(), third);
        movement.arrive();
        assert_eq!(movement, Movement::Staying { current: third });
        assert_eq!(movement.target(), third);
        assert_eq!(movement.phase(), 0);
        movement.select_manual(third, false);
        assert_eq!(movement, Movement::Staying { current: third });
    }

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
