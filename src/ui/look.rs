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
use scatter::{Scatter, firefly_drift, lead_ring_radius, scatter, shock_outline};

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
    /// A jagged polygon (a rock fragment, flash or shock wave): a filled shape when `stroke` is 0,
    /// otherwise an outline of that width. Its corners come from [`vertices`].
    Poly {
        offset: [f32; 2],
        radius: f32,
        sides: u8,
        rotation: f32,
        seed: u32,
        stroke: f32,
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
    /// A rock's glint: three uneven pointed spikes (thin diamonds) from the centre, the longest
    /// `arm` long and `width` across. Their directions and lengths come from [`spikes`].
    Shards {
        arm: f32,
        width: f32,
        seed: u32,
        rgb: [f32; 3],
        opacity: f32,
    },
}

impl Mark {
    /// The same mark with every length (radius, offset, width, arm, stroke) multiplied by
    /// `scale`. Colour, opacity, side count, rotation and seed are not lengths and stay as they
    /// are, so a window of another size shows the same shapes, only larger or smaller.
    pub(crate) fn scaled(self, scale: f32) -> Self {
        let by = |value: f32| value * scale;
        let offset = |value: [f32; 2]| [by(value[0]), by(value[1])];
        match self {
            Self::Glow {
                offset: o,
                radius,
                rgb,
                opacity,
            } => Self::Glow {
                offset: offset(o),
                radius: by(radius),
                rgb,
                opacity,
            },
            Self::Haze {
                offset: o,
                radius,
                rgb,
                opacity,
            } => Self::Haze {
                offset: offset(o),
                radius: by(radius),
                rgb,
                opacity,
            },
            Self::Ring {
                offset: o,
                radius,
                width,
                rgb,
                opacity,
            } => Self::Ring {
                offset: offset(o),
                radius: by(radius),
                width: by(width),
                rgb,
                opacity,
            },
            Self::Dot {
                offset: o,
                radius,
                rgb,
                opacity,
            } => Self::Dot {
                offset: offset(o),
                radius: by(radius),
                rgb,
                opacity,
            },
            Self::Poly {
                offset: o,
                radius,
                sides,
                rotation,
                seed,
                stroke,
                rgb,
                opacity,
            } => Self::Poly {
                offset: offset(o),
                radius: by(radius),
                sides,
                rotation,
                seed,
                stroke: by(stroke),
                rgb,
                opacity,
            },
            Self::Cross {
                arm,
                width,
                rgb,
                opacity,
            } => Self::Cross {
                arm: by(arm),
                width: by(width),
                rgb,
                opacity,
            },
            Self::Shards {
                arm,
                width,
                seed,
                rgb,
                opacity,
            } => Self::Shards {
                arm: by(arm),
                width: by(width),
                seed,
                rgb,
                opacity,
            },
        }
    }
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

/// Warm yellow-green of a firefly: always the bright night colour, whatever the appearance.
const FIREFLY: [f32; 3] = [0.78, 0.92, 0.38];

/// The colour a particle's spread is taken around: the situation's own colour, except that the
/// autumn fireflies have a colour of their own (the situation's dark indigo is the night they
/// light).
fn light_colour(shape: VisualShape, rgb: [f32; 3]) -> [f32; 3] {
    match shape {
        VisualShape::Firefly => FIREFLY,
        _ => rgb,
    }
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

/// Three spikes from a rock's glint: where each points (an angle, in radians) and how long it is
/// (a share of the full arm). The first points anywhere; the others are uneven turns from it, so
/// no two point opposite and it never reads as a plus sign; the lengths differ too. The same
/// `seed` always gives the same spikes.
pub(crate) fn spikes(seed: u32) -> [(f32, f32); 3] {
    let unit = |salt: u32| {
        let mixed = seed
            .wrapping_add(salt.wrapping_mul(0x9E37_79B9))
            .wrapping_mul(0x85EB_CA6B);
        ((mixed ^ (mixed >> 15)) % 1000) as f32 / 1000.0
    };
    let first = TAU * unit(0);
    [
        (first, 1.0),
        (first + TAU * (0.27 + 0.08 * unit(1)), 0.5 + 0.2 * unit(2)),
        (first + TAU * (0.60 + 0.10 * unit(3)), 0.7 + 0.2 * unit(4)),
    ]
}

/// The glint on a particle: a thin plus sign, except for a rock, whose light is three uneven
/// pointed spikes (a rock has no neat right angles).
fn glint(input: &ParticleInput, arm: f32, width: f32, rgb: [f32; 3], opacity: f32) -> Mark {
    let rgb = lighten(rgb, 0.6);
    if input.shape == VisualShape::Impact {
        Mark::Shards {
            arm: arm * 2.0,
            width: width * 2.4,
            seed: seed(input.serial, 740),
            rgb,
            opacity,
        }
    } else {
        Mark::Cross {
            arm,
            width,
            rgb,
            opacity,
        }
    }
}

/// The place and shape of one polygon, so a glow can be laid round exactly the same shape.
#[derive(Clone, Copy)]
pub(crate) struct Facet {
    pub(crate) offset: [f32; 2],
    pub(crate) radius: f32,
    pub(crate) sides: u8,
    pub(crate) rotation: f32,
    pub(crate) seed: u32,
}

/// An angular glow: filled copies of `facet` scaled up by each layer's first number and drawn at
/// that layer's share of `opacity`, so the light fades outward in the shape's own corners. It
/// stands in for the round blurred shadow where the shape must have no round edge.
pub(crate) fn facet_glow(
    look: &mut Look,
    facet: &Facet,
    rgb: [f32; 3],
    opacity: f32,
    layers: &[(f32, f32)],
) {
    for &(scale, share) in layers {
        let opacity = opacity * share;
        if opacity <= 0.0 {
            continue;
        }
        look.push(Mark::Poly {
            offset: facet.offset,
            radius: facet.radius * scale,
            sides: facet.sides,
            rotation: facet.rotation,
            seed: facet.seed,
            stroke: 0.0,
            rgb,
            opacity,
        });
    }
}

/// A value for shaping the `index`-th polygon of this particle, from its serial.
pub(crate) fn seed(serial: u64, index: u64) -> u32 {
    (mix(serial ^ index.wrapping_mul(0x2545_f491_4f6c_dd1d)) >> 32) as u32
}

/// The most corners a polygon mark has.
pub(crate) const MAX_SIDES: usize = 6;

/// The corners of a jagged polygon around its centre, relative to it: `sides` corners (3 to
/// [`MAX_SIDES`]) spread round by `rotation`, each nudged along and in or out by amounts taken
/// from `seed`, so a rock fragment is uneven but the same seed always gives the same shape. The
/// nudge stays inside each corner's own sector, so the outline never crosses itself.
pub(crate) fn vertices(
    sides: u8,
    radius: f32,
    rotation: f32,
    seed: u32,
) -> ([[f32; 2]; MAX_SIDES], usize) {
    let count = usize::from(sides).clamp(3, MAX_SIDES);
    let radius = finite_or_zero(radius).max(0.0);
    let rotation = finite_or_zero(rotation);
    let sector = TAU / count as f32;
    let mut corners = [[0.0; 2]; MAX_SIDES];
    for (index, corner) in corners.iter_mut().enumerate().take(count) {
        let bits = mix(u64::from(seed) << 8 | index as u64);
        let along = (bits & 0xffff) as f32 / 65_535.0 - 0.5;
        let reach = ((bits >> 16) & 0xffff) as f32 / 65_535.0;
        let angle = rotation + sector * (index as f32 + 0.5 * along);
        let length = radius * (0.55 + 0.9 * reach);
        *corner = [angle.cos() * length, angle.sin() * length];
    }
    (corners, count)
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
        let rock = input.shape == VisualShape::Impact;
        if rock {
            // Angular layers of the core's own shape instead of a round glow.
            let facets = Facet {
                offset: [0.0, 0.0],
                radius: core,
                sides: 6,
                rotation: 0.2,
                seed: seed(input.serial, 5),
            };
            if dark {
                facet_glow(
                    look,
                    &facets,
                    lighten(rgb, 0.35),
                    opacity,
                    &[(2.0, 0.22), (3.0, 0.1)],
                );
            } else {
                facet_glow(look, &facets, deepen(rgb, 0.75), opacity, &[(1.6, 0.2)]);
            }
        } else if dark {
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
        let rim = if dark {
            lighten(rgb, 0.5)
        } else {
            deepen(rgb, 0.6)
        };
        if input.shape == VisualShape::Impact {
            // A faceted rock: a jagged outline and a small bright facet.
            look.push(Mark::Poly {
                offset: [0.0, 0.0],
                radius: core,
                sides: 6,
                rotation: TAU * phase,
                seed: seed(input.serial, 710),
                stroke: 1.0,
                rgb: rim,
                opacity: opacity * 0.55,
            });
            look.push(Mark::Poly {
                offset: [-core * 0.32, -core * 0.34],
                radius: (core * 0.3).max(1.0),
                sides: 3,
                rotation: TAU * second,
                seed: seed(input.serial, 711),
                stroke: 0.0,
                rgb: [1.0, 1.0, 1.0],
                opacity: opacity * 0.6,
            });
        } else {
            look.push(Mark::Ring {
                offset: [0.0, 0.0],
                radius: core,
                width: 1.0,
                rgb: rim,
                opacity: opacity * 0.55,
            });
            look.push(Mark::Dot {
                offset: [-core * 0.32, -core * 0.34],
                radius: (core * 0.24).max(0.8),
                rgb: [1.0, 1.0, 1.0],
                opacity: opacity * 0.6,
            });
        }
    }
    if !vivid {
        return;
    }
    // An out-of-focus disc beside the particle, its place and size from the serial.
    let bearing = TAU * phase;
    let distance = core * (1.8 + 1.2 * second);
    if input.shape == VisualShape::Impact {
        // A rock's out-of-focus shape is angular too.
        look.push(Mark::Poly {
            offset: [bearing.cos() * distance, bearing.sin() * distance],
            radius: core * (0.9 + 0.5 * phase) * 1.3,
            sides: 5,
            rotation: TAU * second,
            seed: seed(input.serial, 720),
            stroke: 0.0,
            rgb,
            opacity: opacity * 0.14,
        });
    } else {
        look.push(Mark::Haze {
            offset: [bearing.cos() * distance, bearing.sin() * distance],
            radius: core * (0.9 + 0.5 * phase) * 1.6,
            rgb,
            opacity: opacity * 0.2,
        });
    }
    look.push(glint(
        input,
        core * 1.8,
        1.0,
        rgb,
        opacity * 0.7 * twinkle(age),
    ));
    match input.shape {
        VisualShape::Point | VisualShape::Impact => {
            // Rainbow fringes just outside the ripple or shock ring.
            let ring = lead_ring_radius(input.shape, radius, age);
            for (index, forward) in [true, false].into_iter().enumerate() {
                let radius = ring * (1.08 + 0.1 * index as f32);
                let tint = rotate(rgb, forward, 0.75);
                if input.shape == VisualShape::Impact {
                    // Rock has no round edge: the fringes are jagged polygons too.
                    // Scaled copies of the shock outline, so they stay just outside it.
                    let (turn, outline) = shock_outline(input.serial);
                    look.push(Mark::Poly {
                        offset: [0.0, 0.0],
                        radius,
                        sides: 6,
                        rotation: turn,
                        seed: outline,
                        stroke: 1.0,
                        rgb: tint,
                        opacity: opacity * 0.35,
                    });
                } else {
                    look.push(Mark::Ring {
                        offset: [0.0, 0.0],
                        radius,
                        width: 1.0,
                        rgb: tint,
                        opacity: opacity * 0.35,
                    });
                }
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
        VisualShape::SlowGlow | VisualShape::Firefly => {}
    }
}

/// The marks of one particle. Reduced motion keeps only the still core light: no ripple, shard,
/// trail or wobble spreads.
pub(crate) fn look(input: &ParticleInput, motion: Motion) -> Look {
    let age = finite_or_zero(input.age).clamp(0.0, 1.0);
    let opacity = finite_or_zero(input.opacity).clamp(0.0, 1.0);
    let radius = finite_or_zero(input.radius).max(0.0);
    let strength = finite_or_zero(input.strength).clamp(0.0, 1.0);
    // A firefly only exists in the night scene, which is dark in either appearance (the night
    // film sees to that), so it always has the bright dark-window look.
    let dark = input.dark || input.shape == VisualShape::Firefly;
    let rgb = tinted(light_colour(input.shape, input.rgb), input.serial, strength);
    let (phase, second) = unit_pair(input.serial ^ 0xa5a5_a5a5_5a5a_5a5a);
    let mut look = Look::new();
    // A firefly is always drawn, at every intensity, whatever its event says.
    if !input.large && input.shape != VisualShape::Firefly {
        // Small particles show only when the viewer asked for more than the calm scene: a
        // faint, tiny, still dot, the cheapest possible mark; vivid adds a brief twinkle.
        if input.intensity != VisualIntensity::Calm {
            let dot = (radius * 0.5).clamp(0.8, 2.0);
            if input.shape == VisualShape::Impact {
                // A small rock chip is a tiny angular fragment, not a round dot.
                look.push(Mark::Poly {
                    offset: [0.0, 0.0],
                    radius: dot * 1.4,
                    sides: 4,
                    rotation: TAU * phase,
                    seed: seed(input.serial, 900),
                    stroke: 0.0,
                    rgb,
                    opacity: opacity * 0.7,
                });
            } else {
                look.push(Mark::Dot {
                    offset: [0.0, 0.0],
                    radius: dot,
                    rgb,
                    opacity: opacity * 0.7,
                });
            }
            if input.intensity == VisualIntensity::Vivid && motion != Motion::Reduced {
                look.push(glint(
                    input,
                    dot * 3.0,
                    0.8,
                    rgb,
                    opacity * 0.5 * twinkle(age),
                ));
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
    let rock = input.shape == VisualShape::Impact;
    // A firefly wanders from where it was lit; every other light stays where it was born, and
    // reduced motion keeps even a firefly still.
    let centre = if input.shape == VisualShape::Firefly && !still {
        firefly_drift(input.serial, age)
    } else {
        [0.0, 0.0]
    };
    // A firefly's glow is wider and softer than the others'.
    let reach = if input.shape == VisualShape::Firefly {
        3.2
    } else {
        2.4
    };
    // The core light. A rock's is a faceted shape, not a round glow; everything else keeps the
    // soft round light.
    if rock {
        look.push(Mark::Poly {
            offset: [0.0, 0.0],
            radius: core,
            sides: 6,
            rotation: 0.2,
            seed: seed(input.serial, 5),
            stroke: 0.0,
            rgb,
            opacity,
        });
    } else {
        look.push(Mark::Glow {
            offset: centre,
            radius: core * wobble,
            rgb,
            opacity,
        });
    }
    if !still {
        // A soft glow around the core at every intensity; light windows get a deeper colour.
        let (halo, tone) = if dark {
            (lighten(rgb, 0.2), 0.28)
        } else {
            (deepen(rgb, 0.8), 0.22)
        };
        if rock {
            // Concentric larger copies of the core's own shape, fainter outwards: an angular glow.
            let facets = Facet {
                offset: [0.0, 0.0],
                radius: core,
                sides: 6,
                rotation: 0.2,
                seed: seed(input.serial, 5),
            };
            facet_glow(
                &mut look,
                &facets,
                halo,
                opacity * tone,
                &[(1.7, 0.65), (2.6, 0.35)],
            );
        } else {
            look.push(Mark::Haze {
                offset: centre,
                radius: core * wobble * reach,
                rgb: halo,
                opacity: opacity * tone,
            });
        }
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
                dark,
            },
        );
    }
    if !still && input.intensity != VisualIntensity::Calm && input.shape != VisualShape::Firefly {
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

    const SHAPES: [VisualShape; 4] = [
        VisualShape::Point,
        VisualShape::Impact,
        VisualShape::SlowGlow,
        VisualShape::Firefly,
    ];
    const ORIGINAL_SHAPES: [VisualShape; 3] = [
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
                // A rock's still core is a polygon, the others' a round glow.
                let (offset, radius) = match marks[0] {
                    Mark::Glow { offset, radius, .. } | Mark::Poly { offset, radius, .. } => {
                        (offset, radius)
                    }
                    other => panic!("only the core remains: {other:?}"),
                };
                assert_eq!(
                    matches!(marks[0], Mark::Poly { stroke: 0.0, .. }),
                    shape == VisualShape::Impact,
                    "{shape:?}: only a rock's still core is a polygon"
                );
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
                    Mark::Ring { radius, .. }
                    | Mark::Poly {
                        radius,
                        stroke: 0.1..,
                        ..
                    } => Some(*radius),
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
    fn a_polygons_corners_are_the_same_for_the_same_seed_and_never_cross_themselves() {
        for sides in 3..=6_u8 {
            for seed in [0_u32, 1, 77, u32::MAX] {
                let (a, count) = vertices(sides, 10.0, 0.4, seed);
                assert_eq!((a, count), vertices(sides, 10.0, 0.4, seed));
                assert_eq!(count, usize::from(sides));
                // Walking the corners goes round the centre one way, so the outline is a simple
                // star-shaped polygon: every corner's bearing is ahead of the previous one.
                let bearing = |p: [f32; 2]| p[1].atan2(p[0]);
                let mut total = 0.0;
                for index in 0..count {
                    let (from, to) = (a[index], a[(index + 1) % count]);
                    let step = (bearing(to) - bearing(from)).rem_euclid(TAU);
                    assert!(
                        step > 0.0 && step < std::f32::consts::PI,
                        "{sides} sides, seed {seed}"
                    );
                    total += step;
                }
                assert!(
                    (total - TAU).abs() < 1e-3,
                    "one turn round the centre, got {total}"
                );
                for corner in &a[..count] {
                    let length = corner[0].hypot(corner[1]);
                    assert!(
                        (5.4..=14.6).contains(&length),
                        "uneven but bounded, got {length}"
                    );
                }
            }
        }
        assert_ne!(
            vertices(5, 10.0, 0.4, 1).0,
            vertices(5, 10.0, 0.4, 2).0,
            "seeds differ"
        );
    }

    #[test]
    fn polygon_corners_stay_finite_for_extreme_inputs() {
        for radius in [0.0, -3.0, 1e30, f32::NAN, f32::INFINITY] {
            for rotation in [0.0, 1e30, f32::NAN, f32::NEG_INFINITY] {
                for sides in [0_u8, 1, 3, 6, 200] {
                    let (corners, count) = vertices(sides, radius, rotation, 5);
                    assert!((3..=MAX_SIDES).contains(&count));
                    assert!(corners[..count].iter().flatten().all(|v| v.is_finite()));
                }
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
        // The original three share one colour in both windows; a firefly and a star are deeper
        // on light (tested with them).
        for shape in ORIGINAL_SHAPES {
            let dark = marks_of(&at(shape, VisualIntensity::Calm, true), Motion::Full);
            let light = marks_of(&at(shape, VisualIntensity::Calm, false), Motion::Full);
            assert!(!dark.iter().any(|mark| matches!(mark, Mark::Cross { .. })));
            let without_halo = |marks: &[Mark]| -> Vec<Mark> {
                marks
                    .iter()
                    .enumerate()
                    // A rock's halo is the two polygons right after its core.
                    .filter(|(index, mark)| {
                        let halo_layer = shape == VisualShape::Impact && (1..=2).contains(index);
                        !(matches!(mark, Mark::Haze { .. }) || halo_layer)
                    })
                    .map(|(_, mark)| *mark)
                    .collect()
            };
            assert_eq!(without_halo(&dark), without_halo(&light), "{shape:?}");
        }
    }

    /// The core light and its first halo: (radius, colour) of each. A rock's are polygons, the
    /// others' a round glow and a blur.
    fn core_and_halo(marks: &[Mark]) -> ((f32, [f32; 3]), (f32, [f32; 3])) {
        match (marks[0], marks[1]) {
            (
                Mark::Glow { radius, rgb, .. },
                Mark::Haze {
                    radius: halo_radius,
                    rgb: halo_rgb,
                    ..
                },
            ) => ((radius, rgb), (halo_radius, halo_rgb)),
            (
                Mark::Poly {
                    radius,
                    rgb,
                    stroke: 0.0,
                    ..
                },
                Mark::Poly {
                    radius: halo_radius,
                    rgb: halo_rgb,
                    stroke: 0.0,
                    ..
                },
            ) => ((radius, rgb), (halo_radius, halo_rgb)),
            other => panic!("the core is not followed by its halo: {other:?}"),
        }
    }

    #[test]
    fn every_intensity_glows_softly_around_the_core_lighter_on_dark_and_deeper_on_light() {
        for shape in ORIGINAL_SHAPES {
            for level in [
                VisualIntensity::Calm,
                VisualIntensity::Standard,
                VisualIntensity::Vivid,
            ] {
                let marks = |dark| marks_of(&at(shape, level, dark), Motion::Full);
                let (dark, light) = (marks(true), marks(false));
                let rock = shape == VisualShape::Impact;
                assert_eq!(
                    matches!(dark[0], Mark::Poly { .. }),
                    rock,
                    "a rock's core is a polygon, the others' a round glow"
                );
                let ((core, core_rgb), (dark_radius, dark_rgb)) = core_and_halo(&dark);
                let (_, (_, light_rgb)) = core_and_halo(&light);
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
                    .filter(|mark| matches!(mark, Mark::Cross { .. } | Mark::Shards { .. }))
                    .count()
            };
            assert_eq!((glints(&calm), glints(&standard)), (0, 0), "{shape:?}");
            // A firefly's light is its glow; only the others add a glint at vivid.
            assert_eq!(
                glints(&vivid) >= 1,
                shape != VisualShape::Firefly,
                "{shape:?}"
            );
        }
    }

    #[test]
    fn a_rocks_glint_is_uneven_spikes_never_a_plus_sign_and_the_others_keep_the_plus() {
        for shape in SHAPES {
            for large in [true, false] {
                let marks = marks_of(
                    &ParticleInput {
                        large,
                        ..at(shape, VisualIntensity::Vivid, true)
                    },
                    Motion::Full,
                );
                let crosses = marks
                    .iter()
                    .filter(|mark| matches!(mark, Mark::Cross { .. }))
                    .count();
                let spikes = marks
                    .iter()
                    .filter(|mark| matches!(mark, Mark::Shards { .. }))
                    .count();
                if shape == VisualShape::Impact {
                    assert_eq!((crosses, spikes), (0, 1), "a rock has one burst of spikes");
                } else if shape == VisualShape::Firefly {
                    // A firefly's light is its glow; the cross belongs to the star.
                    assert_eq!((crosses, spikes), (0, 0), "a firefly has no glint");
                } else {
                    assert_eq!((crosses, spikes), (1, 0), "{shape:?} keeps its plus sign");
                }
            }
        }
    }

    #[test]
    fn glint_spikes_are_uneven_never_opposite_and_the_same_for_the_same_seed() {
        let mut longest_first = 0;
        for seed in (0..3000_u32).map(|n| n.wrapping_mul(2_654_435_761)) {
            let found = spikes(seed);
            assert_eq!(found, spikes(seed), "deterministic");
            let angles = found.map(|(angle, _)| angle);
            for a in 0..3 {
                for b in a + 1..3 {
                    // Compare the turn between two spikes to a half turn: a plus sign has them
                    // exactly opposite.
                    let turn = (angles[a] - angles[b]).rem_euclid(TAU);
                    assert!(
                        (turn - std::f32::consts::PI).abs() > 0.3,
                        "seed {seed}: spikes {a} and {b} point nearly opposite"
                    );
                }
            }
            let lengths = found.map(|(_, share)| share);
            assert!(lengths.iter().all(|share| (0.4..=1.0).contains(share)));
            assert!(lengths[0] > lengths[1] && lengths[0] > lengths[2]);
            assert_ne!(lengths[1], lengths[2], "uneven lengths");
            if found[0].0 > std::f32::consts::PI {
                longest_first += 1;
            }
        }
        // The direction is spread over the whole circle, not fixed.
        assert!((1000..2000).contains(&longest_first), "{longest_first}");
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
    fn a_rock_look_has_no_ring_or_round_dot_at_any_intensity_in_dark_or_light_large_or_small() {
        for level in [
            VisualIntensity::Calm,
            VisualIntensity::Standard,
            VisualIntensity::Vivid,
        ] {
            for dark in [true, false] {
                for step in 0..=20 {
                    for large in [true, false] {
                        let marks = marks_of(
                            &ParticleInput {
                                age: step as f32 / 20.0,
                                large,
                                ..at(VisualShape::Impact, level, dark)
                            },
                            Motion::Full,
                        );
                        assert!(
                            !marks.iter().any(|mark| matches!(
                                mark,
                                Mark::Ring { .. }
                                    | Mark::Dot { .. }
                                    | Mark::Glow { .. }
                                    | Mark::Haze { .. }
                            )),
                            "{level:?} dark={dark} step {step} large={large}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_rainbow_fringes_of_a_rock_are_scaled_copies_of_its_shock_outline() {
        let marks = marks_of(
            &ParticleInput {
                age: 0.5,
                ..at(VisualShape::Impact, VisualIntensity::Vivid, true)
            },
            Motion::Full,
        );
        let (turn, outline) = shock_outline(41);
        let outlines: Vec<(f32, u32, f32)> = marks
            .iter()
            .filter_map(|mark| match *mark {
                Mark::Poly {
                    radius,
                    rotation,
                    seed,
                    stroke,
                    ..
                } if stroke > 0.0 && seed == outline && rotation == turn => {
                    Some((radius, seed, stroke))
                }
                _ => None,
            })
            .collect();
        // The sharp outline, its wide faint copy, the rim and the two fringes share the shape;
        // the fringes are the larger ones, outside the shock outline.
        let shock = lead_ring_radius(VisualShape::Impact, 6.0, 0.5);
        let outside = outlines.iter().filter(|o| o.0 > shock * 1.07).count();
        assert_eq!(outside, 2, "{outlines:?}");
    }

    #[test]
    fn reduced_motion_keeps_only_the_core_light_at_every_intensity() {
        for shape in SHAPES {
            for dark in [true, false] {
                let calm = marks_of(&at(shape, VisualIntensity::Calm, dark), Motion::Reduced);
                assert_eq!(calm.len(), 1);
                for intensity in [VisualIntensity::Standard, VisualIntensity::Vivid] {
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
                                            Mark::Poly {
                                                offset,
                                                radius,
                                                sides,
                                                rotation,
                                                seed,
                                                stroke,
                                                rgb,
                                                opacity,
                                            } => {
                                                let (corners, count) =
                                                    vertices(sides, radius, rotation, seed);
                                                assert!((3..=MAX_SIDES).contains(&count));
                                                let mut numbers = vec![
                                                    offset[0], offset[1], radius, rotation, stroke,
                                                    rgb[0], rgb[1], rgb[2],
                                                ];
                                                numbers.extend(
                                                    corners[..count].iter().flatten().copied(),
                                                );
                                                (numbers, opacity)
                                            }
                                            Mark::Cross {
                                                arm,
                                                width,
                                                rgb,
                                                opacity,
                                            } => {
                                                (vec![arm, width, rgb[0], rgb[1], rgb[2]], opacity)
                                            }
                                            Mark::Shards {
                                                arm,
                                                width,
                                                seed,
                                                rgb,
                                                opacity,
                                            } => {
                                                let mut numbers =
                                                    vec![arm, width, rgb[0], rgb[1], rgb[2]];
                                                for (angle, share) in spikes(seed) {
                                                    numbers.extend([angle, share]);
                                                }
                                                (numbers, opacity)
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

    #[test]
    fn scaling_a_mark_changes_only_its_lengths() {
        let rgb = [0.1, 0.2, 0.3];
        let marks = [
            Mark::Glow {
                offset: [1.0, -2.0],
                radius: 3.0,
                rgb,
                opacity: 0.4,
            },
            Mark::Haze {
                offset: [1.0, -2.0],
                radius: 3.0,
                rgb,
                opacity: 0.4,
            },
            Mark::Ring {
                offset: [1.0, -2.0],
                radius: 3.0,
                width: 0.5,
                rgb,
                opacity: 0.4,
            },
            Mark::Dot {
                offset: [1.0, -2.0],
                radius: 3.0,
                rgb,
                opacity: 0.4,
            },
            Mark::Poly {
                offset: [1.0, -2.0],
                radius: 3.0,
                sides: 5,
                rotation: 0.7,
                seed: 9,
                stroke: 1.5,
                rgb,
                opacity: 0.4,
            },
            Mark::Cross {
                arm: 3.0,
                width: 0.5,
                rgb,
                opacity: 0.4,
            },
            Mark::Shards {
                arm: 3.0,
                width: 0.5,
                seed: 9,
                rgb,
                opacity: 0.4,
            },
        ];
        for mark in marks {
            // A window of the default size draws exactly what it always drew.
            assert_eq!(mark.scaled(1.0), mark);
            // Twice as big: the lengths double, and nothing else moves.
            let twice = match mark {
                Mark::Glow { .. } => Mark::Glow {
                    offset: [2.0, -4.0],
                    radius: 6.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Haze { .. } => Mark::Haze {
                    offset: [2.0, -4.0],
                    radius: 6.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Ring { .. } => Mark::Ring {
                    offset: [2.0, -4.0],
                    radius: 6.0,
                    width: 1.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Dot { .. } => Mark::Dot {
                    offset: [2.0, -4.0],
                    radius: 6.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Poly { .. } => Mark::Poly {
                    offset: [2.0, -4.0],
                    radius: 6.0,
                    sides: 5,
                    rotation: 0.7,
                    seed: 9,
                    stroke: 3.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Cross { .. } => Mark::Cross {
                    arm: 6.0,
                    width: 1.0,
                    rgb,
                    opacity: 0.4,
                },
                Mark::Shards { .. } => Mark::Shards {
                    arm: 6.0,
                    width: 1.0,
                    seed: 9,
                    rgb,
                    opacity: 0.4,
                },
            };
            assert_eq!(mark.scaled(2.0), twice);
        }
    }

    /// The colour of a mark that lights (a core, halo, dot, ...).
    fn mark_rgb(mark: &Mark) -> [f32; 3] {
        match *mark {
            Mark::Glow { rgb, .. }
            | Mark::Haze { rgb, .. }
            | Mark::Dot { rgb, .. }
            | Mark::Ring { rgb, .. }
            | Mark::Cross { rgb, .. }
            | Mark::Poly { rgb, .. }
            | Mark::Shards { rgb, .. } => rgb,
        }
    }

    const LEVELS: [VisualIntensity; 3] = [
        VisualIntensity::Calm,
        VisualIntensity::Standard,
        VisualIntensity::Vivid,
    ];

    #[test]
    fn fireflies_are_warm_yellow_green_deeper_on_light_and_not_the_situations_colour() {
        let mean_core = |shape, dark| {
            let mut sum = [0.0_f32; 3];
            for serial in 0..100_u64 {
                let own = marks_of(
                    &ParticleInput {
                        serial,
                        ..at(shape, VisualIntensity::Calm, dark)
                    },
                    Motion::Full,
                );
                // The situation's dark indigo does not tint what it lights.
                let indigo = marks_of(
                    &ParticleInput {
                        serial,
                        rgb: [0.16, 0.20, 0.45],
                        ..at(shape, VisualIntensity::Calm, dark)
                    },
                    Motion::Full,
                );
                assert_eq!(mark_rgb(&own[0]), mark_rgb(&indigo[0]));
                let rgb = mark_rgb(&own[0]);
                assert!(rgb.iter().all(|channel| (0.0..=1.0).contains(channel)));
                for channel in 0..3 {
                    sum[channel] += rgb[channel] / 100.0;
                }
            }
            sum
        };
        let [r, g, b] = mean_core(VisualShape::Firefly, true);
        assert!(
            g > b + 0.25 && r > b + 0.1 && g >= r,
            "yellow-green: {r} {g} {b}"
        );
        let (dark, light) = (
            mean_core(VisualShape::Firefly, true),
            mean_core(VisualShape::Firefly, false),
        );
        assert_eq!(
            dark, light,
            "the same bright yellow-green on a light and a dark window"
        );
        assert!(
            dark.iter().sum::<f32>() > 1.6,
            "bright, never deepened: {dark:?}"
        );
    }

    #[test]
    fn a_firefly_wanders_a_little_from_where_it_was_lit_and_a_reduced_motion_one_stays() {
        let centre = |serial, age, motion| match marks_of(
            &ParticleInput {
                serial,
                age,
                ..at(VisualShape::Firefly, VisualIntensity::Calm, true)
            },
            motion,
        )[0]
        {
            Mark::Glow { offset, .. } => offset,
            other => panic!("a firefly's core is a glow: {other:?}"),
        };
        for serial in [1_u64, 2, 77, 4_000_000] {
            assert_eq!(
                centre(serial, 0.0, Motion::Full),
                [0.0, 0.0],
                "lit where it was born"
            );
            let path: Vec<[f32; 2]> = (0..=200)
                .map(|step| centre(serial, step as f32 / 200.0, Motion::Full))
                .collect();
            assert_eq!(
                path,
                (0..=200)
                    .map(|step| centre(serial, step as f32 / 200.0, Motion::Full))
                    .collect::<Vec<_>>()
            );
            for pair in path.windows(2) {
                let jump = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                assert!(jump < 1.5, "a gentle drift, not a jump: {jump}");
            }
            let (xs, ys): (Vec<f32>, Vec<f32>) = path.iter().map(|p| (p[0], p[1])).unzip();
            let range = |values: &[f32]| {
                values.iter().cloned().fold(f32::MIN, f32::max)
                    - values.iter().cloned().fold(f32::MAX, f32::min)
            };
            // A few per cent of the default window's width, never beyond a tenth of it (96 px),
            // and only a little way down from where it was lit (the lower-to-middle band).
            assert!(
                range(&xs) > 12.0 && range(&xs) <= 180.0,
                "wanders sideways: {}",
                range(&xs)
            );
            assert!(xs.iter().all(|x| x.abs() <= 90.0));
            assert!(ys.iter().all(|y| (-100.0..=35.0).contains(y)));
            assert!(range(&ys) > 3.0, "and up and down: {}", range(&ys));
            for age in [0.0, 0.3, 0.8, 1.0] {
                assert_eq!(
                    centre(serial, age, Motion::Reduced),
                    [0.0, 0.0],
                    "reduced motion is static"
                );
            }
        }
        assert_ne!(centre(1, 0.5, Motion::Full), centre(2, 0.5, Motion::Full));
    }

    #[test]
    fn every_level_of_a_firefly_has_sound_marks_inside_the_budget_and_light_only_deepens() {
        for shape in SHAPES
            .into_iter()
            .filter(|shape| *shape == VisualShape::Firefly)
        {
            for intensity in LEVELS {
                for dark in [true, false] {
                    for motion in [Motion::Full, Motion::Reduced] {
                        for large in [true, false] {
                            for serial in [0_u64, 1, 99, u64::MAX] {
                                for step in 0..=10 {
                                    let input = ParticleInput {
                                        age: step as f32 / 10.0,
                                        large,
                                        serial,
                                        ..at(shape, intensity, dark)
                                    };
                                    let marks = marks_of(&input, motion);
                                    assert!(
                                        marks.len() <= 12 && marks.len() <= MAX_MARKS,
                                        "{shape:?} {intensity:?}: {}",
                                        marks.len()
                                    );
                                    for mark in &marks {
                                        assert!(
                                            mark_rgb(mark).iter().all(|c| (0.0..=1.0).contains(c))
                                        );
                                        let (opacity, lengths) = match *mark {
                                            Mark::Glow {
                                                opacity,
                                                radius,
                                                offset,
                                                ..
                                            }
                                            | Mark::Haze {
                                                opacity,
                                                radius,
                                                offset,
                                                ..
                                            }
                                            | Mark::Dot {
                                                opacity,
                                                radius,
                                                offset,
                                                ..
                                            } => (opacity, vec![radius, offset[0], offset[1]]),
                                            Mark::Cross {
                                                opacity,
                                                arm,
                                                width,
                                                ..
                                            } => (opacity, vec![arm, width]),
                                            other => panic!("unexpected mark {other:?}"),
                                        };
                                        assert!(
                                            (0.0..=1.0).contains(&opacity)
                                                && lengths.iter().all(|v| v.is_finite())
                                        );
                                        if let Mark::Glow { radius, .. }
                                        | Mark::Haze { radius, .. }
                                        | Mark::Dot { radius, .. } = *mark
                                        {
                                            assert!(radius > 0.0);
                                        }
                                    }
                                    // A firefly is the same bright night light on a light and a
                                    // dark window: it never takes the deepened light-window look.
                                    assert_eq!(
                                        marks,
                                        marks_of(
                                            &ParticleInput {
                                                dark: !dark,
                                                ..input
                                            },
                                            motion
                                        ),
                                        "{shape:?} {intensity:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_firefly_gains_marks_with_each_level_and_keeps_its_core_first() {
        let count =
            |intensity| marks_of(&at(VisualShape::Firefly, intensity, true), Motion::Full).len();
        assert!(
            count(VisualIntensity::Calm) >= 2,
            "a core and a soft halo even when calm"
        );
        assert!(count(VisualIntensity::Standard) > count(VisualIntensity::Calm));
        assert!(count(VisualIntensity::Vivid) > count(VisualIntensity::Standard));
        for intensity in LEVELS {
            let marks = marks_of(&at(VisualShape::Firefly, intensity, true), Motion::Full);
            assert!(matches!(marks[0], Mark::Glow { .. }));
            // A crowd thins fireflies from the finest decoration, never the core.
            let mut budget = Budget::new(400);
            let allowed: Vec<bool> = marks
                .iter()
                .enumerate()
                .map(|(i, m)| budget.allows(i, m))
                .collect();
            assert!(allowed[0]);
        }
    }
}
