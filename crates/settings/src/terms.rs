//! Workspace terminology (CFG-008, FR-W04, docs/20 §2.2 `terminology.overrides`): a workspace may
//! rename product terms ("Request" → "Ticket"). The stored value holds only the overridden terms;
//! everything else falls back to the product defaults here. Labels are display copy for the UI,
//! emails and exports; API values and field names (`kind: "sop"`) never change. `en` only (A-36).

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

/// The setting key.
pub const KEY: &str = "terminology.overrides";

/// Terms a workspace may rename, with their product defaults (singular, plural).
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    ("request", "Request", "Requests"),
    ("process", "Process", "Processes"),
    ("step", "Step", "Steps"),
    ("sop", "SOP", "SOPs"),
    ("case", "Case", "Cases"),
    ("work", "Work", "Work"),
];

/// The term names, for the catalogue.
pub const TERMS: &[&str] = &["request", "process", "step", "sop", "case", "work"];

/// Longest label, in characters.
pub const MAX_LEN: usize = 40;

/// One term's labels, as people see them at the start of a sentence or in a heading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Label {
    pub singular: String,
    pub plural: String,
}

/// Validates and normalises a stored value: an object of known terms, each `{singular, plural}`
/// of 1–40 characters without control characters (trimmed). Returns the normalised object.
pub fn validate(v: &Value) -> Result<Value, String> {
    let obj = v.as_object().ok_or("expected an object of term → {singular, plural}")?;
    let mut out = Map::new();
    for (term, labels) in obj {
        if !TERMS.contains(&term.as_str()) {
            return Err(format!("unknown term {term}; terms are {}", TERMS.join(", ")));
        }
        let l = labels.as_object().ok_or_else(|| format!("{term}: expected {{singular, plural}}"))?;
        if let Some(extra) = l.keys().find(|k| !matches!(k.as_str(), "singular" | "plural")) {
            return Err(format!("{term}: unknown field {extra}"));
        }
        let mut norm = Map::new();
        for field in ["singular", "plural"] {
            let s = l
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= MAX_LEN && !s.chars().any(char::is_control))
                .ok_or_else(|| format!("{term}.{field}: expected text of 1 to {MAX_LEN} characters"))?;
            norm.insert(field.to_owned(), json!(s));
        }
        out.insert(term.clone(), Value::Object(norm));
    }
    Ok(Value::Object(out))
}

/// Every term with the workspace label where one is set (a malformed value falls back to defaults).
pub fn effective(stored: &Value) -> BTreeMap<&'static str, Label> {
    let stored = validate(stored).unwrap_or_else(|_| json!({}));
    DEFAULTS
        .iter()
        .map(|(term, one, many)| {
            let o = stored.get(term);
            let pick =
                |f: &str, d: &str| o.and_then(|o| o.get(f)).and_then(Value::as_str).unwrap_or(d).to_owned();
            (*term, Label { singular: pick("singular", one), plural: pick("plural", many) })
        })
        .collect()
}

/// A label for use mid-sentence: each word's first letter is lowered unless the word is an
/// acronym (its first two letters are capitals, like "SOP", "SOPs" or "HR"). Mirrors
/// `web/src/lib/terms.ts`.
pub fn lower(label: &str) -> String {
    label
        .split(' ')
        .map(|w| {
            let mut letters = w.chars().filter(|c| c.is_alphabetic());
            let acronym = matches!((letters.next(), letters.next()), (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase());
            if acronym {
                return w.to_owned();
            }
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_lowercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `terms` variable for notification templates: `terms.<term>.singular`, `.plural` (heading
/// case) and `.lower`, `.lower_plural` (mid-sentence).
pub fn template_context(stored: &Value) -> Value {
    let map: Map<String, Value> = effective(stored)
        .into_iter()
        .map(|(t, l)| {
            (
                t.to_owned(),
                json!({"singular": l.singular, "plural": l.plural, "lower": lower(&l.singular), "lower_plural": lower(&l.plural)}),
            )
        })
        .collect();
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_terms_only_with_bounded_labels() {
        let ok = validate(&json!({"request": {"singular": " Ticket ", "plural": "Tickets"}})).expect("valid");
        assert_eq!(ok, json!({"request": {"singular": "Ticket", "plural": "Tickets"}}));
        assert_eq!(validate(&json!({})).expect("empty is valid"), json!({}));
        for bad in [
            json!({"invoice": {"singular": "Bill", "plural": "Bills"}}),
            json!({"request": {"singular": "Ticket"}}),
            json!({"request": {"singular": "", "plural": "Tickets"}}),
            json!({"request": {"singular": "   ", "plural": "Tickets"}}),
            json!({"request": {"singular": "x".repeat(41), "plural": "Tickets"}}),
            json!({"request": {"singular": "Tick\u{7}et", "plural": "Tickets"}}),
            json!({"request": {"singular": "Ticket", "plural": "Tickets", "colour": "red"}}),
            json!({"request": "Ticket"}),
            json!(["request"]),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
        assert!(validate(&json!({"sop": {"singular": "x".repeat(40), "plural": "é".repeat(40)}})).is_ok());
    }

    #[test]
    fn effective_falls_back_to_defaults() {
        let e = effective(&json!({"request": {"singular": "Ticket", "plural": "Tickets"}}));
        assert_eq!(e["request"], Label { singular: "Ticket".into(), plural: "Tickets".into() });
        assert_eq!(e["sop"], Label { singular: "SOP".into(), plural: "SOPs".into() });
        assert_eq!(e.len(), TERMS.len());
        assert_eq!(effective(&json!("garbage"))["request"].singular, "Request");
        assert!(DEFAULTS.iter().map(|d| d.0).eq(TERMS.iter().copied()));
    }

    #[test]
    fn lower_keeps_acronyms() {
        assert_eq!(lower("Ticket"), "ticket");
        assert_eq!(lower("Change Request"), "change request");
        assert_eq!(lower("SOP"), "SOP");
        assert_eq!(lower("SOPs"), "SOPs");
        assert_eq!(lower("HR Case"), "HR case");
        assert_eq!(lower("A"), "a");
    }

    #[test]
    fn template_context_has_every_term() {
        let c = template_context(&json!({"request": {"singular": "Ticket", "plural": "Tickets"}}));
        assert_eq!(c["request"]["lower"], "ticket");
        assert_eq!(c["request"]["plural"], "Tickets");
        assert_eq!(c["sop"]["lower_plural"], "SOPs");
    }
}
