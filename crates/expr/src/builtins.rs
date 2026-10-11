//! The Procera standard library: signatures for the checker and implementations for the
//! evaluator, side by side so they cannot drift apart. All functions are pure and bounded.

use crate::ast::{Expr, ExprKind, Literal};
use crate::diag::{Code, Diagnostic, Span};
use crate::types::Type;
use crate::value;

/// Methods (called as `value.name(…)`), listed so a global call can suggest the method form.
const METHODS: &[&str] = &[
    "contains",
    "startsWith",
    "endsWith",
    "lower",
    "upper",
    "trim",
    "replace",
    "split",
    "substring",
    "join",
    "year",
    "month",
    "day",
    "weekday",
    "hour",
    "minute",
    "inDays",
    "inHours",
    "inMinutes",
    "inSeconds",
    "all",
    "exists",
    "exists_one",
    "map",
    "filter",
];

pub(crate) struct CallSite<'a> {
    pub name: &'a str,
    pub name_span: Span,
    pub span: Span,
    pub receiver: Option<(Type, Span)>,
    /// Argument types, all non-null (the checker rejects nullable arguments first).
    pub args: &'a [Type],
    pub arg_exprs: &'a [Expr],
}

impl CallSite<'_> {
    fn arity(&self, allowed: &[usize]) -> Result<(), Diagnostic> {
        if allowed.contains(&self.args.len()) {
            return Ok(());
        }
        let want = allowed.iter().map(ToString::to_string).collect::<Vec<_>>().join(" or ");
        Err(Diagnostic::new(
            Code::WrongArity,
            self.span,
            format!("`{}` takes {want} argument(s), got {}", self.name, self.args.len()),
        ))
    }

    fn bad_arg(&self, i: usize, expected: &str) -> Diagnostic {
        let span = self.arg_exprs.get(i).map_or(self.span, |e| e.span);
        let got = self.args.get(i).map_or_else(|| "nothing".to_string(), ToString::to_string);
        Diagnostic::new(
            Code::ArgumentType,
            span,
            format!("argument {} of `{}` must be {expected}, not {got}", i + 1, self.name),
        )
    }

    fn lit_str(&self, i: usize) -> Option<(&str, Span)> {
        match self.arg_exprs.get(i).map(|e| (&e.kind, e.span)) {
            Some((ExprKind::Lit(Literal::String(s)), span)) => Some((s.as_str(), span)),
            _ => None,
        }
    }

    /// Validate a literal string argument at compile time.
    fn literal<T>(&self, i: usize, parse: impl Fn(&str) -> Result<T, String>) -> Result<(), Diagnostic> {
        if let Some((s, span)) = self.lit_str(i) {
            parse(s).map_err(|msg| Diagnostic::new(Code::InvalidLiteral, span, msg))?;
        }
        Ok(())
    }
}

fn scalar_for_string(t: &Type) -> bool {
    t.is_numeric()
        || t.is_stringy()
        || matches!(t, Type::Bool | Type::Date | Type::DateTime | Type::Duration | Type::Money(_))
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

/// Result type of a call, or a diagnostic.
pub(crate) fn signature(c: &CallSite<'_>) -> Result<Type, Diagnostic> {
    match &c.receiver {
        Some((rt, _)) => method_signature(c, rt),
        None => global_signature(c),
    }
}

fn global_signature(c: &CallSite<'_>) -> Result<Type, Diagnostic> {
    use Type::*;
    let a = c.args;
    let t = match c.name {
        "size" => {
            c.arity(&[1])?;
            match &a[0] {
                List(_) => Int,
                t if t.is_stringy() => Int,
                _ => return Err(c.bad_arg(0, "a string or list")),
            }
        }
        "string" => {
            c.arity(&[1])?;
            if !scalar_for_string(&a[0]) {
                return Err(c.bad_arg(0, "a scalar value"));
            }
            String
        }
        "int" | "decimal" => {
            c.arity(&[1])?;
            if !(a[0].is_stringy() || a[0].is_numeric()) {
                return Err(c.bad_arg(0, "a string or number"));
            }
            if c.name == "int" {
                c.literal(0, |s| {
                    s.trim().parse::<i64>().map(|_| ()).map_err(|_| format!("`{s}` is not an integer"))
                })?;
                Int
            } else {
                c.literal(0, |s| {
                    rust_decimal::Decimal::from_str_exact(s.trim())
                        .map(|_| ())
                        .map_err(|_| format!("`{s}` is not a decimal"))
                })?;
                Decimal
            }
        }
        "money" => {
            c.arity(&[2])?;
            if !a[0].is_numeric() {
                return Err(c.bad_arg(0, "a number"));
            }
            let Some((cur, span)) = c.lit_str(1) else {
                return Err(Diagnostic::new(
                    Code::InvalidLiteral,
                    c.arg_exprs.get(1).map_or(c.span, |e| e.span),
                    "the currency must be a literal ISO 4217 code, e.g. \"INR\"",
                ));
            };
            if cur.len() != 3 || !cur.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(Diagnostic::new(
                    Code::InvalidLiteral,
                    span,
                    format!("`{cur}` is not an ISO 4217 code"),
                ));
            }
            Money(cur.to_string())
        }
        "date" => {
            c.arity(&[1])?;
            match &a[0] {
                t if t.is_stringy() => c.literal(0, value::parse_date)?,
                DateTime => {}
                _ => return Err(c.bad_arg(0, "a string or datetime")),
            }
            Date
        }
        "datetime" => {
            c.arity(&[1])?;
            match &a[0] {
                t if t.is_stringy() => c.literal(0, value::parse_datetime)?,
                Date => {}
                _ => return Err(c.bad_arg(0, "a string or date")),
            }
            DateTime
        }
        "duration" => {
            c.arity(&[1])?;
            if !a[0].is_stringy() {
                return Err(c.bad_arg(0, "a string"));
            }
            c.literal(0, value::parse_duration)?;
            Duration
        }
        "now" => {
            c.arity(&[0])?;
            DateTime
        }
        "today" => {
            c.arity(&[0])?;
            Date
        }
        "abs" => {
            c.arity(&[1])?;
            match &a[0] {
                t @ (Int | Decimal | Money(_) | Duration) => t.clone(),
                _ => return Err(c.bad_arg(0, "a number, money or duration")),
            }
        }
        "round" | "floor" | "ceil" => {
            c.arity(if c.name == "round" { &[1, 2] } else { &[1] })?;
            if a.len() == 2 {
                if a[1] != Int {
                    return Err(c.bad_arg(1, "an integer number of places"));
                }
                if let Some(ExprKind::Lit(Literal::Int(p))) = c.arg_exprs.get(1).map(|e| &e.kind)
                    && !(0..=28).contains(p)
                {
                    return Err(Diagnostic::new(
                        Code::InvalidLiteral,
                        c.arg_exprs[1].span,
                        "places must be 0 to 28",
                    ));
                }
            }
            match (&a[0], c.name) {
                (Int, "round") => Decimal,
                (t @ (Int | Decimal | Money(_)), _) => t.clone(),
                _ => return Err(c.bad_arg(0, "a number or money")),
            }
        }
        "sum" => {
            c.arity(&[1])?;
            match &a[0] {
                List(e) => match &**e {
                    Never => Int,
                    t @ (Int | Decimal | Money(_) | Duration) => t.clone(),
                    _ => return Err(c.bad_arg(0, "a list of numbers, money or durations")),
                },
                _ => return Err(c.bad_arg(0, "a list")),
            }
        }
        "avg" => {
            c.arity(&[1])?;
            match &a[0] {
                List(e) if matches!(**e, Never | Int | Decimal) => Decimal,
                _ => return Err(c.bad_arg(0, "a list of numbers")),
            }
        }
        "min" | "max" => {
            c.arity(&[1, 2])?;
            if a.len() == 1 {
                match &a[0] {
                    List(e) if ordered(e) => (**e).clone(),
                    _ => return Err(c.bad_arg(0, "a list of comparable values")),
                }
            } else {
                match a[0].join(&a[1]) {
                    Some(j) if ordered(&j) => j,
                    _ => return Err(c.bad_arg(1, &format!("comparable with {}", a[0]))),
                }
            }
        }
        n if METHODS.contains(&n) => {
            return Err(Diagnostic::new(
                Code::UnknownFunction,
                c.name_span,
                format!("`{n}` is a method: write `value.{n}(…)`"),
            ));
        }
        n => {
            return Err(Diagnostic::new(
                Code::UnknownFunction,
                c.name_span,
                format!("unknown function `{n}`"),
            ));
        }
    };
    Ok(t)
}

fn method_signature(c: &CallSite<'_>, rt: &Type) -> Result<Type, Diagnostic> {
    use Type::*;
    let a = c.args;
    let stringy_args = |n: usize| -> Result<(), Diagnostic> {
        c.arity(&[n])?;
        match a.iter().position(|t| !t.is_stringy()) {
            Some(i) => Err(c.bad_arg(i, "a string")),
            None => Ok(()),
        }
    };
    let t = match (rt, c.name) {
        (List(_), "size") => {
            c.arity(&[0])?;
            Int
        }
        (s, "size") if s.is_stringy() => {
            c.arity(&[0])?;
            Int
        }
        (s, "contains" | "startsWith" | "endsWith") if s.is_stringy() => {
            stringy_args(1)?;
            Bool
        }
        (s, "lower" | "upper" | "trim") if s.is_stringy() => {
            c.arity(&[0])?;
            String
        }
        (s, "replace") if s.is_stringy() => {
            stringy_args(2)?;
            String
        }
        (s, "split") if s.is_stringy() => {
            stringy_args(1)?;
            Type::list(String)
        }
        (s, "substring") if s.is_stringy() => {
            c.arity(&[1, 2])?;
            if let Some(i) = a.iter().position(|t| *t != Int) {
                return Err(c.bad_arg(i, "an integer"));
            }
            String
        }
        (List(e), "join") if e.is_stringy() || **e == Never => {
            stringy_args(1)?;
            String
        }
        (Date | DateTime, "year" | "month" | "day" | "weekday") | (DateTime, "hour" | "minute") => {
            c.arity(&[0])?;
            Int
        }
        (Duration, "inDays" | "inHours" | "inMinutes" | "inSeconds") => {
            c.arity(&[0])?;
            Int
        }
        (rt, n) => {
            return Err(Diagnostic::new(
                Code::UnknownFunction,
                c.name_span,
                format!("{rt} has no method `{n}`"),
            ));
        }
    };
    Ok(t)
}
