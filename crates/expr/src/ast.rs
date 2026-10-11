//! Abstract syntax tree. Serializable so the compiler can store the parsed form (docs/04 §4.5).

use crate::diag::Span;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// Height of this subtree (a leaf is 1). Bounded by [`crate::MAX_AST_DEPTH`], which also
    /// bounds evaluator recursion.
    pub depth: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Decimal(Decimal),
    String(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    And,
    Or,
}

impl BinaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::In => "in",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
        }
    }
}

/// The CEL comprehension macros. Each iterates its list once, so evaluation stays bounded by
/// the input size (no recursion, no ranges).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroKind {
    All,
    Exists,
    ExistsOne,
    Map,
    Filter,
}

impl MacroKind {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "all" => MacroKind::All,
            "exists" => MacroKind::Exists,
            "exists_one" => MacroKind::ExistsOne,
            "map" => MacroKind::Map,
            "filter" => MacroKind::Filter,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExprKind {
    Lit(Literal),
    Ident(String),
    Member {
        target: Box<Expr>,
        field: String,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    /// `name(args)` when `target` is `None`, otherwise the method call `target.name(args)`.
    /// `ret` is the resolved result type, filled in by the checker; aggregates use it to produce
    /// a correctly typed zero for an empty list.
    Call {
        target: Option<Box<Expr>>,
        name: String,
        name_span: Span,
        args: Vec<Expr>,
        ret: Option<crate::types::Type>,
    },
    /// `target.kind(var, body)`.
    Macro {
        kind: MacroKind,
        target: Box<Expr>,
        var: String,
        var_span: Span,
        body: Box<Expr>,
    },
    List(Vec<Expr>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Cond {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    /// Inserted by the type checker (never parsed) where an integer-typed value flows into a
    /// decimal-typed position, so every decimal-typed expression evaluates to a decimal and
    /// `/` means the same thing however the value was produced.
    Widen {
        inner: Box<Expr>,
        to: crate::types::Type,
    },
}

impl Expr {
    /// Dotted path for an identifier or a field chain (`initiator.manager`), used for
    /// null-narrowing. `None` for anything else.
    pub(crate) fn path(&self) -> Option<String> {
        match &self.kind {
            ExprKind::Ident(n) => Some(n.clone()),
            ExprKind::Member { target, field } => target.path().map(|p| format!("{p}.{field}")),
            _ => None,
        }
    }
}
