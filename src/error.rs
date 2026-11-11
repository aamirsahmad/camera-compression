//! Error type for the crate.

use std::path::{Path, PathBuf};

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Anything that can go wrong while compressing a batch.
///
/// Every variant carries the path it applies to, because a batch run reports
/// failures long after the call that produced them.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A filesystem operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The path being read, written, or listed.
        path: PathBuf,
        /// The underlying OS error.
        source: std::io::Error,
    },

    /// The source argument was neither a directory nor a file list.
    #[error("source is neither a directory nor a file: {0}")]
    InvalidSource(PathBuf),

    /// Walking the source turned up nothing to do.
    #[error("no jpgs found under {0}")]
    NoInputs(PathBuf),

    /// A JPEG could not be decoded, resized, or encoded.
    #[error("{path}: {reason}")]
    Jpeg {
        /// The image that failed.
        path: PathBuf,
        /// What went wrong, in terms the user can act on.
        reason: String,
    },

    /// One or more files failed during a batch run.
    #[error("{failed} of {total} file(s) failed")]
    Batch {
        /// How many inputs failed.
        failed: usize,
        /// How many inputs were attempted.
        total: usize,
    },
}

impl Error {
    /// Attach a path to an [`std::io::Error`].
    pub(crate) fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// Build a [`Error::Jpeg`] for `path`.
    pub(crate) fn jpeg(path: impl AsRef<Path>, reason: impl Into<String>) -> Self {
        Self::Jpeg {
            path: path.as_ref().to_path_buf(),
            reason: reason.into(),
        }
    }
}
