//! Argument parsing, by hand.
//!
//! The surface is two positionals and ten flags, which does not justify a
//! parser dependency. Supported forms:
//!
//! * `--name value`, `--name=value`, `--name` for switches
//! * `-n value`, `-n=value`, `-n` for switches
//! * `--` ends option parsing, so a path may begin with `-`
//!
//! Short flags are not bundled: `-fk` is an error rather than `-f -k`.

use camera_compression::{Chroma, Config, MetadataPolicy, DEFAULT_MAX_WIDTH, DEFAULT_QUALITY};
use std::path::PathBuf;

/// What the user asked for.
pub enum Parsed {
    /// Run a compression pass.
    Run {
        /// Directory to walk, or a file listing paths.
        source: PathBuf,
        /// Settings for the run.
        config: Config,
    },
    /// Print usage and exit successfully.
    Help,
    /// Print the version and exit successfully.
    Version,
}

/// Usage text, printed for `--help`.
pub const HELP: &str = concat!(
    "camera-compression ",
    env!("CARGO_PKG_VERSION"),
    "
Batch downscale and recompress camera JPEGs, preserving metadata

USAGE:
    camera-compression [OPTIONS] <SOURCE> <DESTINATION>

ARGS:
    <SOURCE>         A directory, walked recursively for jpgs, or a file
                     holding a newline-delimited list of paths
    <DESTINATION>    Where the compressed jpgs land, flat. Created if absent

OPTIONS:
    -w, --width <PX>          Max width of the upright image [default: 2048]
    -H, --height <PX>         Max height. Without this, only width constrains
                              the result
    -q, --quality <Q>         JPEG quality, 0-100 [default: 92]
    -c, --chroma <FACTOR>     Subsampling: 444, 422 or 420 [default: 422]
    -j, --threads <N>         Worker threads [default: available cores]
        --baseline            Write baseline JPEGs instead of progressive
        --strip-metadata      Drop EXIF, XMP and ICC instead of carrying them
        --xmp                 Also carry over XMP (off by default; bulky)
    -f, --force               Re-encode inputs whose output already exists
    -k, --keep-going          Finish the batch even if some files fail
    -h, --help                Print this help
    -V, --version             Print version

Sizes are an upper bound and never upscale. Re-running skips inputs whose
output already exists, so an interrupted batch resumes where it stopped."
);

/// Parse process arguments, excluding the program name.
///
/// # Errors
///
/// Returns a human-readable message when an argument is unknown, missing a
/// value, or out of range.
pub fn parse<I, S>(args: I) -> Result<Parsed, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut positional: Vec<String> = Vec::new();
    let mut opts = Options::default();

    let mut index = 0;
    let mut options_ended = false;

    while index < args.len() {
        let arg = args[index].clone();
        index += 1;

        if options_ended || !arg.starts_with('-') || arg == "-" {
            positional.push(arg);
            continue;
        }
        if arg == "--" {
            options_ended = true;
            continue;
        }

        // Split `--name=value` / `-n=value` into flag and inline value.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };

        // Pull the value for options that take one, from `=` or the next argument.
        let mut take_value = |flag: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            match args.get(index).cloned() {
                Some(v) => {
                    index += 1;
                    Ok(v)
                }
                None => Err(format!("{flag} needs a value")),
            }
        };

        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-w" | "--width" => opts.width = number(&flag, &take_value(&flag)?)?,
            "-H" | "--height" => opts.height = Some(number(&flag, &take_value(&flag)?)?),
            "-q" | "--quality" => opts.quality = quality(&flag, &take_value(&flag)?)?,
            "-c" | "--chroma" => opts.chroma = parse_chroma(&take_value(&flag)?)?,
            "-j" | "--threads" => opts.threads = Some(threads(&flag, &take_value(&flag)?)?),
            "--baseline" => opts.baseline = true,
            "--strip-metadata" => opts.strip_metadata = true,
            "--xmp" => opts.xmp = true,
            "-f" | "--force" => opts.force = true,
            "-k" | "--keep-going" => opts.keep_going = true,
            other => return Err(unknown_flag(other)),
        }

        // A switch given `--force=1` is a mistake worth naming.
        if inline.is_some() && is_switch(&flag) {
            return Err(format!("{flag} is a switch and takes no value"));
        }
    }

    opts.finish(&positional)
}

/// Option values as collected from the command line, before validation.
#[allow(clippy::struct_excessive_bools)]
struct Options {
    width: u32,
    height: Option<u32>,
    quality: u8,
    chroma: Chroma,
    threads: Option<usize>,
    baseline: bool,
    strip_metadata: bool,
    xmp: bool,
    force: bool,
    keep_going: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: DEFAULT_MAX_WIDTH,
            height: None,
            quality: DEFAULT_QUALITY,
            chroma: Chroma::default(),
            threads: None,
            baseline: false,
            strip_metadata: false,
            xmp: false,
            force: false,
            keep_going: false,
        }
    }
}

impl Options {
    /// Pair the collected options with the two required paths.
    fn finish(self, positional: &[String]) -> Result<Parsed, String> {
        let (source, destination) = match positional {
            [source, destination] => (PathBuf::from(source), PathBuf::from(destination)),
            [] | [_] => return Err("expected <SOURCE> and <DESTINATION>; see --help".to_string()),
            more => {
                return Err(format!(
                    "expected 2 paths, got {}: {}",
                    more.len(),
                    more.join(", ")
                ))
            }
        };

        let metadata = if self.strip_metadata {
            MetadataPolicy::none()
        } else {
            MetadataPolicy {
                xmp: self.xmp,
                ..MetadataPolicy::default()
            }
        };

        Ok(Parsed::Run {
            source,
            config: Config {
                out_dir: destination,
                max_width: Some(self.width),
                max_height: self.height,
                quality: self.quality,
                chroma: self.chroma,
                progressive: !self.baseline,
                metadata,
                force: self.force,
                keep_going: self.keep_going,
                threads: self.threads,
            },
        })
    }
}

/// True for flags that never take a value.
fn is_switch(flag: &str) -> bool {
    matches!(
        flag,
        "--baseline" | "--strip-metadata" | "--xmp" | "-f" | "--force" | "-k" | "--keep-going"
    )
}

/// Parse and range-check a quality value.
fn quality(flag: &str, raw: &str) -> Result<u8, String> {
    let value: u16 = number(flag, raw)?;
    if value > 100 {
        return Err(format!("quality must be between 0 and 100, got {value}"));
    }
    Ok(value as u8)
}

/// Parse a worker count, which must be positive to make any progress.
fn threads(flag: &str, raw: &str) -> Result<usize, String> {
    let value: usize = number(flag, raw)?;
    if value == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    Ok(value)
}

/// Parse an integral option value, naming the flag when it does not parse.
fn number<T: std::str::FromStr>(flag: &str, raw: &str) -> Result<T, String> {
    raw.parse()
        .map_err(|_| format!("{flag} expects a whole number, got `{raw}`"))
}

/// Map the user-facing subsampling names onto [`Chroma`].
fn parse_chroma(raw: &str) -> Result<Chroma, String> {
    match raw {
        "444" => Ok(Chroma::None),
        "422" => Ok(Chroma::Half),
        "420" => Ok(Chroma::Quarter),
        other => Err(format!("--chroma expects 444, 422 or 420, got `{other}`")),
    }
}

/// Suggest the closest real flag, so a typo does not send anyone to the docs.
fn unknown_flag(flag: &str) -> String {
    const FLAGS: &[&str] = &[
        "--width",
        "--height",
        "--quality",
        "--chroma",
        "--threads",
        "--baseline",
        "--strip-metadata",
        "--xmp",
        "--force",
        "--keep-going",
        "--help",
        "--version",
    ];
    let trimmed = flag.trim_start_matches('-');
    let suggestion = FLAGS
        .iter()
        .find(|candidate| candidate.trim_start_matches('-').starts_with(trimmed));

    match suggestion {
        Some(name) => format!("unknown flag `{flag}`; did you mean `{name}`?"),
        None => format!("unknown flag `{flag}`; see --help"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse and unwrap to the run case, which is what most tests care about.
    fn run(args: &[&str]) -> (PathBuf, Config) {
        match parse(args.iter().copied()).expect("should parse") {
            Parsed::Run { source, config } => (source, config),
            _ => panic!("expected a run"),
        }
    }

    fn err(args: &[&str]) -> String {
        match parse(args.iter().copied()) {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn positionals_become_source_and_destination() {
        let (source, config) = run(&["in", "out"]);
        assert_eq!(source, PathBuf::from("in"));
        assert_eq!(config.out_dir, PathBuf::from("out"));
    }

    #[test]
    fn defaults_match_the_library() {
        let (_, config) = run(&["in", "out"]);
        assert_eq!(config.max_width, Some(DEFAULT_MAX_WIDTH));
        assert_eq!(config.max_height, None);
        assert_eq!(config.quality, DEFAULT_QUALITY);
        assert_eq!(config.chroma, Chroma::Half);
        assert!(config.progressive);
        assert_eq!(config.metadata, MetadataPolicy::default());
        assert!(!config.force);
        assert!(!config.keep_going);
        assert_eq!(config.threads, None);
    }

    #[test]
    fn long_options_accept_both_separated_and_equals_forms() {
        for args in [
            ["in", "out", "--width", "1024"],
            ["in", "out", "--width=1024", "--force"],
        ] {
            let (_, config) = run(&args);
            assert_eq!(config.max_width, Some(1024));
        }
    }

    #[test]
    fn short_options_accept_both_forms() {
        assert_eq!(run(&["in", "out", "-w", "800"]).1.max_width, Some(800));
        assert_eq!(run(&["in", "out", "-w=800"]).1.max_width, Some(800));
    }

    #[test]
    fn every_option_maps_onto_the_config() {
        let (_, config) = run(&[
            "in",
            "out",
            "-w",
            "1000",
            "-H",
            "900",
            "-q",
            "70",
            "-c",
            "420",
            "-j",
            "3",
            "--baseline",
            "--xmp",
            "-f",
            "-k",
        ]);
        assert_eq!(config.max_width, Some(1000));
        assert_eq!(config.max_height, Some(900));
        assert_eq!(config.quality, 70);
        assert_eq!(config.chroma, Chroma::Quarter);
        assert_eq!(config.threads, Some(3));
        assert!(!config.progressive, "--baseline turns progressive off");
        assert!(config.metadata.xmp);
        assert!(config.force);
        assert!(config.keep_going);
    }

    #[test]
    fn chroma_names_map_to_subsampling() {
        assert_eq!(run(&["in", "out", "-c", "444"]).1.chroma, Chroma::None);
        assert_eq!(run(&["in", "out", "-c", "422"]).1.chroma, Chroma::Half);
        assert_eq!(run(&["in", "out", "-c", "420"]).1.chroma, Chroma::Quarter);
    }

    #[test]
    fn strip_metadata_beats_xmp() {
        let (_, config) = run(&["in", "out", "--strip-metadata", "--xmp"]);
        assert!(
            config.metadata.is_empty(),
            "--strip-metadata should win over --xmp"
        );
    }

    #[test]
    fn help_and_version_short_circuit_before_positionals_are_required() {
        assert!(matches!(parse(["--help"]), Ok(Parsed::Help)));
        assert!(matches!(parse(["-h"]), Ok(Parsed::Help)));
        assert!(matches!(parse(["--version"]), Ok(Parsed::Version)));
        assert!(matches!(parse(["-V"]), Ok(Parsed::Version)));
    }

    #[test]
    fn double_dash_allows_paths_that_look_like_flags() {
        let (source, config) = run(&["--", "-weird-dir", "-out"]);
        assert_eq!(source, PathBuf::from("-weird-dir"));
        assert_eq!(config.out_dir, PathBuf::from("-out"));
    }

    #[test]
    fn a_bare_dash_is_treated_as_a_path() {
        let (source, _) = run(&["-", "out"]);
        assert_eq!(source, PathBuf::from("-"));
    }

    #[test]
    fn missing_positionals_are_rejected() {
        assert!(err(&[]).contains("SOURCE"));
        assert!(err(&["only-one"]).contains("SOURCE"));
    }

    #[test]
    fn extra_positionals_are_rejected() {
        let message = err(&["a", "b", "c"]);
        assert!(message.contains("got 3"), "{message}");
    }

    #[test]
    fn an_option_without_its_value_is_rejected() {
        assert!(err(&["in", "out", "--width"]).contains("needs a value"));
        assert!(err(&["in", "out", "-q"]).contains("needs a value"));
    }

    #[test]
    fn non_numeric_values_are_rejected_by_name() {
        let message = err(&["in", "out", "--width", "wide"]);
        assert!(message.contains("--width"), "{message}");
        assert!(message.contains("wide"), "{message}");
    }

    #[test]
    fn quality_is_range_checked() {
        assert!(err(&["in", "out", "-q", "101"]).contains("between 0 and 100"));
        assert_eq!(run(&["in", "out", "-q", "0"]).1.quality, 0);
        assert_eq!(run(&["in", "out", "-q", "100"]).1.quality, 100);
    }

    #[test]
    fn zero_threads_is_rejected() {
        assert!(err(&["in", "out", "-j", "0"]).contains("at least 1"));
    }

    #[test]
    fn unknown_chroma_is_rejected() {
        assert!(err(&["in", "out", "-c", "411"]).contains("444, 422 or 420"));
    }

    #[test]
    fn an_unknown_flag_suggests_the_closest_match() {
        let message = err(&["in", "out", "--wid"]);
        assert!(message.contains("--width"), "{message}");

        let message = err(&["in", "out", "--nonsense"]);
        assert!(message.contains("unknown flag"), "{message}");
    }

    #[test]
    fn short_flags_are_not_bundled() {
        // `-fk` would silently mean something else if bundling were assumed.
        assert!(err(&["in", "out", "-fk"]).contains("unknown flag"));
    }

    #[test]
    fn giving_a_switch_a_value_is_rejected() {
        assert!(err(&["in", "out", "--force=yes"]).contains("takes no value"));
    }

    #[test]
    fn options_may_appear_before_or_between_positionals() {
        let (source, config) = run(&["-f", "in", "-q", "80", "out"]);
        assert_eq!(source, PathBuf::from("in"));
        assert_eq!(config.out_dir, PathBuf::from("out"));
        assert!(config.force);
        assert_eq!(config.quality, 80);
    }

    #[test]
    fn help_text_documents_every_flag_the_parser_accepts() {
        for flag in [
            "--width",
            "--height",
            "--quality",
            "--chroma",
            "--threads",
            "--baseline",
            "--strip-metadata",
            "--xmp",
            "--force",
            "--keep-going",
            "--help",
            "--version",
        ] {
            assert!(HELP.contains(flag), "{flag} is missing from HELP");
        }
    }
}
