# Third-party notices

This project is pure Rust with no C or system dependencies. Every crate that
reaches a released binary is permissively licensed:

| Component | Licence |
| --- | --- |
| `jpeg-encoder` | MIT / Apache-2.0 |
| `zune-jpeg` | MIT / Apache-2.0 / Zlib |
| `fast_image_resize` | MIT / Apache-2.0 |
| `kamadak-exif` | BSD-2-Clause |
| `clap`, `indicatif`, `thiserror` | MIT / Apache-2.0 |

Nothing here imposes obligations beyond reproducing the copyright notices, which
release archives satisfy by shipping this file next to `LICENSE-MIT` and
`LICENSE-APACHE`.

To regenerate a full dependency licence inventory:

```bash
cargo install cargo-about
cargo about generate --format json > licences.json
```

## Historical note

Releases before the switch to `jpeg-encoder` statically linked mozjpeg
(`IJG AND Zlib AND BSD-3-Clause`), which carried acknowledgement requirements
this project no longer has.
