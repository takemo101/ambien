//! What one visible particle looks like, decided from its event values alone.
//!
//! The same event at the same age always gives the same marks: colour spread, ripples, splash,
//! shards, bubbles and every texture all come from the event's own numbers (shape, strength,
//! radius, serial) and its age, never from a random source in the painter. The result is a few
//! hundred bytes of plain marks the scene turns into quads; nothing here allocates or touches the
//! GPU, so the rules are testable on their own.

mod scatter;

use std::f32::consts::TAU;

use crate::{controls::VisualIntensity, situations::VisualShape, ui::appearance::Motion};
use scatter::{Scatter, lead_ring_radius, scatter};

/// The most marks one particle can have (a vivid rock impact: core and its halo, shock ring and
/// its halo, up to twenty shards and seven sparks with halos, dust and the texture marks).
pub(crate) const MAX_MARKS: usize = 64;

/// The most decoration marks the scene draws in one frame, over every live particle.
pub(crate) const MARK_BUDGET: usize = 1800;
/// A particle always keeps at least this many marks (its core light comes first).
const MIN_MARKS: usize = 6;

/// How many marks each live particle may draw this frame, so the total stays near the budget:
/// the finest decorations are given up first and the core light never is.
pub(crate) fn mark_cap(live: usize) -> usize {
    (MARK_BUDGET / live.max(1)).clamp(MIN_MARKS, MAX_MARKS)
}

/// The frame's blurred-shadow marks (Glow and Haze, the costly ones) are shared out evenly among
/// the live particles, about this many in all.
pub(crate) const SHADOW_BUDGET: usize = 400;
/// Each particle keeps at least its core light and that light's halo.
const MIN_SHADOWS: usize = 2;

/// Glow and Haze are drawn with a blurred shadow, the costliest primitive here.
fn is_shadow(mark: &Mark) -> bool {
    matches!(mark, Mark::Glow { .. } | Mark::Haze { .. })
}

/// What the scene may draw this frame. Each particle is asked mark by mark, core light first: the
/// core is always allowed; a decoration is allowed while the particle has drawn fewer than its
/// mark cap and, if it is a blurred shadow, fewer than its own share of the frame's shadows. Every
/// particle gets the same share, so a crowd thins every particle alike instead of starving the
/// ones painted last (the newest, brightest ones).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    marks_per_particle: usize,
    shadows_per_particle: usize,
    marks: usize,
    shadows: usize,
}

impl Budget {
    pub(crate) fn new(live: usize) -> Self {
        Self {
            marks_per_particle: mark_cap(live),
            // At least the core and its halo, even in a very large crowd.
            shadows_per_particle: (SHADOW_BUDGET / live.max(1)).max(MIN_SHADOWS),
            marks: 0,
            shadows: 0,
        }
    }

    pub(crate) fn allows(&mut self, index: usize, mark: &Mark) -> bool {
        if index == 0 {
            // A new particle starts: its core light is always drawn.
            self.marks = 1;
            self.shadows = usize::from(is_shadow(mark));
            return true;
        }
        if self.marks >= self.marks_per_particle {
            return false;
        }
        if is_shadow(mark) {
            if self.shadows >= self.shadows_per_particle {
                return false;
            }
            self.shadows += 1;
        }
        self.marks += 1;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mark {
    /// Soft light: a blurred shadow plus a filled disc, the particle's original look.
    Glow {
        offset: [f32; 2],
        radius: f32,
        rgb: [f32; 3],
        opacity: f32,
    },
    /// A soft blur of light: a blurred shadow with no disc on top, so it has no hard edge.
    Haze {
        offset: [f32; 2],
        radius: f32,
        rgb: [f32; 3],
        opacity: f32,
    },
    /// A thin circle: a ripple, a shock wave or a bubble's outline, at an offset from the particle.
    Ring {
        offset: [f32; 2],
        radius: f32,
        width: f32,
        rgb: [f32; 3],
        opacity: f32,
    },
    /// A small hard-edged disc (a shard or a trail bead), cheap to draw.
    Dot {
        offset: [f32; 2],
        radius: f32,
        rgb: [f32; 3],
        opacity: f32,
    },
    /// A thin plus sign (a glint): two crossing bars of the given half length.
    Cross {
        arm: f32,
        width: f32,
        rgb: [f32; 3],
        opacity: f32,
    },
}

/// Everything the look depends on. `opacity` is the particle's fade envelope for this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParticleInput {
    pub(crate) shape: VisualShape,
    /// 0 when born, 1 when expired.
    pub(crate) age: f32,
    pub(crate) opacity: f32,
    pub(crate) radius: f32,
    pub(crate) strength: f32,
    pub(crate) serial: u64,
    pub(crate) rgb: [f32; 3],
    /// A large particle is drawn at every intensity; a small one only above calm.
    pub(crate) large: bool,
    pub(crate) intensity: VisualIntensity,
    /// The window is on a dark appearance. Vivid's bloom brightens on dark and becomes a deeper
    /// colour film on light.
    pub(crate) dark: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    marks: [Option<Mark>; MAX_MARKS],
    len: usize,
}

impl Look {
    fn new() -> Self {
        Self {
            marks: [None; MAX_MARKS],
            len: 0,
        }
    }

    fn push(&mut self, mark: Mark) {
        // A look never needs more than MAX_MARKS; overflowing is a bug the tests must trip on,
        // not something to drop silently.
        debug_assert!(
            self.len < MAX_MARKS,
            "look pushed more than MAX_MARKS marks"
        );
        if self.len < MAX_MARKS {
            self.marks[self.len] = Some(mark);
            self.len += 1;
        }
    }

    pub(crate) fn marks(&self) -> impl Iterator<Item = &Mark> {
        self.marks[..self.len].iter().flatten()
    }
}

/// A well-mixed value from a serial, so neighbouring serials look unrelated.
fn mix(serial: u64) -> u64 {
    let mut value = serial.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Two independent values in 0..1 taken from the serial.
fn unit_pair(serial: u64) -> (f32, f32) {
    let bits = mix(serial);
    let low = (bits & 0xffff_ffff) as f32 / 4_294_967_295.0;
    let high = (bits >> 32) as f32 / 4_294_967_295.0;
    (low, high)
}

/// The situation's colour with a spread in brightness and a small warm/cool tilt, taken from the
/// event so the same particle always has the same colour.
fn tinted(rgb: [f32; 3], serial: u64, strength: f32) -> [f32; 3] {
    let (unit, _) = unit_pair(serial);
    let spread = unit * 2.0 - 1.0;
    let light = 1.0 + 0.18 * spread + 0.12 * (strength.clamp(0.0, 1.0) - 0.5);
    let tilt = 0.16 * spread;
    [
        (rgb[0] * (1.0 - tilt) * light).clamp(0.0, 1.0),
        (rgb[1] * light).clamp(0.0, 1.0),
        (rgb[2] * (1.0 + tilt) * light).clamp(0.0, 1.0),
    ]
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn lighten(rgb: [f32; 3], amount: f32) -> [f32; 3] {
    rgb.map(|channel| (channel + (1.0 - channel) * amount).clamp(0.0, 1.0))
}

fn deepen(rgb: [f32; 3], keep: f32) -> [f32; 3] {
    rgb.map(|channel| (channel * keep).clamp(0.0, 1.0))
}

/// The colour turned part of the way around the rainbow by rotating its channels: `forward`
/// takes red's place from green, otherwise from blue.
fn rotate(rgb: [f32; 3], forward: bool, amount: f32) -> [f32; 3] {
    let turned = if forward {
        [rgb[1], rgb[2], rgb[0]]
    } else {
        [rgb[2], rgb[0], rgb[1]]
    };
    std::array::from_fn(|index| {
        (rgb[index] + (turned[index] - rgb[index]) * amount).clamp(0.0, 1.0)
    })
}

/// A brief twinkle that is dark when born and expired and brightest in the middle.
fn twinkle(age: f32) -> f32 {
    (std::f32::consts::PI * age.clamp(0.0, 1.0)).sin().powi(2)
}

/// What standard and vivid add on top of a large particle's calm marks. `size` is the event
/// radius and the animated core radius, `state` the strength, age and opacity, `unit` two values
/// from the serial. The caller has already left out reduced motion (core light only).
fn texture(
    look: &mut Look,
    input: &ParticleInput,
    rgb: [f32; 3],
    size: [f32; 2],
    state: [f32; 3],
    unit: [f32; 2],
) {
    let [radius, core] = size;
    let [_, age, opacity] = state;
    let [phase, second] = unit;
    let dark = input.dark;
    // A glass bead: a thin rim and a bright highlight toward the light, top left.
    let vivid = input.intensity == VisualIntensity::Vivid;
    if vivid {
        // This GPUI has no additive blend, so the dark bloom is a larger, lighter glow laid over
        // the core; light mode gets a deeper colour film instead of a brightening. It is pushed
        // before the bead so the rim and highlight stay on top of it.
        if dark {
            look.push(Mark::Glow {
                offset: [0.0, 0.0],
                radius: core * 2.0,
                rgb: lighten(rgb, 0.35),
                opacity: opacity * 0.35,
            });
        } else {
            look.push(Mark::Glow {
                offset: [0.0, 0.0],
                radius: core * 1.6,
                rgb: deepen(rgb, 0.75),
                opacity: opacity * 0.28,
            });
        }
    }
    if core >= 1.0 {
        look.push(Mark::Ring {
            offset: [0.0, 0.0],
            radius: core,
            width: 1.0,
            rgb: if dark {
                lighten(rgb, 0.5)
            } else {
                deepen(rgb, 0.6)
            },
            opacity: opacity * 0.55,
        });
        look.push(Mark::Dot {
            offset: [-core * 0.32, -core * 0.34],
            radius: (core * 0.24).max(0.8),
            rgb: [1.0, 1.0, 1.0],
            opacity: opacity * 0.6,
        });
    }
    if !vivid {
        return;
    }
    // An out-of-focus disc beside the particle, its place and size from the serial.
    let bearing = TAU * phase;
    let distance = core * (1.8 + 1.2 * second);
    look.push(Mark::Haze {
        offset: [bearing.cos() * distance, bearing.sin() * distance],
        radius: core * (0.9 + 0.5 * phase) * 1.6,
        rgb,
        opacity: opacity * 0.2,
    });
    look.push(Mark::Cross {
        arm: core * 1.8,
        width: 1.0,
        rgb: lighten(rgb, 0.6),
        opacity: opacity * 0.7 * twinkle(age),
    });
    match input.shape {
        VisualShape::Point | VisualShape::Impact => {
            // Rainbow fringes just outside the ripple or shock ring.
            let ring = lead_ring_radius(input.shape, radius, age);
            for (index, forward) in [true, false].into_iter().enumerate() {
                look.push(Mark::Ring {
                    offset: [0.0, 0.0],
                    radius: ring * (1.08 + 0.1 * index as f32),
                    width: 1.0,
                    rgb: rotate(rgb, forward, 0.75),
                    opacity: opacity * 0.35,
                });
            }
            if input.shape == VisualShape::Point {
                // A falling drop drags a comet tail of fading beads above it.
                for step in 1..=3_u8 {
                    let step = f32::from(step);
                    look.push(Mark::Dot {
                        offset: [0.0, -radius * 1.4 * step * (1.0 - 0.6 * age)],
                        radius: (core * (0.5 - 0.1 * step)).max(0.8),
                        rgb,
                        opacity: opacity * 0.4 / step,
                    });
                }
            }
        }
        VisualShape::SlowGlow => {}
    }
}

/// The marks of one particle. Reduced motion keeps only the still core light: no ripple, shard,
/// trail or wobble spreads.
pub(crate) fn look(input: &ParticleInput, motion: Motion) -> Look {
    let age = finite_or_zero(input.age).clamp(0.0, 1.0);
    let opacity = finite_or_zero(input.opacity).clamp(0.0, 1.0);
    let radius = finite_or_zero(input.radius).max(0.0);
    let strength = finite_or_zero(input.strength).clamp(0.0, 1.0);
    let rgb = tinted(input.rgb, input.serial, strength);
    let (phase, second) = unit_pair(input.serial ^ 0xa5a5_a5a5_5a5a_5a5a);
    let mut look = Look::new();
    if !input.large {
        // Small particles show only when the viewer asked for more than the calm scene: a
        // faint, tiny, still dot, the cheapest possible mark; vivid adds a brief twinkle.
        if input.intensity != VisualIntensity::Calm {
            let dot = (radius * 0.5).clamp(0.8, 2.0);
            look.push(Mark::Dot {
                offset: [0.0, 0.0],
                radius: dot,
                rgb,
                opacity: opacity * 0.7,
            });
            if input.intensity == VisualIntensity::Vivid && motion != Motion::Reduced {
                look.push(Mark::Cross {
                    arm: dot * 3.0,
                    width: 0.8,
                    rgb: lighten(rgb, 0.6),
                    opacity: opacity * 0.5 * twinkle(age),
                });
            }
        }
        return look;
    }
    let core = radius * motion.shape_scale(input.shape, age);
    let still = motion == Motion::Reduced;
    // A deep bubble's outline breathes a little; the others hold their size.
    let wobble = if input.shape == VisualShape::SlowGlow && !still {
        1.0 + 0.07 * (TAU * (age * 1.5 + phase)).sin()
    } else {
        1.0
    };
    look.push(Mark::Glow {
        offset: [0.0, 0.0],
        radius: core * wobble,
        rgb,
        opacity,
    });
    if !still {
        // A soft glow around the core at every intensity; light windows get a deeper colour.
        let (halo, tone) = if input.dark {
            (lighten(rgb, 0.2), 0.28)
        } else {
            (deepen(rgb, 0.8), 0.22)
        };
        look.push(Mark::Haze {
            offset: [0.0, 0.0],
            radius: core * wobble * 2.4,
            rgb: halo,
            opacity: opacity * tone,
        });
    }
    if !still {
        scatter(
            &mut look,
            &Scatter {
                shape: input.shape,
                intensity: input.intensity,
                rgb,
                radius,
                strength,
                age,
                opacity,
                serial: input.serial,
                dark: input.dark,
            },
        );
    }
    if !still && input.intensity != VisualIntensity::Calm {
        texture(
            &mut look,
            input,
            rgb,
            [radius, core],
            [strength, age, opacity],
            [phase, second],
        );
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(shape: VisualShape, age: f32, strength: f32, serial: u64) -> ParticleInput {
        ParticleInput {
            shape,
            age,
            opacity: 0.6,
            radius: 6.0,
            strength,
            serial,
            large: true,
            intensity: VisualIntensity::Calm,
            dark: true,
            rgb: [0.5, 0.72, 0.92],
        }
    }

    const SHAPES: [VisualShape; 3] = [
        VisualShape::Point,
        VisualShape::Impact,
        VisualShape::SlowGlow,
    ];

    #[test]
    fn the_same_event_at_the_same_age_always_looks_the_same() {
        for shape in SHAPES {
            for motion in [Motion::Full, Motion::Reduced] {
                let a: Vec<Mark> = look(&input(shape, 0.4, 0.9, 77), motion)
                    .marks()
                    .copied()
                    .collect();
                let b: Vec<Mark> = look(&input(shape, 0.4, 0.9, 77), motion)
                    .marks()
                    .copied()
                    .collect();
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn the_colour_spread_belongs_to_the_event_and_stays_a_valid_colour() {
        let mut colours = std::collections::BTreeSet::new();
        for serial in 0..200_u64 {
            let rgb = tinted([0.5, 0.72, 0.92], serial, 0.7);
            assert!(rgb.iter().all(|channel| (0.0..=1.0).contains(channel)));
            colours.insert(rgb.map(f32::to_bits));
            assert_eq!(rgb, tinted([0.5, 0.72, 0.92], serial, 0.7));
        }
        assert!(colours.len() > 100, "particles do not all share one colour");
        // The spread is a tilt around the situation's colour, not a different colour.
        let rgb = tinted([0.5, 0.72, 0.92], 5, 0.5);
        assert!((rgb[1] - 0.72).abs() < 0.2);
    }

    #[test]
    fn reduced_motion_keeps_only_the_still_core_light() {
        for shape in SHAPES {
            for age in [0.0, 0.3, 0.8, 1.0] {
                let marks: Vec<Mark> = look(&input(shape, age, 1.0, 9), Motion::Reduced)
                    .marks()
                    .copied()
                    .collect();
                assert_eq!(marks.len(), 1);
                let Mark::Glow { offset, radius, .. } = marks[0] else {
                    panic!("only the core glow remains");
                };
                assert_eq!(offset, [0.0, 0.0]);
                assert_eq!(radius, 6.0, "it never swells or wobbles");
            }
        }
    }

    #[test]
    fn a_ripple_spreads_and_a_shock_ring_leaves_the_core_over_the_particles_life() {
        let ring_radius = |shape, age| {
            look(&input(shape, age, 0.5, 3), Motion::Full)
                .marks()
                .find_map(|mark| match mark {
                    Mark::Ring { radius, .. } => Some(*radius),
                    _ => None,
                })
                .unwrap()
        };
        for shape in [VisualShape::Point, VisualShape::Impact] {
            let mut previous = ring_radius(shape, 0.0);
            for step in 1..=20 {
                let radius = ring_radius(shape, step as f32 / 20.0);
                assert!(radius > previous, "spreads at step {step}");
                previous = radius;
            }
        }
    }

    #[test]
    fn a_small_particle_is_nothing_when_calm_a_dot_when_standard_and_twinkles_when_vivid() {
        let small = |intensity, motion| {
            look(
                &ParticleInput {
                    large: false,
                    intensity,
                    ..input(VisualShape::Point, 0.3, 0.2, 5)
                },
                motion,
            )
            .marks()
            .copied()
            .collect::<Vec<Mark>>()
        };
        for motion in [Motion::Full, Motion::Reduced] {
            assert!(small(VisualIntensity::Calm, motion).is_empty());
            for intensity in [VisualIntensity::Standard, VisualIntensity::Vivid] {
                let marks = small(intensity, motion);
                let Mark::Dot {
                    offset,
                    radius,
                    opacity,
                    ..
                } = marks[0]
                else {
                    panic!("a small particle leads with a plain dot");
                };
                assert_eq!(offset, [0.0, 0.0]);
                assert!((0.8..=2.0).contains(&radius), "stays tiny, got {radius}");
                assert!(
                    opacity > 0.0 && opacity < 0.6,
                    "fainter than a large particle"
                );
                let twinkles = intensity == VisualIntensity::Vivid && motion == Motion::Full;
                assert_eq!(marks.len(), if twinkles { 2 } else { 1 });
                if twinkles {
                    assert!(matches!(marks[1], Mark::Cross { .. }));
                }
            }
        }
    }

    fn marks_of(input: &ParticleInput, motion: Motion) -> Vec<Mark> {
        look(input, motion).marks().copied().collect()
    }

    fn at(shape: VisualShape, intensity: VisualIntensity, dark: bool) -> ParticleInput {
        ParticleInput {
            intensity,
            dark,
            ..input(shape, 0.5, 0.9, 41)
        }
    }

    #[test]
    fn calm_has_no_texture_and_only_its_core_halo_changes_colour_between_dark_and_light() {
        for shape in SHAPES {
            let dark = marks_of(&at(shape, VisualIntensity::Calm, true), Motion::Full);
            let light = marks_of(&at(shape, VisualIntensity::Calm, false), Motion::Full);
            assert!(!dark.iter().any(|mark| matches!(mark, Mark::Cross { .. })));
            let without_halo = |marks: &[Mark]| -> Vec<Mark> {
                marks
                    .iter()
                    .filter(|mark| !matches!(mark, Mark::Haze { .. }))
                    .copied()
                    .collect()
            };
            assert_eq!(without_halo(&dark), without_halo(&light), "{shape:?}");
        }
    }

    #[test]
    fn every_intensity_glows_softly_around_the_core_lighter_on_dark_and_deeper_on_light() {
        for shape in SHAPES {
            for level in [
                VisualIntensity::Calm,
                VisualIntensity::Standard,
                VisualIntensity::Vivid,
            ] {
                let marks = |dark| marks_of(&at(shape, level, dark), Motion::Full);
                let (dark, light) = (marks(true), marks(false));
                assert!(
                    matches!(dark[0], Mark::Glow { .. }),
                    "the core light stays first"
                );
                let (
                    Mark::Haze {
                        radius: dark_radius,
                        rgb: dark_rgb,
                        ..
                    },
                    Mark::Haze { rgb: light_rgb, .. },
                ) = (dark[1], light[1])
                else {
                    panic!("{shape:?} {level:?}: the core is followed by its soft halo");
                };
                let Mark::Glow {
                    radius: core,
                    rgb: core_rgb,
                    ..
                } = dark[0]
                else {
                    unreachable!()
                };
                assert!(dark_radius > core, "the halo spreads wider than the core");
                for ((&d, &l), &c) in dark_rgb.iter().zip(&light_rgb).zip(&core_rgb) {
                    assert!(d >= c && l <= c, "dark lightens, light deepens");
                }
            }
        }
    }

    #[test]
    fn the_budget_gives_every_particle_the_same_core_and_shadow_share_in_a_crowd() {
        let shadow = |core: bool| {
            let (offset, radius, rgb, opacity) = ([0.0; 2], 4.0, [0.5; 3], 0.5);
            if core {
                Mark::Glow {
                    offset,
                    radius,
                    rgb,
                    opacity,
                }
            } else {
                Mark::Haze {
                    offset,
                    radius,
                    rgb,
                    opacity,
                }
            }
        };
        let dot = Mark::Dot {
            offset: [0.0; 2],
            radius: 1.0,
            rgb: [0.5; 3],
            opacity: 0.5,
        };
        for live in [1, 10, 40, 200, 500] {
            let mut budget = Budget::new(live);
            let mut per_particle = Vec::new();
            for _ in 0..live {
                assert!(
                    budget.allows(0, &shadow(true)),
                    "the core light is never given up"
                );
                let (mut shadows, mut marks) = (1, 1);
                for index in 1..MAX_MARKS {
                    // Every other decoration is a blurred shadow, the rest are cheap dots.
                    let mark = if index % 2 == 1 { shadow(false) } else { dot };
                    if budget.allows(index, &mark) {
                        marks += 1;
                        shadows += usize::from(index % 2 == 1);
                    }
                }
                assert!(marks <= mark_cap(live), "{live} live: the mark cap holds");
                per_particle.push(shadows);
            }
            assert!(
                per_particle.windows(2).all(|pair| pair[0] == pair[1]),
                "{live} live: the last particle drawn is not starved: {per_particle:?}"
            );
            assert!(per_particle[0] >= 2, "{live} live: the core keeps its halo");
            let total: usize = per_particle.iter().sum();
            assert!(
                total <= SHADOW_BUDGET.max(2 * live),
                "{live} live: {total} shadows are within the budget"
            );
        }
        // A lone particle gets all its marks.
        let mut lone = Budget::new(1);
        assert!((0..MAX_MARKS).all(|index| lone.allows(index, &dot)));
    }

    #[test]
    fn each_intensity_has_more_marks_and_only_vivid_has_glints() {
        for shape in SHAPES {
            let marks = |intensity| marks_of(&at(shape, intensity, true), Motion::Full);
            let (calm, standard, vivid) = (
                marks(VisualIntensity::Calm),
                marks(VisualIntensity::Standard),
                marks(VisualIntensity::Vivid),
            );
            // Standard adds the glass bead (a rim ring and a highlight) and a bigger scatter.
            assert!(standard.len() >= calm.len() + 2, "{shape:?}");
            assert!(vivid.len() > standard.len(), "{shape:?}");
            let glints = |marks: &[Mark]| {
                marks
                    .iter()
                    .filter(|mark| matches!(mark, Mark::Cross { .. }))
                    .count()
            };
            assert_eq!((glints(&calm), glints(&standard)), (0, 0), "{shape:?}");
            assert!(glints(&vivid) >= 1, "{shape:?}");
        }
    }

    #[test]
    fn the_mark_cap_shrinks_with_the_crowd_but_keeps_the_core_and_the_budget() {
        assert_eq!(mark_cap(0), MAX_MARKS);
        assert_eq!(mark_cap(1), MAX_MARKS);
        let mut previous = MAX_MARKS;
        for live in 1..=512 {
            let cap = mark_cap(live);
            assert!(cap <= previous, "never grows with more particles");
            assert!(cap >= MIN_MARKS, "the core light always stays");
            if cap > MIN_MARKS {
                assert!(
                    cap * live <= MARK_BUDGET,
                    "{live} particles stay within the budget"
                );
            }
            previous = cap;
        }
    }

    #[test]
    fn reduced_motion_keeps_only_the_core_light_at_every_intensity() {
        for shape in SHAPES {
            let calm = marks_of(&at(shape, VisualIntensity::Calm, true), Motion::Reduced);
            assert_eq!(calm.len(), 1);
            for intensity in [VisualIntensity::Standard, VisualIntensity::Vivid] {
                for dark in [true, false] {
                    assert_eq!(
                        marks_of(&at(shape, intensity, dark), Motion::Reduced),
                        calm,
                        "{shape:?} {intensity:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn vivid_blooms_lighter_on_dark_and_lays_a_deeper_film_on_light() {
        let bloom = |dark| {
            let marks = marks_of(
                &at(VisualShape::Point, VisualIntensity::Vivid, dark),
                Motion::Full,
            );
            let mut glows = marks.iter().filter_map(|mark| match *mark {
                Mark::Glow { rgb, radius, .. } => Some((rgb, radius)),
                _ => None,
            });
            let (_, core) = glows.next().expect("the core light");
            let (rgb, radius) = glows.next().expect("vivid adds a second, larger glow");
            assert!(radius > core);
            rgb
        };
        let calm_core = tinted([0.5, 0.72, 0.92], 41, 0.9);
        let (dark, light) = (bloom(true), bloom(false));
        for channel in 0..3 {
            assert!(dark[channel] >= calm_core[channel], "dark brightens");
            assert!(light[channel] <= calm_core[channel], "light deepens");
        }
        assert_ne!(dark, light);
    }

    #[test]
    fn every_intensity_gives_finite_valid_marks_within_the_capacity() {
        let extremes = [0.0, 1.0, -1.0, 1e9, f32::NAN, f32::INFINITY];
        for shape in SHAPES {
            for motion in [Motion::Full, Motion::Reduced] {
                for intensity in [
                    VisualIntensity::Calm,
                    VisualIntensity::Standard,
                    VisualIntensity::Vivid,
                ] {
                    for (large, dark) in [(true, true), (true, false), (false, true)] {
                        for &age in &extremes {
                            for &strength in &extremes {
                                for &radius in &extremes {
                                    let look = look(
                                        &ParticleInput {
                                            shape,
                                            age,
                                            opacity: age,
                                            radius,
                                            strength,
                                            serial: u64::MAX,
                                            large,
                                            intensity,
                                            dark,
                                            rgb: [1.0, 0.0, 0.5],
                                        },
                                        motion,
                                    );
                                    assert!(look.marks().count() <= MAX_MARKS);
                                    for mark in look.marks() {
                                        let (numbers, opacity) = match *mark {
                                            Mark::Glow {
                                                offset,
                                                radius,
                                                rgb,
                                                opacity,
                                            }
                                            | Mark::Haze {
                                                offset,
                                                radius,
                                                rgb,
                                                opacity,
                                            }
                                            | Mark::Dot {
                                                offset,
                                                radius,
                                                rgb,
                                                opacity,
                                            } => (
                                                vec![
                                                    offset[0], offset[1], radius, rgb[0], rgb[1],
                                                    rgb[2],
                                                ],
                                                opacity,
                                            ),
                                            Mark::Ring {
                                                offset,
                                                radius,
                                                width,
                                                rgb,
                                                opacity,
                                            } => (
                                                vec![
                                                    offset[0], offset[1], radius, width, rgb[0],
                                                    rgb[1], rgb[2],
                                                ],
                                                opacity,
                                            ),
                                            Mark::Cross {
                                                arm,
                                                width,
                                                rgb,
                                                opacity,
                                            } => {
                                                (vec![arm, width, rgb[0], rgb[1], rgb[2]], opacity)
                                            }
                                        };
                                        assert!(numbers.iter().all(|value| value.is_finite()));
                                        assert!((0.0..=1.0).contains(&opacity));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
