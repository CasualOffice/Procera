//! Procera workflow IR v1: model, compiler, canonical form and semantic diff (BPMS-080).
//!
//! The IR is the contract between authoring and execution (docs/11, ADR-0003). This slice adds
//! the structural checks; semantic checks and the compile API follow.

mod check;
mod diag;
mod env;
mod expr;
pub mod model;

pub use diag::{Code, Diagnostic, Severity};
pub use env::{Environment, Resolution};
pub use expr::{Analysis, CONTEXT_ROOTS, ExpressionChecker, LexicalChecker};
pub use model::IrDocument;

/// Structural checks (docs/11 §11 checks 1-6, 12) over a parsed document, sorted.
pub fn check_structure(doc: &IrDocument) -> Vec<Diagnostic> {
    let mut d = check::run(doc, &LexicalChecker, None);
    d.sort();
    d.dedup();
    d
}
