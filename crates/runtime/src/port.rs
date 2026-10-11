//! The expression-evaluation port (docs/04 §4.5, docs/11 §6).
//!
//! The interpreter never parses or evaluates expressions itself. It hands the stored source
//! (`expr:<lang>: <body>`), the compiler's checked form of it (when the program has one) and a
//! read-only [`Scope`] to an [`Evaluator`]. The production engine is [`crate::CelEvaluator`]
//! (procera-expr, CEL per ADR-0010), which owns the step and memory budget; the
//! [`crate::testkit::MiniEvaluator`] is a small stand-in for tests.

use procera_expr::{CheckedExpr, Schema};
use procera_ir::model::Expr;
use serde_json::Value;
use std::collections::BTreeMap;
use time::OffsetDateTime;

/// What an expression may read. Everything is borrowed from the instance state; evaluation is
/// pure (no clock, no I/O): `now` is the triggering event's recorded time (docs/11 §13.10).
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// Process variables by name.
    pub variables: &'a BTreeMap<String, Value>,
    /// `instance`: `{ id, started_at }` (docs/11 §13.10).
    pub instance: &'a Value,
    /// `initiator`: the principal who started the instance, as supplied at start.
    pub initiator: &'a Value,
    /// The triggering event's time.
    pub now: OffsetDateTime,
    /// The declared variables and process context, with their expression types (from the
    /// compiled program), so values can be marshalled by type.
    pub schema: &'a Schema,
}

/// Why an evaluation failed. The interpreter turns every failure into an incident on the token
/// (docs/04 §4.5: "never a panic").
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// The evaluation exceeded its step or memory budget (incident `IR-R010`).
    #[error("expression budget exhausted: {0}")]
    Budget(String),
    /// Any other failure: parse error, type error, missing value.
    #[error("expression failed: {0}")]
    Failed(String),
}

/// Evaluates IR expressions and rule tables. Implementations must be deterministic: the same
/// expression and scope always give the same result.
pub trait Evaluator: std::fmt::Debug {
    /// Evaluates an expression to a JSON value. `checked` is the compiler's checked form of
    /// `expr` for this field (docs/04 §4.5), or `None` when the program carries none.
    fn eval(&self, expr: &Expr, checked: Option<&CheckedExpr>, scope: &Scope<'_>)
    -> Result<Value, EvalError>;

    /// Evaluates a rule-table decision (docs/11 §13.1): the `output` column of the rule named by
    /// `rule` (a `refs.rules` key, resolved to `reference` when declared). `Value::Null` means
    /// no row matched, which takes the `default` branch. Rule tables are BPMS-087.
    fn eval_rule(
        &self,
        rule: &str,
        reference: Option<&str>,
        output: &str,
        scope: &Scope<'_>,
    ) -> Result<Value, EvalError>;
}

/// The `instance` value exposed to expressions.
pub(crate) fn instance_value(id: uuid::Uuid, started_at: OffsetDateTime) -> Value {
    let started = started_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    serde_json::json!({ "id": id.to_string(), "started_at": started })
}
