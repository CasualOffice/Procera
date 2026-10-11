//! Procera workflow IR v1: model, compiler, canonical form and semantic diff (BPMS-080).
//!
//! The IR is the contract between authoring and execution (docs/11, ADR-0003). This slice holds
//! the data model and the diagnostic codes; the compiler follows.

mod diag;
pub mod model;

pub use diag::{Code, Diagnostic, Severity};
pub use model::IrDocument;
