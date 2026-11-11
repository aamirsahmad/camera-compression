# camera-compression

Batch downscale and recompress camera JPEGs, keeping the EXIF that `ffmpeg`
throws away.

Built for the boring job of turning a card full of 33 MP frames into something
you can actually share — without losing the lens and exposure data, and without
silently doubling the size of every portrait shot.

## Install

Prebuilt binaries for Linux, macOS, and Windows are attached to each
[release](https://github.com/aamirsahmad/camera-compression/releases). The musl
Linux builds are fully static and run on any distribution.

Building from source needs nothing but a Rust toolchain — the crate is pure
Rust, with no C dependencies and no system libraries:

```bash
cargo build --release
# or
cargo install camera-compression
```

## Usage

```bash
camera-compression <source> <destination>
```

`source` is either a directory, walked recursively for jpgs, or a file holding a
newline-delimited list of paths — auto-detected. `destination` is created if
absent, and output is flat.

```bash
# walk a month of shoots
camera-compression ~/Pictures/Camera/2026/07 ~/Pictures/compressed

# or drive it from an explicit list
camera-compression paths.txt ~/Pictures/compressed
```

Re-running skips inputs whose output already exists, so an interrupted batch
resumes where it stopped.

Directory walking matches `find <dir> -type f -iname '*.jpg'`: case-insensitive
`.jpg` (so **not** `.jpeg`), and symlinks are not followed. Because output is
flat, identical filenames in different subdirectories collide — the tool warns
and names them rather than dropping one silently.

## Why not just use ffmpeg

Benchmarked against an equivalent `ffmpeg` + GNU `parallel` pipeline on 187 Sony ILCE-7M4 frames
(7008×4672, 4:2:2), warm cache, 16 cores.

### It resamples in RGB, not subsampled YUV

`-vf scale=...:flags=lanczos` resamples in YUV 4:2:2, so chroma is resampled
from half-width data. Resize only, no encode, against an independent 16-bit
Lanczos reference:

| resize | SSIM | PSNR |
| --- | --- | --- |
| ffmpeg `scale=2048:-1:flags=lanczos` | 0.989446 | 47.73 dB |
| ffmpeg forced into RGB16 | 0.997497 | 55.91 dB |
| `fast_image_resize` Lanczos3 (this tool) | **0.998352** | **56.86 dB** |

Forcing ffmpeg through `format=gbrp16le` recovers ~8 dB, which confirms the
loss is swscale's YUV path rather than an artifact of the reference.

### It keeps your metadata

ffmpeg replaces ~50 KB of EXIF and ~57 KB of XMP with a `Lavc` comment. Camera,
lens, date, exposure, and GPS are all lost:

| | Make | Model | Lens | Date |
| --- | --- | --- | --- | --- |
| source | SONY | ILCE-7M4 | E 28-75mm F2.8 A063 | 2026:05:27 17:35:52 |
| ffmpeg | — | — | — | — |
| this tool | SONY | ILCE-7M4 | E 28-75mm F2.8 A063 | 2026:05:27 17:35:52 |

Because the pixels are physically rotated during resize, the preserved EXIF
`Orientation` tag is rewritten to `1` so viewers don't rotate a second time.

### It doesn't silently bloat portrait shots

ffmpeg autorotates from EXIF, and rotating 4:2:2 makes the subsampling axis
invalid — so swscale quietly promotes those frames to 4:4:4:

Of 187 frames, ffmpeg promoted 16 of the portrait ones to 4:4:4. This tool
emits 4:2:2 for all 187.

### What it costs

| 187 files | ffmpeg | this tool |
| --- | --- | --- |
| wall clock | 8.47 s | **4.78 s** |
| total output | **214,728 KB** | 246,180 KB (237,012 KB stripped) |

Faster, but the files are about 15% larger. That is the deliberate trade
described below.

### Encoder choice

The encoder is [`jpeg-encoder`](https://crates.io/crates/jpeg-encoder), picked
so the crate stays pure Rust — no C toolchain, no nasm, no system libraries, and
cross-compilation that just works.

It costs bitrate. Measured against mozjpeg on these same frames, at matched SSIM,
mozjpeg produced files roughly a third smaller — it does trellis quantization
(rate-distortion optimal coefficient search) and ships perceptually tuned
quantization tables, neither of which `jpeg-encoder` has.

If output size matters more to you than build simplicity, mozjpeg is the better
encoder and this is the wrong default.

## Options

```
<SOURCE>                 Directory to walk recursively, or a file listing paths
<DESTINATION>            Output directory, created if absent
-w, --width <PX>         Max width of the upright image [default: 2048]
-H, --height <PX>        Max height; without it only width constrains the result
-q, --quality <Q>        JPEG quality 0-100 [default: 92]
-c, --chroma <444|422|420>  Output subsampling [default: 422]
-j, --threads <N>        Worker threads [default: core count]
    --baseline           Baseline instead of progressive JPEG
    --strip-metadata     Drop EXIF/XMP/ICC (saved ~9 MB across 187 files)
    --xmp                Also carry XMP over (off by default; ~57 KB of padding)
-f, --force              Re-encode even if the output exists
-k, --keep-going         Finish the batch instead of halting on first failure
```

Notes:

- Options accept `--name value` or `--name=value`, and `--` ends option parsing
  so a path may begin with `-`. Short flags are not bundled: write `-f -k`,
  not `-fk`.
- `--width`/`--height` are an upper bound and never upscale.
- The default `422` matches what the camera recorded. Use `420` for web
  delivery — it is meaningfully smaller and the chroma loss is invisible at
  normal viewing sizes.

## Design notes

**No rayon, no tokio.** The work is CPU-bound, so tokio would only add
`spawn_blocking` ceremony around what is really a thread pool. `std::thread::scope`
plus an `AtomicUsize` index is a dozen lines, borrows the file list without an
`Arc`, and gives dynamic load balancing — per-file cost varies with content, so
a static split would leave threads idle.

**Crash-safe writes.** Output goes to `<name>.jpg.part` and is renamed into
place, so an interrupted run never leaves a truncated file that the
skip-if-exists check would later trust.

## Use as a library

The CLI is a thin shell over `camera_compression`, so the pipeline is usable
directly. Progress goes through a trait, and the library never prints.

```rust
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
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the module layout and development
workflow.

## Cheaper alternative

If you would rather not add a binary, most of the quality gap closes inside a
shell script:

```bash
ffmpeg -y -loglevel error -i "$in" \
  -vf "format=gbrp16le,scale=2048:-1:flags=lanczos+accurate_rnd,format=yuvj422p" \
  -q:v 2 "$out"
exiftool -tagsFromFile "$in" -all:all -overwrite_original "$out"
```

That recovers the resample precision and pins 4:2:2. It does not give you the
resume-safe writes, the collision warning, or a single-process pipeline.

## Licence

Dual-licensed under either [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.

Every dependency is permissively licensed and there are no C or system
components — see [THIRD-PARTY.md](THIRD-PARTY.md).
