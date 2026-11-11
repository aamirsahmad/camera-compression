//! EXIF orientation handling.
//!
//! `ffmpeg -i` autorotates by default. A port that ignores the orientation tag
//! writes every portrait frame out sideways, so this is not optional.

/// Orientation value meaning "already upright".
pub const UPRIGHT: u16 = 1;

/// Read the EXIF orientation tag, defaulting to [`UPRIGHT`].
///
/// Values outside the valid 1-8 range are treated as upright rather than
/// trusted, since a bogus tag should not rotate anyone's photos.
#[must_use]
pub fn read(jpeg: &[u8]) -> u16 {
    let mut cursor = std::io::Cursor::new(jpeg);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|e| {
            e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        })
        .filter(|v| (1..=8).contains(v))
        .map_or(UPRIGHT, |v| v as u16)
}

/// True for the four orientations that swap width and height.
#[must_use]
pub fn transposes(orientation: u16) -> bool {
    (5..=8).contains(&orientation)
}

/// Rotate or flip an RGB8 buffer to upright, returning it with its new size.
///
/// Called after downscaling, so it moves a few MB rather than the ~98 MB of a
/// full 33 MP frame.
///
/// # Panics
///
/// Panics if `buf` is shorter than `w * h * 3`.
#[must_use]
pub fn apply(buf: &[u8], w: u32, h: u32, orientation: u16) -> (Vec<u8>, u32, u32) {
    assert!(
        buf.len() >= w as usize * h as usize * 3,
        "buffer is smaller than the stated {w}x{h} RGB image"
    );
    if orientation <= UPRIGHT || orientation > 8 {
        return (buf.to_vec(), w, h);
    }

    let (src_w, src_h) = (w as usize, h as usize);
    let (out_w, out_h) = if transposes(orientation) {
        (src_h, src_w)
    } else {
        (src_w, src_h)
    };

    let mut out = vec![0u8; src_w * src_h * 3];
    for y in 0..out_h {
        for x in 0..out_w {
            let (sx, sy) = source_pixel(x, y, src_w, src_h, orientation);
            let from = (sy * src_w + sx) * 3;
            let to = (y * out_w + x) * 3;
            out[to..to + 3].copy_from_slice(&buf[from..from + 3]);
        }
    }
    (out, out_w as u32, out_h as u32)
}

/// Map a destination pixel back to its source pixel for the given orientation.
///
/// Values follow the EXIF specification, where 2/4/5/7 include a mirror.
fn source_pixel(x: usize, y: usize, w: usize, h: usize, orientation: u16) -> (usize, usize) {
    match orientation {
        2 => (w - 1 - x, y),         // mirror horizontal
        3 => (w - 1 - x, h - 1 - y), // rotate 180
        4 => (x, h - 1 - y),         // mirror vertical
        5 => (y, x),                 // transpose
        6 => (y, h - 1 - x),         // rotate 90 CW
        7 => (w - 1 - y, h - 1 - x), // transverse
        8 => (w - 1 - y, x),         // rotate 270 CW
        _ => (x, y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x3 image (w=2, h=3) whose pixels are distinguishable by red channel:
    ///
    /// ```text
    ///   1 2
    ///   3 4
    ///   5 6
    /// ```
    const W: u32 = 2;
    const H: u32 = 3;

    fn sample() -> Vec<u8> {
        (1..=6).flat_map(|v| [v, 0, 0]).collect()
    }

    /// Reduce a buffer to its red channel, for legible assertions.
    fn reds(buf: &[u8]) -> Vec<u8> {
        buf.as_chunks::<3>().0.iter().map(|p| p[0]).collect()
    }

    #[test]
    fn upright_is_returned_unchanged() {
        let (out, w, h) = apply(&sample(), W, H, UPRIGHT);
        assert_eq!((w, h), (W, H));
        assert_eq!(reds(&out), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn orientation_zero_and_out_of_range_are_treated_as_upright() {
        for bogus in [0, 9, 255] {
            let (out, w, h) = apply(&sample(), W, H, bogus);
            assert_eq!((w, h), (W, H), "orientation {bogus} should not transpose");
            assert_eq!(reds(&out), vec![1, 2, 3, 4, 5, 6]);
        }
    }

    #[test]
    fn mirror_horizontal_swaps_columns() {
        let (out, w, h) = apply(&sample(), W, H, 2);
        assert_eq!((w, h), (W, H));
        assert_eq!(reds(&out), vec![2, 1, 4, 3, 6, 5]);
    }

    #[test]
    fn rotate_180_reverses_every_pixel() {
        let (out, w, h) = apply(&sample(), W, H, 3);
        assert_eq!((w, h), (W, H));
        assert_eq!(reds(&out), vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn mirror_vertical_swaps_rows() {
        let (out, w, h) = apply(&sample(), W, H, 4);
        assert_eq!((w, h), (W, H));
        assert_eq!(reds(&out), vec![5, 6, 3, 4, 1, 2]);
    }

    #[test]
    fn transpose_reflects_across_the_main_diagonal() {
        let (out, w, h) = apply(&sample(), W, H, 5);
        assert_eq!((w, h), (H, W), "transposing orientations swap the axes");
        assert_eq!(reds(&out), vec![1, 3, 5, 2, 4, 6]);
    }

    #[test]
    fn rotate_90_cw_moves_the_first_column_to_the_top_row() {
        let (out, w, h) = apply(&sample(), W, H, 6);
        assert_eq!((w, h), (H, W));
        assert_eq!(reds(&out), vec![5, 3, 1, 6, 4, 2]);
    }

    #[test]
    fn transverse_reflects_across_the_anti_diagonal() {
        let (out, w, h) = apply(&sample(), W, H, 7);
        assert_eq!((w, h), (H, W));
        assert_eq!(reds(&out), vec![6, 4, 2, 5, 3, 1]);
    }

    #[test]
    fn rotate_270_cw_moves_the_last_column_to_the_top_row() {
        let (out, w, h) = apply(&sample(), W, H, 8);
        assert_eq!((w, h), (H, W));
        assert_eq!(reds(&out), vec![2, 4, 6, 1, 3, 5]);
    }

    #[test]
    fn every_orientation_preserves_the_full_set_of_pixels() {
        for orientation in 1..=8 {
            let (out, w, h) = apply(&sample(), W, H, orientation);
            assert_eq!(
                w * h,
                W * H,
                "orientation {orientation} changed pixel count"
            );
            let mut got = reds(&out);
            got.sort_unstable();
            assert_eq!(
                got,
                vec![1, 2, 3, 4, 5, 6],
                "orientation {orientation} lost or duplicated pixels"
            );
        }
    }

    #[test]
    fn transposes_matches_the_exif_spec() {
        for o in [1, 2, 3, 4] {
            assert!(!transposes(o), "{o} should not transpose");
        }
        for o in [5, 6, 7, 8] {
            assert!(transposes(o), "{o} should transpose");
        }
    }

    #[test]
    #[should_panic(expected = "smaller than the stated")]
    fn apply_rejects_a_buffer_that_is_too_small() {
        let _ = apply(&[0, 0, 0], 4, 4, 3);
    }

    #[test]
    fn read_defaults_to_upright_when_there_is_no_exif() {
        assert_eq!(read(b"not a jpeg at all"), UPRIGHT);
        assert_eq!(read(&[]), UPRIGHT);
    }
}
