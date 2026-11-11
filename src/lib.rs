//! Batch downscale and recompress camera JPEGs.
//!
//! Three things this does that an `ffmpeg -vf scale` pipeline does not:
//!
//! * resamples in full RGB rather than subsampled YUV 4:2:2, worth roughly 8 dB
//!   PSNR against a high-precision reference
//! * preserves EXIF, XMP, and ICC instead of discarding all of it
//! * pins chroma subsampling, so autorotated portrait frames are not silently
//!   promoted to 4:4:4 and doubled in size
//!
//! # Example
//!
//! ```no_run
//! use camera_compression::{batch, source, Config, Silent};
//! use std::path::Path;
//!
//! let files = source::collect(Path::new("photos"))?;
//! let stats = batch::run(&files, &Config::new("compressed"), &Silent)?;
//! println!("{} written", stats.written);
//! # Ok::<(), camera_compression::Error>(())
//! ```

pub mod batch;
pub mod config;
pub mod error;
pub mod metadata;
pub mod orient;
pub mod pipeline;
pub mod source;

pub use batch::{Progress, Silent, Stats};
pub use config::{Chroma, Config, MetadataPolicy, DEFAULT_MAX_WIDTH, DEFAULT_QUALITY};
pub use error::{Error, Result};
pub use pipeline::Outcome;
