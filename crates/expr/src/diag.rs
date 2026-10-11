//! Compile-time diagnostics: stable codes plus byte spans into the source.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Half-open byte range `[start, end)` into the expression source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        // Sources are capped at MAX_SOURCE_LEN, far below u32::MAX.
        Span { start: start as u32, end: end as u32 }
    }

    /// Smallest span covering both.
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    /// 1-based line and column (in characters) of the span start, for editors.
    pub fn line_col(self, source: &str) -> (u32, u32) {
        let mut line = 1;
        let mut col = 1;
        for (i, ch) in source.char_indices() {
            if i >= self.start as usize {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// Stable diagnostic codes (docs/11 §6.1). Never renumber; append only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Code {
    /// Unexpected token or malformed syntax.
    Syntax,
    /// Invalid character, unterminated string or bad escape.
    Lexical,
    /// Number literal out of range or with too many digits.
    BadNumber,
    /// Source longer than [`crate::MAX_SOURCE_LEN`].
    SourceTooLong,
    /// Nesting deeper than [`crate::MAX_AST_DEPTH`].
    TooDeep,
    /// Reserved word used as an identifier.
    Reserved,
    UnknownVariable,
    UnknownField,
    UnknownFunction,
    WrongArity,
    /// Operand types don't fit the operator.
    OperatorType,
    /// Function argument has the wrong type.
    ArgumentType,
    /// A value that may be null is used where a value is required.
    Nullable,
    /// Result type doesn't match what the context expects.
    ExpectedType,
    /// A condition may evaluate to null (docs/11 §6: null must not reach branch selection).
    NullableCondition,
    /// String literal compared with an enum is not one of its values.
    UnknownEnumValue,
    /// Money values of different currencies combined.
    CurrencyMismatch,
    /// A literal argument is invalid (bad date, duration with months, non-literal currency…).
    InvalidLiteral,
    /// List literal elements have no common type.
    MixedList,
    /// A `restricted` variable is read in a context that must never carry restricted data.
    RestrictedInContext,
    /// Comprehension macro misuse (`list.all(x, …)` needs an identifier first).
    MacroShape,
    /// Values of this type cannot be ordered.
    NotComparable,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Syntax => "EXPR-E001",
            Code::Lexical => "EXPR-E002",
            Code::BadNumber => "EXPR-E003",
            Code::SourceTooLong => "EXPR-E004",
            Code::TooDeep => "EXPR-E005",
            Code::Reserved => "EXPR-E006",
            Code::UnknownVariable => "EXPR-E010",
            Code::UnknownField => "EXPR-E011",
            Code::UnknownFunction => "EXPR-E012",
            Code::WrongArity => "EXPR-E013",
            Code::OperatorType => "EXPR-E014",
            Code::ArgumentType => "EXPR-E015",
            Code::Nullable => "EXPR-E016",
            Code::ExpectedType => "EXPR-E017",
            Code::NullableCondition => "EXPR-E018",
            Code::UnknownEnumValue => "EXPR-E019",
            Code::CurrencyMismatch => "EXPR-E020",
            Code::InvalidLiteral => "EXPR-E021",
            Code::MixedList => "EXPR-E022",
            Code::RestrictedInContext => "EXPR-E023",
            Code::MacroShape => "EXPR-E024",
            Code::NotComparable => "EXPR-E025",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One compile-time problem. All diagnostics are errors: an expression either compiles or not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Code,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Self {
        Diagnostic { code, span, message: message.into() }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}..{}: {}", self.code, self.span.start, self.span.end, self.message)
    }
}
