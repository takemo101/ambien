//! The marks that scatter around a particle: a rain drop's ripples and splash, a rock impact's
//! flash ring, shards, sparks and dust, a deep bubble's rising bubbles.
//!
//! Everything is a pure function of the event's own numbers and its age: how many pieces, where
//! each flies and how big it is come from the serial and the strength, never a random source in
//! the painter. Pieces are pushed most important first, so a frame that has to draw fewer marks
//! loses the finest ones.

use std::f32::consts::TAU;

use super::{Facet, Look, Mark, deepen, facet_glow, lighten, mix, seed};
use crate::{
    controls::VisualIntensity,
    situations::{VisualShape, bout_envelope},
};

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
    /// The particle's height in the window (0 top, 1 bottom).
    pub(super) y: f32,
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

/// The same glow for a piece that must have no round edge: one filled copy of its own shape,
/// `scale` times larger and faint, from standard up. Dark windows get a lighter light, light
/// windows a deeper colour.
fn facet_halo(look: &mut Look, s: &Scatter, facet: &Facet, scale: f32, opacity: f32) {
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
    facet_glow(look, facet, rgb, opacity * level * tone, &[(scale, 1.0)]);
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
        VisualShape::SlowGlow | VisualShape::Firefly => 0.0,
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
        VisualShape::Firefly => firefly(look, s, soft),
    }
}

/// The most a firefly strays from where it was lit, in logical pixels at the default window size
/// (a 600 px short side): sideways either way (about a tenth of the default window's width) and
/// up or down alike, but never out of the window: it stops short of the top and bottom edges by
/// `WINDOW_MARGIN` of the short side, so a firefly lit high may wander down and one lit low up.
const WANDER_SIDE: f32 = 90.0;
const WANDER_VERTICAL: f32 = 100.0;
/// Share of the window's height kept clear above and below the wandering firefly (the short
/// side is the window's height in any window at least as wide as it is tall; in a taller one
/// the limit is only more cautious).
const WINDOW_MARGIN: f32 = 0.06;
const SHORT_SIDE: f32 = 600.0;
/// Steps of the numeric integral of its heading; the same fixed number at every age, so the
/// position is a smooth function of age.
const WANDER_STEPS: u32 = 32;

/// `limit * tanh(value / limit)`: close to `value` near zero, never beyond `limit`.
fn squash(value: f32, limit: f32) -> f32 {
    limit * (value / limit).tanh()
}

/// How far a firefly has wandered from where it was lit, in logical pixels at the default window
/// size. It flies a path of its own length (90 to 180 px over its life, so a slow wanderer and a
/// brisker one) whose heading turns smoothly: a start direction plus three slow sinusoids of
/// unrelated rates, so it curves gently and never bends sharply or doubles back along a line. The
/// reach is softly kept inside the band above, and the whole is a pure function of the event's
/// serial and its age (0 when it is lit), with no random source in the painter.
pub(super) fn firefly_drift(serial: u64, age: f32, y: f32) -> [f32; 2] {
    let age = age.clamp(0.0, 1.0);
    if age == 0.0 {
        return [0.0, 0.0];
    }
    let start = TAU * unit(serial, 1);
    let length = 90.0 + 90.0 * unit(serial, 2);
    // (turns over the life, swing in radians, phase), slowest and widest first.
    let wave = |index: u64, turns: f32, swing: f32| {
        (
            turns * (0.8 + 0.4 * unit(serial, 10 + index)),
            swing * (0.7 + 0.6 * unit(serial, 20 + index)),
            TAU * unit(serial, 30 + index),
        )
    };
    let waves = [wave(0, 0.8, 1.0), wave(1, 1.7, 0.35), wave(2, 2.8, 0.15)];
    let heading = |at: f32| {
        waves.iter().fold(start, |sum, (turns, swing, phase)| {
            sum + swing * (TAU * turns * at + phase).sin()
        })
    };
    let step = age / WANDER_STEPS as f32;
    let mut at = [0.0_f32, 0.0];
    for index in 0..WANDER_STEPS {
        let angle = heading((index as f32 + 0.5) * step);
        at[0] += angle.cos() * step * length;
        at[1] += angle.sin() * step * length;
    }
    // Screen y grows downwards. How far it may go either way is what is left of the window
    // (inside the margin) from where it was lit, at most `WANDER_VERTICAL`.
    let room = |share: f32| (share * SHORT_SIDE).clamp(1.0, WANDER_VERTICAL);
    let limit = if at[1] >= 0.0 {
        room(1.0 - WINDOW_MARGIN - y)
    } else {
        room(y - WINDOW_MARGIN)
    };
    [squash(at[0], WANDER_SIDE), squash(at[1], limit)]
}

/// A firefly's decoration, on top of its core light and halo (which the look places at the
/// firefly's drifted position): from standard a bright little lamp and a wide soft glow; vivid
/// adds a faint trail of its recent positions, an out-of-focus disc beside it and a bloom.
fn firefly(look: &mut Look, s: &Scatter, soft: f32) {
    if s.intensity == VisualIntensity::Calm {
        return;
    }
    // The core swells with the insect's song, exactly as the look sizes it.
    let core = s.radius * (0.8 + 0.4 * bout_envelope(s.age));
    let here = firefly_drift(s.serial, s.age, s.y);
    // A firefly is always the bright night look (the look passes `dark` for it in any appearance).
    let lamp = lighten(s.rgb, 0.55);
    look.push(Mark::Dot {
        offset: here,
        radius: (core * 0.35).max(0.8),
        rgb: lamp,
        opacity: soft * 0.9,
    });
    halo(look, s, here, core * 5.5, soft * 0.45);
    if s.intensity != VisualIntensity::Vivid {
        return;
    }
    for step in 1..=3_u8 {
        let behind = firefly_drift(s.serial, s.age - 0.015 * f32::from(step), s.y);
        look.push(Mark::Dot {
            offset: behind,
            radius: (core * (0.32 - 0.07 * f32::from(step))).max(0.7),
            rgb: s.rgb,
            opacity: soft * 0.4 / f32::from(step),
        });
    }
    let bearing = TAU * unit(s.serial, 5);
    let distance = core * (2.2 + 1.4 * unit(s.serial, 6));
    look.push(Mark::Haze {
        offset: [
            here[0] + bearing.cos() * distance,
            here[1] + bearing.sin() * distance,
        ],
        radius: core * (1.4 + 0.8 * unit(s.serial, 7)),
        rgb: s.rgb,
        opacity: soft * 0.18,
    });
    // No additive blend: a firefly is always lit on the night scene, so a larger, lighter glow.
    let (bloom, tone, size) = (lighten(s.rgb, 0.35), 0.28, 2.2);
    look.push(Mark::Glow {
        offset: here,
        radius: core * size,
        rgb: bloom,
        opacity: soft * tone,
    });
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
/// angular dust. Nothing here is round: even its glows are enlarged copies of a piece's own shape.
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
    // Where shard `i` is, how big and how it is turned, so its glow can be laid round the same
    // shape afterwards. Each shard tumbles at its own rate.
    let shard = |i: u64| {
        let angle = TAU * (i as f32 / shards as f32 + 0.08 * unit(s.serial, i));
        let speed = u * (32.0 + 48.0 * unit(s.serial, 100 + i));
        let distance = speed * (1.0 - fade * fade);
        Facet {
            offset: [
                angle.cos() * distance,
                angle.sin() * distance + 0.5 * u * 70.0 * s.age * s.age,
            ],
            radius: u * (1.3 + 3.0 * unit(s.serial, 200 + i)) * (1.0 - 0.45 * s.age),
            sides: 3 + (i % 3) as u8,
            rotation: TAU * unit(s.serial, 600 + i) + (unit(s.serial, 500 + i) - 0.5) * 9.0 * s.age,
            seed: seed(s.serial, 700 + i),
        }
    };
    for i in 0..shards {
        let facet = shard(i);
        look.push(Mark::Poly {
            offset: facet.offset,
            radius: facet.radius,
            sides: facet.sides,
            rotation: facet.rotation,
            seed: facet.seed,
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
    // A diamond pointing along its flight.
    let spark = |i: u64| {
        let angle = TAU * unit(s.serial, 400 + i);
        let speed = u * (55.0 + 40.0 * unit(s.serial, 410 + i));
        let distance = speed * (1.0 - (1.0 - burn) * (1.0 - burn));
        Facet {
            offset: [angle.cos() * distance, angle.sin() * distance],
            radius: u * 1.5,
            sides: 4,
            rotation: angle,
            seed: seed(s.serial, 800 + i),
        }
    };
    for i in 0..sparks {
        let facet = spark(i);
        look.push(Mark::Poly {
            offset: facet.offset,
            radius: facet.radius,
            sides: facet.sides,
            rotation: facet.rotation,
            seed: facet.seed,
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
    // The glows, last.
    let glowing = match s.intensity {
        VisualIntensity::Calm => 0,
        VisualIntensity::Standard => 6,
        VisualIntensity::Vivid => 10,
    };
    for i in 0..shards.min(glowing) {
        facet_halo(look, s, &shard(i), 2.2, soft * 0.5);
    }
    for i in 0..sparks {
        facet_halo(look, s, &spark(i), 3.2, soft * 0.6 * (1.0 - burn));
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

    const SHAPES: [VisualShape; 4] = [
        VisualShape::Point,
        VisualShape::Impact,
        VisualShape::SlowGlow,
        VisualShape::Firefly,
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
                y: 0.5,
                dark,
            },
        );
        look.marks().copied().collect()
    }

    /// The base colour `scatter_in` is given.
    const BASE: [f32; 3] = [0.5, 0.6, 0.7];

    /// A filled polygon in a strictly deeper colour than the particle's: in light mode that is
    /// a rock's angular glow, since the pieces themselves are never deeper than the base colour.
    fn is_deeper_glow(mark: &Mark) -> bool {
        matches!(mark, Mark::Poly { rgb, stroke: 0.0, .. }
            if rgb.iter().zip(&BASE).all(|(channel, base)| channel < base))
    }

    /// The glows laid round the pieces in light mode: blurs for rain and deep, angular copies for
    /// rock. Light mode only ever deepens, so these are told by colour for the polygons.
    fn glows(shape: VisualShape, level: VisualIntensity, age: f32) -> Vec<f32> {
        scatter_in(shape, level, 1.0, age, false)
            .iter()
            .filter_map(|mark| match *mark {
                Mark::Haze { opacity, .. } => Some(opacity),
                Mark::Poly { opacity, .. } if is_deeper_glow(mark) => Some(opacity),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn pieces_get_a_soft_halo_from_standard_up_and_vivid_glows_more_than_standard() {
        for shape in SHAPES {
            let halos = |level| glows(shape, level, 0.6);
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
            let strongest = |halos: &[f32]| halos.iter().copied().fold(0.0, f32::max);
            assert!(
                strongest(&vivid) > strongest(&standard),
                "{shape:?} vivid is brighter"
            );
        }
    }

    #[test]
    fn a_halo_is_a_lighter_light_on_dark_and_a_deeper_colour_on_light_never_a_brightening() {
        for shape in [VisualShape::Point, VisualShape::SlowGlow] {
            let hazes = |dark| -> Vec<([f32; 3], f32)> {
                scatter_in(shape, VisualIntensity::Vivid, 1.0, 0.6, dark)
                    .iter()
                    .filter_map(|mark| match *mark {
                        Mark::Haze { rgb, opacity, .. } => Some((rgb, opacity)),
                        _ => None,
                    })
                    .collect()
            };
            let (dark, light) = (hazes(true), hazes(false));
            assert_eq!(dark.len(), light.len());
            for (dark, light) in dark.iter().zip(&light) {
                for ((&dark_channel, &light_channel), &base) in
                    dark.0.iter().zip(&light.0).zip(&BASE)
                {
                    assert!(dark_channel >= base, "dark brightens");
                    assert!(light_channel <= base, "light deepens");
                }
                assert!(light.1 <= dark.1, "light is the fainter film");
            }
        }
        // A rock's glows are angular copies of its pieces: none is ever deeper than the base in a
        // dark window, and a light window has them, all deeper.
        let deeper = |dark| {
            scatter_in(VisualShape::Impact, VisualIntensity::Vivid, 1.0, 0.3, dark)
                .iter()
                .filter(|mark| is_deeper_glow(mark))
                .count()
        };
        assert_eq!(deeper(true), 0);
        assert!(deeper(false) > 0);
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
                        !marks.iter().any(|mark| matches!(
                            mark,
                            Mark::Ring { .. }
                                | Mark::Dot { .. }
                                | Mark::Glow { .. }
                                | Mark::Haze { .. }
                        )),
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
            // A rock's angular glows are told from its pieces in light mode, where they alone
            // are a deeper colour than the particle's.
            let rock_count = |strength| {
                scatter_in(VisualShape::Impact, level, strength, 0.5, false)
                    .iter()
                    .filter(|mark| {
                        matches!(mark, Mark::Poly { stroke: 0.0, .. }) && !is_deeper_glow(mark)
                    })
                    .count()
            };
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
                                Mark::Cross { .. } | Mark::Shards { .. } => ([0.0; 2], 0.0, 0.0),
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

    #[test]
    fn a_fireflys_wander_turns_smoothly_stays_inside_the_window_and_differs_from_firefly_to_firefly()
     {
        let steps = 600;
        let mut ends = std::collections::BTreeSet::new();
        let (mut widest, mut sharpest) = (0.0_f32, 0.0_f32);
        let (mut highest, mut lowest) = (0.0_f32, 0.0_f32);
        for serial in 1..=300_u64 {
            // Lit anywhere from the top tenth to the bottom tenth of the window.
            let y = 0.10 + 0.80 * ((serial * 7) % 101) as f32 / 100.0;
            let path = path_again(serial, y, steps);
            assert_eq!(path[0], [0.0, 0.0], "lit where it was born");
            assert_eq!(path, self::path_again(serial, y, steps), "a pure function");
            ends.insert(path[steps].map(f32::to_bits));
            let mut previous: Option<[f32; 2]> = None;
            for pair in path.windows(2) {
                let velocity = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
                // Never more than a gentle stroll: 180 px over the life is 0.3 px a step.
                assert!(
                    velocity[0].hypot(velocity[1]) < 0.4,
                    "{serial}: {velocity:?}"
                );
                // Slowing to a crawl against the soft edge is not a bend; only consecutive
                // steps of a real stroll are compared.
                if velocity[0].hypot(velocity[1]) > 0.04 {
                    if let Some(before) = previous {
                        let turn = (velocity[1].atan2(velocity[0]) - before[1].atan2(before[0]))
                            .rem_euclid(TAU);
                        let turn = turn.min(TAU - turn);
                        sharpest = sharpest.max(turn);
                    }
                    previous = Some(velocity);
                } else {
                    previous = None;
                }
            }
            assert!(path.iter().all(|p| p[0].abs() <= WANDER_SIDE));
            // Always inside the window, with its margin (final height in a 600 px window).
            assert!(
                path.iter().all(|p| (WINDOW_MARGIN * 600.0 - 0.01
                    ..=(1.0 - WINDOW_MARGIN) * 600.0 + 0.01)
                    .contains(&(y * 600.0 + p[1]))),
                "{serial} at {y}"
            );
            assert!(path.iter().all(|p| p[1].abs() <= WANDER_VERTICAL));
            let sideways = path.iter().map(|p| p[0].abs()).fold(0.0, f32::max);
            widest = widest.max(sideways);
            if (0.4..=0.6).contains(&y) {
                highest = highest.min(path.iter().map(|p| p[1]).fold(0.0, f32::min));
                lowest = lowest.max(path.iter().map(|p| p[1]).fold(0.0, f32::max));
            }
        }
        // No sharp bend anywhere (the heading changes by well under a tenth of a turn per
        // 1/600 of the life).
        assert!(sharpest < 0.25, "sharpest turn {sharpest}");
        // A tenth of a 960 px window is 96 px: the wander can use most of it, never more.
        assert!(widest > 30.0 && widest <= 90.0, "{widest}");
        // From mid-window it wanders both up and down, a good way either way.
        assert!(highest < -40.0 && lowest > 40.0, "{highest} {lowest}");
        assert!(ends.len() > 280, "every firefly goes its own way");
    }

    #[test]
    fn a_firefly_lit_near_an_edge_stays_clear_of_it_and_can_go_the_other_way() {
        for (y, up_room, down_room) in [(0.10, 24.0, 100.0), (0.90, 100.0, 24.0)] {
            let (mut up, mut down) = (0.0_f32, 0.0_f32);
            for serial in 1..=300_u64 {
                for step in 0..=300 {
                    let offset = firefly_drift(serial, step as f32 / 300.0, y)[1];
                    up = up.min(offset);
                    down = down.max(offset);
                }
            }
            assert!(
                -up <= up_room + 0.01 && down <= down_room + 0.01,
                "{y}: {up} {down}"
            );
            // The way into the window is open: a good stretch of it is used.
            if y < 0.5 {
                assert!(down > 50.0, "{y}: {down}");
            } else {
                assert!(-up > 50.0, "{y}: {up}");
            }
        }
    }

    fn path_again(serial: u64, y: f32, steps: usize) -> Vec<[f32; 2]> {
        (0..=steps)
            .map(|step| firefly_drift(serial, step as f32 / steps as f32, y))
            .collect()
    }
}
