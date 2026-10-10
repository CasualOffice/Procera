//! RFC 9457 problem details, the only error body the API returns
//! (docs/08-API-SPEC.md §3).

use serde::{Deserialize, Serialize};

/// Base URI for problem `type` values. The domain is a placeholder until
/// trademark/domain clearance (OQ-01).
pub const PROBLEM_TYPE_BASE: &str = "https://procera.dev/errors/";

/// A field-level error inside a problem (`errors[]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// JSON Pointer (RFC 6901) to the offending field.
    pub pointer: String,
    pub code: String,
    pub message: String,
}

/// `application/problem+json` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    /// Stable machine-readable code, e.g. `validation_failed`.
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<FieldError>,
}

impl Problem {
    pub fn new(status: u16, code: &str, title: &str) -> Self {
        Self {
            type_uri: format!("{PROBLEM_TYPE_BASE}{code}"),
            title: title.to_owned(),
            status,
            code: code.to_owned(),
            detail: None,
            trace_id: None,
            errors: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn not_found() -> Self {
        Self::new(404, "not_found", "Not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_per_rfc9457() -> Result<(), serde_json::Error> {
        let p = Problem::new(409, "already_decided", "Already decided").with_detail("Decided by R. Iyer");
        let v = serde_json::to_value(&p)?;
        assert_eq!(v["type"], "https://procera.dev/errors/already_decided");
        assert_eq!(v["status"], 409);
        assert!(v.get("errors").is_none(), "empty errors must be omitted");
        Ok(())
    }
}
