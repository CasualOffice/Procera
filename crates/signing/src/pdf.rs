//! The PDF side of signing: checking an uploaded document, then stamping the signers' values onto
//! it and appending a certificate of completion (FR-DS05, ESIG-01). Pure functions over bytes.
//!
//! Stamps use the standard PDF fonts (Helvetica; Times-Italic for typed signatures) with WinAnsi
//! encoding, so nothing is embedded; characters outside it print as `?`. A drawn signature is a PNG,
//! placed as an image with its transparency. The original page content is wrapped in `q … Q` so
//! whatever graphics state it leaves can't move the stamps, and a page that inherits its resources
//! gets its own copy before fonts are added. Encrypted documents and rotated pages are refused.

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// Largest page count accepted.
pub const MAX_PAGES: usize = 500;
/// Largest drawn signature, in pixels per side.
const MAX_SIGNATURE_PX: u32 = 2000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PdfError {
    #[error("{0}")]
    Unsupported(String),
    #[error("the document couldn't be read as a PDF")]
    Unreadable,
    #[error("couldn't write the signed PDF: {0}")]
    Write(String),
}

/// A field's box as fractions of its page, from the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// What to draw in one field.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// A typed signature (in a script-like face) or initials.
    Script(String),
    /// A drawn signature or initials (PNG bytes).
    Image(Vec<u8>),
    /// Plain text: a name, a date, an answer.
    Text(String),
    /// A ticked checkbox (nothing is drawn for an unticked one).
    Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// 1-based.
    pub page: u32,
    pub rect: Rect,
    pub mark: Mark,
}

/// The certificate of completion: plain lines under headings, laid out on as many pages as needed.
#[derive(Debug, Clone, Default)]
pub struct Certificate {
    pub title: String,
    pub sections: Vec<(String, Vec<String>)>,
}

/// The page sizes (width, height in points) of a PDF that can be signed.
pub fn inspect(bytes: &[u8]) -> Result<Vec<(f64, f64)>, PdfError> {
    let doc = Document::load_mem(bytes).map_err(|_| PdfError::Unreadable)?;
    if doc.is_encrypted() {
        return Err(PdfError::Unsupported(
            "This PDF is password-protected. Remove the password and upload it again.".into(),
        ));
    }
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(PdfError::Unsupported("This PDF has no pages.".into()));
    }
    if pages.len() > MAX_PAGES {
        return Err(PdfError::Unsupported(format!("A document can have at most {MAX_PAGES} pages.")));
    }
    let mut sizes = Vec::with_capacity(pages.len());
    for (n, id) in &pages {
        if inherited(&doc, *id, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0).rem_euclid(360) != 0 {
            return Err(PdfError::Unsupported(format!(
                "Page {n} is rotated. Rotated pages can't be signed yet: save the PDF with its pages upright and upload it again."
            )));
        }
        sizes.push(media_size(&doc, *id)?);
    }
    Ok(sizes)
}

/// The signed document: `bytes` with every stamp drawn and the certificate appended.
pub fn stamp(bytes: &[u8], stamps: &[Stamp], certificate: &Certificate) -> Result<Vec<u8>, PdfError> {
    let mut doc = Document::load_mem(bytes).map_err(|_| PdfError::Unreadable)?;
    let pages = doc.get_pages();
    let mut by_page: std::collections::BTreeMap<u32, Vec<&Stamp>> = Default::default();
    for s in stamps {
        by_page.entry(s.page).or_default().push(s);
    }
    let fonts = font_objects(&mut doc);
    for (page_no, list) in by_page {
        let page_id =
            *pages.get(&page_no).ok_or_else(|| PdfError::Write(format!("page {page_no} doesn't exist")))?;
        let (w, h) = media_size(&doc, page_id)?;
        own_resources(&mut doc, page_id)?;
        add_fonts(&mut doc, page_id, &fonts)?;
        let mut ops = Vec::new();
        for (i, s) in list.iter().enumerate() {
            let bx = s.rect.x * w;
            let bw = s.rect.w * w;
            let bh = s.rect.h * h;
            let by = h - (s.rect.y + s.rect.h) * h; // PDF space: origin bottom-left
            match &s.mark {
                Mark::Text(t) => text_ops(&mut ops, "PrcH", t, bx + 2.0, by, bw - 4.0, bh, 0.62),
                Mark::Script(t) => text_ops(&mut ops, "PrcS", t, bx + 2.0, by, bw - 4.0, bh, 0.8),
                Mark::Tick => text_ops(&mut ops, "PrcZ", "4", bx, by, bw, bh, 0.9), // ZapfDingbats ✔
                Mark::Image(png) => {
                    let name = format!("PrcI{page_no}x{i}");
                    let (img, iw, ih) = image_object(&mut doc, png)?;
                    doc.add_xobject(page_id, name.as_bytes(), img)
                        .map_err(|e| PdfError::Write(e.to_string()))?;
                    // Fit inside the box, keeping the aspect ratio, aligned to the bottom-left.
                    let scale = (bw / f64::from(iw)).min(bh / f64::from(ih));
                    let (dw, dh) = (f64::from(iw) * scale, f64::from(ih) * scale);
                    ops.push(Operation::new("q", vec![]));
                    ops.push(Operation::new(
                        "cm",
                        vec![dw.into(), 0.into(), 0.into(), dh.into(), bx.into(), by.into()],
                    ));
                    ops.push(Operation::new("Do", vec![Object::Name(name.into_bytes())]));
                    ops.push(Operation::new("Q", vec![]));
                }
            }
        }
        wrap_and_append(&mut doc, page_id, Content { operations: ops })?;
    }
    append_certificate(&mut doc, certificate, &fonts)?;
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| PdfError::Write(e.to_string()))?;
    Ok(out)
}

/// Whether `png` is a drawn signature that can be placed (a readable PNG, at most 2000 px a side).
pub fn check_png(png: &[u8]) -> Result<(), PdfError> {
    image_object(&mut Document::with_version("1.7"), png).map(|_| ())
}

// ---- helpers ----

/// A page attribute, looking up the page tree when the page inherits it.
fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut id = Some(page);
    for _ in 0..32 {
        let dict = doc.get_dictionary(id?).ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(match v {
                Object::Reference(r) => doc.get_object(*r).ok()?,
                other => other,
            });
        }
        id = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

fn media_size(doc: &Document, page: ObjectId) -> Result<(f64, f64), PdfError> {
    let mb = inherited(doc, page, b"MediaBox").and_then(|o| o.as_array().ok()).ok_or(PdfError::Unreadable)?;
    let n: Vec<f64> = mb
        .iter()
        .filter_map(|o| o.as_float().map(f64::from).ok().or_else(|| o.as_i64().ok().map(|i| i as f64)))
        .collect();
    match n.as_slice() {
        [x0, y0, x1, y1] if x1 > x0 && y1 > y0 => Ok((x1 - x0, y1 - y0)),
        _ => Err(PdfError::Unreadable),
    }
}

/// Gives a page that inherits its resources its own copy, so adding fonts doesn't hide the rest.
fn own_resources(doc: &mut Document, page: ObjectId) -> Result<(), PdfError> {
    let has_own = doc.get_dictionary(page).map(|d| d.has(b"Resources")).unwrap_or(false);
    if has_own {
        // A shared resource dictionary (an indirect object) is copied too: other pages keep theirs.
        if let Ok(Object::Reference(r)) = doc.get_dictionary(page).and_then(|d| d.get(b"Resources")).cloned()
        {
            let copy = doc.get_object(r).map_err(|e| PdfError::Write(e.to_string()))?.clone();
            doc.get_dictionary_mut(page).map_err(|e| PdfError::Write(e.to_string()))?.set("Resources", copy);
        }
        return Ok(());
    }
    let copy = match inherited(doc, page, b"Resources") {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    doc.get_dictionary_mut(page).map_err(|e| PdfError::Write(e.to_string()))?.set("Resources", copy);
    Ok(())
}

struct Fonts {
    helvetica: ObjectId,
    bold: ObjectId,
    script: ObjectId,
    zapf: ObjectId,
}

fn font(doc: &mut Document, base: &str, winansi: bool) -> ObjectId {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"Font".to_vec()));
    d.set("Subtype", Object::Name(b"Type1".to_vec()));
    d.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    if winansi {
        d.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    }
    doc.add_object(d)
}

fn font_objects(doc: &mut Document) -> Fonts {
    Fonts {
        helvetica: font(doc, "Helvetica", true),
        bold: font(doc, "Helvetica-Bold", true),
        script: font(doc, "Times-Italic", true),
        zapf: font(doc, "ZapfDingbats", false),
    }
}

fn add_fonts(doc: &mut Document, page: ObjectId, f: &Fonts) -> Result<(), PdfError> {
    let page_dict = doc.get_dictionary_mut(page).map_err(|e| PdfError::Write(e.to_string()))?;
    let res = page_dict
        .get_mut(b"Resources")
        .and_then(Object::as_dict_mut)
        .map_err(|e| PdfError::Write(e.to_string()))?;
    // The font dictionary may be indirect; resolve it into a direct copy we can extend.
    let existing = res.get(b"Font").cloned().ok();
    let mut fonts = match existing {
        Some(Object::Dictionary(d)) => d,
        Some(Object::Reference(r)) => doc.get_dictionary(r).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    fonts.set("PrcH", Object::Reference(f.helvetica));
    fonts.set("PrcB", Object::Reference(f.bold));
    fonts.set("PrcS", Object::Reference(f.script));
    fonts.set("PrcZ", Object::Reference(f.zapf));
    let page_dict = doc.get_dictionary_mut(page).map_err(|e| PdfError::Write(e.to_string()))?;
    let res = page_dict
        .get_mut(b"Resources")
        .and_then(Object::as_dict_mut)
        .map_err(|e| PdfError::Write(e.to_string()))?;
    res.set("Font", fonts);
    Ok(())
}

/// Text in WinAnsi bytes; anything it can't encode becomes `?`.
fn winansi(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c as u32 {
            0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
            0x2018 => 0x91,
            0x2019 => 0x92,
            0x201c => 0x93,
            0x201d => 0x94,
            0x2013 => 0x96,
            0x2014 => 0x97,
            0x20ac => 0x80,
            0x2026 => 0x85,
            _ => b'?',
        })
        .collect()
}

/// Rough Helvetica advance width (em fraction), to shrink long text into its box.
fn approx_width(s: &str) -> f64 {
    s.chars()
        .map(|c| {
            if c.is_ascii_uppercase() || c == 'm' || c == 'w' {
                0.68
            } else if c == ' ' {
                0.28
            } else {
                0.52
            }
        })
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn text_ops(ops: &mut Vec<Operation>, font: &str, text: &str, x: f64, y: f64, w: f64, h: f64, fill: f64) {
    let mut size = (h * fill).clamp(4.0, 36.0);
    let need = approx_width(text) * size;
    if need > w && need > 0.0 {
        size = (size * w / need).max(4.0);
    }
    let baseline = y + (h - size) / 2.0 + size * 0.22;
    ops.push(Operation::new("BT", vec![]));
    ops.push(Operation::new("Tf", vec![Object::Name(font.as_bytes().to_vec()), size.into()]));
    ops.push(Operation::new("Td", vec![x.into(), baseline.into()]));
    ops.push(Operation::new("Tj", vec![Object::String(winansi(text), StringFormat::Literal)]));
    ops.push(Operation::new("ET", vec![]));
}

/// Wraps the page's existing content in `q … Q` and appends `content` after it.
fn wrap_and_append(
    doc: &mut Document,
    page: ObjectId,
    content: Content<Vec<Operation>>,
) -> Result<(), PdfError> {
    let encoded = content.encode().map_err(|e| PdfError::Write(e.to_string()))?;
    let existing: Vec<Object> = match doc.get_dictionary(page).and_then(|d| d.get(b"Contents")) {
        Ok(Object::Reference(r)) => vec![Object::Reference(*r)],
        Ok(Object::Array(a)) => a.clone(),
        _ => vec![],
    };
    let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let mut close_and_draw = b"\nQ\n".to_vec();
    close_and_draw.extend_from_slice(&encoded);
    let draw = doc.add_object(Stream::new(Dictionary::new(), close_and_draw));
    let mut list = vec![Object::Reference(open)];
    list.extend(existing);
    list.push(Object::Reference(draw));
    doc.get_dictionary_mut(page).map_err(|e| PdfError::Write(e.to_string()))?.set("Contents", list);
    Ok(())
}

/// A PNG as an image XObject (RGB) with its alpha as a soft mask.
fn image_object(doc: &mut Document, png: &[u8]) -> Result<(ObjectId, u32, u32), PdfError> {
    let bad = || PdfError::Unsupported("The drawn signature isn't a readable PNG image.".into());
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| bad())?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > MAX_SIGNATURE_PX || h > MAX_SIGNATURE_PX {
        return Err(bad());
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or_else(bad)?];
    let frame = reader.next_frame(&mut buf).map_err(|_| bad())?;
    let data = &buf[..frame.buffer_size()];
    let (mut rgb, mut alpha) =
        (Vec::with_capacity((w * h * 3) as usize), Vec::with_capacity((w * h) as usize));
    match frame.color_type {
        png::ColorType::Rgba => data.as_chunks::<4>().0.iter().for_each(|p| {
            rgb.extend_from_slice(&p[..3]);
            alpha.push(p[3]);
        }),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().for_each(|p| {
            rgb.extend_from_slice(p);
            alpha.push(255);
        }),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().for_each(|p| {
            rgb.extend_from_slice(&[p[0]; 3]);
            alpha.push(p[1]);
        }),
        png::ColorType::Grayscale => data.iter().for_each(|g| {
            rgb.extend_from_slice(&[*g; 3]);
            alpha.push(255);
        }),
        png::ColorType::Indexed => return Err(bad()),
    }
    let image_dict = |cs: &str| {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"XObject".to_vec()));
        d.set("Subtype", Object::Name(b"Image".to_vec()));
        d.set("Width", i64::from(w));
        d.set("Height", i64::from(h));
        d.set("ColorSpace", Object::Name(cs.as_bytes().to_vec()));
        d.set("BitsPerComponent", 8);
        d
    };
    let mut mask = Stream::new(image_dict("DeviceGray"), alpha);
    let _ = mask.compress();
    let mask_id = doc.add_object(mask);
    let mut dict = image_dict("DeviceRGB");
    dict.set("SMask", Object::Reference(mask_id));
    let mut img = Stream::new(dict, rgb);
    let _ = img.compress();
    Ok((doc.add_object(img), w, h))
}

/// Appends the certificate pages (A4) at the end of the document.
fn append_certificate(doc: &mut Document, cert: &Certificate, f: &Fonts) -> Result<(), PdfError> {
    const W: f64 = 595.0;
    const H: f64 = 842.0;
    const MARGIN: f64 = 56.0;
    const LINE: f64 = 13.5;
    let pages_id = doc
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|e| PdfError::Write(e.to_string()))?;
    // Lay the lines out first, wrapping long ones at about 95 characters.
    let mut lines: Vec<(&str, String)> = vec![("PrcB", cert.title.clone()), ("PrcH", String::new())];
    for (head, body) in &cert.sections {
        lines.push(("PrcB", head.clone()));
        for l in body {
            let mut rest = l.as_str();
            while rest.chars().count() > 95 {
                let cut = rest
                    .char_indices()
                    .take(95)
                    .filter(|(_, c)| *c == ' ')
                    .map(|(i, _)| i)
                    .last()
                    .unwrap_or_else(|| rest.char_indices().nth(95).map_or(rest.len(), |(i, _)| i));
                lines.push(("PrcH", rest[..cut].to_owned()));
                rest = rest[cut..].trim_start();
            }
            lines.push(("PrcH", rest.to_owned()));
        }
        lines.push(("PrcH", String::new()));
    }
    let per_page = ((H - 2.0 * MARGIN) / LINE) as usize;
    let chunks: Vec<_> = lines.chunks(per_page.max(1)).collect();
    let total = chunks.len();
    for (n, chunk) in chunks.into_iter().enumerate() {
        let mut ops = Vec::new();
        let mut y = H - MARGIN;
        for (font, text) in chunk {
            let size = if *font == "PrcB" && y == H - MARGIN && n == 0 {
                16.0
            } else if *font == "PrcB" {
                11.0
            } else {
                9.5
            };
            if !text.is_empty() {
                ops.push(Operation::new("BT", vec![]));
                ops.push(Operation::new("Tf", vec![Object::Name(font.as_bytes().to_vec()), size.into()]));
                ops.push(Operation::new("Td", vec![MARGIN.into(), y.into()]));
                ops.push(Operation::new("Tj", vec![Object::String(winansi(text), StringFormat::Literal)]));
                ops.push(Operation::new("ET", vec![]));
            }
            y -= if size > 12.0 { LINE * 1.6 } else { LINE };
        }
        let footer = format!("Certificate of completion · page {} of {total}", n + 1);
        ops.push(Operation::new("BT", vec![]));
        ops.push(Operation::new("Tf", vec![Object::Name(b"PrcH".to_vec()), 8.into()]));
        ops.push(Operation::new("Td", vec![MARGIN.into(), (MARGIN / 2.0).into()]));
        ops.push(Operation::new("Tj", vec![Object::String(winansi(&footer), StringFormat::Literal)]));
        ops.push(Operation::new("ET", vec![]));
        let content = Content { operations: ops }.encode().map_err(|e| PdfError::Write(e.to_string()))?;
        let mut stream = Stream::new(Dictionary::new(), content);
        let _ = stream.compress();
        let content_id = doc.add_object(stream);
        let mut fonts = Dictionary::new();
        fonts.set("PrcH", Object::Reference(f.helvetica));
        fonts.set("PrcB", Object::Reference(f.bold));
        let mut res = Dictionary::new();
        res.set("Font", fonts);
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_id));
        page.set("MediaBox", vec![0.into(), 0.into(), W.into(), H.into()]);
        page.set("Resources", res);
        page.set("Contents", Object::Reference(content_id));
        let page_id = doc.add_object(page);
        let pages = doc.get_dictionary_mut(pages_id).map_err(|e| PdfError::Write(e.to_string()))?;
        let mut kids = pages.get(b"Kids").and_then(Object::as_array).cloned().unwrap_or_default();
        kids.push(Object::Reference(page_id));
        let count = pages.get(b"Count").and_then(Object::as_i64).unwrap_or(0) + 1;
        pages.set("Kids", kids);
        pages.set("Count", count);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small PDF: `n` A4 pages with a line of text, resources inherited from the page tree.
    pub(crate) fn sample(n: usize) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = font(&mut doc, "Helvetica", true);
        let mut fonts = Dictionary::new();
        fonts.set("F1", Object::Reference(font));
        let mut res = Dictionary::new();
        res.set("Font", fonts);
        let res_id = doc.add_object(res);
        let mut kids = vec![];
        for i in 0..n {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 14.into()]),
                    Operation::new("Td", vec![72.into(), 760.into()]),
                    Operation::new(
                        "Tj",
                        vec![Object::string_literal(format!("Offer letter, page {}", i + 1))],
                    ),
                    Operation::new("ET", vec![]),
                    Operation::new("cm", vec![2.into(), 0.into(), 0.into(), 2.into(), 0.into(), 0.into()]), // unbalanced state
                ],
            };
            let c = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(pages_id));
            page.set("Contents", Object::Reference(c));
            kids.push(Object::Reference(doc.add_object(page)));
        }
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", n as i64);
        pages.set("Kids", kids);
        pages.set("Resources", Object::Reference(res_id));
        pages.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));
        let cat = doc.add_object(catalog);
        doc.trailer.set("Root", Object::Reference(cat));
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    pub(crate) fn tiny_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 4, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[0u8; 4 * 2 * 4]).unwrap();
        }
        out
    }

    #[test]
    fn inspects_page_sizes_and_refuses_what_it_cannot_sign() {
        assert_eq!(inspect(&sample(2)).unwrap(), vec![(595.0, 842.0), (595.0, 842.0)]);
        assert_eq!(inspect(b"not a pdf"), Err(PdfError::Unreadable));
    }

    #[test]
    fn stamps_values_keeps_inherited_fonts_and_appends_the_certificate() {
        let src = sample(2);
        let stamps = vec![
            Stamp {
                page: 1,
                rect: Rect { x: 0.1, y: 0.8, w: 0.3, h: 0.05 },
                mark: Mark::Script("Ann Asker".into()),
            },
            Stamp {
                page: 1,
                rect: Rect { x: 0.5, y: 0.8, w: 0.2, h: 0.03 },
                mark: Mark::Text("14 Oct 2026".into()),
            },
            Stamp { page: 2, rect: Rect { x: 0.1, y: 0.1, w: 0.04, h: 0.03 }, mark: Mark::Tick },
            Stamp { page: 2, rect: Rect { x: 0.5, y: 0.5, w: 0.3, h: 0.06 }, mark: Mark::Image(tiny_png()) },
        ];
        let cert = Certificate {
            title: "Certificate of completion".into(),
            sections: vec![(
                "Document".into(),
                vec!["Offer letter".into(), "SHA-256 ".to_owned() + &"ab".repeat(32)],
            )],
        };
        let out = stamp(&src, &stamps, &cert).unwrap();
        if let Ok(dir) = std::env::var("PROCERA_SIGNING_DUMP") {
            std::fs::write(format!("{dir}/stamped.pdf"), &out).unwrap(); // to look at it by eye
        }
        let doc = Document::load_mem(&out).unwrap();
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 3, "two pages plus one certificate page");
        let p1 = pages[&1];
        let text = String::from_utf8_lossy(&doc.get_page_content(p1)).into_owned();
        assert!(text.starts_with("q"), "original content is wrapped");
        assert!(text.contains("(Ann Asker) Tj") && text.contains("(14 Oct 2026) Tj"));
        let (res, _) = doc.get_page_resources(p1).unwrap();
        let fonts = res.unwrap().get(b"Font").unwrap().as_dict().unwrap();
        assert!(fonts.has(b"F1") && fonts.has(b"PrcS"), "the page keeps its own font and gains ours");
        let cert_text = String::from_utf8_lossy(&doc.get_page_content(pages[&3])).into_owned();
        assert!(cert_text.contains("Certificate of completion"));
        // Re-inspecting the result still works (a signed copy is a valid PDF).
        assert_eq!(inspect(&out).unwrap().len(), 3);
    }

    #[test]
    fn non_latin_text_becomes_question_marks_not_garbage() {
        assert_eq!(
            winansi("Zoë – “ok” 名"),
            vec![b'Z', b'o', 0xeb, b' ', 0x96, b' ', 0x93, b'o', b'k', 0x94, b' ', b'?']
        );
    }
}
