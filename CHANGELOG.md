# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Replaced the mozjpeg encoder with pure-Rust `jpeg-encoder`. The crate now has
  no C dependencies, so building needs only a Rust toolchain — no C compiler and
  no nasm — and cross-compilation works without a native toolchain per target.
- `Config::quality` is now `u8` rather than `f32`, matching the encoder.
- Replaced `clap` with a hand-written argument parser in `src/cli.rs`, removing
  16 transitive crates. The command-line interface is unchanged.
- The default quality moved from 90 to **92**. Encoder quality scales are not
  comparable; 92 was measured to land at the same SSIM as the previous
  mozjpeg q90 on Sony ILCE-7M4 frames, so output quality is unchanged.

### Added

- Static `x86_64` and `aarch64` musl release builds, which run on any Linux
  distribution.

### Notes

- Output is roughly a third larger than the mozjpeg builds at matched quality,
  since `jpeg-encoder` does no trellis quantization. End-to-end throughput
  roughly doubled in exchange.

## [0.1.0] - 2026-08-06

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
- Options for size bounds, quality, chroma, thread count, progressive/baseline,
  metadata policy, and `--keep-going`.
- Library API (`camera_compression`) separate from the CLI, so the pipeline is
  usable and testable without argument parsing.

[Unreleased]: https://github.com/aamirsahmad/camera-compression/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/aamirsahmad/camera-compression/releases/tag/v0.1.0
