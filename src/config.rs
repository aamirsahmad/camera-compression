//! Knobs for a compression run.
//!
//! Deliberately free of any CLI dependency, so the library can be driven from
//! tests or other programs without going through argument parsing.

use std::path::PathBuf;

/// Chroma subsampling of the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chroma {
    /// 4:4:4 — no chroma subsampling.
    None,
    /// 4:2:2 — half horizontal chroma resolution, what most cameras record.
    #[default]
    Half,
    /// 4:2:0 — half in both axes, the usual choice for web delivery.
    Quarter,
}

impl Chroma {
    /// Horizontal and vertical size, in luma samples, of one chroma sample.
    #[must_use]
    pub fn pixel_sizes(self) -> (u8, u8) {
        match self {
            Self::None => (1, 1),
            Self::Half => (2, 1),
            Self::Quarter => (2, 2),
        }
    }
}

/// Which metadata segments to carry from input to output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetadataPolicy {
    /// Carry the EXIF APP1 segment, rewriting orientation to upright.
    pub exif: bool,
    /// Carry the XMP APP1 segment. Sony writes ~57 KB of mostly padding.
    pub xmp: bool,
    /// Carry the ICC colour profile.
    pub icc: bool,
}

impl MetadataPolicy {
    /// Keep everything, XMP included.
    #[must_use]
    pub fn all() -> Self {
        Self {
            exif: true,
            xmp: true,
            icc: true,
        }
    }

    /// Keep nothing, matching ffmpeg's behaviour.
    #[must_use]
    pub fn none() -> Self {
        Self {
            exif: false,
            xmp: false,
            icc: false,
        }
    }

    /// True when no segment would be written.
    #[must_use]
    pub fn is_empty(self) -> bool {
        !self.exif && !self.xmp && !self.icc
    }
}

/// Default keeps EXIF and ICC but drops XMP, which is bulky and rarely wanted.
impl Default for MetadataPolicy {
    fn default() -> Self {
        Self {
            exif: true,
            xmp: false,
            icc: true,
        }
    }
}

/// Settings for a compression run.
///
/// Construct with [`Config::new`] and override fields as needed:
///
/// ```
/// use camera_compression::{Chroma, Config};
///
/// let cfg = Config {
///     quality: 82,
///     chroma: Chroma::Quarter,
///     ..Config::new("/tmp/out")
/// };
/// assert_eq!(cfg.max_width, Some(2048));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Directory that compressed files are written into, flat.
    pub out_dir: PathBuf,
    /// Upper bound on the width of the upright image. Never upscales.
    pub max_width: Option<u32>,
    /// Upper bound on the height of the upright image. Never upscales.
    pub max_height: Option<u32>,
    /// JPEG quality, 0-100.
    pub quality: u8,
    /// Chroma subsampling of the output.
    pub chroma: Chroma,
    /// Write progressive rather than baseline JPEGs.
    pub progressive: bool,
    /// Which metadata segments to preserve.
    pub metadata: MetadataPolicy,
    /// Re-encode inputs whose output already exists.
    pub force: bool,
    /// Keep processing after a file fails.
    pub keep_going: bool,
    /// Worker threads. `None` uses the available core count.
    pub threads: Option<usize>,
}

/// Width used when the caller does not specify one.
pub const DEFAULT_MAX_WIDTH: u32 = 2048;

/// Quality used when the caller does not specify one.
///
/// Encoder quality scales are not comparable across implementations. This value
/// was picked by measurement: on Sony ILCE-7M4 frames it lands at SSIM ~0.9865,
/// matching what mozjpeg produced at its own q90 before this crate moved to a
/// pure-Rust encoder.
pub const DEFAULT_QUALITY: u8 = 92;

impl Config {
    /// Defaults writing into `out_dir`: 2048 px wide, quality 92, 4:2:2,
    /// progressive, EXIF and ICC preserved.
    #[must_use]
    pub fn new(out_dir: impl Into<PathBuf>) -> Self {
        Self {
            out_dir: out_dir.into(),
            max_width: Some(DEFAULT_MAX_WIDTH),
            max_height: None,
            quality: DEFAULT_QUALITY,
            chroma: Chroma::default(),
            progressive: true,
            metadata: MetadataPolicy::default(),
            force: false,
            keep_going: false,
            threads: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chroma_maps_to_sample_sizes() {
        assert_eq!(Chroma::None.pixel_sizes(), (1, 1));
        assert_eq!(Chroma::Half.pixel_sizes(), (2, 1));
        assert_eq!(Chroma::Quarter.pixel_sizes(), (2, 2));
    }

    #[test]
    fn chroma_defaults_to_camera_native_422() {
        assert_eq!(Chroma::default(), Chroma::Half);
    }

    #[test]
    fn metadata_default_keeps_exif_and_icc_but_not_xmp() {
        let p = MetadataPolicy::default();
        assert!(p.exif, "EXIF is the whole point of preserving metadata");
        assert!(p.icc);
        assert!(!p.xmp, "XMP is bulky and off by default");
        assert!(!p.is_empty());
    }

    #[test]
    fn metadata_none_is_empty_and_all_is_not() {
        assert!(MetadataPolicy::none().is_empty());
        assert!(!MetadataPolicy::all().is_empty());
    }

    #[test]
    fn config_new_applies_documented_defaults() {
        let cfg = Config::new("/tmp/out");
        assert_eq!(cfg.out_dir, PathBuf::from("/tmp/out"));
        assert_eq!(cfg.max_width, Some(DEFAULT_MAX_WIDTH));
        assert_eq!(cfg.max_height, None);
        assert_eq!(cfg.quality, DEFAULT_QUALITY);
        assert!(cfg.progressive);
        assert!(!cfg.force);
        assert!(!cfg.keep_going);
    }

    #[test]
    fn config_supports_struct_update_syntax() {
        let cfg = Config {
            quality: 70,
            ..Config::new("/tmp/out")
        };
        assert_eq!(cfg.quality, 70);
        assert_eq!(cfg.max_width, Some(DEFAULT_MAX_WIDTH));
    }
}
