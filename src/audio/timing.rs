use std::fmt;

use crate::situations::VoiceLatencyFrames;

/// Logical generated-sample time, never wall or host time.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct WorldSeconds(f64);

impl WorldSeconds {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// Nanoseconds from the Mach absolute-time origin used by macOS CPAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MachNanos(u64);

impl MachNanos {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A nonzero output-generation identity; it is not a world clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamEpoch(u64);

impl StreamEpoch {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingError {
    InvalidRate,
    InvalidDelay,
    InvalidTimebase,
    TimebaseUnavailable(i32),
    Overflow,
}

impl fmt::Display for TimingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRate => f.write_str("presentation sample rate is zero"),
            Self::InvalidDelay => f.write_str("dry delay is nonfinite or negative"),
            Self::InvalidTimebase => {
                f.write_str("Mach timebase has a zero numerator or denominator")
            }
            Self::TimebaseUnavailable(code) => write!(f, "mach_timebase_info failed ({code})"),
            Self::Overflow => f.write_str("presentation time exceeds Mach nanoseconds"),
        }
    }
}

impl std::error::Error for TimingError {}

/// A cached native timebase. Construction belongs outside the audio callback.
pub struct MachClock {
    numer: u32,
    denom: u32,
}

// The Darwin ABI is two uint32_t fields and a kern_return_t (int32_t).
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    fn mach_absolute_time() -> u64;
}

impl MachClock {
    pub fn new() -> Result<Self, TimingError> {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        // SAFETY: `info` has the Darwin repr(C) layout, is initialized and
        // exclusively writable for the duration of this synchronous call.
        let result = unsafe { mach_timebase_info(&mut info) };
        if result != 0 {
            return Err(TimingError::TimebaseUnavailable(result));
        }
        if info.numer == 0 || info.denom == 0 {
            return Err(TimingError::InvalidTimebase);
        }
        Ok(Self {
            numer: info.numer,
            denom: info.denom,
        })
    }

    pub fn now(&self) -> Result<MachNanos, TimingError> {
        // SAFETY: Darwin's mach_absolute_time takes no pointers or arguments,
        // has no caller preconditions, and returns ticks at the Mach origin.
        self.convert_ticks(unsafe { mach_absolute_time() })
    }

    fn convert_ticks(&self, ticks: u64) -> Result<MachNanos, TimingError> {
        if self.numer == 0 || self.denom == 0 {
            return Err(TimingError::InvalidTimebase);
        }
        let nanos = u128::from(ticks)
            .checked_mul(u128::from(self.numer))
            .ok_or(TimingError::Overflow)?
            / u128::from(self.denom);
        let nanos = u64::try_from(nanos).map_err(|_| TimingError::Overflow)?;
        Ok(MachNanos::new(nanos))
    }
}

/// Maps a dry-path birth to host playback time at this stream's actual rate.
/// Integer offset arithmetic avoids losing frame bits in an usize-to-f64 cast.
pub fn presentation_time(
    playback: MachNanos,
    frame_offset: usize,
    dry_delay: VoiceLatencyFrames,
    sample_rate: u32,
) -> Result<MachNanos, TimingError> {
    if sample_rate == 0 {
        return Err(TimingError::InvalidRate);
    }
    let delay = dry_delay.get();
    if !delay.is_finite() || delay < 0.0 {
        return Err(TimingError::InvalidDelay);
    }
    let offset_scaled = (frame_offset as u128)
        .checked_mul(1_000_000_000)
        .ok_or(TimingError::Overflow)?;
    let rate = u128::from(sample_rate);
    let whole = u64::try_from(offset_scaled / rate).map_err(|_| TimingError::Overflow)?;
    // Include the offset remainder before truncation, so fractional-frame
    // dry delays and frame offsets together receive one ns quantization.
    let delayed =
        (delay * 1_000_000_000.0 + (offset_scaled % rate) as f64) / f64::from(sample_rate);
    // u64::MAX rounds to 2^64 as f64; reject that exclusive upper bound
    // explicitly rather than accepting Rust's saturating float-to-int cast.
    if !delayed.is_finite() || delayed >= 18_446_744_073_709_551_616.0 {
        return Err(TimingError::Overflow);
    }
    let nanos = playback
        .get()
        .checked_add(whole)
        .and_then(|value| value.checked_add(delayed as u64))
        .ok_or(TimingError::Overflow)?;
    Ok(MachNanos::new(nanos))
}

#[cfg(test)]
mod native_clock_tests {
    use super::*;

    #[test]
    fn cached_native_clock_reads_monotonic_mach_time() {
        let clock = MachClock::new().unwrap();
        let first = clock.now().unwrap();
        let second = clock.now().unwrap();
        assert!(second >= first);
        assert!(first.get() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay(frames: f64) -> VoiceLatencyFrames {
        VoiceLatencyFrames::new(frames).unwrap()
    }

    #[test]
    fn presentation_includes_callback_offset_and_dry_delay_at_actual_rate() {
        let playback = MachNanos::new(1_000_000_000);
        assert_eq!(
            presentation_time(playback, 480, delay(48.0), 48_000)
                .unwrap()
                .get(),
            1_011_000_000
        );
        assert_eq!(
            presentation_time(playback, 441, delay(44.1), 44_100)
                .unwrap()
                .get(),
            1_011_000_000
        );
        assert_eq!(
            presentation_time(playback, 1, delay(0.5), 48_000)
                .unwrap()
                .get(),
            1_000_031_250
        );
    }

    #[test]
    fn timing_faults_reject_zero_rate_and_overflow_without_saturation() {
        assert_eq!(
            presentation_time(MachNanos::new(0), 0, delay(0.0), 0),
            Err(TimingError::InvalidRate)
        );
        assert_eq!(
            presentation_time(MachNanos::new(u64::MAX), 1, delay(0.0), 48_000),
            Err(TimingError::Overflow)
        );
        assert_eq!(
            presentation_time(MachNanos::new(0), usize::MAX, delay(0.0), 1),
            Err(TimingError::Overflow)
        );
        assert_eq!(
            presentation_time(MachNanos::new(0), 0, delay(f64::MAX), 1),
            Err(TimingError::Overflow)
        );
        assert_eq!(
            presentation_time(MachNanos::new(u64::MAX), 0, delay(0.0), 48_000)
                .unwrap()
                .get(),
            u64::MAX
        );
    }

    #[test]
    fn units_reject_nonfinite_world_and_zero_epoch() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(WorldSeconds::new(invalid).is_none());
            assert!(VoiceLatencyFrames::new(invalid).is_none());
        }
        assert_eq!(WorldSeconds::new(0.0).unwrap().get(), 0.0);
        assert!(StreamEpoch::new(0).is_none());
        assert_eq!(StreamEpoch::new(1).unwrap().get(), 1);
    }

    #[test]
    fn timebase_conversion_uses_checked_integer_nanoseconds() {
        let clock = MachClock {
            numer: 125,
            denom: 3,
        };
        assert_eq!(clock.convert_ticks(24).unwrap().get(), 1_000);
        assert_eq!(clock.convert_ticks(1).unwrap().get(), 41);
        assert_eq!(clock.convert_ticks(u64::MAX), Err(TimingError::Overflow));
        let invalid = MachClock { numer: 1, denom: 0 };
        assert_eq!(invalid.convert_ticks(1), Err(TimingError::InvalidTimebase));
    }
}
