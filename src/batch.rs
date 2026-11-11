//! Parallel driver over a list of inputs.
//!
//! Uses `std::thread` rather than a runtime. The work is CPU-bound, so an async
//! executor would only add `spawn_blocking` ceremony around what is really a
//! thread pool, and `std::thread::scope` lets workers borrow the input list
//! without an `Arc`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::pipeline::{self, Outcome};

/// Threads to use when the core count cannot be determined.
const FALLBACK_THREADS: usize = 4;

/// Tally of a completed run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Files compressed and written.
    pub written: usize,
    /// Files skipped because the output existed or the input vanished.
    pub skipped: usize,
    /// Files that failed.
    pub failed: usize,
    /// Total bytes written.
    pub bytes: u64,
}

impl Stats {
    /// Total inputs accounted for.
    #[must_use]
    pub fn attempted(&self) -> usize {
        self.written + self.skipped + self.failed
    }

    /// True if any input failed.
    #[must_use]
    pub fn had_failures(&self) -> bool {
        self.failed > 0
    }
}

/// Callbacks for reporting progress as a run proceeds.
///
/// Implementations are called from worker threads and must be cheap. The
/// library never writes to the terminal itself, which keeps it usable from
/// tests and other programs.
pub trait Progress: Sync {
    /// One input finished, in any outcome.
    fn advance(&self) {}

    /// A single file failed. `message` already includes the path.
    fn file_failed(&self, message: &str) {
        let _ = message;
    }
}

/// Discards every report. Useful in tests and non-interactive callers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Silent;

impl Progress for Silent {}

/// Compress every file in `files` into `config.out_dir`.
///
/// The output directory is created if absent. Processing stops early on the
/// first failure unless [`Config::keep_going`] is set.
///
/// Per-file failures are counted in [`Stats::failed`] rather than returned as
/// an error, so a partial run stays reportable. Deciding whether failures are
/// fatal is the caller's policy; see [`Stats::had_failures`].
///
/// # Errors
///
/// Returns [`Error::Io`] only if the output directory cannot be created, which
/// prevents the run from starting at all.
pub fn run(files: &[PathBuf], config: &Config, progress: &dyn Progress) -> Result<Stats> {
    std::fs::create_dir_all(&config.out_dir).map_err(|e| Error::io(&config.out_dir, e))?;

    let counters = Counters::default();
    let next = AtomicUsize::new(0);
    let halt = AtomicBool::new(false);

    std::thread::scope(|scope| {
        for _ in 0..thread_count(config, files.len()) {
            scope.spawn(|| {
                // An atomic index gives dynamic load balancing, which matters
                // because per-file cost varies with image content.
                while !halt.load(Ordering::Relaxed) {
                    let Some(path) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    match pipeline::process(path, config) {
                        Ok(Outcome::Written { bytes }) => {
                            counters.written.fetch_add(1, Ordering::Relaxed);
                            counters.bytes.fetch_add(bytes as usize, Ordering::Relaxed);
                        }
                        Ok(Outcome::SkippedExists | Outcome::SkippedMissing) => {
                            counters.skipped.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            counters.failed.fetch_add(1, Ordering::Relaxed);
                            progress.file_failed(&e.to_string());
                            if !config.keep_going {
                                halt.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    progress.advance();
                }
            });
        }
    });

    Ok(counters.snapshot())
}

/// Shared tallies, incremented from every worker.
#[derive(Default)]
struct Counters {
    written: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    bytes: AtomicUsize,
}

impl Counters {
    /// Read every counter once the workers have joined.
    fn snapshot(&self) -> Stats {
        Stats {
            written: self.written.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed) as u64,
        }
    }
}

/// Worker count: the configured value, else the core count.
///
/// Capped at the number of inputs, so a two-file run does not spawn sixteen
/// threads, and floored at one so an empty list still terminates.
fn thread_count(config: &Config, inputs: usize) -> usize {
    config
        .threads
        .or_else(|| std::thread::available_parallelism().ok().map(Into::into))
        .unwrap_or(FALLBACK_THREADS)
        .clamp(1, inputs.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(threads: Option<usize>) -> Config {
        Config {
            threads,
            ..Config::new("/tmp/unused")
        }
    }

    #[test]
    fn thread_count_honours_an_explicit_setting() {
        assert_eq!(thread_count(&config_with(Some(3)), 100), 3);
    }

    #[test]
    fn thread_count_never_exceeds_the_number_of_inputs() {
        assert_eq!(thread_count(&config_with(Some(16)), 2), 2);
    }

    #[test]
    fn thread_count_is_at_least_one_even_with_no_inputs() {
        assert_eq!(thread_count(&config_with(Some(8)), 0), 1);
        assert!(thread_count(&config_with(None), 0) >= 1);
    }

    #[test]
    fn thread_count_defaults_to_something_usable() {
        let n = thread_count(&config_with(None), 1000);
        assert!(n >= 1, "must spawn at least one worker");
    }

    #[test]
    fn run_on_an_empty_list_creates_the_output_dir_and_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("created/here");
        let config = Config::new(&out);

        let stats = run(&[], &config, &Silent).unwrap();
        assert_eq!(stats, Stats::default());
        assert!(out.is_dir(), "output directory should be created up front");
    }

    #[test]
    fn run_counts_missing_inputs_as_skipped_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new(dir.path().join("out"));
        let files = vec![
            PathBuf::from("/no/such/a.jpg"),
            PathBuf::from("/no/such/b.jpg"),
        ];

        let stats = run(&files, &config, &Silent).unwrap();
        assert_eq!(stats.skipped, 2);
        assert_eq!(stats.written, 0);
        assert_eq!(stats.failed, 0);
    }
}
