//! Helpers for building real JPEGs on the fly.
//!
//! Tests synthesize their inputs rather than checking binary fixtures into the
//! repository, so there is nothing to keep in sync and nothing opaque to review.

use std::path::{Path, PathBuf};

/// An RGB gradient, distinguishable enough that a rotation is detectable.
#[must_use]
pub fn gradient(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.push((x * 255 / width.max(1)) as u8);
            pixels.push((y * 255 / height.max(1)) as u8);
            pixels.push(128);
        }
    }
    pixels
}

/// Encode RGB8 pixels as a baseline JPEG.
#[must_use]
pub fn encode_jpeg(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut out, 95);
    encoder
        .encode(
            pixels,
            u16::try_from(width).expect("test image width fits in u16"),
            u16::try_from(height).expect("test image height fits in u16"),
            jpeg_encoder::ColorType::Rgb,
        )
        .expect("encode test jpeg");
    out
}

/// Write a synthetic JPEG of the given size to `path`.
pub fn write_jpeg(path: &Path, width: u32, height: u32) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    let jpeg = encode_jpeg(&gradient(width, height), width, height);
    std::fs::write(path, jpeg).expect("write jpeg");
}

/// Write a synthetic JPEG carrying an EXIF orientation tag.
///
/// The EXIF block is spliced in as an APP1 segment directly after SOI, which is
/// where a camera would put it.
pub fn write_jpeg_with_orientation(path: &Path, width: u32, height: u32, orientation: u16) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    let base = encode_jpeg(&gradient(width, height), width, height);
    let exif = exif_payload(orientation);

    let mut out = Vec::with_capacity(base.len() + exif.len() + 4);
    out.extend_from_slice(&base[..2]); // SOI
    out.push(0xFF);
    out.push(0xE1); // APP1
    out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(&exif);
    out.extend_from_slice(&base[2..]);

    std::fs::write(path, out).expect("write jpeg");
}

/// A little-endian EXIF payload whose IFD0 holds only an orientation tag.
#[must_use]
fn exif_payload(orientation: u16) -> Vec<u8> {
    let mut out = b"Exif\0\0".to_vec();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes()); // IFD0 follows the header

    out.extend_from_slice(&1u16.to_le_bytes()); // one entry
    out.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    out.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    out.extend_from_slice(&1u32.to_le_bytes()); // count
    out.extend_from_slice(&orientation.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // pad the value field
    out.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
    out
}

/// Decode a JPEG back to dimensions, for asserting on output geometry.
#[must_use]
pub fn dimensions(jpeg: &[u8]) -> (u32, u32) {
    let mut decoder = zune_jpeg::JpegDecoder::new(jpeg);
    decoder.decode_headers().expect("decode headers");
    let (w, h) = decoder.dimensions().expect("dimensions");
    (w as u32, h as u32)
}

/// Read the EXIF orientation tag of a JPEG on disk, if any.
#[must_use]
pub fn orientation_of(path: &Path) -> Option<u16> {
    let data = std::fs::read(path).expect("read output");
    let mut cursor = std::io::Cursor::new(&data);
    let reader = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    reader
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .map(|v| v as u16)
}

/// Sorted file names inside `dir`.
#[must_use]
pub fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read output dir")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Path helper that keeps tests terse.
#[must_use]
pub fn at(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}
