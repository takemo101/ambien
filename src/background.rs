//! The scene's background image: reading and shrinking the picked file, keeping a private copy in
//! the support folder, blurring it, and turning it into a ready-to-paint GPUI image.
//!
//! Everything here is plain data in, plain data out, so none of it needs a window. The caller
//! runs the heavy functions (`import`, `load_saved`, `Source::render`) off the UI thread.

use std::{
    fmt, fs,
    io::{self, Cursor},
    path::Path,
    sync::Arc,
    time::Duration,
};

use gpui::RenderImage;
use image::{
    DynamicImage, Frame, ImageBuffer, ImageDecoder, ImageFormat, ImageReader, RgbImage, RgbaImage,
    imageops::{self, FilterType},
    metadata::Orientation,
};

use crate::persistence::{self, SaveError};

/// The copy's file name inside the support folder; the only name the state file may hold.
pub(crate) const FILE_NAME: &str = "background.png";
/// The longest side of the stored copy, in pixels.
pub(crate) const MAX_LONG_SIDE: u32 = 2560;
/// How long the blur slider must rest before the blur is recomputed.
pub(crate) const BLUR_DEBOUNCE: Duration = Duration::from_millis(150);
/// A file larger than this is refused before it is read.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// An image with more pixels than this is refused before its pixels are decoded.
const MAX_PIXELS: u64 = 100_000_000;
/// The blur at its maximum, as a fraction of the long side.
const MAX_SIGMA_FRACTION: f32 = 0.03;

#[derive(Debug)]
pub(crate) enum BackgroundError {
    Io(io::Error),
    NotAFile,
    FileTooLarge(u64),
    TooManyPixels { width: u32, height: u32 },
    Decode(image::ImageError),
    Encode(image::ImageError),
    Save(SaveError),
}

impl fmt::Display for BackgroundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "ファイルを読めません（{error}）"),
            Self::NotAFile => formatter.write_str("通常のファイルではありません"),
            Self::FileTooLarge(bytes) => write!(
                formatter,
                "ファイルが大きすぎます（{} MB、上限 {} MB）",
                bytes.div_ceil(1024 * 1024),
                MAX_FILE_BYTES / (1024 * 1024)
            ),
            Self::TooManyPixels { width, height } => write!(
                formatter,
                "画素数が多すぎます（{width}×{height}、上限 {} 百万画素）",
                MAX_PIXELS / 1_000_000
            ),
            Self::Decode(error) => write!(
                formatter,
                "PNG・JPEG・WebP・GIF として開けません（{error}）"
            ),
            Self::Encode(error) => write!(formatter, "画像を保存用に変換できません（{error}）"),
            Self::Save(error) => write!(formatter, "保存フォルダへ書き込めません（{error}）"),
        }
    }
}

impl std::error::Error for BackgroundError {}

impl From<io::Error> for BackgroundError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The long side `blur` (0..=1) blurs by: nothing at 0, 3% of `long_side` at 1.
pub(crate) fn blur_sigma(blur: f32, long_side: u32) -> f32 {
    if !blur.is_finite() {
        return 0.0;
    }
    blur.clamp(0.0, 1.0) * MAX_SIGMA_FRACTION * long_side as f32
}

/// Where to draw an image so it covers the whole window: the aspect ratio is kept and the middle
/// is shown. Returns `(x, y, width, height)` relative to the window's origin; the parts outside
/// `0..window` are cut off. `None` when any size is not a positive finite number.
pub(crate) fn cover_rect(
    image_width: f32,
    image_height: f32,
    window_width: f32,
    window_height: f32,
) -> Option<(f32, f32, f32, f32)> {
    let usable = |value: f32| value.is_finite() && value > 0.0;
    if ![image_width, image_height, window_width, window_height]
        .into_iter()
        .all(usable)
    {
        return None;
    }
    let scale = (window_width / image_width).max(window_height / image_height);
    let (width, height) = (image_width * scale, image_height * scale);
    if !usable(width) || !usable(height) {
        return None;
    }
    Some((
        (window_width - width) / 2.0,
        (window_height - height) / 2.0,
        width,
        height,
    ))
}

/// The size with the long side capped at `max_long_side`; the aspect ratio is kept and an image
/// is never enlarged.
fn fit_long_side(width: u32, height: u32, max_long_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max_long_side {
        return (width, height);
    }
    let shrink = |side: u32| {
        let scaled =
            (u64::from(side) * u64::from(max_long_side) + u64::from(long) / 2) / u64::from(long);
        (scaled as u32).max(1)
    };
    (shrink(width), shrink(height))
}

/// Reads one image file, refusing a big file or a huge picture before any pixel is decoded.
/// A GIF gives its first frame. Transparent parts are laid over black, so the result is opaque.
fn read_image(path: &Path) -> Result<RgbaImage, BackgroundError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(BackgroundError::NotAFile);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(BackgroundError::FileTooLarge(metadata.len()));
    }
    let mut decoder = ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()
        .map_err(BackgroundError::Decode)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(BackgroundError::TooManyPixels { width, height });
    }
    // A camera photo is stored sideways with an EXIF tag saying how to turn it; a missing or
    // unreadable tag means upright.
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut upright = DynamicImage::from_decoder(decoder).map_err(BackgroundError::Decode)?;
    upright.apply_orientation(orientation);
    let mut image = upright.into_rgba8();
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha != 255 {
            for channel in &mut pixel.0[..3] {
                *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
    let (fit_width, fit_height) = fit_long_side(image.width(), image.height(), MAX_LONG_SIDE);
    if (fit_width, fit_height) != image.dimensions() {
        image = imageops::resize(&image, fit_width, fit_height, FilterType::Lanczos3);
    }
    Ok(image)
}

/// Reads the picked file, shrinks it, and stores the copy as `background.png` in `support_dir`
/// (replacing any earlier copy only when everything succeeded). The source file is never touched.
pub(crate) fn import(source: &Path, support_dir: &Path) -> Result<RgbaImage, BackgroundError> {
    let image = read_image(source)?;
    let rgb = RgbImage::from_raw(
        image.width(),
        image.height(),
        image
            .pixels()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
    )
    .expect("an RGB buffer of the same dimensions");
    let mut png = Cursor::new(Vec::new());
    rgb.write_to(&mut png, ImageFormat::Png)
        .map_err(BackgroundError::Encode)?;
    persistence::write_file_atomically(&support_dir.join(FILE_NAME), png.get_ref())
        .map_err(BackgroundError::Save)?;
    Ok(image)
}

/// Reads the stored copy at startup.
pub(crate) fn load_saved(support_dir: &Path) -> Result<RgbaImage, BackgroundError> {
    read_image(&support_dir.join(FILE_NAME))
}

/// Deletes the stored copy; one that is already gone is fine.
pub(crate) fn remove(support_dir: &Path) -> io::Result<()> {
    match fs::remove_file(support_dir.join(FILE_NAME)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Three box blurs of these widths approximate a Gaussian of `sigma` pixels.
fn box_widths(sigma: f32) -> [usize; 3] {
    const PASSES: f32 = 3.0;
    let variance12 = 12.0 * sigma * sigma;
    let ideal = (variance12 / PASSES + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let lower_f = lower as f32;
    let wider = ((variance12 - PASSES * lower_f * lower_f - 4.0 * PASSES * lower_f - 3.0 * PASSES)
        / (-4.0 * lower_f - 4.0))
        .round() as i32;
    std::array::from_fn(|pass| {
        (if (pass as i32) < wider {
            lower
        } else {
            lower + 2
        }) as usize
    })
}

/// One horizontal box blur with edge pixels repeated beyond the border.
fn blur_rows(source: &[u8], target: &mut [u8], width: usize, radius: usize) {
    let window = (2 * radius + 1) as u32;
    let last = width - 1;
    for (row, out) in source
        .chunks_exact(width * 4)
        .zip(target.chunks_exact_mut(width * 4))
    {
        let mut sums = [0_u32; 4];
        for offset in 0..=2 * radius {
            let x = (offset as isize - radius as isize).clamp(0, last as isize) as usize;
            for channel in 0..4 {
                sums[channel] += u32::from(row[x * 4 + channel]);
            }
        }
        for x in 0..width {
            for channel in 0..4 {
                out[x * 4 + channel] = ((sums[channel] + window / 2) / window) as u8;
            }
            let entering = (x + radius + 1).min(last);
            let leaving = x.saturating_sub(radius);
            for channel in 0..4 {
                sums[channel] += u32::from(row[entering * 4 + channel]);
                sums[channel] -= u32::from(row[leaving * 4 + channel]);
            }
        }
    }
}

/// One vertical box blur, walking row by row so memory is read in order.
fn blur_columns(source: &[u8], target: &mut [u8], width: usize, radius: usize) {
    let stride = width * 4;
    let height = source.len() / stride;
    let window = (2 * radius + 1) as u32;
    let last = height - 1;
    let mut sums = vec![0_u32; stride];
    for offset in 0..=2 * radius {
        let y = (offset as isize - radius as isize).clamp(0, last as isize) as usize;
        for (sum, value) in sums.iter_mut().zip(&source[y * stride..(y + 1) * stride]) {
            *sum += u32::from(*value);
        }
    }
    for y in 0..=last {
        for (out, sum) in target[y * stride..(y + 1) * stride].iter_mut().zip(&sums) {
            *out = ((sum + window / 2) / window) as u8;
        }
        let entering = (y + radius + 1).min(last);
        let leaving = y.saturating_sub(radius);
        let (entering_row, leaving_row) = (
            &source[entering * stride..(entering + 1) * stride],
            &source[leaving * stride..(leaving + 1) * stride],
        );
        for ((sum, add), subtract) in sums.iter_mut().zip(entering_row).zip(leaving_row) {
            *sum = *sum + u32::from(*add) - u32::from(*subtract);
        }
    }
}

/// A Gaussian-like blur (three separable box passes) of `sigma` pixels. A `sigma` too small to
/// move a pixel returns the image unchanged. Cost does not grow with `sigma`.
pub(crate) fn blur_rgba(image: &RgbaImage, sigma: f32) -> RgbaImage {
    let (width, height) = (image.width() as usize, image.height() as usize);
    if !sigma.is_finite() || sigma <= 0.0 || width == 0 || height == 0 {
        return image.clone();
    }
    let mut current = image.as_raw().clone();
    let mut scratch = vec![0_u8; current.len()];
    for box_width in box_widths(sigma) {
        let radius = (box_width - 1) / 2;
        if radius == 0 {
            continue;
        }
        blur_rows(&current, &mut scratch, width, radius);
        blur_columns(&scratch, &mut current, width, radius);
    }
    RgbaImage::from_raw(image.width(), image.height(), current)
        .expect("a buffer of the original size")
}

/// The picture as shown: BGRA bytes, as GPUI expects. GPUI blends a picture with straight
/// (not premultiplied) alpha and gives `paint_image` no opacity argument, so the opacity is
/// written into every pixel's alpha.
fn bake(image: &RgbaImage, opacity: f32) -> Arc<RenderImage> {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut bytes = image.as_raw().clone();
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = alpha;
    }
    let buffer = ImageBuffer::from_raw(image.width(), image.height(), bytes)
        .expect("a buffer of the original size");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

/// The shrunken picture plus the newest blurred copy, so a change of opacity alone never blurs
/// again. Cloning shares the pixel buffers.
#[derive(Clone)]
pub(crate) struct Source {
    original: Arc<RgbaImage>,
    blurred: Option<(u32, Arc<RgbaImage>)>,
}

/// A finished picture for the scene, with the blurred copy it was made from.
pub(crate) struct Rendered {
    pub image: Arc<RenderImage>,
    blur_key: u32,
    blurred: Option<Arc<RgbaImage>>,
}

impl Source {
    pub(crate) fn new(image: RgbaImage) -> Self {
        Self {
            original: Arc::new(image),
            blurred: None,
        }
    }

    /// The picture for `blur` and `opacity` (both 0..=1). Heavy when the blur is new: run it off
    /// the UI thread.
    pub(crate) fn render(&self, blur: f32, opacity: f32) -> Rendered {
        let sigma = blur_sigma(blur, self.original.width().max(self.original.height()));
        if sigma <= 0.0 {
            return Rendered {
                image: bake(&self.original, opacity),
                blur_key: 0,
                blurred: None,
            };
        }
        let key = sigma.to_bits();
        let blurred = match &self.blurred {
            Some((cached, pixels)) if *cached == key => pixels.clone(),
            _ => Arc::new(blur_rgba(&self.original, sigma)),
        };
        Rendered {
            image: bake(&blurred, opacity),
            blur_key: key,
            blurred: Some(blurred),
        }
    }

    /// Keeps the blurred copy of a finished picture for the next `render`.
    pub(crate) fn remember(&mut self, rendered: &Rendered) {
        self.blurred = rendered
            .blurred
            .clone()
            .map(|pixels| (rendered.blur_key, pixels));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use image::Rgba;

    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ambien-background-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-3
    }

    fn rect(image: (f32, f32), window: (f32, f32)) -> (f32, f32, f32, f32) {
        cover_rect(image.0, image.1, window.0, window.1).unwrap()
    }

    #[test]
    fn cover_fills_the_window_keeps_the_aspect_and_centres_the_crop() {
        // Landscape picture in a portrait window: the height fits, the sides are cut off.
        let (x, y, width, height) = rect((200.0, 100.0), (100.0, 100.0));
        assert!(close(height, 100.0) && close(width, 200.0));
        assert!(close(x, -50.0) && close(y, 0.0));
        // Portrait picture in a landscape window: the width fits, top and bottom are cut off.
        let (x, y, width, height) = rect((100.0, 200.0), (100.0, 50.0));
        assert!(close(width, 100.0) && close(height, 200.0));
        assert!(close(x, 0.0) && close(y, -75.0));
        // The same ratio fills it exactly, at any size.
        let (x, y, width, height) = rect((40.0, 30.0), (400.0, 300.0));
        assert!(close(x, 0.0) && close(y, 0.0) && close(width, 400.0) && close(height, 300.0));
        // Small pictures are enlarged to cover.
        let (_, _, width, height) = rect((2.0, 1.0), (640.0, 640.0));
        assert!(close(width, 1280.0) && close(height, 640.0));
        // An extreme ratio still covers and stays finite.
        let (x, y, width, height) = rect((10_000.0, 1.0), (300.0, 200.0));
        assert!(width >= 300.0 && close(height, 200.0) && x < 0.0 && close(y, 0.0));
        assert!(close(x + width / 2.0, 150.0));
    }

    #[test]
    fn cover_refuses_sizes_it_cannot_scale() {
        for sizes in [
            (0.0, 10.0, 10.0, 10.0),
            (10.0, 0.0, 10.0, 10.0),
            (10.0, 10.0, 0.0, 10.0),
            (10.0, 10.0, 10.0, 0.0),
            (-1.0, 10.0, 10.0, 10.0),
            (f32::NAN, 10.0, 10.0, 10.0),
            (10.0, f32::INFINITY, 10.0, 10.0),
            (10.0, 10.0, f32::NAN, 10.0),
            (10.0, 10.0, 10.0, f32::NEG_INFINITY),
        ] {
            assert_eq!(cover_rect(sizes.0, sizes.1, sizes.2, sizes.3), None);
        }
    }

    #[test]
    fn the_long_side_is_capped_and_a_small_picture_is_never_enlarged() {
        assert_eq!(fit_long_side(5120, 2560, 2560), (2560, 1280));
        assert_eq!(fit_long_side(1000, 4000, 2000), (500, 2000));
        assert_eq!(fit_long_side(2560, 1600, 2560), (2560, 1600));
        assert_eq!(fit_long_side(100, 50, 2560), (100, 50));
        // A hairline stays at least one pixel.
        assert_eq!(fit_long_side(100_000, 1, 2560), (2560, 1));
    }

    #[test]
    fn blur_converts_to_pixels_from_the_long_side() {
        assert_eq!(blur_sigma(0.0, 2560), 0.0);
        assert!(close(blur_sigma(1.0, 2560), 76.8));
        assert!(close(blur_sigma(0.5, 1000), 15.0));
        // Out-of-range and non-finite values are held to the ends.
        assert!(close(blur_sigma(7.0, 1000), 30.0));
        assert_eq!(blur_sigma(-1.0, 1000), 0.0);
        assert_eq!(blur_sigma(f32::NAN, 1000), 0.0);
    }

    fn hard_edge() -> RgbaImage {
        RgbaImage::from_fn(96, 12, |x, _| {
            if x < 48 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        })
    }

    #[test]
    fn no_blur_changes_nothing_and_blur_smooths_an_edge_without_changing_the_brightness() {
        let edge = hard_edge();
        assert_eq!(blur_rgba(&edge, 0.0), edge);
        let blurred = blur_rgba(&edge, 4.0);
        // Away from the edge the picture is untouched; across it the steps become a ramp.
        assert_eq!(blurred.get_pixel(5, 3)[0], 0);
        assert_eq!(blurred.get_pixel(90, 3)[0], 255);
        let ramp: Vec<u8> = (40..56).map(|x| blurred.get_pixel(x, 3)[0]).collect();
        assert!(ramp.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(
            ramp.iter()
                .filter(|value| (1..255).contains(*value))
                .count()
                >= 8
        );
        assert!(blurred.pixels().all(|pixel| pixel[3] == 255));
        // The mean brightness stays put (up to rounding).
        let mean = |image: &RgbaImage| {
            image.pixels().map(|pixel| f64::from(pixel[1])).sum::<f64>()
                / f64::from(image.width() * image.height())
        };
        assert!((mean(&blurred) - mean(&edge)).abs() < 1.0);
    }

    #[test]
    fn a_blur_wider_than_the_picture_keeps_a_flat_picture_flat_and_never_goes_out_of_bounds() {
        let flat = RgbaImage::from_pixel(7, 3, Rgba([40, 90, 200, 255]));
        assert_eq!(blur_rgba(&flat, 500.0), flat);
        let thin = RgbaImage::from_fn(1, 9, |_, y| Rgba([(y * 20) as u8, 0, 0, 255]));
        let blurred = blur_rgba(&thin, 80.0);
        assert_eq!(blurred.dimensions(), (1, 9));
        let spread = |image: &RgbaImage| {
            let values: Vec<u8> = image.pixels().map(|pixel| pixel[0]).collect();
            values.iter().max().unwrap() - values.iter().min().unwrap()
        };
        assert!(spread(&blurred) < spread(&thin));
    }

    #[test]
    fn rendering_gives_bgra_with_the_opacity_in_alpha_and_reuses_a_remembered_blur() {
        let image = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let mut source = Source::new(image);
        let rendered = source.render(0.0, 0.5);
        assert_eq!(rendered.image.as_bytes(0).unwrap()[..4], [30, 20, 10, 128]);
        assert_eq!(rendered.image.as_bytes(0).unwrap().len(), 8 * 8 * 4);
        assert!(rendered.blurred.is_none());
        // Opacity at the ends: invisible and fully shown.
        assert_eq!(source.render(0.0, 0.0).image.as_bytes(0).unwrap()[3], 0);
        assert_eq!(source.render(0.0, 1.0).image.as_bytes(0).unwrap()[3], 255);
        // A blurred render keeps its copy, and the next opacity change shares it.
        let blurred = source.render(1.0, 1.0);
        source.remember(&blurred);
        let again = source.render(1.0, 0.25);
        assert!(Arc::ptr_eq(
            again.blurred.as_ref().unwrap(),
            blurred.blurred.as_ref().unwrap()
        ));
        assert_eq!(again.image.as_bytes(0).unwrap()[3], 64);
        // A new blur amount recomputes it.
        let other = source.render(0.5, 1.0);
        assert!(!Arc::ptr_eq(
            other.blurred.as_ref().unwrap(),
            blurred.blurred.as_ref().unwrap()
        ));
    }

    #[test]
    fn importing_shrinks_stores_a_private_copy_and_leaves_the_source_alone() {
        let folder = Folder::new();
        let support = folder.0.join("support");
        fs::create_dir_all(&support).unwrap();
        // 3000×10 for the shrink; one clear pixel for the lay-over-black rule.
        let mut wide = RgbaImage::from_pixel(3000, 10, Rgba([200, 100, 50, 255]));
        wide.put_pixel(0, 0, Rgba([255, 255, 255, 0]));
        let source = folder.write("wide.png", &encode(&wide, ImageFormat::Png));
        let before = fs::read(&source).unwrap();
        let image = import(&source, &support).unwrap();
        assert_eq!(image.dimensions(), (2560, 9));
        assert_eq!(fs::read(&source).unwrap(), before);
        let stored = support.join(FILE_NAME);
        assert_eq!(
            fs::metadata(&stored).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Only the copy remains: no temporary file is left behind.
        assert_eq!(fs::read_dir(&support).unwrap().count(), 1);
        let saved = load_saved(&support).unwrap();
        assert_eq!(saved, image);
        assert!(saved.pixels().all(|pixel| pixel[3] == 255));
        // The source is not needed any more.
        fs::remove_file(&source).unwrap();
        assert_eq!(load_saved(&support).unwrap(), image);
    }

    #[test]
    fn a_camera_photo_with_an_exif_turn_is_stored_upright() {
        let folder = Folder::new();
        // Left half red, right half blue, 16×8, as a JPEG.
        let picture = RgbaImage::from_fn(16, 8, |x, _| {
            if x < 8 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        });
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(picture)
            .into_rgb8()
            .write_to(&mut jpeg, ImageFormat::Jpeg)
            .unwrap();
        let jpeg = jpeg.into_inner();
        assert_eq!(
            read_image(&folder.write("plain.jpg", &jpeg))
                .unwrap()
                .dimensions(),
            (16, 8)
        );
        // An APP1 segment right after SOI: big-endian TIFF with one entry, Orientation = 6
        // ("rotate 90° clockwise to display").
        let mut app1 =
            b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0".to_vec();
        let length = (app1.len() + 2) as u16;
        let mut turned = jpeg[..2].to_vec();
        turned.extend_from_slice(&[0xFF, 0xE1]);
        turned.extend_from_slice(&length.to_be_bytes());
        turned.append(&mut app1);
        turned.extend_from_slice(&jpeg[2..]);
        let image = read_image(&folder.write("turned.jpg", &turned)).unwrap();
        // The turned picture is portrait now, and what was the left (red) half is on top.
        assert_eq!(image.dimensions(), (8, 16));
        let (top, bottom) = (image.get_pixel(4, 2), image.get_pixel(4, 13));
        assert!(top[0] > 200 && top[2] < 60, "{top:?}");
        assert!(bottom[2] > 200 && bottom[0] < 60, "{bottom:?}");
    }

    #[test]
    fn jpeg_webp_and_gif_files_are_accepted() {
        let folder = Folder::new();
        let picture = RgbaImage::from_pixel(16, 8, Rgba([90, 120, 30, 255]));
        let jpeg = DynamicImage::ImageRgba8(picture.clone()).into_rgb8();
        let mut jpeg_bytes = Cursor::new(Vec::new());
        jpeg.write_to(&mut jpeg_bytes, ImageFormat::Jpeg).unwrap();
        for (name, bytes) in [
            ("a.jpg", jpeg_bytes.into_inner()),
            ("b.gif", encode(&picture, ImageFormat::Gif)),
            ("c.webp", encode(&picture, ImageFormat::WebP)),
        ] {
            let path = folder.write(name, &bytes);
            assert_eq!(read_image(&path).unwrap().dimensions(), (16, 8), "{name}");
        }
    }

    #[test]
    fn a_missing_corrupt_or_unsupported_file_is_an_error_and_keeps_the_old_copy() {
        let folder = Folder::new();
        let support = folder.0.join("support");
        fs::create_dir_all(&support).unwrap();
        let first = folder.write(
            "first.png",
            &encode(
                &RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255])),
                ImageFormat::Png,
            ),
        );
        let kept = import(&first, &support).unwrap();
        let stored = fs::read(support.join(FILE_NAME)).unwrap();

        let corrupt = folder.write("corrupt.png", b"\x89PNG\r\n\x1a\nnot really a png");
        assert!(matches!(
            import(&corrupt, &support),
            Err(BackgroundError::Decode(_))
        ));
        let text = folder.write("notes.png", b"hello");
        assert!(matches!(
            import(&text, &support),
            Err(BackgroundError::Decode(_))
        ));
        assert!(matches!(
            import(&folder.0.join("missing.png"), &support),
            Err(BackgroundError::Io(_))
        ));
        assert!(matches!(
            import(&folder.0, &support),
            Err(BackgroundError::NotAFile)
        ));
        // Every failure left the earlier copy exactly as it was, and the message is readable.
        assert_eq!(fs::read(support.join(FILE_NAME)).unwrap(), stored);
        assert_eq!(load_saved(&support).unwrap(), kept);
        assert!(
            import(&corrupt, &support)
                .unwrap_err()
                .to_string()
                .contains("開けません")
        );
    }

    #[test]
    fn a_big_file_or_a_huge_picture_is_refused_before_decoding() {
        let folder = Folder::new();
        let support = folder.0.join("support");
        fs::create_dir_all(&support).unwrap();
        // A sparse file one byte over the limit is refused by its size alone.
        let big = folder.0.join("big.png");
        let file = fs::File::create(&big).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(matches!(
            import(&big, &support),
            Err(BackgroundError::FileTooLarge(size)) if size == MAX_FILE_BYTES + 1
        ));
        // A valid small PNG whose header claims 20000×6000 (120 megapixels) is refused from the
        // header: the pixel data is never decoded (it does not even exist).
        let mut png = encode(
            &RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 255])),
            ImageFormat::Png,
        );
        png[16..20].copy_from_slice(&20_000_u32.to_be_bytes());
        png[20..24].copy_from_slice(&6_000_u32.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        let huge = folder.write("huge.png", &png);
        assert!(matches!(
            import(&huge, &support),
            Err(BackgroundError::TooManyPixels {
                width: 20_000,
                height: 6_000
            })
        ));
        // Nothing was stored for either refusal.
        assert!(!support.join(FILE_NAME).exists());
    }

    #[test]
    fn removal_deletes_the_copy_and_a_missing_copy_is_not_an_error() {
        let folder = Folder::new();
        let source = folder.write(
            "p.png",
            &encode(
                &RgbaImage::from_pixel(2, 2, Rgba([9, 9, 9, 255])),
                ImageFormat::Png,
            ),
        );
        import(&source, &folder.0).unwrap();
        assert!(folder.names().contains(&FILE_NAME.to_owned()));
        remove(&folder.0).unwrap();
        assert!(!folder.names().contains(&FILE_NAME.to_owned()));
        remove(&folder.0).unwrap();
        assert!(matches!(load_saved(&folder.0), Err(BackgroundError::Io(_))));
    }
}
