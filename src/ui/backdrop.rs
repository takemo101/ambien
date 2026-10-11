//! The coloured light behind the particles, decided from the world alone.
//!
//! Standard lays slowly drifting colour fields, one pair per situation, weighted by how much that
//! situation is heard. Vivid adds aurora ribbons whose colour flows with the world time. Everything
//! comes from the world time, the situations' contributions and breath, and the viewport size:
//! the same inputs always give the same items, nothing is random, and a fixed array bounds the
//! cost. Calm has no backdrop; reduced motion freezes the drift.

use std::f32::consts::TAU;

use crate::{controls::VisualIntensity, ui::appearance::Motion};

/// The most items a backdrop can hold: the fields of `MAX_SITUATIONS` situations plus the aurora.
pub(crate) const MAX_ITEMS: usize = 80;
const FIELDS_PER_SITUATION: usize = 2;
const RIBBONS: usize = 3;
const RIBBON_POINTS: usize = 14;
/// Situations that can colour the backdrop: what is left of `MAX_ITEMS` after the aurora.
pub(crate) const MAX_SITUATIONS: usize =
    (MAX_ITEMS - RIBBONS * RIBBON_POINTS) / FIELDS_PER_SITUATION;

/// One soft light: a small core with a wide blur, centred on a normalized position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) center: [f32; 2],
    /// Logical pixels.
    pub(crate) radius: f32,
    /// Logical pixels of blur around the core.
    pub(crate) blur: f32,
    pub(crate) rgb: [f32; 3],
    pub(crate) opacity: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct Backdrop {
    items: [Option<Item>; MAX_ITEMS],
    len: usize,
}

impl Backdrop {
    fn new() -> Self {
        Self {
            items: [None; MAX_ITEMS],
            len: 0,
        }
    }

    fn push(&mut self, item: Item) {
        debug_assert!(self.len < MAX_ITEMS, "backdrop overflowed MAX_ITEMS");
        if self.len < MAX_ITEMS {
            self.items[self.len] = Some(item);
            self.len += 1;
        }
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &Item> {
        self.items[..self.len].iter().flatten()
    }
}

/// What the backdrop depends on. `contributions` and `breath` have one entry per situation; the
/// colours are the situations' own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Input<'a> {
    pub(crate) world_seconds: f64,
    pub(crate) contributions: &'a [f32],
    pub(crate) breath: &'a [f32],
    pub(crate) colors: &'a [[f32; 3]],
    /// Viewport size in logical pixels.
    pub(crate) size: [f32; 2],
    /// The window is on a dark appearance; light appearance gets deeper, more opaque colours so
    /// the light does not wash out.
    pub(crate) dark: bool,
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// A fixed, well-spread value in 0..1 from two small integers (no random source).
fn spread(a: usize, b: usize) -> f32 {
    let mut value = (a as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (b as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 29;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    ((value >> 40) as f32) / 16_777_216.0
}

fn hue(h: f32, saturation: f32, value: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (
        value * (1.0 - saturation),
        value * (1.0 - saturation * f),
        value * (1.0 - saturation * (1.0 - f)),
    );
    match sector as i32 % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

/// A phase in radians that advances `rate` radians per second. Reduced in f64 so it stays
/// continuous and precise however long the world has been running.
fn angle(time: f64, rate: f32) -> f32 {
    (time * f64::from(rate)).rem_euclid(f64::from(TAU)) as f32
}

pub(crate) fn backdrop(input: &Input<'_>, intensity: VisualIntensity, motion: Motion) -> Backdrop {
    let mut out = Backdrop::new();
    if intensity == VisualIntensity::Calm {
        return out;
    }
    let short = finite(input.size[0]).min(finite(input.size[1])).max(0.0);
    if short <= 0.0 {
        return out;
    }
    // Reduced motion freezes the drift; the colours still follow what is heard.
    let time = if motion == Motion::Reduced || !input.world_seconds.is_finite() {
        0.0
    } else {
        input.world_seconds.max(0.0)
    };
    let tone = if input.dark { (1.0, 1.0) } else { (0.78, 1.15) };
    let count = input
        .contributions
        .len()
        .min(input.breath.len())
        .min(input.colors.len())
        .min(MAX_SITUATIONS);
    let mut heard = 0.0_f32;
    let mut mixed = [0.0_f32; 3];
    for index in 0..count {
        let weight = finite(input.contributions[index]).clamp(0.0, 1.0);
        if weight <= 0.0 {
            continue;
        }
        let breath = finite(input.breath[index]).clamp(0.0, 1.0);
        let rgb = input.colors[index].map(|channel| (finite(channel) * tone.0).clamp(0.0, 1.0));
        heard += weight;
        for channel in 0..3 {
            mixed[channel] += rgb[channel] * weight;
        }
        for field in 0..FIELDS_PER_SITUATION {
            let phase = TAU * spread(index, field);
            let drift = 0.12 + 0.10 * spread(field, index + 7);
            let speed = TAU * (0.045 + 0.03 * spread(index + 3, field));
            let x = 0.5 + 0.34 * (angle(time, speed) + phase).sin();
            let y = 0.5 + 0.30 * (angle(time, speed * 0.8) + phase * 1.7 + TAU * drift).cos();
            out.push(Item {
                center: [x, y],
                radius: short * 0.22,
                blur: short * 0.14,
                rgb,
                opacity: (0.20 * weight * (0.8 + 0.2 * breath) * tone.1).clamp(0.0, 1.0),
            });
        }
    }
    if intensity == VisualIntensity::Vivid && heard > 0.0 {
        let total = heard.min(1.0);
        let base = mixed.map(|channel| channel / heard);
        // The hue the ribbons orbit around follows what is heard; time slides it slowly.
        let anchor = (base[0] * 0.2 + base[2] * 0.6 + 0.2).clamp(0.0, 1.0);
        for ribbon in 0..RIBBONS {
            let r = ribbon as f32;
            let centre_y = 0.28 + 0.26 * r;
            let amplitude = 0.07 + 0.025 * r;
            let wave = 5.5 - r * 0.8;
            let flow = angle(time, if ribbon % 2 == 0 { 0.30 } else { -0.22 });
            let shimmer = angle(time, 0.35);
            let hue_shift = angle(time, 0.08);
            let pulse = angle(time, 0.5);
            for point in 0..RIBBON_POINTS {
                let u = (point as f32 + 0.5) / RIBBON_POINTS as f32;
                let y = centre_y
                    + amplitude * (u * wave + flow + r * 1.7).sin()
                    + 0.03 * (u * wave * 2.3 - shimmer).sin();
                let colour = hue(
                    0.55 + 0.35 * anchor + 0.2 * r + 0.12 * (u * 2.5 + hue_shift).sin(),
                    0.65,
                    if input.dark { 1.0 } else { 0.8 },
                );
                let edge = (u * std::f32::consts::PI).sin();
                out.push(Item {
                    center: [u, y],
                    radius: short * 0.07,
                    blur: short * 0.05,
                    rgb: colour,
                    opacity: (0.12
                        * total
                        * edge
                        * (0.55 + 0.45 * (u * 6.0 + pulse + r).sin())
                        * tone.1)
                        .clamp(0.0, 1.0),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLORS: [[f32; 3]; 3] = [[0.50, 0.72, 0.92], [0.65, 0.65, 0.65], [0.12, 0.28, 0.48]];

    fn input<'a>(world: f64, contributions: &'a [f32], breath: &'a [f32]) -> Input<'a> {
        Input {
            world_seconds: world,
            contributions,
            breath,
            colors: &COLORS,
            size: [960.0, 600.0],
            dark: true,
        }
    }

    fn items(input: &Input<'_>, intensity: VisualIntensity, motion: Motion) -> Vec<Item> {
        backdrop(input, intensity, motion)
            .items()
            .copied()
            .collect()
    }

    #[test]
    fn calm_has_no_backdrop_and_a_silent_world_has_none_at_any_intensity() {
        let heard = input(10.0, &[1.0, 0.0, 0.0], &[0.5, 0.0, 0.0]);
        assert!(items(&heard, VisualIntensity::Calm, Motion::Full).is_empty());
        let silent = input(10.0, &[0.0; 3], &[0.9; 3]);
        for intensity in [VisualIntensity::Standard, VisualIntensity::Vivid] {
            assert!(items(&silent, intensity, Motion::Full).is_empty());
        }
    }

    #[test]
    fn the_same_world_always_gives_the_same_backdrop() {
        let heard = input(123.456, &[0.6, 0.4, 0.0], &[0.5, 0.3, 0.0]);
        for intensity in [VisualIntensity::Standard, VisualIntensity::Vivid] {
            assert_eq!(
                items(&heard, intensity, Motion::Full),
                items(&heard, intensity, Motion::Full)
            );
        }
    }

    #[test]
    fn standard_has_two_fields_per_heard_situation_and_vivid_adds_the_aurora() {
        let heard = input(5.0, &[0.7, 0.3, 0.0], &[0.5, 0.5, 0.5]);
        assert_eq!(
            items(&heard, VisualIntensity::Standard, Motion::Full).len(),
            4
        );
        let vivid = items(&heard, VisualIntensity::Vivid, Motion::Full);
        assert_eq!(vivid.len(), 4 + RIBBONS * RIBBON_POINTS);
        assert!(vivid.len() <= MAX_ITEMS);
    }

    #[test]
    fn the_light_follows_how_much_each_situation_is_heard_and_mixes_in_a_transition() {
        let opacity_of = |contribution: f32| {
            items(
                &input(5.0, &[contribution, 0.0, 0.0], &[0.5; 3]),
                VisualIntensity::Standard,
                Motion::Full,
            )
            .iter()
            .map(|item| item.opacity)
            .fold(0.0, f32::max)
        };
        assert!(opacity_of(1.0) > opacity_of(0.5));
        assert!(opacity_of(0.5) > opacity_of(0.1));
        // Halfway through a transition both situations are lit.
        let mid = items(
            &input(5.0, &[0.5, 0.5, 0.0], &[0.5; 3]),
            VisualIntensity::Standard,
            Motion::Full,
        );
        let colours: std::collections::BTreeSet<[u32; 3]> =
            mid.iter().map(|item| item.rgb.map(f32::to_bits)).collect();
        assert_eq!(colours.len(), 2);
    }

    #[test]
    fn the_drift_follows_time_but_reduced_motion_stands_still() {
        let at = |world: f64| input(world, &[1.0, 0.0, 0.0], &[0.5; 3]);
        let moving = items(&at(1.0), VisualIntensity::Vivid, Motion::Full);
        let later = items(&at(30.0), VisualIntensity::Vivid, Motion::Full);
        assert_ne!(moving, later, "full motion drifts");
        assert_eq!(
            items(&at(1.0), VisualIntensity::Vivid, Motion::Reduced),
            items(&at(30.0), VisualIntensity::Vivid, Motion::Reduced),
            "reduced motion does not move"
        );
    }

    #[test]
    fn the_drift_has_no_jump_however_long_the_world_has_run() {
        let at = |world: f64| input(world, &[1.0, 0.5, 0.0], &[0.5; 3]);
        for world in [3_600.0, 86_400.0, 1.0e7] {
            let before = items(&at(world - 0.01), VisualIntensity::Vivid, Motion::Full);
            let after = items(&at(world + 0.01), VisualIntensity::Vivid, Motion::Full);
            assert_eq!(before.len(), after.len());
            for (before, after) in before.iter().zip(&after) {
                assert!((before.center[0] - after.center[0]).abs() < 0.01);
                assert!((before.center[1] - after.center[1]).abs() < 0.01);
            }
        }
        // The phase still advances by the right amount a quarter second after a very long run.
        for world in [3_600.0, 86_400.0, 1.0e7] {
            let step = (angle(world + 0.25, 1.0) - angle(world, 1.0)).rem_euclid(TAU);
            assert!(
                (step - 0.25).abs() < 1e-4,
                "phase stalled at {world}: {step}"
            );
        }
    }

    #[test]
    fn light_appearance_gets_deeper_colours_not_a_washed_out_copy() {
        let mut light = input(5.0, &[1.0, 0.0, 0.0], &[0.5; 3]);
        let dark = items(&light, VisualIntensity::Standard, Motion::Full);
        light.dark = false;
        let day = items(&light, VisualIntensity::Standard, Motion::Full);
        assert_eq!(dark.len(), day.len());
        for (dark, day) in dark.iter().zip(&day) {
            assert!(day.rgb[0] < dark.rgb[0] && day.rgb[2] < dark.rgb[2]);
            assert!(day.opacity > dark.opacity);
        }
    }

    #[test]
    fn extreme_inputs_give_finite_valid_items_within_the_capacity() {
        let extremes = [0.0_f32, 1.0, -1.0, 1e9, f32::NAN, f32::INFINITY];
        for &world in &[0.0, 1e12, f64::NAN, f64::INFINITY, -5.0] {
            for &weight in &extremes {
                for &size in &extremes {
                    let contributions = [weight; 20];
                    let breath = [weight; 20];
                    let colors = [[weight, 1.0, 0.0]; 20];
                    let backdrop = backdrop(
                        &Input {
                            world_seconds: world,
                            contributions: &contributions,
                            breath: &breath,
                            colors: &colors,
                            size: [size, size],
                            dark: weight > 0.5,
                        },
                        VisualIntensity::Vivid,
                        Motion::Full,
                    );
                    assert!(backdrop.items().count() <= MAX_ITEMS);
                    for item in backdrop.items() {
                        let numbers = [
                            item.center[0],
                            item.center[1],
                            item.radius,
                            item.blur,
                            item.rgb[0],
                            item.rgb[1],
                            item.rgb[2],
                        ];
                        assert!(numbers.iter().all(|value| value.is_finite()));
                        assert!((0.0..=1.0).contains(&item.opacity));
                        assert!(item.radius >= 0.0 && item.blur >= 0.0);
                    }
                }
            }
        }
    }
}
