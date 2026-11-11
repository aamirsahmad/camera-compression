//! Single-image pipeline: decode, downscale, encode.
//!
//! Resampling happens in full RGB rather than ffmpeg's subsampled YUV 4:2:2,
//! which is worth roughly 8 dB PSNR against a high-precision reference.

use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::{metadata, orient};

/// Suffix for partially written output, renamed into place on success.
const PARTIAL_SUFFIX: &str = "jpg.part";

/// What happened to one input file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file was compressed and written.
    Written {
        /// Size of the output in bytes.
        bytes: u64,
    },
    /// Output already existed and `force` was not set.
    SkippedExists,
    /// The input disappeared between listing and processing.
    SkippedMissing,
}

/// Compress `input` into `config.out_dir`.
///
/// Missing inputs are skipped rather than treated as errors, matching the
/// shell script's `[ -f ]` guard: a file list can outlive the files in it.
///
/// # Errors
///
/// Returns [`Error::Jpeg`] if the image cannot be decoded, resized, or encoded,
/// or [`Error::Io`] if reading the input or writing the output fails.
pub fn process(input: &Path, config: &Config) -> Result<Outcome> {
    if !input.is_file() {
        return Ok(Outcome::SkippedMissing);
    }
    let stem = input
        .file_stem()
        .ok_or_else(|| Error::jpeg(input, "path has no file stem"))?;
    let out_path = config.out_dir.join(stem).with_extension("jpg");
    if out_path.exists() && !config.force {
        return Ok(Outcome::SkippedExists);
    }

    let data = std::fs::read(input).map_err(|e| Error::io(input, e))?;
    let decoded = decode(input, &data)?;
    let jpeg = downscale_and_encode(input, &data, decoded, config)?;

    let bytes = jpeg.len() as u64;
    write_atomically(&out_path, &jpeg)?;
    Ok(Outcome::Written { bytes })
}

/// An RGB8 image and its dimensions.
struct Decoded {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// Decode a JPEG to RGB8.
fn decode(path: &Path, data: &[u8]) -> Result<Decoded> {
    let mut decoder = zune_jpeg::JpegDecoder::new(data);
    decoder
        .decode_headers()
        .map_err(|e| Error::jpeg(path, format!("decode headers: {e}")))?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| Error::jpeg(path, "no dimensions in header"))?;
    let pixels = decoder
        .decode()
        .map_err(|e| Error::jpeg(path, format!("decode: {e}")))?;

    Ok(Decoded {
        pixels,
        width: width as u32,
        height: height as u32,
    })
}

/// Resize to fit, rotate upright, then encode.
fn downscale_and_encode(
    path: &Path,
    original: &[u8],
    decoded: Decoded,
    config: &Config,
) -> Result<Vec<u8>> {
    // Size bounds apply to the *upright* image, matching ffmpeg's
    // rotate-then-scale order.
    let orientation = orient::read(original);
    let (upright_w, upright_h) = if orient::transposes(orientation) {
        (decoded.height, decoded.width)
    } else {
        (decoded.width, decoded.height)
    };
    let (target_w, target_h) = fit(upright_w, upright_h, config.max_width, config.max_height);

    // Resizing happens before rotation, so swap the axes back for the
    // transposing orientations.
    let (resize_w, resize_h) = if orient::transposes(orientation) {
        (target_h, target_w)
    } else {
        (target_w, target_h)
    };

    let source = Image::from_vec_u8(
        decoded.width,
        decoded.height,
        decoded.pixels,
        PixelType::U8x3,
    )
    .map_err(|e| Error::jpeg(path, format!("wrap source: {e}")))?;
    let mut resized = Image::new(resize_w, resize_h, PixelType::U8x3);
    Resizer::new()
        .resize(
            &source,
            &mut resized,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
        )
        .map_err(|e| Error::jpeg(path, format!("resize: {e}")))?;

    let (pixels, out_w, out_h) = orient::apply(resized.buffer(), resize_w, resize_h, orientation);
    encode(path, &pixels, out_w, out_h, original, config)
}

/// Encode RGB8 pixels as a JPEG, attaching preserved metadata.
fn encode(
    path: &Path,
    pixels: &[u8],
    width: u32,
    height: u32,
    original: &[u8],
    config: &Config,
) -> Result<Vec<u8>> {
    // JPEG stores dimensions in 16 bits, so anything larger is unrepresentable
    // rather than merely awkward. Reject it instead of silently truncating.
    let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(Error::jpeg(
            path,
            format!("{width}x{height} exceeds JPEG's 65535 px limit"),
        ));
    };

    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, config.quality);
    let (horizontal, vertical) = config.chroma.pixel_sizes();
    if let Some(sampling) = SamplingFactor::from_factors(horizontal, vertical) {
        encoder.set_sampling_factor(sampling);
    }
    encoder.set_progressive(config.progressive);
    // Computing Huffman tables from the actual symbol counts costs one extra
    // pass and is worth a few percent over the JPEG Annex K defaults.
    encoder.set_optimized_huffman_tables(true);

    attach_metadata(path, &mut encoder, original, config)?;

    encoder
        .encode(pixels, w, h, ColorType::Rgb)
        .map_err(|e| Error::jpeg(path, format!("encode: {e}")))?;
    Ok(out)
}

/// Copy the preserved APP segments onto the encoder.
fn attach_metadata<W: jpeg_encoder::JfifWrite>(
    path: &Path,
    encoder: &mut Encoder<W>,
    original: &[u8],
    config: &Config,
) -> Result<()> {
    if config.metadata.is_empty() {
        return Ok(());
    }
    let md = metadata::extract(original);

    if config.metadata.exif {
        if let Some(mut exif) = md.exif {
            // Pixels are already upright; clear the tag so viewers do not
            // rotate a second time.
            metadata::normalize_orientation(&mut exif);
            // The payload already carries its `Exif\0\0` identifier, so it goes
            // out as a raw APP1 rather than through `add_exif_metadata`, which
            // would prepend a second one.
            encoder
                .add_app_segment(1, exif)
                .map_err(|e| Error::jpeg(path, format!("attach exif: {e}")))?;
        }
    }
    if config.metadata.xmp {
        if let Some(xmp) = md.xmp {
            encoder
                .add_app_segment(1, xmp)
                .map_err(|e| Error::jpeg(path, format!("attach xmp: {e}")))?;
        }
    }
    if config.metadata.icc {
        if let Some(icc) = md.icc {
            // Re-chunks across APP2 segments as the ICC spec requires.
            encoder
                .add_icc_profile(&icc)
                .map_err(|e| Error::jpeg(path, format!("attach icc profile: {e}")))?;
        }
    }
    Ok(())
}

/// Write via a sibling temporary file, then rename into place.
///
/// An interrupted run therefore never leaves a truncated output that the
/// skip-if-exists check would later trust.
///
/// The destination directory is created if absent, so [`process`] is usable on
/// its own rather than only after [`crate::batch::run`] has prepared the way.
/// The extra `stat` is nothing next to decoding a 33 MP frame.
fn write_atomically(out_path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let partial = out_path.with_extension(PARTIAL_SUFFIX);
    std::fs::write(&partial, bytes).map_err(|e| Error::io(&partial, e))?;
    if let Err(e) = std::fs::rename(&partial, out_path) {
        // Leaving a stray .part behind would be mistaken for real output.
        let _ = std::fs::remove_file(&partial);
        return Err(Error::io(out_path, e));
    }
    Ok(())
}

/// Scale `w` x `h` to fit within the given bounds, preserving aspect ratio.
///
/// Either bound may be absent, in which case only the other constrains the
/// result. Never upscales: an image already inside the bounds is unchanged.
#[must_use]
pub fn fit(w: u32, h: u32, max_w: Option<u32>, max_h: Option<u32>) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (w, h);
    }
    let scale = match (max_w, max_h) {
        (Some(mw), Some(mh)) => {
            f64::min(f64::from(mw) / f64::from(w), f64::from(mh) / f64::from(h))
        }
        (Some(mw), None) => f64::from(mw) / f64::from(w),
        (None, Some(mh)) => f64::from(mh) / f64::from(h),
        (None, None) => 1.0,
    };
    if scale >= 1.0 {
        return (w, h);
    }
    (
        ((f64::from(w) * scale).round() as u32).max(1),
        ((f64::from(h) * scale).round() as u32).max(1),
    )
}

/// Path that [`process`] would write for `input`.
///
/// Exposed so callers can predict collisions in the flat output directory.
#[must_use]
pub fn output_path(input: &Path, out_dir: &Path) -> Option<PathBuf> {
    Some(out_dir.join(input.file_stem()?).with_extension("jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scales_down_to_the_width_bound() {
        assert_eq!(fit(7008, 4672, Some(2048), None), (2048, 1365));
    }

    #[test]
    fn fit_scales_down_to_the_height_bound() {
        assert_eq!(fit(4672, 7008, Some(2048), None), (2048, 3072));
    }

    #[test]
    fn fit_uses_whichever_bound_binds_first() {
        // Landscape 3:2 inside a square box is limited by width.
        assert_eq!(fit(3000, 2000, Some(1200), Some(1200)), (1200, 800));
        // Portrait 2:3 in the same box is limited by height.
        assert_eq!(fit(2000, 3000, Some(1200), Some(1200)), (800, 1200));
    }

    #[test]
    fn fit_never_upscales() {
        assert_eq!(fit(800, 600, Some(2048), None), (800, 600));
        assert_eq!(fit(800, 600, Some(2048), Some(2048)), (800, 600));
        assert_eq!(fit(1, 1, Some(4096), Some(4096)), (1, 1));
    }

    #[test]
    fn fit_without_bounds_is_the_identity() {
        assert_eq!(fit(1234, 567, None, None), (1234, 567));
    }

    #[test]
    fn fit_preserves_aspect_ratio_within_a_rounding_step() {
        let (w, h) = fit(7008, 4672, Some(2048), None);
        let before = f64::from(7008) / f64::from(4672);
        let after = f64::from(w) / f64::from(h);
        assert!(
            (before - after).abs() < 0.001,
            "aspect drifted: {before} -> {after}"
        );
    }

    #[test]
    fn fit_clamps_extreme_downscales_to_one_pixel() {
        // A very wide panorama squeezed to 1 px wide must keep a real height.
        let (w, h) = fit(10_000, 10, Some(1), None);
        assert_eq!(w, 1);
        assert!(h >= 1, "height collapsed to zero");
    }

    #[test]
    fn fit_passes_degenerate_sizes_through_untouched() {
        assert_eq!(fit(0, 100, Some(50), None), (0, 100));
        assert_eq!(fit(100, 0, Some(50), None), (100, 0));
    }

    #[test]
    fn output_path_is_the_stem_with_a_jpg_extension() {
        let got = output_path(Path::new("/in/a/b/DSC01209.JPG"), Path::new("/out"));
        assert_eq!(got, Some(PathBuf::from("/out/DSC01209.jpg")));
    }

    #[test]
    fn output_path_is_flat_so_subdirectories_collapse() {
        let a = output_path(Path::new("/in/24/S.JPG"), Path::new("/out"));
        let b = output_path(Path::new("/in/25/S.jpg"), Path::new("/out"));
        assert_eq!(a, b, "flat output should collide for equal stems");
    }

    #[test]
    fn process_skips_an_input_that_no_longer_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new(dir.path());
        let got = process(Path::new("/no/such/file.jpg"), &config).unwrap();
        assert_eq!(got, Outcome::SkippedMissing);
    }
}
