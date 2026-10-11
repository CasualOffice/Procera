//! Procera workflow IR v1: model, compiler, canonical form and semantic diff (BPMS-080).
//!
//! The IR is the contract between authoring (step builder, BPMN import, templates, AI drafts, API)
//! and execution (docs/11, ADR-0003). This crate turns an authored IR v1 document into the
//! validated, normalised, checksummed IR, or into diagnostics with stable codes:
//!
//! ```text
//! source JSON ─ schema (IR-E100) ─ typed parse ─ §11 checks ─┬─ errors → diagnostics only
//!                                                            └─ ok → normalize → JCS bytes → sha256
//! ```
//!
//! The compiler is pure and deterministic: no I/O, clock or randomness. Workspace facts (are the
//! referenced artifacts published, is the connector enabled, does the group exist) come in
//! through [`Environment`]; expression parsing goes through [`ExpressionChecker`] (the engine
//! itself is BPMS-081).
//!
//! ```
//! use procera_ir::{CompileContext, compile};
//! let source = serde_json::json!({
//!     "schemaVersion": 1,
//!     "process": { "key": "hello", "version": 1, "name": { "en": "Hello" } },
//!     "variables": [],
//!     "nodes": [ { "id": "start", "kind": "start" }, { "id": "done", "kind": "end" } ],
//!     "edges": [ { "id": "e1", "from": "start", "to": "done" } ]
//! });
//! let out = compile(&source, &CompileContext::default());
//! assert!(!out.has_errors());
//! assert_eq!(out.compiled.unwrap().checksum.len(), 64);
//! ```

mod canon;
mod check;
mod diag;
mod diff;
mod env;
mod expr;
pub mod model;

pub use canon::{COMPILER_VERSION, canonical_bytes, checksum_of, normalize};
pub use diag::{Code, Diagnostic, Severity};
pub use diff::{Change, ChangeKind, DiffError, Section, SemanticDiff, diff};
pub use env::{Environment, Resolution};
pub use expr::{Analysis, CONTEXT_ROOTS, ExpressionChecker, LexicalChecker};
pub use model::IrDocument;

use serde_json::Value;
use std::sync::OnceLock;

/// The IR v1 JSON Schema (docs/11 §13).
pub const SCHEMA_V1: &str = include_str!("../../../schemas/ir/v1.schema.json");

/// What the compiler may consult. All of it is caller-provided and read-only.
#[derive(Clone, Copy)]
pub struct CompileContext<'a> {
    /// Expression parser/analyser (BPMS-081 supplies the real engine).
    pub expressions: &'a dyn ExpressionChecker,
    /// Workspace snapshot for checks 8-10; `None` skips them and reports `IR-I001`.
    pub environment: Option<&'a dyn Environment>,
    /// The previous published version, to produce a semantic diff.
    pub previous: Option<&'a IrDocument>,
}

impl Default for CompileContext<'_> {
    fn default() -> Self {
        Self { expressions: &LexicalChecker, environment: None, previous: None }
    }
}

impl std::fmt::Debug for CompileContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompileContext")
            .field("environment", &self.environment.is_some())
            .field("previous", &self.previous.map(|p| p.process.version))
            .finish_non_exhaustive()
    }
}

/// A successfully compiled IR.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The normal form (see [`normalize`]).
    pub ir: IrDocument,
    /// RFC 8785 canonical JSON of `ir`.
    pub canonical: Vec<u8>,
    /// Lowercase hex SHA-256 of `canonical`.
    pub checksum: String,
}

/// Everything a compile produces.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileOutput {
    /// Present only when there are no error diagnostics.
    pub compiled: Option<Compiled>,
    /// Sorted by code, then pointer; no duplicates.
    pub diagnostics: Vec<Diagnostic>,
    /// Against [`CompileContext::previous`], when given and the compile succeeded.
    pub diff: Option<SemanticDiff>,
}

impl CompileOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    fn failed(mut diagnostics: Vec<Diagnostic>) -> Self {
        diagnostics.sort();
        diagnostics.dedup();
        Self { compiled: None, diagnostics, diff: None }
    }
}

fn validator() -> Result<&'static jsonschema::Validator, &'static str> {
    static V: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA_V1).map_err(|e| e.to_string())?;
        jsonschema::validator_for(&schema).map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(String::as_str)
}

/// Shape check against the IR v1 JSON Schema. Each violation is an `IR-E100` at its location.
pub fn validate_schema(source: &Value) -> Vec<Diagnostic> {
    match validator() {
        Ok(v) => v
            .iter_errors(source)
            .map(|e| Diagnostic::new(Code::E100, e.instance_path().to_string(), e.to_string()))
            .collect(),
        Err(e) => vec![Diagnostic::new(Code::E100, "", format!("IR schema unavailable: {e}"))],
    }
}

/// Compiles an authored IR v1 document (docs/11 §11).
pub fn compile(source: &Value, ctx: &CompileContext<'_>) -> CompileOutput {
    let shape = validate_schema(source);
    if !shape.is_empty() {
        return CompileOutput::failed(shape);
    }
    let doc: IrDocument = match serde_json::from_value(source.clone()) {
        Ok(d) => d,
        Err(e) => return CompileOutput::failed(vec![Diagnostic::new(Code::E100, "", e.to_string())]),
    };
    let mut diagnostics = check::run(&doc, ctx.expressions, ctx.environment);
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return CompileOutput { compiled: None, diagnostics, diff: None };
    }
    let ir = normalize(&doc);
    let canonical = match canonical_bytes(&ir) {
        Ok(b) => b,
        Err(e) => return CompileOutput::failed(vec![Diagnostic::new(Code::E100, "", e.to_string())]),
    };
    let checksum = checksum_of(&canonical);
    let diff = ctx.previous.map(|prev| diff(&normalize(prev), &ir));
    CompileOutput { compiled: Some(Compiled { ir, canonical, checksum }), diagnostics, diff }
}

/// [`compile`] from JSON text; unparseable text is a single `IR-E100`.
pub fn compile_str(source: &str, ctx: &CompileContext<'_>) -> CompileOutput {
    match serde_json::from_str::<Value>(source) {
        Ok(v) => compile(&v, ctx),
        Err(e) => CompileOutput::failed(vec![Diagnostic::new(Code::E100, "", format!("not JSON: {e}"))]),
    }
}
