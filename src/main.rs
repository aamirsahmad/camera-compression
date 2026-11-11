//! Command-line front end.
//!
//! Everything here is argument parsing and terminal output; the actual work
//! lives in the library so it can be tested and reused without a CLI.

mod cli;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camera_compression::{Config, Error, Progress, Result, batch, source};
use cli::Parsed;
use indicatif::{ProgressBar, ProgressStyle};

/// How many colliding names to list before abbreviating.
const SHOWN_COLLISIONS: usize = 5;

/// Renders a progress bar and failure lines to the terminal.
struct Bar(ProgressBar);

impl Progress for Bar {
    fn advance(&self) {
        self.0.inc(1);
    }

    fn file_failed(&self, message: &str) {
        // `suspend` keeps the message from being overwritten by the bar.
        self.0.suspend(|| eprintln!("FAIL {message}"));
    }
}

fn main() -> ExitCode {
    let parsed = match cli::parse(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::FAILURE;
        }
    };

    match parsed {
        Parsed::Help => {
            println!("{}", cli::HELP);
            ExitCode::SUCCESS
        }
        Parsed::Version => {
            println!("camera-compression {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Parsed::Run { source, config } => match run(&source, &config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Collect inputs, warn about collisions, then hand off to the library.
fn run(src: &Path, config: &Config) -> Result<()> {
    let files = source::collect(src)?;
    if files.is_empty() {
        return Err(Error::NoInputs(src.to_path_buf()));
    }
    warn_about_collisions(&files);

    let bar = Bar(progress_bar(files.len()));
    let stats = batch::run(&files, config, &bar)?;
    bar.0.finish_and_clear();

    // The tally prints either way, so a partially failed batch stays legible.
    report(stats, config.keep_going);

    if stats.had_failures() {
        return Err(Error::Batch {
            failed: stats.failed,
            total: files.len(),
        });
    }
    Ok(())
}

/// Flat output means two subdirectories can hold the same filename.
fn warn_about_collisions(files: &[PathBuf]) {
    let dupes = source::colliding_stems(files);
    if dupes.is_empty() {
        return;
    }
    eprintln!(
        "warning: {} duplicate filename(s) across subdirectories; the flat output keeps only \
         one of each: {}{}",
        dupes.len(),
        dupes
            .iter()
            .take(SHOWN_COLLISIONS)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        if dupes.len() > SHOWN_COLLISIONS {
            ", ..."
        } else {
            ""
        }
    );
}

/// A bar that degrades to a plain counter when the template is rejected.
fn progress_bar(total: usize) -> ProgressBar {
    let bar = ProgressBar::new(total as u64);
    if let Ok(style) = ProgressStyle::with_template(
        "{bar:40.cyan/blue} {pos}/{len} ({percent}%) {elapsed_precise} eta {eta}",
    ) {
        bar.set_style(style.progress_chars("=> "));
    }
    bar
}

/// Print the closing summary line.
fn report(stats: batch::Stats, keep_going: bool) {
    let mib = stats.bytes as f64 / (1024.0 * 1024.0);
    println!(
        "{} written ({mib:.1} MiB), {} skipped, {} failed",
        stats.written, stats.skipped, stats.failed
    );
    if stats.failed > 0 && !keep_going {
        eprintln!("stopped early; pass --keep-going to process the rest");
    }
}
