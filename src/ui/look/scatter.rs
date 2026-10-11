//! The marks that scatter around a particle: a rain drop's ripples and splash, a rock impact's
//! flash ring, shards, sparks and dust, a deep bubble's rising bubbles.
//!
//! Everything is a pure function of the event's own numbers and its age: how many pieces, where
//! each flies and how big it is come from the serial and the strength, never a random source in
//! the painter. Pieces are pushed most important first, so a frame that has to draw fewer marks
//! loses the finest ones.

use std::f32::consts::TAU;

use super::{Look, Mark, deepen, lighten, mix, seed};
use crate::{controls::VisualIntensity, situations::VisualShape};

/// What the scatter depends on. `opacity` is the particle's envelope for this frame.
pub(super) struct Scatter {
    pub(super) shape: VisualShape,
    pub(super) intensity: VisualIntensity,
    pub(super) rgb: [f32; 3],
    /// The event's radius in logical pixels, already sanitised.
    pub(super) radius: f32,
    pub(super) strength: f32,
    pub(super) age: f32,
    pub(super) opacity: f32,
    pub(super) serial: u64,
    /// The window is on a dark appearance.
    pub(super) dark: bool,
}

/// A value in 0..1 for the `index`-th piece of this particle.
fn unit(serial: u64, index: u64) -> f32 {
    let bits = mix(serial ^ index.wrapping_mul(0x2545_f491_4f6c_dd1d));
    ((bits >> 40) as f32) / 16_777_216.0
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// `base` pieces, plus up to `extra` more for a strong particle (inclusive of `base + extra`).
fn count(base: u64, extra: u64, strength: f32) -> u64 {
    base + (strength * (extra as f32 + 0.999)) as u64
}

/// A soft glow around a piece, from standard up: a blurred shadow with no hard disc. Dark
/// windows get a lighter light, light windows a deeper colour (never a brightening).
fn halo(look: &mut Look, s: &Scatter, offset: [f32; 2], radius: f32, opacity: f32) {
    let level = match s.intensity {
        VisualIntensity::Calm => return,
        VisualIntensity::Standard => 0.6,
        VisualIntensity::Vivid => 1.0,
    };
    let (rgb, tone) = if s.dark {
        (lighten(s.rgb, 0.25), 0.4)
    } else {
        (deepen(s.rgb, 0.8), 0.32)
    };
    let opacity = opacity * level * tone;
    if opacity <= 0.0 {
        // A burnt-out piece has nothing to glow; a transparent shadow would only cost a
        // primitive and a slot of the frame's budget.
        return;
    }
    look.push(Mark::Haze {
        offset,
        radius,
        rgb,
        opacity,
    });
}

/// A wide, faint ring laid on the lead ripple or shock ring, centred on its edge, so the edge
/// reads as softened on both sides. Every intensity has it.
fn ring_halo(look: &mut Look, s: &Scatter, radius: f32, width: f32, opacity: f32) {
    let band = width * 4.0 + 2.0;
    look.push(Mark::Ring {
        offset: [0.0, 0.0],
        // A quad border is drawn inside its bounds, so grow the bounds by half the band.
        radius: radius + band * 0.5,
        width: band,
        rgb: s.rgb,
        opacity: opacity * 0.18,
    });
}

/// The on-screen scale of a scatter, from the event's radius: 1.0 is a modest splash.
fn rain_unit(radius: f32) -> f32 {
    (radius * 0.7).clamp(0.5, 3.0)
}
fn rock_unit(radius: f32) -> f32 {
    (radius * 0.45).clamp(0.5, 3.0)
}
fn deep_unit(radius: f32) -> f32 {
    (radius * 0.35).clamp(0.5, 3.0)
}

/// Radius of the first ring a rain drop or rock impact sends out (the rainbow fringes follow it).
pub(super) fn lead_ring_radius(shape: VisualShape, radius: f32, age: f32) -> f32 {
    match shape {
        VisualShape::Point => rain_unit(radius) * (2.0 + 15.0 * ease_out(age)),
        VisualShape::Impact => rock_unit(radius) * (2.5 + 13.0 * ease_out(age)),
        VisualShape::SlowGlow => 0.0,
    }
}

/// The turn and the corner seed of a rock's shock outline. The rainbow fringes use the same, so
/// they are scaled copies that stay outside it instead of crossing it.
pub(super) fn shock_outline(serial: u64) -> (f32, u32) {
    (TAU * unit(serial, 2), seed(serial, 3))
}

pub(super) fn scatter(look: &mut Look, s: &Scatter) {
    // Pieces fade more gently than the core flash: the envelope's square root.
    let soft = s.opacity.sqrt();
    match s.shape {
        VisualShape::Point => rain(look, s, soft),
        VisualShape::Impact => rock(look, s, soft),
        VisualShape::SlowGlow => deep(look, s, soft),
    }
}

/// A drop landing: ripples spread out one after another and a crown of droplets is thrown up and
/// falls back.
fn rain(look: &mut Look, s: &Scatter, soft: f32) {
    let u = rain_unit(s.radius);
    let ripples = if s.intensity == VisualIntensity::Calm {
        2
    } else {
        3
    };
    for k in 0..ripples {
        let delay = 0.10 * k as f32;
        if s.age < delay {
            continue;
        }
        let t = ((s.age - delay) / (1.0 - delay)).clamp(0.0, 1.0);
        look.push(Mark::Ring {
            offset: [0.0, 0.0],
            radius: u * (2.0 + 15.0 * ease_out(t)),
            width: 1.6 * (1.0 - t) + 0.5,
            rgb: s.rgb,
            opacity: soft * 0.8 * (1.0 - t).powf(0.7) * (1.0 - 0.22 * k as f32),
        });
        if k == 0 {
            ring_halo(
                look,
                s,
                u * (2.0 + 15.0 * ease_out(t)),
                1.6 * (1.0 - t) + 0.5,
                soft * 0.8 * (1.0 - t).powf(0.7),
            );
        }
    }
    let drops = match s.intensity {
        VisualIntensity::Calm => count(4, 2, s.strength),
        VisualIntensity::Standard => count(7, 3, s.strength),
        VisualIntensity::Vivid => count(10, 6, s.strength),
    };
    let flight = s.age * 1.1;
    let halos = match s.intensity {
        VisualIntensity::Calm => 0,
        VisualIntensity::Standard => 5,
        VisualIntensity::Vivid => 8,
    };
    for i in 0..drops {
        let angle = -TAU / 4.0 + (unit(s.serial, i) - 0.5) * 2.3;
        let speed = u * (22.0 + 30.0 * unit(s.serial, 100 + i));
        let offset = [
            angle.cos() * speed * flight,
            angle.sin() * speed * flight + 0.5 * u * 62.0 * flight * flight,
        ];
        let radius = u * (0.9 + 0.9 * unit(s.serial, 200 + i)) * (1.0 - 0.5 * s.age);
        if i < halos {
            halo(look, s, offset, radius * 4.0, soft * 0.6);
        }
        look.push(Mark::Dot {
            offset,
            radius,
            rgb: lighten(s.rgb, 0.35),
            opacity: soft * 0.9,
        });
    }
}

/// A rock impact, all in hard angles: a jagged flash, a jagged shock outline, uneven shards that
/// tumble as they are thrown out and pulled down, and from standard up diamond sparks and a puff of
/// angular dust. Only the core's soft light and the halos (blurs) are round.
fn rock(look: &mut Look, s: &Scatter, soft: f32) {
    let u = rock_unit(s.radius);
    let fade = 1.0 - s.age;
    // The flash lives only for the first third of the impact.
    let flash = (1.0 - 3.0 * s.age).max(0.0);
    if flash > 0.0 {
        look.push(Mark::Poly {
            offset: [0.0, 0.0],
            radius: u * 9.0 * flash,
            sides: 6,
            rotation: 0.3,
            seed: seed(s.serial, 1),
            stroke: 0.0,
            rgb: lighten(s.rgb, 0.7),
            opacity: soft * 0.9 * flash,
        });
    }
    let ring = lead_ring_radius(VisualShape::Impact, s.radius, s.age);
    let (turn, outline) = shock_outline(s.serial);
    // A stroke wider than the outline's inner radius would overlap itself into a blob; a jagged
    // hexagon's edges are at least 0.39 of its radius from the centre.
    let reach = ring * 0.7;
    let width = (3.0 * fade + 0.8).min(reach);
    look.push(Mark::Poly {
        offset: [0.0, 0.0],
        radius: ring,
        sides: 6,
        rotation: turn,
        seed: outline,
        stroke: width,
        rgb: s.rgb,
        opacity: soft * 0.9 * fade,
    });
    // The same outline again, wide and faint, so its edge reads as softened.
    look.push(Mark::Poly {
        offset: [0.0, 0.0],
        radius: ring,
        sides: 6,
        rotation: turn,
        seed: outline,
        stroke: (width * 4.0 + 2.0).min(reach),
        rgb: s.rgb,
        opacity: soft * 0.9 * fade * 0.18,
    });
    let shards = match s.intensity {
        VisualIntensity::Calm => count(6, 3, s.strength),
        VisualIntensity::Standard => count(10, 4, s.strength),
        VisualIntensity::Vivid => count(13, 7, s.strength),
    };
    // Where shard `i` is and how big, so its halo can be laid on it afterwards.
    let shard = |i: u64| {
        let angle = TAU * (i as f32 / shards as f32 + 0.08 * unit(s.serial, i));
        let speed = u * (32.0 + 48.0 * unit(s.serial, 100 + i));
        let distance = speed * (1.0 - fade * fade);
        let offset = [
            angle.cos() * distance,
            angle.sin() * distance + 0.5 * u * 70.0 * s.age * s.age,
        ];
        let size = u * (1.3 + 3.0 * unit(s.serial, 200 + i)) * (1.0 - 0.45 * s.age);
        (offset, size)
    };
    for i in 0..shards {
        let (offset, size) = shard(i);
        // Each shard tumbles at its own rate.
        let spin = (unit(s.serial, 500 + i) - 0.5) * 9.0 * s.age;
        look.push(Mark::Poly {
            offset,
            radius: size,
            sides: 3 + (i % 3) as u8,
            rotation: TAU * unit(s.serial, 600 + i) + spin,
            seed: seed(s.serial, 700 + i),
            stroke: 0.0,
            rgb: if i % 3 == 0 {
                lighten(s.rgb, 0.25)
            } else {
                s.rgb
            },
            opacity: soft * 0.95,
        });
    }
    let sparks = match s.intensity {
        VisualIntensity::Calm => 0,
        VisualIntensity::Standard => 4,
        VisualIntensity::Vivid => 7,
    };
    let burn = (s.age * 2.2).min(1.0);
    let spark = |i: u64| {
        let angle = TAU * unit(s.serial, 400 + i);
        let speed = u * (55.0 + 40.0 * unit(s.serial, 410 + i));
        let distance = speed * (1.0 - (1.0 - burn) * (1.0 - burn));
        (angle, [angle.cos() * distance, angle.sin() * distance])
    };
    for i in 0..sparks {
        let (angle, offset) = spark(i);
        // A diamond pointing along its flight.
        look.push(Mark::Poly {
            offset,
            radius: u * 1.5,
            sides: 4,
            rotation: angle,
            seed: seed(s.serial, 800 + i),
            stroke: 0.0,
            rgb: lighten(s.rgb, 0.8),
            opacity: soft * 0.95 * (1.0 - burn),
        });
    }
    if s.intensity != VisualIntensity::Calm {
        for i in 0..4 {
            let angle = TAU * unit(s.serial, 300 + i);
            let distance = u * (2.0 + 5.0 * s.age) * (0.5 + unit(s.serial, 310 + i));
            look.push(Mark::Poly {
                offset: [angle.cos() * distance, angle.sin() * distance],
                radius: u * (3.0 + 6.0 * s.age),
                sides: 5,
                rotation: TAU * unit(s.serial, 320 + i),
                seed: seed(s.serial, 330 + i),
                stroke: 0.0,
                rgb: s.rgb,
                opacity: soft * 0.13,
            });
        }
    }
    // The halos, last.
    let halos = match s.intensity {
        VisualIntensity::Calm => 0,
        VisualIntensity::Standard => 6,
        VisualIntensity::Vivid => 10,
    };
    for i in 0..shards.min(halos) {
        let (offset, size) = shard(i);
        halo(look, s, offset, size * 2.5, soft * 0.5);
    }
    for i in 0..sparks {
        let (_, offset) = spark(i);
        halo(look, s, offset, u * 5.0, soft * 0.6 * (1.0 - burn));
    }
}

/// Bubbles that leave one after another, sway as they rise and burst at the end.
fn deep(look: &mut Look, s: &Scatter, soft: f32) {
    let u = deep_unit(s.radius);
    let bubbles = match s.intensity {
        VisualIntensity::Calm => count(3, 2, s.strength),
        VisualIntensity::Standard => count(5, 3, s.strength),
        VisualIntensity::Vivid => count(7, 4, s.strength),
    };
    for i in 0..bubbles {
        let delay = 0.28 * unit(s.serial, i);
        if s.age <= delay {
            continue;
        }
        let t = ((s.age - delay) / (1.0 - delay)).clamp(0.0, 1.0);
        let size = u * (1.8 + 4.8 * unit(s.serial, 100 + i).powi(2));
        let rise =
            u * (34.0 + 46.0 * unit(s.serial, 200 + i)) * ease_out(t * 0.9 + 0.1) * t.max(0.2);
        let sway = (TAU * (t * 1.6 + unit(s.serial, 300 + i))).sin()
            * u
            * (3.0 + 3.5 * unit(s.serial, 400 + i));
        let offset = [
            (unit(s.serial, 500 + i) - 0.5) * u * 18.0 + sway,
            u * 6.0 - rise,
        ];
        let burst = ((t - 0.9) / 0.1).clamp(0.0, 1.0);
        let alpha = soft * (t * 8.0).min(1.0) * (1.0 - 0.7 * t) * (1.0 - burst);
        halo(look, s, offset, size * 2.4, alpha * 0.5);
        look.push(Mark::Ring {
            offset,
            radius: size * (1.0 + 0.5 * burst),
            width: 1.0,
            rgb: lighten(s.rgb, 0.45),
            opacity: alpha * 0.85,
        });
        look.push(Mark::Dot {
            offset,
            radius: size,
            rgb: s.rgb,
            opacity: alpha * 0.10,
        });
        look.push(Mark::Dot {
            offset: [offset[0] - size * 0.35, offset[1] - size * 0.4],
            radius: (size * 0.26).max(0.7),
            rgb: [1.0, 1.0, 1.0],
            opacity: alpha * 0.75,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPES: [VisualShape; 3] = [
        VisualShape::Point,
        VisualShape::Impact,
        VisualShape::SlowGlow,
    ];
    const LEVELS: [VisualIntensity; 3] = [
        VisualIntensity::Calm,
        VisualIntensity::Standard,
        VisualIntensity::Vivid,
    ];

    fn scatter_of(
        shape: VisualShape,
        intensity: VisualIntensity,
        strength: f32,
        age: f32,
    ) -> Vec<Mark> {
        scatter_in(shape, intensity, strength, age, true)
    }

    fn scatter_in(
        shape: VisualShape,
        intensity: VisualIntensity,
        strength: f32,
        age: f32,
        dark: bool,
    ) -> Vec<Mark> {
        let mut look = Look::new();
        scatter(
            &mut look,
            &Scatter {
                shape,
                intensity,
                rgb: [0.5, 0.6, 0.7],
                radius: 3.0,
                strength,
                age,
                opacity: 0.8,
                serial: 12345,
                dark,
            },
        );
        look.marks().copied().collect()
    }

    fn hazes(marks: &[Mark]) -> Vec<([f32; 2], f32, [f32; 3], f32)> {
        marks
            .iter()
            .filter_map(|mark| match *mark {
                Mark::Haze {
                    offset,
                    radius,
                    rgb,
                    opacity,
                } => Some((offset, radius, rgb, opacity)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn pieces_get_a_soft_halo_from_standard_up_and_vivid_glows_more_than_standard() {
        for shape in SHAPES {
            let halos = |level| hazes(&scatter_of(shape, level, 1.0, 0.6));
            assert!(
                halos(VisualIntensity::Calm).is_empty(),
                "{shape:?} calm has none"
            );
            let (standard, vivid) = (
                halos(VisualIntensity::Standard),
                halos(VisualIntensity::Vivid),
            );
            assert!(!standard.is_empty(), "{shape:?}");
            assert!(vivid.len() >= standard.len(), "{shape:?}");
            let strongest = |halos: &[([f32; 2], f32, [f32; 3], f32)]| {
                halos.iter().map(|halo| halo.3).fold(0.0, f32::max)
            };
            assert!(
                strongest(&vivid) > strongest(&standard),
                "{shape:?} vivid is brighter"
            );
        }
    }

    #[test]
    fn a_halo_is_a_lighter_light_on_dark_and_a_deeper_colour_on_light_never_a_brightening() {
        for shape in SHAPES {
            let rgb = [0.5, 0.6, 0.7];
            let dark = hazes(&scatter_in(shape, VisualIntensity::Vivid, 1.0, 0.6, true));
            let light = hazes(&scatter_in(shape, VisualIntensity::Vivid, 1.0, 0.6, false));
            assert_eq!(dark.len(), light.len());
            for (dark, light) in dark.iter().zip(&light) {
                for ((&dark_channel, &light_channel), &base) in
                    dark.2.iter().zip(&light.2).zip(&rgb)
                {
                    assert!(dark_channel >= base, "dark brightens");
                    assert!(light_channel <= base, "light deepens");
                }
                assert!(light.3 <= dark.3, "light is the fainter film");
            }
        }
    }

    #[test]
    fn the_lead_ring_has_a_faint_wide_halo_at_every_intensity() {
        for shape in [VisualShape::Point, VisualShape::Impact] {
            for level in LEVELS {
                let rings: Vec<(f32, f32)> = scatter_of(shape, level, 0.5, 0.4)
                    .iter()
                    .filter_map(|mark| match *mark {
                        Mark::Ring { width, opacity, .. }
                        | Mark::Poly {
                            stroke: width,
                            opacity,
                            ..
                        } if width > 0.0 => Some((width, opacity)),
                        _ => None,
                    })
                    .collect();
                let (sharp, wide) = (
                    rings[0],
                    rings.iter().find(|ring| ring.0 > rings[0].0 * 2.0),
                );
                let wide =
                    wide.unwrap_or_else(|| panic!("{shape:?} {level:?} has a wide halo ring"));
                assert!(wide.1 < sharp.1, "the halo is fainter than the ring");
            }
        }
    }

    /// The thrown pieces: round dots, and the filled polygons rock throws instead.
    fn dots(marks: &[Mark]) -> Vec<([f32; 2], f32)> {
        marks
            .iter()
            .filter_map(|mark| match *mark {
                Mark::Dot { offset, radius, .. }
                | Mark::Poly {
                    offset,
                    radius,
                    stroke: 0.0,
                    ..
                } => Some((offset, radius)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn rock_is_all_hard_angles_no_circles_at_any_intensity_or_age() {
        for level in LEVELS {
            for strength in [0.0, 0.5, 1.0] {
                for step in 0..=20 {
                    let marks =
                        scatter_of(VisualShape::Impact, level, strength, step as f32 / 20.0);
                    assert!(
                        !marks
                            .iter()
                            .any(|mark| matches!(mark, Mark::Ring { .. } | Mark::Dot { .. })),
                        "{level:?} {strength} {step}: rock has a circle"
                    );
                    assert!(
                        marks.iter().any(|mark| matches!(mark, Mark::Poly { .. })),
                        "{level:?} rock is drawn with polygons"
                    );
                }
            }
        }
    }

    #[test]
    fn the_piece_counts_follow_the_spec_for_each_shape_and_intensity() {
        // (weak, strong) counts per intensity: rain droplets, rock shards + sparks + dust, deep
        // bubbles (three marks each).
        let rain = [(4, 6), (7, 10), (10, 16)];
        let rock = [(6, 9), (18, 22), (24, 31)];
        let deep = [(3, 5), (5, 8), (7, 11)];
        for (index, level) in LEVELS.into_iter().enumerate() {
            let rain_count =
                |strength| dots(&scatter_of(VisualShape::Point, level, strength, 0.5)).len();
            assert_eq!(
                (rain_count(0.0), rain_count(1.0)),
                rain[index],
                "{level:?} rain"
            );
            let rock_count =
                |strength| dots(&scatter_of(VisualShape::Impact, level, strength, 0.5)).len();
            assert_eq!(
                (rock_count(0.0), rock_count(1.0)),
                rock[index],
                "{level:?} rock"
            );
            // One outline ring per bubble.
            let deep_count = |strength| {
                scatter_of(VisualShape::SlowGlow, level, strength, 0.9)
                    .iter()
                    .filter(|mark| matches!(mark, Mark::Ring { .. }))
                    .count()
            };
            assert_eq!(
                (deep_count(0.0), deep_count(1.0)),
                deep[index],
                "{level:?} deep"
            );
        }
    }

    #[test]
    fn the_same_particle_scatters_the_same_way_and_a_stronger_one_throws_more() {
        for shape in SHAPES {
            for level in LEVELS {
                assert_eq!(
                    scatter_of(shape, level, 0.7, 0.4),
                    scatter_of(shape, level, 0.7, 0.4)
                );
                assert!(
                    scatter_of(shape, level, 1.0, 0.9).len()
                        >= scatter_of(shape, level, 0.0, 0.9).len()
                );
            }
        }
    }

    #[test]
    fn rock_shards_fly_outward_then_fall_and_rain_crowns_up_then_falls_back() {
        let centre_y = |marks: &[Mark]| {
            let pieces = dots(marks);
            pieces.iter().map(|(offset, _)| offset[1]).sum::<f32>() / pieces.len() as f32
        };
        let spread = |marks: &[Mark]| {
            let pieces = dots(marks);
            pieces
                .iter()
                .map(|(offset, _)| offset[0].hypot(offset[1]))
                .sum::<f32>()
                / pieces.len() as f32
        };
        let early = scatter_of(VisualShape::Impact, VisualIntensity::Vivid, 0.8, 0.1);
        let later = scatter_of(VisualShape::Impact, VisualIntensity::Vivid, 0.8, 0.6);
        assert!(spread(&later) > spread(&early), "shards travel outward");
        assert!(
            centre_y(&later) > centre_y(&early),
            "gravity pulls them down"
        );
        // Rain: thrown upward at first (negative y), back down by the end.
        let rise = centre_y(&scatter_of(
            VisualShape::Point,
            VisualIntensity::Vivid,
            0.8,
            0.2,
        ));
        let fall = centre_y(&scatter_of(
            VisualShape::Point,
            VisualIntensity::Vivid,
            0.8,
            0.9,
        ));
        assert!(rise < 0.0, "the crown goes up first, got {rise}");
        assert!(fall > rise, "and falls back");
    }

    #[test]
    fn bubbles_leave_one_after_another_rise_and_burst_at_the_end() {
        let bubbles = |age| scatter_of(VisualShape::SlowGlow, VisualIntensity::Vivid, 1.0, age);
        assert!(bubbles(0.0).is_empty(), "none have left at birth");
        assert!(
            bubbles(0.05).len() < bubbles(0.5).len(),
            "they leave one by one"
        );
        let height = |age| {
            let rings: Vec<f32> = bubbles(age)
                .iter()
                .filter_map(|mark| match *mark {
                    Mark::Ring { offset, .. } => Some(offset[1]),
                    _ => None,
                })
                .collect();
            rings.iter().sum::<f32>() / rings.len() as f32
        };
        assert!(height(0.8) < height(0.4), "they rise (y gets smaller)");
        // At the very end every bubble has burst: nothing is left to see.
        let opacity_sum: f32 = bubbles(1.0)
            .iter()
            .map(|mark| match *mark {
                Mark::Ring { opacity, .. } | Mark::Dot { opacity, .. } => opacity,
                _ => 0.0,
            })
            .sum();
        assert_eq!(opacity_sum, 0.0);
    }

    #[test]
    fn every_piece_stays_finite_and_near_the_particle() {
        for shape in SHAPES {
            for level in LEVELS {
                for strength in [0.0, 0.5, 1.0] {
                    for step in 0..=20 {
                        for mark in scatter_of(shape, level, strength, step as f32 / 20.0) {
                            let (offset, radius, opacity) = match mark {
                                Mark::Dot {
                                    offset,
                                    radius,
                                    opacity,
                                    ..
                                }
                                | Mark::Haze {
                                    offset,
                                    radius,
                                    opacity,
                                    ..
                                }
                                | Mark::Glow {
                                    offset,
                                    radius,
                                    opacity,
                                    ..
                                }
                                | Mark::Ring {
                                    offset,
                                    radius,
                                    opacity,
                                    ..
                                } => (offset, radius, opacity),
                                Mark::Poly {
                                    offset,
                                    radius,
                                    opacity,
                                    ..
                                } => (offset, radius, opacity),
                                Mark::Cross { .. } => ([0.0; 2], 0.0, 0.0),
                            };
                            assert!(
                                offset
                                    .iter()
                                    .chain([&radius, &opacity])
                                    .all(|v| v.is_finite())
                            );
                            assert!(
                                offset[0].abs() < 400.0 && offset[1].abs() < 400.0,
                                "{offset:?}"
                            );
                            assert!(radius >= 0.0 && (0.0..=1.0).contains(&opacity));
                        }
                    }
                }
            }
        }
    }
}
