//! DOCX export of the block model (BPMS-042, ADR-0013): real Word structure, not a picture of the
//! page. Headings use the built-in Heading 1-3 styles with outline levels (navigation pane and
//! accessibility checkers see them), lists are Word numbering (bullets, and numbers restarting per
//! list), tables have a header row, links are hyperlinks, bold and italic are run properties.
//! Deterministic: the same content and metadata give the same bytes (fixed package timestamps).

use docx_rs::{
    AbstractNumbering, Docx, Hyperlink, HyperlinkType, IndentLevel, Level, LevelJc, LevelOverride, LevelText,
    NumberFormat, Numbering, NumberingId, Paragraph, Run, SpecialIndentType, Start, Style, StyleType, Table,
    TableCell, TableRow,
};
use serde_json::Value;

use crate::blocks::safe_href;

/// What the cover lines say about the version.
#[derive(Debug, Clone)]
pub struct DocxMeta<'a> {
    pub title: &'a str,
    /// e.g. "Policy · version 3 (2.0) · published 2026-10-11 · status published"
    pub subtitle: &'a str,
    /// Set for deprecated or retired versions.
    pub superseded_note: Option<&'a str>,
    pub footer: &'a str,
    /// RFC 3339 instant written to the package properties, so output is reproducible.
    pub stamp: &'a str,
}

const BULLETS: usize = 1;
const NUMBERS: usize = 2;

fn runs(text: &str, runs: Option<&Value>) -> Vec<Run> {
    let Some(rs) = runs.and_then(Value::as_array) else { return vec![Run::new().add_text(text)] };
    rs.iter()
        .map(|r| {
            let mut run = Run::new().add_text(r.get("text").and_then(Value::as_str).unwrap_or(""));
            if r.get("bold").and_then(Value::as_bool) == Some(true) {
                run = run.bold();
            }
            if r.get("italic").and_then(Value::as_bool) == Some(true) {
                run = run.italic();
            }
            run
        })
        .collect()
}

/// A paragraph from text plus optional runs; runs with a safe `href` become hyperlinks.
fn para(text: &str, rs: Option<&Value>) -> Paragraph {
    let mut p = Paragraph::new();
    match rs.and_then(Value::as_array) {
        Some(list) => {
            for (r, run) in list.iter().zip(runs(text, rs)) {
                match r.get("href").and_then(Value::as_str).filter(|h| safe_href(h) && !h.starts_with('/')) {
                    Some(h) => {
                        p = p.add_hyperlink(
                            Hyperlink::new(h, HyperlinkType::External)
                                .add_run(run.color("1F4E79").underline("single")),
                        );
                    }
                    None => p = p.add_run(run),
                }
            }
        }
        None => p = p.add_run(Run::new().add_text(text)),
    }
    p
}

fn level(format: &str, text: &str) -> Level {
    Level::new(0, Start::new(1), NumberFormat::new(format), LevelText::new(text), LevelJc::new("left"))
        .indent(Some(720), Some(SpecialIndentType::Hanging(360)), None, None)
}

/// The .docx bytes for a version's content.
pub fn blocks_docx(content: &Value, meta: &DocxMeta<'_>) -> Result<Vec<u8>, String> {
    let empty = vec![];
    let blocks = content.get("blocks").and_then(Value::as_array).unwrap_or(&empty);
    let mut doc = Docx::new()
        .created_at(meta.stamp)
        .updated_at(meta.stamp)
        .add_style(Style::new("Title", StyleType::Paragraph).name("Title").size(48).bold())
        .add_style(
            Style::new("Heading1", StyleType::Paragraph).name("Heading 1").size(32).bold().outline_lvl(0),
        )
        .add_style(
            Style::new("Heading2", StyleType::Paragraph).name("Heading 2").size(28).bold().outline_lvl(1),
        )
        .add_style(
            Style::new("Heading3", StyleType::Paragraph).name("Heading 3").size(24).bold().outline_lvl(2),
        )
        .add_abstract_numbering(AbstractNumbering::new(BULLETS).add_level(level("bullet", "•")))
        .add_abstract_numbering(AbstractNumbering::new(NUMBERS).add_level(level("decimal", "%1.")))
        .add_paragraph(Paragraph::new().style("Title").add_run(Run::new().add_text(meta.title)))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(meta.subtitle).color("5F5F5A").size(20)));
    if let Some(note) = meta.superseded_note {
        doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(note).bold().color("B42318")));
    }
    // Each list gets its own numbering instance, so numbered lists restart at 1.
    let mut next_num = 1usize;
    for b in blocks {
        let text = b.get("text").and_then(Value::as_str).unwrap_or("");
        match b.get("type").and_then(Value::as_str).unwrap_or("") {
            "heading" => {
                let lvl = b.get("level").and_then(Value::as_i64).unwrap_or(2).clamp(1, 3);
                doc = doc.add_paragraph(para(text, b.get("runs")).style(&format!("Heading{lvl}")));
            }
            "callout" => {
                let label = if b.get("tone").and_then(Value::as_str) == Some("warning") {
                    "Warning: "
                } else {
                    "Note: "
                };
                let mut p = Paragraph::new().add_run(Run::new().add_text(label).bold());
                for r in para(text, b.get("runs")).children {
                    p.children.push(r);
                }
                doc = doc.add_paragraph(p.indent(Some(360), None, None, None));
            }
            "list" => {
                let ordered = b.get("ordered").and_then(Value::as_bool) == Some(true);
                let id = next_num;
                next_num += 1;
                doc = doc.add_numbering(
                    Numbering::new(id, if ordered { NUMBERS } else { BULLETS })
                        .add_override(LevelOverride::new(0).start(1)),
                );
                let item_runs = b.get("item_runs").and_then(Value::as_array);
                for (i, it) in b.get("items").and_then(Value::as_array).unwrap_or(&empty).iter().enumerate() {
                    doc = doc.add_paragraph(
                        para(it.as_str().unwrap_or(""), item_runs.and_then(|r| r.get(i)))
                            .numbering(NumberingId::new(id), IndentLevel::new(0)),
                    );
                }
            }
            "table" => {
                let rows: Vec<TableRow> = b
                    .get("rows")
                    .and_then(Value::as_array)
                    .unwrap_or(&empty)
                    .iter()
                    .enumerate()
                    .map(|(ri, r)| {
                        TableRow::new(
                            r.as_array()
                                .unwrap_or(&empty)
                                .iter()
                                .map(|c| {
                                    let mut run = Run::new().add_text(c.as_str().unwrap_or(""));
                                    if ri == 0 {
                                        run = run.bold();
                                    }
                                    TableCell::new().add_paragraph(Paragraph::new().add_run(run))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                doc = doc.add_table(Table::new(rows));
            }
            // Pictures stay in the app for now; the document says what is there (alt text, caption).
            "image" => {
                let cap = b.get("caption").and_then(Value::as_str).filter(|c| !c.is_empty());
                let line = format!("[Image: {text}]{}", cap.map(|c| format!(" {c}")).unwrap_or_default());
                doc = doc.add_paragraph(
                    Paragraph::new().add_run(Run::new().add_text(line).italic().color("5F5F5A")),
                );
            }
            _ => doc = doc.add_paragraph(para(text, b.get("runs"))),
        }
    }
    doc = doc
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text(meta.footer).color("5F5F5A").size(16)));
    let mut out = std::io::Cursor::new(Vec::new());
    doc.build().pack(&mut out).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Read;

    fn meta() -> DocxMeta<'static> {
        DocxMeta {
            title: "Travel",
            subtitle: "Policy · version 1",
            superseded_note: None,
            footer: "Exported from Procera.",
            stamp: "2026-10-11T00:00:00Z",
        }
    }

    fn document_xml(bytes: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        let mut s = String::new();
        zip.by_name("word/document.xml").expect("document part").read_to_string(&mut s).expect("utf-8");
        s
    }

    #[test]
    fn structure_survives_as_word_structure() {
        let content = json!({"blocks": [
            {"type": "heading", "level": 1, "text": "Scope"},
            {"type": "paragraph", "text": "Book early. See portal.", "runs": [{"text": "Book early", "bold": true}, {"text": ". See "}, {"text": "portal", "href": "https://travel.example"}, {"text": "."}]},
            {"type": "list", "ordered": true, "items": ["One", "Two"]},
            {"type": "list", "ordered": true, "items": ["Again"]},
            {"type": "callout", "tone": "warning", "text": "No first class."},
            {"type": "table", "rows": [["Grade", "Limit"], ["A", "100"]]}
        ]});
        let xml = document_xml(&blocks_docx(&content, &meta()).expect("builds"));
        assert!(xml.contains("Heading1") && xml.contains("Scope"));
        assert!(xml.contains("<w:b ") || xml.contains("<w:b/>"), "bold run");
        assert!(xml.contains("w:hyperlink"), "link");
        assert!(xml.contains("w:numId"), "numbered list");
        assert!(xml.contains("Warning: ") && xml.contains("w:tbl"));
    }

    #[test]
    fn deterministic_and_marks_superseded() {
        let c = json!({"blocks": [{"type": "paragraph", "text": "x"}]});
        assert_eq!(blocks_docx(&c, &meta()).unwrap(), blocks_docx(&c, &meta()).unwrap());
        let m = DocxMeta {
            superseded_note: Some("Superseded: this is version 1, not the current version."),
            ..meta()
        };
        assert!(document_xml(&blocks_docx(&c, &m).unwrap()).contains("Superseded"));
    }

    #[test]
    fn unsafe_links_stay_plain_text() {
        let c = json!({"blocks": [{"type": "paragraph", "text": "x", "runs": [{"text": "x", "href": "javascript:alert(1)"}]}]});
        assert!(!document_xml(&blocks_docx(&c, &meta()).unwrap()).contains("w:hyperlink"));
    }
}
