//! Procera workflow IR v1: model, compiler, canonical form and semantic diff (BPMS-080).
//!
//! The IR is the contract between authoring and execution (docs/11, ADR-0003). This slice holds
//! the data model, the diagnostic codes and the compiler's two ports; the checks follow.

mod diag;
mod env;
mod expr;
pub mod model;

pub use diag::{Code, Diagnostic, Severity};
pub use env::{Environment, Resolution};
pub use expr::{Analysis, CONTEXT_ROOTS, ExpressionChecker, LexicalChecker};
pub use model::IrDocument;
