//! The star field behind a starry situation (秋の夜長): a dense scatter of tiny hard-edged dots.
//!
//! It is background painting, not visual events: nothing here goes through the event ring or the
//! displayed-particle capacity, and the number drawn has a fixed upper bound ([`MAX_STARS`]) that
//! the intensity only lowers. Every property of star `i` is a pure function of its index (where
//! it is, how big, what colour, how bright at its best, how long and when it twinkles) and, for the
//! brightness, of the world time, so the sky is the same every run and no random source is used by
//! the painter. Nothing is cached or allocated: the scene asks for each star in turn.
//!
//! The stars must read clearly on a dark window: a typical star at its bright phase is well above
//! half white, the brightest reach full white, and at any moment most of them are lit. A dot a
//! pixel or two across is only that bright if it fills whole physical pixels, so [`pixel_quad`]
//! snaps every star to the pixel grid instead of letting a sub-pixel quad smear over four pixels.

use std::f32::consts::TAU;

use crate::{controls::VisualIntensity, ui::appearance::Motion};

/// The night film: a deep indigo wash and how opaque it is on a light and on a dark window.
const FILM_RGB: [f32; 3] = [0.03, 0.05, 0.14];
const FILM_LIGHT: f32 = 0.78;
const FILM_DARK: f32 = 0.30;

/// The most stars one frame can draw: the vivid cap. The lower levels have lower caps; every level
/// draws a prefix of the same sky.
pub(crate) const MAX_STARS: usize = 1400;
const CALM_CAP: usize = 600;
const STANDARD_CAP: usize = 1000;
/// Stars at the default window size (960 by 600 logical px), which the count scales from.
const CALM_STARS: usize = 260;
const STANDARD_STARS: usize = 450;
const VIVID_STARS: usize = 650;
pub(crate) const REFERENCE_AREA: f32 = 960.0 * 600.0;

/// A star's diameter in logical pixels at the default window size, before layout scaling. Most
/// are tiny (up to 1.6); the bright ones (about one in seven) may reach 2.0.
pub(crate) const MIN_DIAMETER: f32 = 0.6;
pub(crate) const MAX_DIAMETER: f32 = 2.0;
const BRIGHT_MIN_DIAMETER: f32 = 1.4;
/// Share of the stars that are the bright, larger ones.
const BRIGHT_SHARE: f32 = 0.15;
/// A twinkle lasts between these many seconds (a long, slow period, different for every star).
pub(crate) const MIN_PERIOD: f64 = 6.0;
pub(crate) const MAX_PERIOD: f64 = 40.0;
/// The brightness at the bottom of a twinkle: a gentle dip, never out.
const DIP: f32 = 0.18;
/// A star dimmer than this is not worth a quad.
pub(crate) const MIN_VISIBLE: f32 = 0.04;
/// Reduced motion holds every star at this share of its best brightness.
const STILL_LEVEL: f32 = 0.85;
/// The most the situation's breath can lower the sky (at full contribution): a mild shimmer.
const BREATH_SHARE: f32 = 0.15;

/// How many stars to draw for `area` square logical pixels of window: the default-window count
/// scaled in proportion to the area (never below the default's), up to the level's cap. Only the
/// count changes with the window: star `i` is the same star at any size, so a bigger window adds
/// stars at the end of the same sky and a smaller one drops them. A non-finite area counts as the
/// default window.
pub(crate) fn count(intensity: VisualIntensity, area: f32) -> usize {
    let (base, cap) = match intensity {
        VisualIntensity::Calm => (CALM_STARS, CALM_CAP),
        VisualIntensity::Standard => (STANDARD_STARS, STANDARD_CAP),
        VisualIntensity::Vivid => (VIVID_STARS, MAX_STARS),
    };
    let scale = if area.is_finite() {
        (area / REFERENCE_AREA).max(1.0)
    } else {
        1.0
    };
    ((base as f32 * scale).round() as usize).min(cap)
}

/// One star as the scene paints it: a hard-edged dot of `diameter` logical pixels (default-window
/// scale) at a normalized position, in this colour and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Star {
    pub(crate) position: [f32; 2],
    pub(crate) diameter: f32,
    pub(crate) rgb: [f32; 3],
    pub(crate) opacity: f32,
}

fn mix(value: u64) -> u64 {
    let mut value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// A value in 0..1 for the `salt`-th property of star `index`.
fn unit(index: usize, salt: u64) -> f32 {
    let bits = mix((index as u64).wrapping_mul(0x2545_f491_4f6c_dd1d) ^ salt.wrapping_mul(0x9e37));
    ((bits >> 40) as f32) / 16_777_216.0
}

/// Whether star `index` is one of the bright, larger ones.
fn is_bright(index: usize) -> bool {
    unit(index, 9) < BRIGHT_SHARE
}

/// Where star `index` is: x spread evenly, y crowded towards the top (`y = u^1.8`: about two
/// thirds of the stars lie in the upper half and few near the bottom).
pub(crate) fn position(index: usize) -> [f32; 2] {
    [unit(index, 1), 0.98 * unit(index, 2).powf(1.8)]
}

/// The seconds one twinkle of star `index` takes, log-uniform in `MIN_PERIOD..=MAX_PERIOD`.
pub(crate) fn period(index: usize) -> f64 {
    MIN_PERIOD * (MAX_PERIOD / MIN_PERIOD).powf(f64::from(unit(index, 3)))
}

/// How bright star `index` is at `time`, from `DIP` up to 1 (a pure function of world time): it
/// swings through a sine, so the bright half of the cycle is longer than the dim half.
fn twinkle(index: usize, time: f64) -> f32 {
    let turns = time / period(index) + f64::from(unit(index, 4));
    let s = 0.5 + 0.5 * (TAU * turns.fract() as f32).sin();
    DIP + (1.0 - DIP) * s
}

/// The brightness a star reaches at the top of its twinkle: 0.58 to 1 for most, 0.9 to 1 for the
/// bright ones, so even the dimmest star's best is over half.
fn best(index: usize) -> f32 {
    if is_bright(index) {
        0.9 + 0.1 * unit(index, 7)
    } else {
        0.58 + 0.42 * unit(index, 7)
    }
}

/// Pale blue-white for most, a few warm whites and pale oranges, and a few of the bright ones
/// pure white. Every colour has a channel at (almost) full, so it can reach white.
fn colour(index: usize) -> [f32; 3] {
    let pick = unit(index, 5);
    let shade = unit(index, 6);
    if is_bright(index) && unit(index, 10) < 0.45 {
        return [1.0, 1.0, 1.0];
    }
    let base = if pick < 0.80 {
        [0.80, 0.90, 1.0]
    } else if pick < 0.92 {
        [1.0, 0.97, 0.88]
    } else {
        [1.0, 0.84, 0.64]
    };
    base.map(|channel| (channel * (0.96 + 0.04 * shade)).clamp(0.0, 1.0))
}

/// The diameter of star `index`: mostly tiny (the cube keeps it near the small end), with the
/// bright ones larger.
fn diameter(index: usize) -> f32 {
    let u = unit(index, 8);
    if is_bright(index) {
        BRIGHT_MIN_DIAMETER + (MAX_DIAMETER - BRIGHT_MIN_DIAMETER) * u
    } else {
        MIN_DIAMETER + 1.0 * u.powi(3)
    }
}

/// How much of the sky shows: the situation's contribution (so it comes and goes with a
/// transition), with only a mild shimmer from its breath: at full contribution it never goes
/// below 0.85.
pub(crate) fn presence(contribution: f32, breath: f32) -> f32 {
    contribution.clamp(0.0, 1.0) * (1.0 - BREATH_SHARE + BREATH_SHARE * breath.clamp(0.0, 1.0))
}

/// The deep-indigo night laid over the whole window behind a starry situation. On a light window
/// the glass is bright, so the film is strong enough to turn it into a dark navy night; on a dark
/// window it only deepens what is there. `None` when the sky is not heard (nothing to paint).
pub(crate) fn film(dark: bool, presence: f32) -> Option<([f32; 3], f32)> {
    let presence = if presence.is_finite() {
        presence.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let alpha = (if dark { FILM_DARK } else { FILM_LIGHT }) * presence;
    (alpha > 0.0).then_some((FILM_RGB, alpha))
}

/// Star `index` at world time `time` seconds. `presence` (0..1) is how much the starry situation
/// is heard right now, so the sky comes and goes with it: at 0 nothing is lit. Reduced motion
/// stops the twinkle and holds a steady light. The stars are always the pale bright ones: the
/// night film behind them makes the window dark in either appearance.
pub(crate) fn star(index: usize, time: f64, presence: f32, motion: Motion) -> Star {
    let presence = if presence.is_finite() {
        presence.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let level = match motion {
        Motion::Full => twinkle(index, if time.is_finite() { time.max(0.0) } else { 0.0 }),
        Motion::Reduced => STILL_LEVEL,
    };
    Star {
        position: position(index),
        diameter: diameter(index),
        rgb: colour(index),
        opacity: best(index) * level * presence,
    }
}

/// A star's dot as it is painted: origin and side in logical pixels, aligned to whole physical
/// pixels, and whether it is big enough to round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PixelQuad {
    pub(crate) origin: [f32; 2],
    pub(crate) size: f32,
    pub(crate) round: bool,
}

/// The dot for a star centred at `center` (absolute logical coordinates in the window) with the
/// given logical `diameter`, on a display of `scale_factor` physical pixels per logical pixel.
/// A quad smaller than a pixel, or one straddling pixel boundaries, is drawn smeared and faint, so
/// the side is rounded to a whole number of physical pixels (at least one) and the origin to the
/// pixel grid: the dot then fills its pixels at the full alpha. Only dots of three or more pixels
/// are rounded; the smaller ones stay square, which is what a 1 or 2 pixel dot is anyway.
pub(crate) fn pixel_quad(center: [f32; 2], diameter: f32, scale_factor: f32) -> PixelQuad {
    let factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let pixels = (diameter * factor).round().max(1.0);
    let corner = |centre: f32| (centre * factor - pixels * 0.5).round() / factor;
    PixelQuad {
        origin: [corner(center[0]), corner(center[1])],
        size: pixels / factor,
        round: pixels >= 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: [VisualIntensity; 3] = [
        VisualIntensity::Calm,
        VisualIntensity::Standard,
        VisualIntensity::Vivid,
    ];

    /// The sky of the default 960 x 600 window.
    fn sky(intensity: VisualIntensity, time: f64, motion: Motion) -> Vec<Star> {
        (0..count(intensity, REFERENCE_AREA))
            .map(|index| star(index, time, 1.0, motion))
            .collect()
    }

    /// The brightest channel of a lit star over black, 0..1 (what a screenshot's max channel is,
    /// ignoring the little the background adds).
    fn lit(star: &Star) -> f32 {
        star.opacity * star.rgb.iter().copied().fold(0.0, f32::max)
    }

    /// Instants spread over several of the longest periods.
    fn instants() -> impl Iterator<Item = f64> {
        (0..200).map(|k| f64::from(k) * 3.37)
    }

    #[test]
    fn the_count_scales_with_the_window_area_inside_per_level_caps_and_never_below_the_default() {
        let area = |scale: f32| REFERENCE_AREA * scale;
        // The default 960 x 600 window.
        assert_eq!(count(VisualIntensity::Calm, area(1.0)), 260);
        assert_eq!(count(VisualIntensity::Standard, area(1.0)), 450);
        assert_eq!(count(VisualIntensity::Vivid, area(1.0)), 650);
        // A smaller window keeps the default's count (never below it).
        for small in [0.0, 0.1, 0.5, 0.999, -5.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                count(VisualIntensity::Standard, area(small)),
                450,
                "{small}"
            );
        }
        // Proportional to the area above the default.
        assert_eq!(count(VisualIntensity::Vivid, area(2.0)), 1300);
        assert_eq!(count(VisualIntensity::Standard, area(2.0)), 900);
        assert_eq!(count(VisualIntensity::Calm, area(1.5)), 390);
        // Each level stops at its cap, whatever the window.
        for huge in [4.0, 100.0, 1e9] {
            assert_eq!(count(VisualIntensity::Calm, area(huge)), 600);
            assert_eq!(count(VisualIntensity::Standard, area(huge)), 1000);
            assert_eq!(count(VisualIntensity::Vivid, area(huge)), 1400);
        }
        assert_eq!(MAX_STARS, 1400);
        // Monotonic in the area and in the level, and always within the one-frame bound.
        for level in LEVELS {
            let mut previous = 0;
            for step in 0..=200 {
                let value = count(level, area(step as f32 * 0.05));
                assert!(value >= previous && value <= MAX_STARS, "{level:?} {step}");
                previous = value;
            }
        }
        for scale in [1.0, 1.7, 3.0, 50.0] {
            let (calm, standard, vivid) = (
                count(VisualIntensity::Calm, area(scale)),
                count(VisualIntensity::Standard, area(scale)),
                count(VisualIntensity::Vivid, area(scale)),
            );
            assert!(calm < standard && standard < vivid, "{scale}");
        }
        // A prefix of the same sky: raising the level or growing the window only adds stars.
        let calm = sky(VisualIntensity::Calm, 5.0, Motion::Full);
        let vivid = sky(VisualIntensity::Vivid, 5.0, Motion::Full);
        assert_eq!(calm[..], vivid[..calm.len()]);
        let grown: Vec<Star> = (0..count(VisualIntensity::Vivid, area(1.5)))
            .map(|index| star(index, 5.0, 1.0, Motion::Full))
            .collect();
        assert_eq!(vivid[..], grown[..vivid.len()]);
    }

    #[test]
    fn the_sky_is_deterministic_and_a_property_of_the_index_alone() {
        for index in [0_usize, 1, 77, 419] {
            assert_eq!(
                star(index, 12.5, 0.8, Motion::Full),
                star(index, 12.5, 0.8, Motion::Full)
            );
            // Where a star is and how big it is never move with time.
            let (early, later) = (
                star(index, 1.0, 1.0, Motion::Full),
                star(index, 91.0, 1.0, Motion::Full),
            );
            assert_eq!(early.position, later.position);
            assert_eq!(early.diameter, later.diameter);
            assert_eq!(early.rgb, later.rgb);
        }
        assert_ne!(position(1), position(2));
    }

    #[test]
    fn stars_are_tiny_hard_dots_inside_the_window_with_valid_colours_and_brightness() {
        for index in 0..MAX_STARS {
            for time in [0.0, 3.3, 17.0, 250.0] {
                let star = star(index, time, 1.0, Motion::Full);
                assert!((0.0..=1.0).contains(&star.position[0]));
                assert!((0.0..1.0).contains(&star.position[1]));
                assert!((MIN_DIAMETER..=MAX_DIAMETER).contains(&star.diameter));
                assert!(star.rgb.iter().all(|c| (0.0..=1.0).contains(c)));
                assert!((0.0..=1.0).contains(&star.opacity), "{}", star.opacity);
            }
        }
    }

    #[test]
    fn most_stars_stay_tiny_and_about_one_in_seven_are_bright_and_up_to_two_pixels() {
        let diameters: Vec<f32> = (0..MAX_STARS).map(diameter).collect();
        let bright = (0..MAX_STARS).filter(|i| is_bright(*i)).count();
        assert!(
            (MAX_STARS / 10..MAX_STARS / 5).contains(&bright),
            "{bright} bright"
        );
        for (index, diameter) in diameters.iter().enumerate() {
            if is_bright(index) {
                assert!((1.4..=MAX_DIAMETER).contains(diameter));
            } else {
                assert!((MIN_DIAMETER..=1.6).contains(diameter));
            }
        }
        assert!(diameters.iter().any(|d| *d > 1.9), "a few reach two pixels");
        let tiny = diameters.iter().filter(|d| **d < 1.0).count();
        assert!(tiny > MAX_STARS / 2, "mostly tiny: {tiny}");
    }

    #[test]
    fn stars_are_denser_in_the_upper_window_and_sparse_below() {
        let positions: Vec<[f32; 2]> = (0..MAX_STARS).map(position).collect();
        let upper = positions.iter().filter(|p| p[1] < 0.33).count();
        let middle = positions
            .iter()
            .filter(|p| (0.33..0.66).contains(&p[1]))
            .count();
        let lower = positions.iter().filter(|p| p[1] >= 0.66).count();
        assert!(upper > middle && middle > lower, "{upper} {middle} {lower}");
        assert!(upper > lower * 2, "{upper} vs {lower}");
        assert!(lower > 0, "a few low stars remain");
        // Spread all across the width.
        for band in 0..5 {
            let lo = band as f32 / 5.0;
            assert!(
                positions
                    .iter()
                    .filter(|p| (lo..lo + 0.2).contains(&p[0]))
                    .count()
                    > 40,
                "band {band}"
            );
        }
    }

    #[test]
    fn most_stars_are_pale_blue_white_a_few_warm_and_a_few_pure_white_in_either_appearance() {
        let (mut blue, mut warm, mut white) = (0, 0, 0);
        for index in 0..MAX_STARS {
            let [r, g, b] = colour(index);
            assert!(r >= 0.75 && g >= 0.75 && b >= 0.55, "pale: {r} {g} {b}");
            assert!(
                r.max(g).max(b) >= 0.95,
                "every colour can reach white: {r} {g} {b}"
            );
            if r > 0.995 && g > 0.995 && b > 0.995 {
                white += 1;
            } else if b > r + 0.05 {
                blue += 1;
            } else if r > b + 0.05 {
                warm += 1;
            }
        }
        assert!(blue > MAX_STARS * 6 / 10, "{blue} blue");
        assert!((15..MAX_STARS * 3 / 10).contains(&warm), "{warm} warm");
        assert!((10..MAX_STARS / 10).contains(&white), "{white} white");
    }

    #[test]
    fn every_star_twinkles_slowly_with_its_own_period_dipping_gently_and_returning_bright() {
        let mut periods = std::collections::BTreeSet::new();
        for index in 0..MAX_STARS {
            let p = period(index);
            assert!((MIN_PERIOD..=MAX_PERIOD).contains(&p), "{p}");
            periods.insert(p.to_bits());
            // Walk two periods: the light spans the dip to full and never jumps.
            let steps = 400_u32;
            let (mut low, mut high, mut previous) = (f32::MAX, 0.0_f32, None::<f32>);
            let mut dim_steps = 0;
            for step in 0..=steps {
                let time = 2.0 * p * f64::from(step) / f64::from(steps);
                let level = twinkle(index, time);
                low = low.min(level);
                high = high.max(level);
                dim_steps += u32::from(level < 0.5);
                if let Some(before) = previous {
                    assert!((level - before).abs() < 0.1, "star {index} jumps");
                }
                previous = Some(level);
            }
            assert!(
                (DIP - 0.005..DIP + 0.02).contains(&low),
                "star {index}: {low}"
            );
            assert!(high > 0.99, "star {index}: {high}");
            // The dim phase is shorter than the bright one.
            assert!(
                dim_steps * 2 < steps,
                "star {index}: dim {dim_steps}/{steps}"
            );
        }
        assert!(
            periods.len() > MAX_STARS * 95 / 100,
            "a period of its own: {}",
            periods.len()
        );
        // Short and long periods both occur.
        assert!((0..MAX_STARS).any(|i| period(i) < 10.0));
        assert!((0..MAX_STARS).any(|i| period(i) > 30.0));
    }

    #[test]
    fn on_a_dark_window_the_stars_are_clearly_visible_peak_typical_and_at_any_instant() {
        // Peak: the brightest reach full white; every star at its bright phase is over half.
        let peaks: Vec<f32> = (0..MAX_STARS)
            .map(|index| {
                let mut at_best = star(index, 0.0, 1.0, Motion::Full);
                at_best.opacity = best(index);
                lit(&at_best)
            })
            .collect();
        let brightest = peaks.iter().copied().fold(0.0, f32::max);
        assert!(brightest >= 235.0 / 255.0, "brightest peak {brightest}");
        assert!(brightest <= 1.0);
        let mut sorted = peaks.clone();
        sorted.sort_by(f32::total_cmp);
        assert!(sorted[0] >= 140.0 / 255.0, "dimmest peak {}", sorted[0]);
        assert!(
            sorted[MAX_STARS / 2] >= 180.0 / 255.0,
            "typical peak {}",
            sorted[MAX_STARS / 2]
        );
        // And the real twinkle does reach those peaks.
        let reached = (0..MAX_STARS)
            .map(|index| {
                (0..400)
                    .map(|k| {
                        lit(&star(
                            index,
                            f64::from(k) * period(index) / 200.0,
                            1.0,
                            Motion::Full,
                        ))
                    })
                    .fold(0.0, f32::max)
            })
            .fold(0.0, f32::max);
        assert!(reached >= 235.0 / 255.0, "reached {reached}");
        // At any instant most stars are above 80/255 (a little over half to two thirds).
        let (mut total, mut instants_seen) = (0.0, 0);
        for time in instants() {
            let now = sky(VisualIntensity::Vivid, time, Motion::Full);
            let share =
                now.iter().filter(|s| lit(s) > 80.0 / 255.0).count() as f32 / now.len() as f32;
            assert!(
                (0.5..=0.75).contains(&share),
                "t={time}: {share} above 80/255"
            );
            total += share;
            instants_seen += 1;
        }
        let mean = total / instants_seen as f32;
        assert!(
            (0.55..=0.68).contains(&mean),
            "mean share above 80/255: {mean}"
        );
        // The median star is itself clearly lit at a typical instant.
        let now = sky(VisualIntensity::Vivid, 101.0, Motion::Full);
        let mut values: Vec<f32> = now.iter().map(lit).collect();
        values.sort_by(f32::total_cmp);
        assert!(values[values.len() / 2] > 90.0 / 255.0);
        // The lower levels are as bright as the vivid sky (only fewer).
        let calm = sky(VisualIntensity::Calm, 101.0, Motion::Full);
        assert!(calm.iter().map(lit).fold(0.0, f32::max) > 200.0 / 255.0);
    }

    #[test]
    fn the_stars_are_not_in_step_and_the_whole_sky_does_not_blink_together() {
        // At one moment the stars are spread over the whole range of brightness.
        let now = sky(VisualIntensity::Vivid, 100.0, Motion::Full);
        let mean = now.iter().map(|s| s.opacity).sum::<f32>() / now.len() as f32;
        let spread =
            (now.iter().map(|s| (s.opacity - mean).powi(2)).sum::<f32>() / now.len() as f32).sqrt();
        assert!(spread > 0.1, "out of step: spread {spread}");
        // Over time the average sky is steady.
        let totals: Vec<f32> = instants()
            .map(|time| {
                sky(VisualIntensity::Vivid, time, Motion::Full)
                    .iter()
                    .map(|s| s.opacity)
                    .sum::<f32>()
                    / count(VisualIntensity::Vivid, REFERENCE_AREA) as f32
            })
            .collect();
        let (low, high) = totals
            .iter()
            .fold((f32::MAX, 0.0_f32), |(l, h), t| (l.min(*t), h.max(*t)));
        assert!(
            high - low < 0.08,
            "the average sky is steady: {low}..{high}"
        );
        assert!(low > 0.4, "and bright: {low}");
    }

    #[test]
    fn reduced_motion_holds_every_star_still_and_clearly_lit() {
        let first = sky(VisualIntensity::Vivid, 0.0, Motion::Reduced);
        for time in [1.0, 13.7, 999.0] {
            assert_eq!(first, sky(VisualIntensity::Vivid, time, Motion::Reduced));
        }
        assert!(
            first.iter().all(|s| s.opacity > 0.4),
            "still stars are steady lights"
        );
        assert!(first.iter().all(|s| lit(s) > 100.0 / 255.0));
    }

    #[test]
    fn the_night_film_is_deep_indigo_stronger_on_light_monotonic_and_absent_without_the_sky() {
        for dark in [true, false] {
            assert_eq!(film(dark, 0.0), None);
            for bad in [f32::NAN, f32::NEG_INFINITY, -1.0] {
                assert_eq!(film(dark, bad), None);
            }
            let mut previous = 0.0;
            for step in 1..=100 {
                let (rgb, alpha) = film(dark, step as f32 / 100.0).unwrap();
                assert!(alpha >= previous, "monotonic in the presence");
                previous = alpha;
                assert!(
                    rgb[2] > rgb[0] && rgb.iter().all(|c| *c <= 0.2),
                    "deep indigo: {rgb:?}"
                );
            }
            assert_eq!(film(dark, 7.0), film(dark, 1.0), "clamped");
        }
        let (_, light) = film(false, 1.0).unwrap();
        let (_, dark) = film(true, 1.0).unwrap();
        assert!(
            light > dark,
            "the bright glass needs more: {light} vs {dark}"
        );
        assert!((0.7..=0.85).contains(&light) && (0.2..=0.4).contains(&dark));
        // Over a bright light-window glass (say 0.9 grey) the film leaves a dark navy night.
        let (rgb, alpha) = film(false, 1.0).unwrap();
        let over = rgb.map(|c| 0.9 * (1.0 - alpha) + c * alpha);
        assert!(
            over.iter().all(|c| *c < 0.35) && over[2] > over[0],
            "{over:?}"
        );
        // A half-heard sky is half as strong.
        let (_, half) = film(false, 0.5).unwrap();
        assert!((half - light * 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_sky_comes_and_goes_with_the_situations_presence_and_zero_lights_nothing() {
        for motion in [Motion::Full, Motion::Reduced] {
            for index in 0..MAX_STARS {
                assert_eq!(star(index, 9.0, 0.0, motion).opacity, 0.0);
                for bad in [f32::NAN, f32::NEG_INFINITY, -1.0] {
                    assert_eq!(star(index, 9.0, bad, motion).opacity, 0.0);
                }
                let (dim, full) = (
                    star(index, 9.0, 0.3, motion).opacity,
                    star(index, 9.0, 1.0, motion).opacity,
                );
                assert!(dim <= full && (dim - 0.3 * full).abs() < 1e-6);
                assert!(
                    star(index, 9.0, 7.0, motion).opacity <= full + 1e-6,
                    "clamped"
                );
            }
        }
    }

    #[test]
    fn a_hostile_time_never_breaks_a_star() {
        for time in [f64::NAN, f64::INFINITY, -5.0, 1e18] {
            let star = star(3, time, 1.0, Motion::Full);
            assert!(star.opacity.is_finite() && (0.0..=1.0).contains(&star.opacity));
        }
    }

    #[test]
    fn presence_follows_the_contribution_and_the_breath_only_shimmers_never_below_85_percent() {
        assert_eq!(presence(0.0, 1.0), 0.0);
        assert_eq!(presence(0.0, 0.0), 0.0);
        assert!((presence(1.0, 1.0) - 1.0).abs() < 1e-6);
        for step in 0..=100 {
            let breath = step as f32 / 100.0;
            assert!(presence(1.0, breath) >= 0.85 - 1e-6, "breath {breath}");
        }
        let mut previous = 0.0;
        for step in 0..=100 {
            let value = presence(step as f32 / 100.0, 0.5);
            assert!(value >= previous, "monotonic in the contribution");
            previous = value;
        }
    }

    #[test]
    fn only_the_autumn_registration_declares_a_star_field() {
        let registry = crate::situations::Registry::new().unwrap();
        for definition in registry.definitions() {
            assert_eq!(
                definition.visual.stars,
                definition.id == "autumn",
                "{}",
                definition.id
            );
        }
    }

    #[test]
    fn a_dot_is_snapped_to_whole_physical_pixels_so_it_is_never_smeared() {
        for factor in [1.0_f32, 1.5, 2.0, 3.0] {
            for index in 0..MAX_STARS {
                let [x, y] = position(index);
                let center = [x * 977.3 + 12.4, y * 611.7 + 3.9];
                // The layout scale of a window 1.3 times the default size.
                let wanted = diameter(index) * 1.3;
                let quad = pixel_quad(center, wanted, factor);
                let physical = |value: f32| value * factor;
                let near_whole = |value: f32| (value - value.round()).abs() < 1e-3;
                assert!(near_whole(physical(quad.size)), "size {}", quad.size);
                assert!(
                    near_whole(physical(quad.origin[0])) && near_whole(physical(quad.origin[1]))
                );
                assert!(physical(quad.size) >= 1.0 - 1e-4, "at least one pixel");
                // Within a pixel of what was asked for (but never under one pixel).
                assert!(
                    (physical(quad.size) - physical(wanted)).abs() <= 1.0 || physical(wanted) < 1.0
                );
                // Centred on the star to within half a pixel.
                for (origin, wanted_centre) in quad.origin.into_iter().zip(center) {
                    let middle = origin + quad.size * 0.5;
                    assert!(
                        (physical(middle) - physical(wanted_centre)).abs() <= 1.0,
                        "factor {factor}"
                    );
                }
                assert_eq!(quad, pixel_quad(center, wanted, factor));
                assert_eq!(quad.round, physical(quad.size) >= 2.999);
            }
        }
        // A 0.6 logical px dot on a 1x display is a whole pixel, not a faint smear.
        let quad = pixel_quad([100.2, 50.7], 0.6, 1.0);
        assert_eq!(quad.size, 1.0);
        assert_eq!(quad.origin, [100.0, 50.0]);
        assert!(!quad.round);
        // On a 2x display 0.6 logical is 1.2 physical: one physical pixel.
        assert_eq!(pixel_quad([10.3, 10.3], 0.6, 2.0).size, 0.5);
        // Hostile scale factors fall back to 1.
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                pixel_quad([5.0, 5.0], 1.0, bad),
                pixel_quad([5.0, 5.0], 1.0, 1.0)
            );
        }
    }
}
