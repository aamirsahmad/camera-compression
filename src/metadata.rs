//! Extraction and rewriting of JPEG APP segments.
//!
//! ffmpeg discards all of this — a 7 MB Sony frame carries ~50 KB of EXIF and
//! ~57 KB of XMP that `ffmpeg -i in.jpg out.jpg` replaces with a `Lavc`
//! comment.

/// APP1 identifier introducing a TIFF-structured EXIF block.
const EXIF_ID: &[u8] = b"Exif\0\0";
/// APP1 identifier introducing an XMP packet.
const XMP_ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// APP2 identifier introducing one chunk of an ICC profile.
const ICC_ID: &[u8] = b"ICC_PROFILE\0";

/// EXIF tag number for image orientation.
const TAG_ORIENTATION: u16 = 0x0112;

/// Metadata worth carrying from an input JPEG to its compressed output.
///
/// Segment payloads are stored verbatim, identifier prefix included, so they
/// can be written straight back out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// APP1 payload beginning with `Exif\0\0`.
    pub exif: Option<Vec<u8>>,
    /// APP1 payload beginning with the XMP namespace URI.
    pub xmp: Option<Vec<u8>>,
    /// ICC profile, reassembled across chunks and stripped of chunk headers.
    pub icc: Option<Vec<u8>>,
}

/// Pull the EXIF, XMP, and ICC segments out of a JPEG.
///
/// Walks the marker segments preceding the first scan. Malformed input yields
/// whatever was found before the damage rather than an error, because a
/// slightly odd metadata block is no reason to refuse to compress an image.
///
/// Segment sizes need no validation: a JPEG's segment length is a `u16`, so
/// anything read here is short enough to write back out. Multi-chunk ICC
/// profiles are the exception, and the encoder re-chunks those on write.
#[must_use]
pub fn extract(jpeg: &[u8]) -> Metadata {
    let mut md = Metadata::default();
    let mut icc_chunks: Vec<(u8, &[u8])> = Vec::new();

    for (marker, payload) in Segments::new(jpeg) {
        match marker {
            0xE1 if payload.starts_with(EXIF_ID) => {
                md.exif.get_or_insert_with(|| payload.to_vec());
            }
            0xE1 if payload.starts_with(XMP_ID) => {
                md.xmp.get_or_insert_with(|| payload.to_vec());
            }
            // ICC_PROFILE\0 <sequence:1> <total:1> <chunk>
            0xE2 if payload.starts_with(ICC_ID) && payload.len() > ICC_ID.len() + 2 => {
                icc_chunks.push((payload[ICC_ID.len()], &payload[ICC_ID.len() + 2..]));
            }
            _ => {}
        }
    }

    if !icc_chunks.is_empty() {
        // Chunks are numbered from 1 and need not arrive in order.
        icc_chunks.sort_by_key(|(sequence, _)| *sequence);
        md.icc = Some(
            icc_chunks
                .iter()
                .flat_map(|(_, chunk)| chunk.iter().copied())
                .collect(),
        );
    }
    md
}

/// Iterator over the `FF xx <len:2> <payload>` segments before the first scan.
struct Segments<'a> {
    jpeg: &'a [u8],
    pos: usize,
}

impl<'a> Segments<'a> {
    /// Start after the SOI marker.
    fn new(jpeg: &'a [u8]) -> Self {
        Self { jpeg, pos: 2 }
    }
}

impl<'a> Iterator for Segments<'a> {
    type Item = (u8, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        // SOI (0xD8) and EOI (0xD9) carry no length; SOS (0xDA) begins
        // entropy-coded data, after which nothing is length-prefixed.
        const SOI: u8 = 0xD8;
        const SOS: u8 = 0xDA;

        let i = self.pos;
        if i + 4 > self.jpeg.len() || self.jpeg[i] != 0xFF {
            return None;
        }
        let marker = self.jpeg[i + 1];
        if (SOI..=SOS).contains(&marker) {
            return None;
        }

        let len = u16::from_be_bytes([self.jpeg[i + 2], self.jpeg[i + 3]]) as usize;
        // A length below 2 cannot even cover itself, and one running past the
        // buffer means the file is truncated.
        if len < 2 || i + 2 + len > self.jpeg.len() {
            return None;
        }

        self.pos = i + 2 + len;
        Some((marker, &self.jpeg[i + 4..i + 2 + len]))
    }
}

/// Force the EXIF orientation tag to [`crate::orient::UPRIGHT`].
///
/// The pixels are physically rotated during resize, so leaving the original tag
/// in place would make viewers rotate a second time. Returns whether a tag was
/// found and rewritten.
pub fn normalize_orientation(exif: &mut [u8]) -> bool {
    // Layout: "Exif\0\0" then a TIFF header, with offsets relative to it.
    let Some(tiff) = exif.get(EXIF_ID.len()..) else {
        return false;
    };
    if tiff.len() < 8 {
        return false;
    }

    let little = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return false,
    };
    let read_u16 = |at: usize| {
        let bytes = [tiff[at], tiff[at + 1]];
        if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let read_u32 = |at: usize| {
        let bytes = [tiff[at], tiff[at + 1], tiff[at + 2], tiff[at + 3]];
        if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };

    let ifd0 = read_u32(4) as usize;
    if ifd0 + 2 > tiff.len() {
        return false;
    }
    let entries = read_u16(ifd0) as usize;

    for entry in 0..entries {
        // Each entry is tag(2) type(2) count(4) value(4).
        let at = ifd0 + 2 + entry * 12;
        if at + 12 > tiff.len() {
            return false;
        }
        if read_u16(at) == TAG_ORIENTATION {
            // A SHORT value lives in the first 2 bytes of the value field.
            let value_at = EXIF_ID.len() + at + 8;
            let upright = if little {
                crate::orient::UPRIGHT.to_le_bytes()
            } else {
                crate::orient::UPRIGHT.to_be_bytes()
            };
            exif[value_at] = upright[0];
            exif[value_at + 1] = upright[1];
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap `payload` in a `FF <marker> <len> ...` segment.
    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        out.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Assemble a JPEG-shaped byte string: SOI, the given segments, then a scan.
    fn jpeg(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for s in segments {
            out.extend_from_slice(s);
        }
        out.extend_from_slice(&segment(0xDA, b"\x01\x00"));
        out.extend_from_slice(b"entropy coded data");
        out
    }

    /// A minimal EXIF payload whose IFD0 holds only an orientation tag.
    fn exif_with_orientation(little: bool, orientation: u16) -> Vec<u8> {
        let u16b = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let u32b = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };

        let mut out = EXIF_ID.to_vec();
        out.extend_from_slice(if little { b"II" } else { b"MM" });
        out.extend_from_slice(&u16b(42));
        out.extend_from_slice(&u32b(8)); // IFD0 sits immediately after the header

        out.extend_from_slice(&u16b(1)); // one entry
        out.extend_from_slice(&u16b(TAG_ORIENTATION));
        out.extend_from_slice(&u16b(3)); // type SHORT
        out.extend_from_slice(&u32b(1)); // count
        out.extend_from_slice(&u16b(orientation));
        out.extend_from_slice(&u16b(0)); // pad the 4-byte value field
        out.extend_from_slice(&u32b(0)); // no IFD1
        out
    }

    #[test]
    fn extract_finds_exif_xmp_and_icc() {
        let exif = exif_with_orientation(true, 8);
        let xmp = [XMP_ID, b"<x:xmpmeta/>"].concat();
        let icc = [ICC_ID, &[1, 1][..], b"profile bytes"].concat();

        let md = extract(&jpeg(&[
            segment(0xE1, &exif),
            segment(0xE1, &xmp),
            segment(0xE2, &icc),
        ]));

        assert_eq!(md.exif.as_deref(), Some(exif.as_slice()));
        assert_eq!(md.xmp.as_deref(), Some(xmp.as_slice()));
        assert_eq!(md.icc.as_deref(), Some(&b"profile bytes"[..]));
    }

    #[test]
    fn extract_returns_nothing_for_a_jpeg_without_metadata() {
        assert_eq!(extract(&jpeg(&[])), Metadata::default());
    }

    #[test]
    fn extract_reassembles_icc_chunks_in_sequence_order() {
        // Deliberately out of order on the wire.
        let second = [ICC_ID, &[2, 2][..], b"second"].concat();
        let first = [ICC_ID, &[1, 2][..], b"first-"].concat();

        let md = extract(&jpeg(&[segment(0xE2, &second), segment(0xE2, &first)]));
        assert_eq!(md.icc.as_deref(), Some(&b"first-second"[..]));
    }

    #[test]
    fn extract_ignores_app_segments_with_unknown_identifiers() {
        let mystery = segment(0xE1, b"SomethingElse\0payload");
        let mpf = segment(0xE2, b"MPF\0data");
        assert_eq!(extract(&jpeg(&[mystery, mpf])), Metadata::default());
    }

    #[test]
    fn extract_stops_at_the_scan_and_ignores_later_bytes() {
        // An EXIF-looking segment after SOS is scan data, not metadata.
        let mut data = jpeg(&[]);
        data.extend_from_slice(&segment(0xE1, &exif_with_orientation(true, 3)));
        assert!(extract(&data).exif.is_none());
    }

    #[test]
    fn extract_keeps_the_first_of_a_duplicated_segment() {
        let a = [XMP_ID, b"first"].concat();
        let b = [XMP_ID, b"second"].concat();
        let md = extract(&jpeg(&[segment(0xE1, &a), segment(0xE1, &b)]));
        assert_eq!(md.xmp.as_deref(), Some(a.as_slice()));
    }

    #[test]
    fn extract_survives_truncated_and_malformed_input() {
        // Each of these must return rather than panic or read out of bounds.
        let exif = exif_with_orientation(true, 1);
        let full = jpeg(&[segment(0xE1, &exif)]);
        for cut in 0..full.len() {
            let _ = extract(&full[..cut]);
        }
        let _ = extract(&[]);
        let _ = extract(&[0xFF]);
        let _ = extract(&[0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF]); // length past the end
        let _ = extract(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x00]); // length below 2
        let _ = extract(&[0xFF, 0xD8, 0x00, 0x00, 0x00, 0x00]); // not a marker
    }

    #[test]
    fn normalize_orientation_rewrites_little_endian_exif() {
        let mut exif = exif_with_orientation(true, 8);
        assert!(normalize_orientation(&mut exif));
        assert_eq!(orient_of(&exif), crate::orient::UPRIGHT);
    }

    #[test]
    fn normalize_orientation_rewrites_big_endian_exif() {
        let mut exif = exif_with_orientation(false, 6);
        assert!(normalize_orientation(&mut exif));
        assert_eq!(orient_of(&exif), crate::orient::UPRIGHT);
    }

    #[test]
    fn normalize_orientation_is_idempotent() {
        let mut exif = exif_with_orientation(true, 8);
        assert!(normalize_orientation(&mut exif));
        let once = exif.clone();
        assert!(normalize_orientation(&mut exif));
        assert_eq!(exif, once);
    }

    #[test]
    fn normalize_orientation_reports_false_when_the_tag_is_absent() {
        // An IFD0 with zero entries.
        let mut exif = EXIF_ID.to_vec();
        exif.extend_from_slice(b"II");
        exif.extend_from_slice(&42u16.to_le_bytes());
        exif.extend_from_slice(&8u32.to_le_bytes());
        exif.extend_from_slice(&0u16.to_le_bytes());
        exif.extend_from_slice(&0u32.to_le_bytes());
        assert!(!normalize_orientation(&mut exif));
    }

    #[test]
    fn normalize_orientation_rejects_malformed_exif_without_panicking() {
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            EXIF_ID.to_vec(),
            [EXIF_ID, b"XX", &42u16.to_le_bytes()[..]].concat(), // bad byte order
            [EXIF_ID, b"II", &42u16.to_le_bytes()[..]].concat(), // header too short
            // IFD0 offset points past the end
            [
                EXIF_ID,
                b"II",
                &42u16.to_le_bytes()[..],
                &9999u32.to_le_bytes()[..],
            ]
            .concat(),
        ];
        for mut case in cases {
            assert!(!normalize_orientation(&mut case));
        }
    }

    #[test]
    fn normalize_orientation_truncated_at_every_length_never_panics() {
        let full = exif_with_orientation(false, 7);
        for cut in 0..full.len() {
            let mut partial = full[..cut].to_vec();
            let _ = normalize_orientation(&mut partial);
        }
    }

    /// Read the orientation back out of a payload built by the test helper.
    fn orient_of(exif: &[u8]) -> u16 {
        let tiff = &exif[EXIF_ID.len()..];
        let little = &tiff[0..2] == b"II";
        // header(8) + entry count(2) + tag(2) + type(2) + count(4) = 18
        let bytes = [tiff[18], tiff[19]];
        if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }
}
