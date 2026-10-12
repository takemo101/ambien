//! The rules of the 8-bit ("Famicom") look, kept apart from the OS call that applies them: the
//! size of a dot, and the colour table that pulls every colour toward the console's palette.

/// A dot is this many logical pixels wide, so the 960x600 scene is 320x200 dots (the Famicom's
/// picture is 256x240).
pub(crate) const BLOCK_LOGICAL: f64 = 3.0;

/// Side of the colour cube. A colour is looked up in a cube of this many steps per channel and
/// blended between its corners, so the table stays small while its corners are palette colours.
pub(crate) const CUBE_DIMENSION: usize = 32;

/// How many device pixels one dot covers on a display whose backing scale is `backing_scale`
/// (2 on Retina), so a dot looks the same size on every display. `None` for a scale that is not a
/// positive finite number.
pub(crate) fn block_pixels(backing_scale: f64) -> Option<f64> {
    (backing_scale.is_finite() && backing_scale > 0.0).then_some(BLOCK_LOGICAL * backing_scale)
}

/// The common NTSC approximation of the Famicom palette (0xRRGGBB): the 54 distinct colours, the
/// duplicate blacks left out.
pub(crate) const PALETTE: [u32; 54] = [
    0x7C7C7C, 0x0000FC, 0x0000BC, 0x4428BC, 0x940084, 0xA80020, 0xA81000, 0x881400, 0x503000,
    0x007800, 0x006800, 0x005800, 0x004058, 0x000000, 0xBCBCBC, 0x0078F8, 0x0058F8, 0x6844FC,
    0xD800CC, 0xE40058, 0xF83800, 0xE45C10, 0xAC7C00, 0x00B800, 0x00A800, 0x00A844, 0x008888,
    0xF8F8F8, 0x3CBCFC, 0x6888FC, 0x9878F8, 0xF878F8, 0xF85898, 0xF87858, 0xFCA044, 0xF8B800,
    0xB8F818, 0x58D854, 0x58F898, 0x00E8D8, 0x787878, 0xFCFCFC, 0xA4E4FC, 0xB8B8F8, 0xD8B8F8,
    0xF8B8F8, 0xF8A4C0, 0xF0D0B0, 0xFCE0A8, 0xF8D878, 0xD8F878, 0xB8F8B8, 0xB8F8D8, 0x00FCFC,
];

fn channels(colour: u32) -> [f32; 3] {
    [
        ((colour >> 16) & 255) as f32,
        ((colour >> 8) & 255) as f32,
        (colour & 255) as f32,
    ]
}

/// How different two colours (0..=255 per channel) look: a "redmean" weighted distance, closer to
/// the eye than a plain RGB distance.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let mean = (a[0] + b[0]) / 2.0;
    let (dr, dg, db) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (2.0 + mean / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - mean) / 256.0) * db * db
}

/// The palette colour nearest to `rgb` (0..=255 per channel).
fn nearest(rgb: [f32; 3]) -> u32 {
    let mut best = (f32::INFINITY, PALETTE[0]);
    for colour in PALETTE {
        let found = distance(rgb, channels(colour));
        if found < best.0 {
            best = (found, colour);
        }
    }
    best.1
}

/// The colour cube for Core Image's `CIColorCubeWithColorSpace` (read as sRGB): `dimension`^3 entries of (r, g, b, a) floats in
/// 0..=1, red varying fastest, then green, then blue. Each entry is the palette colour nearest to
/// the colour at that corner, opaque.
pub(crate) fn colour_cube(dimension: usize) -> Vec<f32> {
    assert!(
        dimension >= 2,
        "a colour cube needs at least two steps per channel"
    );
    let step = (dimension - 1) as f32;
    let mut data = Vec::with_capacity(dimension * dimension * dimension * 4);
    for blue in 0..dimension {
        for green in 0..dimension {
            for red in 0..dimension {
                let corner = [
                    red as f32 / step * 255.0,
                    green as f32 / step * 255.0,
                    blue as f32 / step * 255.0,
                ];
                let [r, g, b] = channels(nearest(corner));
                data.extend_from_slice(&[r / 255.0, g / 255.0, b / 255.0, 1.0]);
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dot_is_three_logical_pixels_on_every_display_scale() {
        assert_eq!(block_pixels(1.0), Some(3.0));
        assert_eq!(block_pixels(2.0), Some(6.0));
        assert_eq!(block_pixels(1.5), Some(4.5));
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(block_pixels(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_palette_has_fifty_four_distinct_colours() {
        let mut sorted = PALETTE.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), PALETTE.len());
        assert!(PALETTE.iter().all(|colour| *colour <= 0xFF_FFFF));
    }

    #[test]
    fn every_corner_of_the_cube_is_an_opaque_palette_colour_and_the_nearest_one() {
        let dimension = 9;
        let cube = colour_cube(dimension);
        assert_eq!(cube.len(), dimension * dimension * dimension * 4);
        let palette: Vec<[f32; 3]> = PALETTE.iter().map(|c| channels(*c)).collect();
        let step = (dimension - 1) as f32;
        for (index, entry) in cube.chunks_exact(4).enumerate() {
            let (red, green, blue) = (
                index % dimension,
                (index / dimension) % dimension,
                index / (dimension * dimension),
            );
            let corner = [
                red as f32 / step * 255.0,
                green as f32 / step * 255.0,
                blue as f32 / step * 255.0,
            ];
            let found = [entry[0] * 255.0, entry[1] * 255.0, entry[2] * 255.0];
            assert_eq!(entry[3], 1.0);
            let chosen = palette
                .iter()
                .find(|colour| colour.iter().zip(found).all(|(a, b)| (a - b).abs() < 0.5))
                .unwrap_or_else(|| panic!("corner {index} is not a palette colour: {found:?}"));
            // No other palette colour is closer.
            let best = palette
                .iter()
                .map(|colour| distance(corner, *colour))
                .fold(f32::INFINITY, f32::min);
            assert!(distance(corner, *chosen) <= best + 1e-3, "corner {index}");
        }
    }

    #[test]
    fn black_and_white_keep_their_ends_and_a_palette_colour_maps_to_itself() {
        let cube = colour_cube(CUBE_DIMENSION);
        let at = |red: usize, green: usize, blue: usize| {
            let index = (red + CUBE_DIMENSION * (green + CUBE_DIMENSION * blue)) * 4;
            [cube[index], cube[index + 1], cube[index + 2]]
        };
        assert_eq!(at(0, 0, 0), [0.0, 0.0, 0.0]);
        let last = CUBE_DIMENSION - 1;
        let white = at(last, last, last);
        assert!(white.iter().all(|channel| *channel >= 0.97), "{white:?}");
        // A palette colour is its own nearest colour.
        for colour in PALETTE {
            assert_eq!(nearest(channels(colour)), colour);
        }
    }
}
