//! Procera workflow IR v1: model, compiler, canonical form and semantic diff (BPMS-080).
//!
//! The IR is the contract between authoring and execution (docs/11, ADR-0003). This slice adds
//! the semantic checks; the compile API, canonical form and diff follow.

mod check;
mod diag;
mod env;
mod expr;
pub mod model;

pub use diag::{Code, Diagnostic, Severity};
pub use env::{Environment, Resolution};
pub use expr::{Analysis, CONTEXT_ROOTS, ExpressionChecker, LexicalChecker};
pub use model::IrDocument;

/// Every docs/11 §11 check over a parsed document, sorted.
pub fn check(
    doc: &IrDocument,
    expressions: &dyn ExpressionChecker,
    env: Option<&dyn Environment>,
) -> Vec<Diagnostic> {
    let mut d = check::run(doc, expressions, env);
    d.sort();
    d.dedup();
    d
}
