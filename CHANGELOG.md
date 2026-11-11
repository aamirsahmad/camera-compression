# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-10

### Changed

- README and package manifest no longer reference the retired shell script or
  its file list. No code changes.

## [0.1.0] - 2026-11-10

First release. Replaces a `parallel` + `ffmpeg` shell pipeline.

### Added

- Batch downscale and recompress, `<source> <destination>`, where the source is
  either a directory walked recursively for jpgs or a newline-delimited list.
- Lanczos3 resampling in full RGB, worth roughly 8 dB PSNR over resampling in
  ffmpeg's subsampled YUV 4:2:2.
- EXIF, XMP, and ICC passthrough, with the orientation tag rewritten to upright
  because the pixels are physically rotated.
- Pinned chroma subsampling, so autorotated portrait frames are not silently
  promoted to 4:4:4 and doubled in size.
- Resume: inputs whose output already exists are skipped unless `--force`.
- Crash-safe output via write-to-`.part`-then-rename.
- Warning when inputs from different subdirectories would collide in the flat
  output directory.
- Options for size bounds, quality (default 92), chroma, thread count,
  progressive/baseline, metadata policy, and `--keep-going`.
- Library API (`camera_compression`) separate from the CLI, so the pipeline is
  usable and testable without argument parsing.
- Static `x86_64` and `aarch64` musl release builds, which run on any Linux
  distribution.

### Notes

- Encoding uses pure-Rust `jpeg-encoder`, so the crate has no C dependencies:
  building needs only a Rust toolchain — no C compiler and no nasm — and
  cross-compilation works without a native toolchain per target.
- Argument parsing is hand-written in `src/cli.rs` rather than using `clap`,
  keeping the dependency tree small.
- Output is roughly a third larger than mozjpeg at matched quality, since
  `jpeg-encoder` does no trellis quantization; end-to-end throughput is roughly
  double. The default quality of 92 was measured to match mozjpeg q90 in SSIM
  on Sony ILCE-7M4 frames.

[Unreleased]: https://github.com/aamirsahmad/camera-compression/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/aamirsahmad/camera-compression/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/aamirsahmad/camera-compression/releases/tag/v0.1.0
