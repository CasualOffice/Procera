//! JPEG metadata stripping (FR-F02 "image EXIF stripping", `files.exif_strip`). No codec and no
//! dependency: the image data is copied byte for byte, only metadata segments are dropped.
//!
//! Dropped: APP1 (Exif, which carries GPS, device and timestamps; XMP), APP13 (Photoshop/IPTC) and
//! COM (comments). Everything else (JFIF APP0, ICC APP2, Adobe APP14, tables, frames, scan data) is
//! kept. A consequence is that the EXIF orientation tag goes too, so a phone photo taken sideways
//! previews unrotated; that is the documented price of removing location data without re-encoding.

const SOI: u8 = 0xD8;
const SOS: u8 = 0xDA;
const EOI: u8 = 0xD9;
const APP1: u8 = 0xE1;
const APP13: u8 = 0xED;
const COM: u8 = 0xFE;

/// `jpeg` without its metadata segments, or `None` when it is not a well-formed JPEG header (the
/// caller then serves nothing rather than the unstripped bytes).
pub fn strip_jpeg(jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != SOI {
        return None;
    }
    let mut out = Vec::with_capacity(jpeg.len());
    out.extend_from_slice(&jpeg[..2]);
    let mut i = 2;
    loop {
        // Fill bytes (0xFF padding) may precede a marker.
        while jpeg.get(i) == Some(&0xFF) && jpeg.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if jpeg.get(i) != Some(&0xFF) {
            return None;
        }
        let marker = *jpeg.get(i + 1)?;
        match marker {
            EOI => {
                out.extend_from_slice(&jpeg[i..i + 2]);
                return Some(out);
            }
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => {
                out.extend_from_slice(&jpeg[i..i + 2]);
                i += 2;
                continue;
            }
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*jpeg.get(i + 2)?, *jpeg.get(i + 3)?]));
        if len < 2 {
            return None;
        }
        let end = i.checked_add(2 + len)?;
        if end > jpeg.len() {
            return None;
        }
        if marker == SOS {
            // From the start of scan on it is entropy-coded data (and any later segments): copy as is.
            out.extend_from_slice(&jpeg[i..]);
            return Some(out);
        }
        if !matches!(marker, APP1 | APP13 | COM) {
            out.extend_from_slice(&jpeg[i..end]);
        }
        i = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(&u16::try_from(body.len() + 2).unwrap_or(0).to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn drops_exif_xmp_iptc_and_comments_but_keeps_image_data() {
        let app0 = seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        let exif = seg(APP1, b"Exif\0\0GPS 51.5N 0.12W");
        let xmp = seg(APP1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>");
        let icc = seg(0xE2, b"ICC_PROFILE\0..");
        let iptc = seg(APP13, b"Photoshop 3.0\0..");
        let com = seg(COM, b"shot on a phone");
        let dqt = seg(0xDB, &[0; 65]);
        let sos = seg(SOS, &[1, 1, 0, 0, 0x3F, 0]);
        let scan = [0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56];
        let mut jpeg = vec![0xFF, SOI];
        for s in [&app0, &exif, &xmp, &icc, &iptc, &com, &dqt, &sos] {
            jpeg.extend_from_slice(s);
        }
        jpeg.extend_from_slice(&scan);
        jpeg.extend_from_slice(&[0xFF, EOI]);

        let out = strip_jpeg(&jpeg).unwrap_or_default();
        let mut want = vec![0xFF, SOI];
        for s in [&app0, &icc, &dqt, &sos] {
            want.extend_from_slice(s);
        }
        want.extend_from_slice(&scan);
        want.extend_from_slice(&[0xFF, EOI]);
        assert_eq!(out, want);
        assert!(!out.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn refuses_malformed_input() {
        assert_eq!(strip_jpeg(b"%PDF-1.7"), None);
        assert_eq!(strip_jpeg(&[0xFF, SOI, 0xFF, APP1, 0x00, 0x40, 1, 2]), None); // truncated segment
        assert_eq!(strip_jpeg(&[0xFF, SOI, 0x00, 0x00]), None);
        assert_eq!(strip_jpeg(&[0xFF, SOI, 0xFF, 0xDB, 0x00, 0x01]), None); // length < 2
    }
}
