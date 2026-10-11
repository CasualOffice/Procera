//! The SOP/policy block model (ADR-0013, BPMS-042): validation and rendering to HTML and Markdown.
//!
//! ```text
//! content = { "blocks": [block…] }
//! block   = {type:"paragraph", text, runs?}
//!         | {type:"heading", level:1..3, text, runs?}
//!         | {type:"list", ordered?, items:[string], item_runs?:[[run]]}
//!         | {type:"callout", tone?:"info"|"warning", text, runs?}
//!         | {type:"table", header?:bool, rows:[[string]]}
//! run     = {text, bold?, italic?, href?}     // inline formatting; `text` is always the plain text
//! ```
//! `text` stays the plain-text source of truth (diffs, search, re-ack hashes); `runs` only add
//! formatting and must concatenate to `text`. Links are `http(s):`, `mailto:` or in-app `/…` paths.

use serde_json::Value;

use crate::LibraryError;

const MAX_BLOCKS: usize = 2000;
const MAX_TEXT: usize = 20_000;
const MAX_ITEMS: usize = 500;
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 20;

fn invalid(m: impl Into<String>) -> LibraryError {
    LibraryError::Invalid(m.into())
}

fn text_of(b: &Value, i: usize) -> Result<&str, LibraryError> {
    let t = b
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("block {i}: text is required")))?;
    if t.chars().count() > MAX_TEXT {
        return Err(invalid(format!("block {i}: text is longer than {MAX_TEXT} characters")));
    }
    Ok(t)
}

/// Whether a link target is safe to render.
pub fn safe_href(h: &str) -> bool {
    let l = h.trim().to_ascii_lowercase();
    l.starts_with("https://")
        || l.starts_with("http://")
        || l.starts_with("mailto:")
        || (l.starts_with('/') && !l.starts_with("//"))
}

fn check_runs(runs: Option<&Value>, text: &str, i: usize) -> Result<(), LibraryError> {
    let Some(runs) = runs else { return Ok(()) };
    let runs = runs.as_array().ok_or_else(|| invalid(format!("block {i}: runs must be a list")))?;
    let mut joined = String::new();
    for r in runs {
        let t = r
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("block {i}: each run needs text")))?;
        joined.push_str(t);
        if let Some(h) = r.get("href") {
            let h = h.as_str().ok_or_else(|| invalid(format!("block {i}: href must be text")))?;
            if !safe_href(h) {
                return Err(invalid(format!("block {i}: links must be http(s), mailto or an in-app path")));
            }
        }
        for k in r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
            if !matches!(k.as_str(), "text" | "bold" | "italic" | "href") {
                return Err(invalid(format!("block {i}: unknown run field {k}")));
            }
        }
    }
    if joined != text {
        return Err(invalid(format!("block {i}: runs must add up to the block text")));
    }
    Ok(())
}

/// Validates content against the block model. Unknown block types or fields are refused.
pub fn validate(content: &Value) -> Result<(), LibraryError> {
    let blocks = content
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("content must be {\"blocks\": [...]}"))?;
    if blocks.len() > MAX_BLOCKS {
        return Err(invalid(format!("a document has at most {MAX_BLOCKS} blocks")));
    }
    for (i, b) in blocks.iter().enumerate() {
        let ty = b.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "paragraph" => {
                let t = text_of(b, i)?;
                check_runs(b.get("runs"), t, i)?;
            }
            "heading" => {
                let t = text_of(b, i)?;
                let level = b.get("level").and_then(Value::as_i64).unwrap_or(2);
                if !(1..=3).contains(&level) {
                    return Err(invalid(format!("block {i}: heading level must be 1 to 3")));
                }
                check_runs(b.get("runs"), t, i)?;
            }
            "callout" => {
                let t = text_of(b, i)?;
                if !matches!(b.get("tone").and_then(Value::as_str).unwrap_or("info"), "info" | "warning") {
                    return Err(invalid(format!("block {i}: callout tone must be info or warning")));
                }
                check_runs(b.get("runs"), t, i)?;
            }
            "list" => {
                let items = b
                    .get("items")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(format!("block {i}: items are required")))?;
                if items.len() > MAX_ITEMS {
                    return Err(invalid(format!("block {i}: at most {MAX_ITEMS} items")));
                }
                let texts: Vec<&str> = items
                    .iter()
                    .map(|x| x.as_str().ok_or_else(|| invalid(format!("block {i}: items must be text"))))
                    .collect::<Result<_, _>>()?;
                if let Some(ir) = b.get("item_runs") {
                    let ir = ir
                        .as_array()
                        .filter(|a| a.len() == texts.len())
                        .ok_or_else(|| invalid(format!("block {i}: item_runs must match items")))?;
                    for (r, t) in ir.iter().zip(&texts) {
                        check_runs(Some(r), t, i)?;
                    }
                }
            }
            "table" => {
                let rows = b
                    .get("rows")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(format!("block {i}: rows are required")))?;
                if rows.is_empty() || rows.len() > MAX_ROWS {
                    return Err(invalid(format!("block {i}: a table has 1 to {MAX_ROWS} rows")));
                }
                for r in rows {
                    let cells = r
                        .as_array()
                        .ok_or_else(|| invalid(format!("block {i}: each row is a list of cells")))?;
                    if cells.is_empty() || cells.len() > MAX_COLS || cells.iter().any(|c| !c.is_string()) {
                        return Err(invalid(format!("block {i}: rows have 1 to {MAX_COLS} text cells")));
                    }
                }
            }
            other => return Err(invalid(format!("block {i}: unknown block type {other:?}"))),
        }
    }
    Ok(())
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

fn runs_html(text: &str, runs: Option<&Value>) -> String {
    let Some(runs) = runs.and_then(Value::as_array) else { return esc(text) };
    runs.iter()
        .map(|r| {
            let mut s = esc(r.get("text").and_then(Value::as_str).unwrap_or(""));
            if r.get("bold").and_then(Value::as_bool) == Some(true) {
                s = format!("<strong>{s}</strong>");
            }
            if r.get("italic").and_then(Value::as_bool) == Some(true) {
                s = format!("<em>{s}</em>");
            }
            if let Some(h) = r.get("href").and_then(Value::as_str).filter(|h| safe_href(h)) {
                s = format!("<a href=\"{}\">{s}</a>", esc(h));
            }
            s
        })
        .collect()
}

/// Accessible HTML for the blocks (no wrapper). Headings start at h2 under the document title.
pub fn blocks_html(content: &Value) -> String {
    let empty = vec![];
    let blocks = content.get("blocks").and_then(Value::as_array).unwrap_or(&empty);
    let mut o = String::new();
    for b in blocks {
        let text = b.get("text").and_then(Value::as_str).unwrap_or("");
        match b.get("type").and_then(Value::as_str).unwrap_or("") {
            "heading" => {
                let l = b.get("level").and_then(Value::as_i64).unwrap_or(2).clamp(1, 3) + 1;
                o.push_str(&format!("<h{l}>{}</h{l}>\n", runs_html(text, b.get("runs"))));
            }
            "list" => {
                let tag = if b.get("ordered").and_then(Value::as_bool) == Some(true) { "ol" } else { "ul" };
                o.push_str(&format!("<{tag}>"));
                let item_runs = b.get("item_runs").and_then(Value::as_array);
                for (i, it) in b.get("items").and_then(Value::as_array).unwrap_or(&empty).iter().enumerate() {
                    let t = it.as_str().unwrap_or("");
                    o.push_str(&format!("<li>{}</li>", runs_html(t, item_runs.and_then(|r| r.get(i)))));
                }
                o.push_str(&format!("</{tag}>\n"));
            }
            "callout" => {
                let tone = b.get("tone").and_then(Value::as_str).unwrap_or("info");
                let label = if tone == "warning" { "Warning" } else { "Note" };
                o.push_str(&format!(
                    "<aside class=\"callout {tone}\" aria-label=\"{label}\"><p>{}</p></aside>\n",
                    runs_html(text, b.get("runs"))
                ));
            }
            "table" => {
                let rows = b.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
                let header = b.get("header").and_then(Value::as_bool).unwrap_or(true);
                o.push_str("<table>");
                for (i, r) in rows.iter().enumerate() {
                    let cells = r.as_array().cloned().unwrap_or_default();
                    let th = header && i == 0;
                    if th {
                        o.push_str("<thead>");
                    } else if header && i == 1 || !header && i == 0 {
                        o.push_str("<tbody>");
                    }
                    o.push_str("<tr>");
                    for c in cells {
                        let c = esc(c.as_str().unwrap_or(""));
                        o.push_str(&if th {
                            format!("<th scope=\"col\">{c}</th>")
                        } else {
                            format!("<td>{c}</td>")
                        });
                    }
                    o.push_str("</tr>");
                    if th {
                        o.push_str("</thead>");
                    }
                }
                if rows.len() > usize::from(header) {
                    o.push_str("</tbody>");
                }
                o.push_str("</table>\n");
            }
            _ => o.push_str(&format!("<p>{}</p>\n", runs_html(text, b.get("runs")))),
        }
    }
    o
}

fn md_runs(text: &str, runs: Option<&Value>) -> String {
    let Some(runs) = runs.and_then(Value::as_array) else { return text.to_owned() };
    runs.iter()
        .map(|r| {
            let mut s = r.get("text").and_then(Value::as_str).unwrap_or("").to_owned();
            if r.get("bold").and_then(Value::as_bool) == Some(true) {
                s = format!("**{s}**");
            }
            if r.get("italic").and_then(Value::as_bool) == Some(true) {
                s = format!("_{s}_");
            }
            if let Some(h) = r.get("href").and_then(Value::as_str).filter(|h| safe_href(h)) {
                s = format!("[{s}]({h})");
            }
            s
        })
        .collect()
}

/// Markdown for the blocks (CommonMark + GFM tables).
pub fn blocks_markdown(content: &Value) -> String {
    let empty = vec![];
    let blocks = content.get("blocks").and_then(Value::as_array).unwrap_or(&empty);
    let mut parts: Vec<String> = vec![];
    for b in blocks {
        let text = b.get("text").and_then(Value::as_str).unwrap_or("");
        parts.push(match b.get("type").and_then(Value::as_str).unwrap_or("") {
            "heading" => format!(
                "{} {}",
                "#".repeat(
                    usize::try_from(b.get("level").and_then(Value::as_i64).unwrap_or(2).clamp(1, 3) + 1)
                        .unwrap_or(2)
                ),
                md_runs(text, b.get("runs"))
            ),
            "list" => {
                let ordered = b.get("ordered").and_then(Value::as_bool) == Some(true);
                let item_runs = b.get("item_runs").and_then(Value::as_array);
                b.get("items")
                    .and_then(Value::as_array)
                    .unwrap_or(&empty)
                    .iter()
                    .enumerate()
                    .map(|(i, it)| {
                        format!(
                            "{} {}",
                            if ordered { format!("{}.", i + 1) } else { "-".into() },
                            md_runs(it.as_str().unwrap_or(""), item_runs.and_then(|r| r.get(i)))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "callout" => format!(
                "> **{}:** {}",
                if b.get("tone").and_then(Value::as_str) == Some("warning") { "Warning" } else { "Note" },
                md_runs(text, b.get("runs"))
            ),
            "table" => {
                let rows: Vec<Vec<String>> = b
                    .get("rows")
                    .and_then(Value::as_array)
                    .unwrap_or(&empty)
                    .iter()
                    .map(|r| {
                        r.as_array()
                            .unwrap_or(&empty)
                            .iter()
                            .map(|c| c.as_str().unwrap_or("").replace('|', "\\|"))
                            .collect()
                    })
                    .collect();
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
                let line = |r: &Vec<String>| {
                    format!(
                        "| {} |",
                        (0..cols)
                            .map(|i| r.get(i).cloned().unwrap_or_default())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )
                };
                let mut out = vec![];
                if let Some(first) = rows.first() {
                    out.push(line(first));
                    out.push(format!("|{}|", vec![" --- "; cols].join("|")));
                    out.extend(rows.iter().skip(1).map(line));
                }
                out.join("\n")
            }
            _ => md_runs(text, b.get("runs")),
        });
    }
    let mut s = parts.join("\n\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_every_block_type_and_refuses_unsafe_or_inconsistent_content() {
        let ok = json!({"blocks": [
            {"type": "heading", "level": 1, "text": "Scope"},
            {"type": "paragraph", "text": "Read the policy.", "runs": [{"text": "Read "}, {"text": "the policy", "bold": true, "href": "/library/x"}, {"text": "."}]},
            {"type": "list", "ordered": true, "items": ["One", "Two"]},
            {"type": "callout", "tone": "warning", "text": "Mind the step"},
            {"type": "table", "rows": [["Role", "Does"], ["Owner", "Approves"]]}
        ]});
        assert!(validate(&ok).is_ok());
        for bad in [
            json!({"blocks": [{"type": "script", "text": "x"}]}),
            json!({"blocks": [{"type": "paragraph", "text": "x", "runs": [{"text": "x", "href": "javascript:alert(1)"}]}]}),
            json!({"blocks": [{"type": "paragraph", "text": "abc", "runs": [{"text": "ab"}]}]}),
            json!({"blocks": [{"type": "heading", "level": 7, "text": "x"}]}),
            json!({"blocks": [{"type": "table", "rows": []}]}),
            json!({"text": "not blocks"}),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn renders_accessible_escaped_html_and_markdown() {
        let c = json!({"blocks": [
            {"type": "heading", "level": 1, "text": "A <b>"},
            {"type": "paragraph", "text": "See docs", "runs": [{"text": "See "}, {"text": "docs", "href": "https://x.test/?a=1&b=2"}]},
            {"type": "list", "items": ["x"]},
            {"type": "table", "rows": [["H1", "H2"], ["a|b", "c"]]}
        ]});
        let h = blocks_html(&c);
        assert!(h.contains("<h2>A &lt;b&gt;</h2>"));
        assert!(h.contains("<a href=\"https://x.test/?a=1&amp;b=2\">docs</a>"));
        assert!(h.contains("<th scope=\"col\">H1</th>") && h.contains("<tbody><tr><td>a|b</td>"));
        let m = blocks_markdown(&c);
        assert!(m.starts_with("## A <b>\n\nSee [docs](https://x.test/?a=1&b=2)\n\n- x\n\n| H1 | H2 |"));
        assert!(m.contains("| a\\|b | c |"));
    }
}
