//! Procera expression engine (BPMS-081).
//!
//! A typed, total, budgeted subset of [CEL](https://cel.dev) with Procera extensions (exact
//! decimals and money, dates, durations, aggregates), as recommended by SP-08 and ADR-0010.
//! The language reference is docs/11 §6.1.
//!
//! Pipeline: [`parse`] → [`check`] (against a [`Schema`]) → [`evaluate`] (with [`Bindings`],
//! an injected [`EvalEnv`] and a [`Budget`]). [`compile`] does the first two.
//!
//! The crate is pure: no I/O, no clock, no randomness, no threads. It builds for
//! `wasm32-unknown-unknown` so the designer can type-check in the browser.

mod ast;
mod builtins;
mod check;
mod diag;
mod eval;
mod lexer;
mod parser;
mod types;
mod value;

pub use ast::{BinaryOp, Expr, ExprKind, Literal, MacroKind, UnaryOp};
pub use check::{CheckOptions, CheckedExpr, ExprContext, check};
pub use diag::{Code, Diagnostic, Span};
pub use eval::{BindingError, Bindings, Budget, EvalEnv, EvalError, Usage, evaluate, evaluate_metered};
pub use parser::parse;
pub use types::{Classification, Schema, Type, VarDecl};
pub use value::{
    Value, format_date, format_datetime, format_duration, parse_date, parse_datetime, parse_duration,
};

/// Longest accepted source, in bytes (SP-08: ≤ 4 KB).
pub const MAX_SOURCE_LEN: usize = 4096;
/// Deepest accepted AST (SP-08: ≤ 32).
pub const MAX_AST_DEPTH: u32 = 32;
/// The IR language tag for this engine: `"expr:cel: amount > 50000"` (docs/11 §6).
pub const LANGUAGE_TAG: &str = "cel";

/// Parse and type-check in one step.
pub fn compile(source: &str, schema: &Schema, opts: &CheckOptions) -> Result<CheckedExpr, Vec<Diagnostic>> {
    let ast = parse(source).map_err(|d| vec![d])?;
    check(source, ast, schema, opts)
}

/// Split an IR expression string `expr:<lang>: <source>` into its language tag and source.
/// Returns `None` when the prefix is missing.
pub fn split_tagged(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("expr:")?;
    let (lang, src) = rest.split_once(':')?;
    Some((lang, src.trim_start()))
}
