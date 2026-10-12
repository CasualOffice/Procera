//! Content type from magic bytes, not from the name or the declared type (FR-F02). The platform deny
//! list (executables, scripts, HTML) always applies (docs/20 §2.10 `files.types.policy`).

/// The detected type of `head` (the first bytes of a file).
pub fn sniff(head: &[u8]) -> &'static str {
    let starts = |sig: &[u8]| head.starts_with(sig);
    if starts(b"%PDF-") {
        "application/pdf"
    } else if starts(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if starts(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        "image/webp"
    } else if starts(b"PK\x03\x04") {
        // Office Open XML documents are zip files; tell them apart by their first part name.
        let s = String::from_utf8_lossy(&head[..head.len().min(2048)]);
        if s.contains("word/") {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        } else if s.contains("xl/") {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        } else if s.contains("ppt/") {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        } else {
            "application/zip"
        }
    } else if starts(b"MZ") {
        "application/x-msdownload"
    } else if starts(&[0x7F, b'E', b'L', b'F']) {
        "application/x-executable"
    } else if starts(&[0xCF, 0xFA, 0xED, 0xFE])
        || starts(&[0xCE, 0xFA, 0xED, 0xFE])
        || starts(&[0xCA, 0xFE, 0xBA, 0xBE])
    {
        "application/x-mach-binary"
    } else if starts(b"#!") {
        "text/x-script"
    } else if is_text(head) {
        let lower = String::from_utf8_lossy(&head[..head.len().min(512)]).to_ascii_lowercase();
        let t = lower.trim_start();
        if t.starts_with("<!doctype html") || t.starts_with("<html") || t.contains("<script") {
            "text/html"
        } else if t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg")) {
            "image/svg+xml"
        } else {
            "text/plain"
        }
    } else {
        "application/octet-stream"
    }
}

pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// The Office Open XML type of a whole file, from its zip central directory rather than a peek at the
/// first entry: a Word document has a `word/document.xml` part, a workbook an `xl/workbook.xml` part.
/// Any other zip (or a broken one) is `None`, whatever it is named.
pub fn ooxml(data: &[u8]) -> Option<&'static str> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(data)).ok()?;
    if zip.index_for_name("word/document.xml").is_some() {
        Some(DOCX)
    } else if zip.index_for_name("xl/workbook.xml").is_some() {
        Some(XLSX)
    } else {
        None
    }
}

fn is_text(head: &[u8]) -> bool {
    !head.is_empty()
        && std::str::from_utf8(&head[..head.len().min(1024).min(head.len())]).is_ok()
        && !head.iter().take(1024).any(|b| *b == 0)
}

/// Types refused everywhere: executables, scripts, and active web content (HTML, SVG can run script).
pub fn denied(sniffed: &str) -> bool {
    matches!(
        sniffed,
        "application/x-msdownload"
            | "application/x-executable"
            | "application/x-mach-binary"
            | "text/x-script"
            | "text/html"
            | "image/svg+xml"
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A stored (uncompressed) zip with these parts.
    pub(crate) fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in parts {
            w.start_file(*name, opts).expect("part");
            w.write_all(body.as_bytes()).expect("write");
        }
        w.finish().expect("zip").into_inner()
    }

    #[test]
    fn office_types_come_from_the_central_directory() {
        let docx = zip_of(&[("[Content_Types].xml", "<Types/>"), ("word/document.xml", "<w:document/>")]);
        assert_eq!(sniff(&docx), DOCX);
        assert_eq!(ooxml(&docx), Some(DOCX));
        let xlsx = zip_of(&[("[Content_Types].xml", "<Types/>"), ("xl/workbook.xml", "<workbook/>")]);
        assert_eq!(ooxml(&xlsx), Some(XLSX));
        // A plain zip, and a zip that merely mentions `word/` without the main part, are not documents.
        assert_eq!(ooxml(&zip_of(&[("notes.txt", "hi")])), None);
        let fake = zip_of(&[("word/readme.txt", "not a document")]);
        assert_eq!(sniff(&fake), DOCX, "the head sniff alone would accept it");
        assert_eq!(ooxml(&fake), None);
        assert_eq!(ooxml(b"PK\x03\x04 truncated"), None);
        assert_eq!(ooxml(b"%PDF-1.7"), None);
    }

    #[test]
    fn sniffs_by_content_and_denies_active_types() {
        assert_eq!(sniff(b"%PDF-1.7 ..."), "application/pdf");
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]), "image/png");
        assert_eq!(sniff(b"MZ\x90\x00"), "application/x-msdownload");
        assert_eq!(sniff(b"#!/bin/sh\nrm -rf /"), "text/x-script");
        assert_eq!(sniff(b"  <!DOCTYPE html><p>x"), "text/html");
        assert_eq!(sniff(b"Hello, policy"), "text/plain");
        assert_eq!(
            sniff(b"PK\x03\x04....word/document.xml"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert!(
            denied("application/x-msdownload")
                && denied("text/html")
                && !denied("application/pdf")
                && !denied("text/plain")
        );
    }
}
