# Contributing

## Getting set up

A Rust toolchain is all you need. The crate is pure Rust — no C compiler, no
system libraries, no build-time assembler.

```bash
cargo build --release
```

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets   # must be clean; lints are configured in Cargo.toml
cargo test
```

CI runs the same three on Linux, macOS, and Windows, plus a `cargo check` against
the MSRV in `Cargo.toml` and a `cargo doc` pass with warnings denied.

Keeping the crate free of C dependencies is deliberate: it is what makes the
release matrix (including static musl builds) work without cross-toolchains.
Weigh that before adding a dependency that links native code.

## Layout

| file | role |
| --- | --- |
| `src/lib.rs` | crate docs and re-exports |
| `src/config.rs` | `Config`, `Chroma`, `MetadataPolicy` — no CLI dependency |
| `src/error.rs` | the crate's error type |
| `src/source.rs` | input discovery: directory walk, file lists, collisions |
| `src/pipeline.rs` | one image: decode, resize, rotate, encode, write |
| `src/batch.rs` | parallel driver and the `Progress` trait |
| `src/metadata.rs` | JPEG APP segment extraction and orientation rewriting |
| `src/orient.rs` | EXIF orientation reading and pixel transforms |
| `src/cli.rs` | hand-rolled argument parsing (binary only) |
| `src/main.rs` | wiring and terminal output, nothing else |

Two conventions worth keeping:

- **The library never prints.** Progress and failures go through the `Progress`
  trait so the CLI owns all terminal output.
- **`Config` knows nothing about clap.** `main.rs` converts parsed arguments into
  it, which keeps the pipeline drivable from tests and other programs.

## Testing

Unit tests live beside the code they cover and stick to pure logic — byte
fixtures are built inline, so there is nothing binary to review. Tests that need
real JPEGs live in `tests/`, where `tests/common/mod.rs` synthesizes them at
runtime rather than checking images into the repository.

When you fix a bug, add the test that would have caught it.
