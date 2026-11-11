//! The snippet from the README, kept compiling by `cargo test --all-targets`.

use camera_compression::{batch, source, Chroma, Config, Silent};
use std::path::Path;

fn main() -> Result<(), camera_compression::Error> {
    let files = source::collect(Path::new("photos"))?;
    let config = Config {
        chroma: Chroma::Quarter, // 4:2:0 for web
        quality: 82,
        ..Config::new("compressed")
    };
    let stats = batch::run(&files, &config, &Silent)?;
    println!("{} written, {} skipped", stats.written, stats.skipped);
    Ok(())
}
