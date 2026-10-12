//! Server-side form validation (BPMS-101 subset, docs/11 §7): types, required fields and unknown
//! fields, before anything reaches the runtime. Values come back normalised (money amounts as
//! exact decimal strings, trimmed text). Principal fields are checked here only for shape (a
//! member id); the caller confirms the member and shapes the principal from the directory.

use std::collections::BTreeMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::catalogue::{Field, Form};

const STRING_MAX: usize = 200;
const TEXT_MAX: usize = 5_000;

/// One invalid field, as an RFC 9457 `errors[]` member (docs/08 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub pointer: String,
    pub code: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: &str, code: &str, message: &str) -> Self {
        Self { pointer: format!("/{field}"), code: code.to_owned(), message: message.to_owned() }
    }

    /// The same error under a body member, e.g. `/input/amount`.
    pub fn under(mut self, member: &str) -> Self {
        self.pointer = format!("/{member}{}", self.pointer);
        self
    }
}

/// Validates `input` against `form`. `partial` keeps absent required fields valid (a step form
/// that edits only some values).
pub fn validate(
    form: &Form,
    input: &Map<String, Value>,
    partial: bool,
) -> Result<BTreeMap<String, Value>, Vec<FieldError>> {
    let mut errors: Vec<FieldError> = input
        .keys()
        .filter(|k| !form.fields.iter().any(|f| &f.name == *k))
        .map(|k| FieldError::new(k, "unknown_field", "This field is not part of the form"))
        .collect();
    let mut out = BTreeMap::new();
    for f in &form.fields {
        match input.get(&f.name) {
            None | Some(Value::Null) => {
                if f.required && !partial {
                    errors.push(FieldError::new(&f.name, "required", "This field is required"));
                }
            }
            Some(Value::Array(a)) if a.is_empty() && f.ty == "file" => {
                if f.required && !partial {
                    errors.push(FieldError::new(&f.name, "required", "Attach a file"));
                }
                // An explicit empty list clears the field (a resubmit that removes every file).
                if partial {
                    out.insert(f.name.clone(), json!([]));
                }
            }
            Some(Value::String(s)) if s.trim().is_empty() && f.ty != "money" => {
                if f.required && !partial {
                    errors.push(FieldError::new(&f.name, "required", "This field is required"));
                }
            }
            Some(v) => match value(f, v) {
                Ok(v) => {
                    out.insert(f.name.clone(), v);
                }
                Err((code, msg)) => errors.push(FieldError::new(&f.name, code, &msg)),
            },
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

fn value(f: &Field, v: &Value) -> Result<Value, (&'static str, String)> {
    let bad = |m: &str| ("invalid", m.to_owned());
    match f.ty {
        "string" | "text" => {
            let s = v.as_str().ok_or_else(|| bad("Must be text"))?.trim();
            let max = if f.ty == "string" { STRING_MAX } else { TEXT_MAX };
            if s.chars().count() > max {
                return Err(("too_long", format!("Must be at most {max} characters")));
            }
            Ok(json!(s))
        }
        "enum" => {
            let s = v.as_str().ok_or_else(|| bad("Must be one of the options"))?;
            if f.options.iter().flatten().any(|o| o == s) {
                Ok(json!(s))
            } else {
                Err((
                    "invalid_option",
                    format!(
                        "Must be one of: {}",
                        f.options.iter().flatten().cloned().collect::<Vec<_>>().join(", ")
                    ),
                ))
            }
        }
        "date" => {
            let s = v.as_str().ok_or_else(|| bad("Must be a date (YYYY-MM-DD)"))?;
            let fmt = time::macros::format_description!("[year]-[month]-[day]");
            time::Date::parse(s, fmt).map_err(|_| bad("Must be a date (YYYY-MM-DD)"))?;
            Ok(json!(s))
        }
        "number" => {
            let d = decimal(v).ok_or_else(|| bad("Must be a number"))?;
            Ok(json!(d.normalize().to_string()))
        }
        "boolean" => v.as_bool().map(Value::Bool).ok_or_else(|| bad("Must be true or false")),
        "principal" => {
            let s = v.as_str().ok_or_else(|| bad("Must be a member"))?;
            uuid::Uuid::parse_str(s).map_err(|_| bad("Must be a member"))?;
            Ok(json!(s))
        }
        "money" => {
            let o = v.as_object().ok_or_else(|| bad("Must be an amount with a currency"))?;
            let amount =
                o.get("amount").and_then(decimal).ok_or_else(|| bad("The amount must be a number"))?;
            if amount <= Decimal::ZERO {
                return Err(("minimum", "Must be more than 0".to_owned()));
            }
            if amount.scale() > 2 {
                return Err(("precision", "Use at most 2 decimal places".to_owned()));
            }
            let currency = match (o.get("currency").and_then(Value::as_str), &f.currency) {
                (None, Some(fixed)) => fixed.clone(),
                (Some(c), Some(fixed)) if c != fixed => {
                    return Err(("currency", format!("The currency must be {fixed}")));
                }
                (Some(c), _) if c.len() == 3 && c.chars().all(|x| x.is_ascii_uppercase()) => c.to_owned(),
                _ => return Err(("currency", "Give a three-letter currency code, like EUR".to_owned())),
            };
            if o.keys().any(|k| k != "amount" && k != "currency") {
                return Err(bad("Only amount and currency are allowed"));
            }
            Ok(json!({ "amount": amount.to_string(), "currency": currency }))
        }
        "file" => {
            // Ids (or `{id}` objects sent back on a resubmit); the caller checks the files themselves.
            let items: Vec<&Value> = match v {
                Value::Array(a) if f.multiple => a.iter().collect(),
                Value::Array(_) => return Err(bad("Attach one file")),
                one => vec![one],
            };
            if items.len() > crate::attach::MAX_FILES {
                return Err(("too_many", format!("Attach at most {} files", crate::attach::MAX_FILES)));
            }
            let mut ids: Vec<String> = Vec::new();
            for i in items {
                let id = i.get("id").unwrap_or(i);
                let id = id
                    .as_str()
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .ok_or_else(|| bad("Must be uploaded files"))?;
                if !ids.contains(&id.to_string()) {
                    ids.push(id.to_string());
                }
            }
            Ok(if f.multiple { json!(ids) } else { json!(ids.first()) })
        }
        other => Err(("invalid", format!("Unsupported field type {other}"))),
    }
}

/// A decimal from a JSON number or numeric string (amounts are strings on the wire, docs/08 §2).
fn decimal(v: &Value) -> Option<Decimal> {
    match v {
        Value::String(s) => Decimal::from_str_exact(s.trim()).ok(),
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Catalogue;

    fn form(key: &str) -> Form {
        let c = Catalogue::builtin().unwrap_or_else(|e| panic!("{e}"));
        c.get(key).unwrap_or_else(|| panic!("{key}")).start_form.clone()
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn valid_input_is_normalised() {
        let ok = validate(
            &form("hr.expense-claim"),
            &obj(json!({"amount": {"amount": 120.5, "currency": "EUR"}, "category": "travel", "description": " Taxi ", "expense_date": "2026-10-01"})),
            false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(ok["amount"], json!({"amount": "120.5", "currency": "EUR"}));
        assert_eq!(ok["description"], json!("Taxi"));
        let pr = validate(
            &form("fin.purchase-requisition"),
            &obj(json!({"amount": {"amount": "72000"}, "vendor": "Acme", "cost_center": "ENG", "justification": "Laptops"})),
            false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(pr["amount"]["currency"], "EUR", "the form currency fills in");
    }

    #[test]
    fn unknown_missing_and_mistyped_fields_are_listed() {
        let errs = validate(
            &form("hr.leave-request"),
            &obj(json!({"leave_type": "holiday", "start_date": "13/10/2026", "extra": 1})),
            false,
        )
        .err()
        .unwrap_or_default();
        let got: Vec<(&str, &str)> = errs.iter().map(|e| (e.pointer.as_str(), e.code.as_str())).collect();
        assert_eq!(
            got,
            [
                ("/extra", "unknown_field"),
                ("/leave_type", "invalid_option"),
                ("/start_date", "invalid"),
                ("/end_date", "required")
            ]
        );
        let money = validate(
            &form("fin.purchase-requisition"),
            &obj(json!({"amount": {"amount": "-1", "currency": "USD"}, "vendor": "A", "cost_center": "B", "justification": "C"})),
            false,
        )
        .err()
        .unwrap_or_default();
        assert_eq!(money[0].code, "minimum");
        // A partial step form accepts absent required fields but still rejects unknown ones.
        assert!(validate(&form("hr.leave-request"), &obj(json!({})), true).is_ok());
        assert!(validate(&form("hr.leave-request"), &obj(json!({"x": 1})), true).is_err());
    }
}
