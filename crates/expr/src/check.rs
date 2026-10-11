//! Static type checker (docs/11 §6, §11 check 7). Also computes the variables an expression
//! reads and their classification, for the compiler and for redaction checks.

use crate::ast::{BinaryOp, Expr, ExprKind, Literal, MacroKind, UnaryOp};
use crate::builtins;
use crate::diag::{Code, Diagnostic, Span};
use crate::types::{Classification, Schema, Type};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where an expression is used. Event-facing contexts (notify templates, fields copied into
/// events) must never read `restricted` variables (docs/11 §5 and §11 check 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExprContext {
    #[default]
    General,
    EventFacing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// The type the context needs, e.g. `Type::Bool` for a decision branch condition.
    pub expected: Option<Type>,
    pub context: ExprContext,
}

impl CheckOptions {
    pub fn expecting(ty: Type) -> Self {
        CheckOptions { expected: Some(ty), context: ExprContext::General }
    }

    /// A branch condition: boolean and never null.
    pub fn condition() -> Self {
        Self::expecting(Type::Bool)
    }
}

/// A parsed, type-checked expression: the form the compiler stores (docs/04 §4.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckedExpr {
    pub source: String,
    /// The AST with checker-inserted widenings; evaluate this.
    pub ast: Expr,
    pub ty: Type,
    /// Root variables read (not comprehension locals), with their classification.
    pub reads: BTreeMap<String, Classification>,
}

impl CheckedExpr {
    /// Highest classification among the variables read (`Public` when it reads none). A result
    /// derived from restricted inputs must be treated as restricted.
    pub fn classification(&self) -> Classification {
        self.reads.values().copied().max().unwrap_or(Classification::Public)
    }
}

/// Type-check a parsed expression against `schema`.
pub fn check(
    source: &str,
    mut ast: Expr,
    schema: &Schema,
    opts: &CheckOptions,
) -> Result<CheckedExpr, Vec<Diagnostic>> {
    let mut c = Checker { schema, diags: Vec::new(), reads: BTreeMap::new(), restricted: Vec::new() };
    let mut ty = c.check(&mut ast, &Scope::default());
    if let (Some(t), Some(expected)) = (ty.clone(), &opts.expected) {
        if expected == &Type::Bool && t.is_nullable() && t.non_null() == &Type::Bool {
            c.err(
                Code::NullableCondition,
                ast.span,
                "condition may be null; compare with `== true` or guard it",
            );
        } else if !t.is_subtype(expected) {
            c.err(Code::ExpectedType, ast.span, format!("expected {expected}, found {t}"));
        } else if needs_widen(&t, expected) {
            widen_if_needed(&mut ast, &t, expected);
            ty = Some(expected.clone());
        }
    }
    if opts.context == ExprContext::EventFacing {
        for (name, span) in std::mem::take(&mut c.restricted) {
            c.err(
                Code::RestrictedInContext,
                span,
                format!(
                    "`{name}` is restricted and cannot be used where values reach events or notifications"
                ),
            );
        }
    }
    match ty {
        Some(ty) if c.diags.is_empty() => {
            Ok(CheckedExpr { source: source.to_string(), ast, ty, reads: c.reads })
        }
        _ => {
            c.diags.sort_by_key(|d| (d.span.start, d.span.end));
            c.diags.dedup();
            Err(c.diags)
        }
    }
}

#[derive(Clone, Default)]
struct Scope {
    locals: Vec<(String, Type)>,
    /// Paths proven non-null on this branch (`x != null ? <here> : …`).
    nonnull: BTreeSet<String>,
}

impl Scope {
    fn with_facts(&self, facts: BTreeSet<String>) -> Scope {
        let mut s = self.clone();
        s.nonnull.extend(facts);
        s
    }

    fn bind(&self, name: &str, ty: Type) -> Scope {
        let mut s = self.clone();
        // A new local hides any narrowing of an outer variable of the same name.
        s.nonnull.retain(|p| p != name && !p.starts_with(&format!("{name}.")));
        s.locals.push((name.to_string(), ty));
        s
    }

    fn narrow(&self, e: &Expr, ty: Type) -> Type {
        match e.path() {
            Some(p) if self.nonnull.contains(&p) => ty.non_null().clone(),
            _ => ty,
        }
    }
}

struct Checker<'s> {
    schema: &'s Schema,
    diags: Vec<Diagnostic>,
    reads: BTreeMap<String, Classification>,
    restricted: Vec<(String, Span)>,
}

/// Paths known non-null when `e` evaluates to `want`.
fn facts(e: &Expr, want: bool) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    match &e.kind {
        ExprKind::Binary { op: op @ (BinaryOp::Ne | BinaryOp::Eq), lhs, rhs } => {
            let null = |x: &Expr| matches!(x.kind, ExprKind::Lit(Literal::Null));
            let other = if null(rhs) {
                Some(lhs)
            } else if null(lhs) {
                Some(rhs)
            } else {
                None
            };
            if let Some(p) = other.and_then(|o| o.path())
                && (*op == BinaryOp::Ne) == want
            {
                out.insert(p);
            }
        }
        ExprKind::Binary { op: BinaryOp::And, lhs, rhs } if want => {
            out.extend(facts(lhs, true));
            out.extend(facts(rhs, true));
        }
        ExprKind::Binary { op: BinaryOp::Or, lhs, rhs } if !want => {
            out.extend(facts(lhs, false));
            out.extend(facts(rhs, false));
        }
        ExprKind::Unary { op: UnaryOp::Not, operand } => out.extend(facts(operand, !want)),
        _ => {}
    }
    out
}

pub(crate) fn needs_widen(from: &Type, to: &Type) -> bool {
    match (from, to) {
        (Type::Int, Type::Decimal) => true,
        (Type::Optional(a), b) => needs_widen(a, b.non_null()),
        (a, Type::Optional(b)) => needs_widen(a, b),
        (Type::List(a), Type::List(b)) => needs_widen(a, b),
        (Type::Record(a), Type::Record(b)) => {
            a.iter().any(|(k, t)| b.get(k).is_some_and(|u| needs_widen(t, u)))
        }
        _ => false,
    }
}

fn widen_if_needed(e: &mut Expr, from: &Type, to: &Type) {
    if needs_widen(from, to) {
        let placeholder = Expr { kind: ExprKind::Lit(Literal::Null), span: e.span, depth: 1 };
        let inner = std::mem::replace(e, placeholder);
        let (span, depth) = (inner.span, inner.depth + 1);
        *e = Expr { kind: ExprKind::Widen { inner: Box::new(inner), to: to.clone() }, span, depth };
    }
}

fn ordered(t: &Type) -> bool {
    matches!(
        t,
        Type::Int
            | Type::Decimal
            | Type::String
            | Type::Date
            | Type::DateTime
            | Type::Duration
            | Type::Money(_)
    )
}

impl Checker<'_> {
    fn err(&mut self, code: Code, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic::new(code, span, msg));
    }

    /// A non-null operand, or a diagnostic.
    fn require_value(&mut self, t: Type, span: Span, what: &str) -> Option<Type> {
        if t.is_nullable() {
            self.err(
                Code::Nullable,
                span,
                format!("{what} may be null; guard it first, e.g. `x != null ? … : …`"),
            );
            None
        } else {
            Some(t)
        }
    }

    fn check(&mut self, e: &mut Expr, scope: &Scope) -> Option<Type> {
        let span = e.span;
        match &mut e.kind {
            ExprKind::Lit(l) => Some(match l {
                Literal::Null => Type::Null,
                Literal::Bool(_) => Type::Bool,
                Literal::Int(_) => Type::Int,
                Literal::Decimal(_) => Type::Decimal,
                Literal::String(_) => Type::String,
            }),
            ExprKind::Ident(name) => {
                let name = name.clone();
                let ty = if let Some((_, t)) = scope.locals.iter().rev().find(|(n, _)| *n == name) {
                    t.clone()
                } else if let Some(decl) = self.schema.get(&name) {
                    self.reads.insert(name.clone(), decl.classification);
                    if decl.classification == Classification::Restricted {
                        self.restricted.push((name.clone(), span));
                    }
                    decl.ty.clone()
                } else {
                    self.err(Code::UnknownVariable, span, format!("unknown variable `{name}`"));
                    return None;
                };
                Some(scope.narrow(e, ty))
            }
            ExprKind::Member { target, field } => {
                let field = field.clone();
                let t = self.check(target, scope)?;
                let Some(fields) = t.non_null().fields() else {
                    self.err(Code::UnknownField, span, format!("{} has no fields", t.non_null()));
                    return None;
                };
                let Some(ft) = fields.get(&field).cloned() else {
                    let known: Vec<&str> = fields.keys().map(String::as_str).collect();
                    self.err(
                        Code::UnknownField,
                        span,
                        format!("{} has no field `{field}` (fields: {})", t.non_null(), known.join(", ")),
                    );
                    return None;
                };
                // Null-safe navigation: a field of a value that may be null may be null.
                let ft = if t.is_nullable() { Type::optional(ft) } else { ft };
                Some(scope.narrow(e, ft))
            }
            ExprKind::Index { target, index } => {
                let t = self.check(target, scope);
                let i = self.check(index, scope);
                let (t, i) = (t?, i?);
                let t = self.require_value(t, target.span, "the list")?;
                let i = self.require_value(i, index.span, "the index")?;
                let Type::List(elem) = t else {
                    self.err(Code::OperatorType, target.span, format!("only lists can be indexed, not {t}"));
                    return None;
                };
                if i != Type::Int {
                    self.err(
                        Code::OperatorType,
                        index.span,
                        format!("list index must be an integer, not {i}"),
                    );
                    return None;
                }
                Some(*elem)
            }
            ExprKind::Unary { op, operand } => {
                let op = *op;
                let t = self.check(operand, scope)?;
                let t = self.require_value(t, operand.span, "the operand")?;
                match (op, &t) {
                    (UnaryOp::Not, Type::Bool) => Some(Type::Bool),
                    (UnaryOp::Neg, Type::Int | Type::Decimal | Type::Money(_) | Type::Duration) => Some(t),
                    _ => {
                        let sym = if op == UnaryOp::Not { "!" } else { "-" };
                        self.err(Code::OperatorType, span, format!("`{sym}` does not apply to {t}"));
                        None
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let op = *op;
                self.binary(op, lhs, rhs, span, scope)
            }
            ExprKind::Cond { cond, then, otherwise } => {
                let ct = self.check(cond, scope);
                let then_scope = scope.with_facts(facts(cond, true));
                let else_scope = scope.with_facts(facts(cond, false));
                let tt = self.check(then, &then_scope);
                let et = self.check(otherwise, &else_scope);
                let ct = ct?;
                if ct.is_nullable() && ct.non_null() == &Type::Bool {
                    self.err(Code::NullableCondition, cond.span, "condition may be null");
                    return None;
                }
                if ct != Type::Bool {
                    self.err(Code::OperatorType, cond.span, format!("condition must be boolean, not {ct}"));
                    return None;
                }
                let (tt, et) = (tt?, et?);
                let Some(j) = tt.join(&et) else {
                    self.err(
                        Code::OperatorType,
                        span,
                        format!("branches have different types: {tt} and {et}"),
                    );
                    return None;
                };
                widen_if_needed(then, &tt, &j);
                widen_if_needed(otherwise, &et, &j);
                Some(j)
            }
            ExprKind::List(items) => {
                let mut types = Vec::with_capacity(items.len());
                for it in items.iter_mut() {
                    types.push(self.check(it, scope));
                }
                let types: Vec<Type> = types.into_iter().collect::<Option<_>>()?;
                let mut acc = Type::Never;
                for (t, it) in types.iter().zip(items.iter()) {
                    match acc.join(t) {
                        Some(j) => acc = j,
                        None => {
                            self.err(Code::MixedList, it.span, format!("list mixes {acc} and {t}"));
                            return None;
                        }
                    }
                }
                for (t, it) in types.iter().zip(items.iter_mut()) {
                    widen_if_needed(it, t, &acc);
                }
                Some(Type::list(acc))
            }
            ExprKind::Macro { kind, target, var, var_span: _, body } => {
                let kind = *kind;
                let var = var.clone();
                let t = self.check(target, scope)?;
                let t = self.require_value(t, target.span, "the list")?;
                let Type::List(elem) = t else {
                    self.err(Code::MacroShape, target.span, format!("comprehensions need a list, not {t}"));
                    return None;
                };
                let bt = self.check(body, &scope.bind(&var, (*elem).clone()))?;
                match kind {
                    MacroKind::Map => Some(Type::list(bt)),
                    _ if bt.is_nullable() && bt.non_null() == &Type::Bool => {
                        self.err(Code::NullableCondition, body.span, "predicate may be null");
                        None
                    }
                    _ if bt != Type::Bool => {
                        self.err(
                            Code::OperatorType,
                            body.span,
                            format!("predicate must be boolean, not {bt}"),
                        );
                        None
                    }
                    MacroKind::Filter => Some(Type::List(elem)),
                    _ => Some(Type::Bool),
                }
            }
            ExprKind::Call { target, name, name_span, args, ret } => {
                let name = name.clone();
                let name_span = *name_span;
                let recv = match target {
                    Some(t) => Some((self.check(t, scope), t.span)),
                    None => None,
                };
                let mut arg_types = Vec::with_capacity(args.len());
                for a in args.iter_mut() {
                    arg_types.push(self.check(a, scope));
                }
                let recv = match recv {
                    Some((t, s)) => Some((self.require_value(t?, s, "the receiver")?, s)),
                    None => None,
                };
                let mut tys = Vec::with_capacity(args.len());
                for (t, a) in arg_types.into_iter().zip(args.iter()) {
                    tys.push(self.require_value(t?, a.span, "the argument")?);
                }
                let call = builtins::CallSite {
                    name: &name,
                    name_span,
                    span,
                    receiver: recv,
                    args: &tys,
                    arg_exprs: args.as_slice(),
                };
                match builtins::signature(&call) {
                    Ok(t) => {
                        *ret = Some(t.clone());
                        Some(t)
                    }
                    Err(d) => {
                        self.diags.push(d);
                        None
                    }
                }
            }
            ExprKind::Widen { inner, to } => {
                let to = to.clone();
                self.check(inner, scope)?;
                Some(to)
            }
        }
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &mut Expr,
        rhs: &mut Expr,
        span: Span,
        scope: &Scope,
    ) -> Option<Type> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let lt = self.check(lhs, scope);
            let rscope = scope.with_facts(facts(lhs, op == BinaryOp::And));
            let rt = self.check(rhs, &rscope);
            let (lt, rt) = (lt?, rt?);
            for (t, e) in [(lt, &*lhs), (rt, &*rhs)] {
                if t.is_nullable() && t.non_null() == &Type::Bool {
                    self.err(Code::NullableCondition, e.span, "operand may be null");
                    return None;
                }
                if t != Type::Bool {
                    self.err(
                        Code::OperatorType,
                        e.span,
                        format!("`{}` needs booleans, not {t}", op.symbol()),
                    );
                    return None;
                }
            }
            return Some(Type::Bool);
        }
        let lt = self.check(lhs, scope);
        let rt = self.check(rhs, scope);
        let (lt, rt) = (lt?, rt?);
        match op {
            BinaryOp::Eq | BinaryOp::Ne => {
                self.enum_literal(&lt, rhs);
                self.enum_literal(&rt, lhs);
                if lt.join(&rt).is_none() {
                    self.mismatch(op, &lt, &rt, span);
                    return None;
                }
                Some(Type::Bool)
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let lt = self.require_value(lt, lhs.span, "the left operand")?;
                let rt = self.require_value(rt, rhs.span, "the right operand")?;
                match lt.join(&rt) {
                    Some(j) if ordered(&j) => Some(Type::Bool),
                    Some(j) => {
                        self.err(Code::NotComparable, span, format!("{j} values cannot be ordered"));
                        None
                    }
                    None => {
                        self.mismatch(op, &lt, &rt, span);
                        None
                    }
                }
            }
            BinaryOp::In => {
                let rt = self.require_value(rt, rhs.span, "the list")?;
                let Type::List(elem) = &rt else {
                    self.err(
                        Code::OperatorType,
                        rhs.span,
                        format!("`in` needs a list on the right, not {rt}"),
                    );
                    return None;
                };
                if let ExprKind::List(items) = &rhs.kind {
                    for it in items {
                        self.enum_literal(&lt, it);
                    }
                }
                if lt.join(elem).is_none() {
                    self.mismatch(op, &lt, &rt, span);
                    return None;
                }
                Some(Type::Bool)
            }
            _ => {
                let lt = self.require_value(lt, lhs.span, "the left operand")?;
                let rt = self.require_value(rt, rhs.span, "the right operand")?;
                match arith(op, &lt, &rt) {
                    Ok(t) => {
                        if matches!(t, Type::List(_)) {
                            widen_if_needed(lhs, &lt, &t);
                            widen_if_needed(rhs, &rt, &t);
                        }
                        Some(t)
                    }
                    Err(()) => {
                        self.mismatch(op, &lt, &rt, span);
                        None
                    }
                }
            }
        }
    }

    fn mismatch(&mut self, op: BinaryOp, lt: &Type, rt: &Type, span: Span) {
        if let (Type::Money(a), Type::Money(b)) = (lt.non_null(), rt.non_null())
            && a != b
        {
            self.err(Code::CurrencyMismatch, span, format!("cannot combine money in {a} with money in {b}"));
            return;
        }
        self.err(Code::OperatorType, span, format!("`{}` does not apply to {lt} and {rt}", op.symbol()));
    }

    /// `severity == "majr"`: a string literal compared with an enum must be one of its values.
    fn enum_literal(&mut self, ty: &Type, other: &Expr) {
        if let (Type::Enum(vals), ExprKind::Lit(Literal::String(s))) = (ty.non_null(), &other.kind)
            && !vals.contains(s)
        {
            self.err(
                Code::UnknownEnumValue,
                other.span,
                format!("\"{s}\" is not one of {}", vals.join(", ")),
            );
        }
    }
}

/// Result type of an arithmetic operator on non-null operands. Mirrored by `eval::arith`.
pub(crate) fn arith(op: BinaryOp, l: &Type, r: &Type) -> Result<Type, ()> {
    use Type::*;
    let num = |t: &Type| t.is_numeric();
    Ok(match (op, l, r) {
        (_, Int, Int)
            if matches!(
                op,
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
            ) =>
        {
            Int
        }
        (BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem, a, b)
            if num(a) && num(b) =>
        {
            Decimal
        }
        (BinaryOp::Add, a, b) if a.is_stringy() && b.is_stringy() => String,
        (BinaryOp::Add, List(a), List(b)) => List(Box::new(a.join(b).ok_or(())?)),
        (BinaryOp::Add | BinaryOp::Sub, Money(a), Money(b)) if a == b => Money(a.clone()),
        (BinaryOp::Add, DateTime, Duration) | (BinaryOp::Add, Duration, DateTime) => DateTime,
        (BinaryOp::Add, Date, Duration) | (BinaryOp::Add, Duration, Date) => Date,
        (BinaryOp::Add | BinaryOp::Sub, Duration, Duration) => Duration,
        (BinaryOp::Sub, DateTime, Duration) => DateTime,
        (BinaryOp::Sub, DateTime, DateTime) => Duration,
        (BinaryOp::Sub, Date, Date) => Duration,
        (BinaryOp::Sub, Date, Duration) => Date,
        (BinaryOp::Mul, Money(c), n) | (BinaryOp::Mul, n, Money(c)) if num(n) => Money(c.clone()),
        (BinaryOp::Div, Money(c), n) if num(n) => Money(c.clone()),
        (BinaryOp::Div, Money(a), Money(b)) if a == b => Decimal,
        (BinaryOp::Mul, Duration, Int) | (BinaryOp::Mul, Int, Duration) => Duration,
        _ => return Err(()),
    })
}
