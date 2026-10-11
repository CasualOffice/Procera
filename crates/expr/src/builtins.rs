//! The Procera standard library: signatures for the checker and implementations for the
//! evaluator, side by side so they cannot drift apart. All functions are pure and bounded.

use crate::ast::{BinaryOp, Expr, ExprKind, Literal};
use crate::diag::{Code, Diagnostic, Span};
use crate::eval::{EvalEnv, EvalError, Meter, arith, compare, num, type_err, widen};
use crate::types::Type;
use crate::value::{self, Value};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use std::cmp::Ordering;
use std::sync::Arc;
use time::{Duration, UtcOffset};

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

fn invalid(function: &str, message: impl Into<String>) -> EvalError {
    EvalError::InvalidArgument { function: function.to_string(), message: message.into() }
}

fn zero_of(t: Option<&Type>) -> Result<Value, EvalError> {
    Ok(match t {
        Some(Type::Int) => Value::Int(0),
        Some(Type::Decimal) => Value::Decimal(Decimal::ZERO),
        Some(Type::Money(c)) => Value::Money { amount: Decimal::ZERO, currency: Arc::from(c.as_str()) },
        Some(Type::Duration) => Value::Duration(Duration::ZERO),
        _ => return Err(type_err("sum result type")),
    })
}

fn round_dec(d: Decimal, places: i64, name: &str) -> Result<Decimal, EvalError> {
    let p = u32::try_from(places)
        .ok()
        .filter(|p| *p <= 28)
        .ok_or_else(|| invalid(name, "places must be 0 to 28"))?;
    Ok(d.round_dp_with_strategy(p, RoundingStrategy::MidpointAwayFromZero))
}

fn local(env: &EvalEnv, t: time::OffsetDateTime) -> time::OffsetDateTime {
    t.to_offset(env.tz)
}

/// Run a built-in on already-evaluated, non-null arguments.
pub(crate) fn call(
    m: &mut Meter<'_>,
    env: &EvalEnv,
    name: &str,
    recv: Option<Value>,
    args: Vec<Value>,
    ret: Option<&Type>,
) -> Result<Value, EvalError> {
    use Value as V;
    let out = match (name, recv.as_ref(), args.as_slice()) {
        ("size", None, [v]) | ("size", Some(v), []) => match v {
            V::List(items) => V::Int(items.len() as i64),
            V::String(s) => {
                m.step(s.len() as u64 / 64)?;
                V::Int(s.chars().count() as i64)
            }
            _ => return Err(type_err("size")),
        },
        ("string", None, [v]) => {
            let s = match v {
                V::Bool(b) => b.to_string(),
                V::Int(i) => i.to_string(),
                V::Decimal(d) => d.to_string(),
                V::String(s) => return Ok(V::String(s.clone())),
                V::Date(d) => value::format_date(*d),
                V::DateTime(t) => value::format_datetime(*t),
                V::Duration(d) => value::format_duration(*d),
                V::Money { amount, currency } => format!("{amount} {currency}"),
                _ => return Err(type_err("string")),
            };
            m.string(s)?
        }
        ("int", None, [v]) => match v {
            V::Int(i) => V::Int(*i),
            V::Decimal(d) => V::Int(d.trunc().to_i64().ok_or(EvalError::Overflow)?),
            V::String(s) => {
                V::Int(s.trim().parse().map_err(|_| invalid("int", format!("`{s}` is not an integer")))?)
            }
            _ => return Err(type_err("int")),
        },
        ("decimal", None, [v]) => match v {
            V::Int(_) | V::Decimal(_) => V::Decimal(num(v).ok_or_else(|| type_err("decimal"))?),
            V::String(s) => V::Decimal(
                Decimal::from_str_exact(s.trim())
                    .map_err(|_| invalid("decimal", format!("`{s}` is not a decimal")))?,
            ),
            _ => return Err(type_err("decimal")),
        },
        ("money", None, [n, V::String(cur)]) => {
            V::Money { amount: num(n).ok_or_else(|| type_err("money"))?, currency: cur.clone() }
        }
        ("date", None, [V::String(s)]) => V::Date(value::parse_date(s).map_err(|e| invalid("date", e))?),
        ("date", None, [V::DateTime(t)]) => V::Date(local(env, *t).date()),
        ("datetime", None, [V::String(s)]) => {
            V::DateTime(value::parse_datetime(s).map_err(|e| invalid("datetime", e))?)
        }
        ("datetime", None, [V::Date(d)]) => {
            V::DateTime(d.midnight().assume_offset(env.tz).to_offset(UtcOffset::UTC))
        }
        ("duration", None, [V::String(s)]) => {
            V::Duration(value::parse_duration(s).map_err(|e| invalid("duration", e))?)
        }
        ("now", None, []) => V::DateTime(env.now),
        ("today", None, []) => V::Date(local(env, env.now).date()),
        ("abs", None, [v]) => match v {
            V::Int(i) => V::Int(i.checked_abs().ok_or(EvalError::Overflow)?),
            V::Decimal(d) => V::Decimal(d.abs()),
            V::Money { amount, currency } => V::Money { amount: amount.abs(), currency: currency.clone() },
            V::Duration(d) if d.is_negative() => V::Duration(d.checked_neg().ok_or(EvalError::Overflow)?),
            V::Duration(d) => V::Duration(*d),
            _ => return Err(type_err("abs")),
        },
        (f @ ("round" | "floor" | "ceil"), None, [v, rest @ ..]) => {
            let places = match rest {
                [] => 0,
                [V::Int(p)] => *p,
                _ => return Err(type_err("round places")),
            };
            let apply = |d: Decimal| -> Result<Decimal, EvalError> {
                match f {
                    "round" => round_dec(d, places, f),
                    "floor" => Ok(d.floor()),
                    _ => Ok(d.ceil()),
                }
            };
            match v {
                V::Int(i) if f != "round" => V::Int(*i),
                V::Int(_) | V::Decimal(_) => V::Decimal(apply(num(v).ok_or_else(|| type_err(f))?)?),
                V::Money { amount, currency } => {
                    V::Money { amount: apply(*amount)?, currency: currency.clone() }
                }
                _ => return Err(type_err(f)),
            }
        }
        ("sum", None, [V::List(items)]) => {
            let mut acc = zero_of(ret)?;
            for it in items.iter() {
                m.step(1)?;
                acc = arith(m, BinaryOp::Add, acc, it.clone())?;
            }
            acc
        }
        ("avg", None, [V::List(items)]) => {
            if items.is_empty() {
                return Err(EvalError::EmptyList { function: "avg".into() });
            }
            let mut acc = Decimal::ZERO;
            for it in items.iter() {
                m.step(1)?;
                acc = acc.checked_add(num(it).ok_or_else(|| type_err("avg"))?).ok_or(EvalError::Overflow)?;
            }
            V::Decimal(acc.checked_div(Decimal::from(items.len())).ok_or(EvalError::Overflow)?)
        }
        (f @ ("min" | "max"), None, args) => {
            let items: Vec<Value> = match args {
                [V::List(items)] => items.iter().cloned().collect(),
                [a, b] => vec![a.clone(), b.clone()],
                _ => return Err(type_err(f)),
            };
            let mut best: Option<Value> = None;
            for it in items {
                m.step(1)?;
                best = Some(match best {
                    None => it,
                    Some(b) => {
                        let ord = compare(&it, &b)?;
                        let better =
                            if f == "min" { ord == Ordering::Less } else { ord == Ordering::Greater };
                        if better { it } else { b }
                    }
                });
            }
            let v = best.ok_or_else(|| EvalError::EmptyList { function: f.to_string() })?;
            match ret {
                Some(t) => widen(v, t),
                None => v,
            }
        }
        (f, Some(V::String(s)), args) => string_method(m, f, s, args)?,
        ("join", Some(V::List(items)), [V::String(sep)]) => {
            let mut parts = Vec::with_capacity(items.len());
            for it in items.iter() {
                match it {
                    V::String(p) => parts.push(p.clone()),
                    _ => return Err(type_err("join")),
                }
            }
            let total =
                parts.iter().map(|p| p.len()).sum::<usize>() + sep.len() * parts.len().saturating_sub(1);
            m.reserve_string(total)?;
            V::String(Arc::from(parts.iter().map(|p| &**p).collect::<Vec<&str>>().join(sep)))
        }
        (f, Some(V::Date(d)), []) => date_part(f, d, None)?,
        (f, Some(V::DateTime(t)), []) => {
            let l = local(env, *t);
            date_part(f, &l.date(), Some(l))?
        }
        (f, Some(V::Duration(d)), []) => V::Int(match f {
            "inDays" => d.whole_days(),
            "inHours" => d.whole_hours(),
            "inMinutes" => d.whole_minutes(),
            "inSeconds" => d.whole_seconds(),
            _ => return Err(type_err(f)),
        }),
        _ => return Err(type_err(name)),
    };
    Ok(out)
}

fn date_part(f: &str, d: &time::Date, t: Option<time::OffsetDateTime>) -> Result<Value, EvalError> {
    Ok(Value::Int(match (f, t) {
        ("year", _) => i64::from(d.year()),
        ("month", _) => i64::from(u8::from(d.month())),
        ("day", _) => i64::from(d.day()),
        ("weekday", _) => i64::from(d.weekday().number_from_monday()),
        ("hour", Some(t)) => i64::from(t.hour()),
        ("minute", Some(t)) => i64::from(t.minute()),
        _ => return Err(type_err(f)),
    }))
}

fn string_method(m: &mut Meter<'_>, f: &str, s: &str, args: &[Value]) -> Result<Value, EvalError> {
    use Value as V;
    let arg = |i: usize| match args.get(i) {
        Some(V::String(a)) => Ok(a.clone()),
        _ => Err(type_err(f)),
    };
    m.step(s.len() as u64 / 64)?;
    Ok(match (f, args.len()) {
        ("contains", 1) => V::Bool(s.contains(&*arg(0)?)),
        ("startsWith", 1) => V::Bool(s.starts_with(&*arg(0)?)),
        ("endsWith", 1) => V::Bool(s.ends_with(&*arg(0)?)),
        ("lower", 0) => m.string(s.to_lowercase())?,
        ("upper", 0) => m.string(s.to_uppercase())?,
        ("trim", 0) => m.string(s.trim().to_string())?,
        ("replace", 2) => {
            let (old, new) = (arg(0)?, arg(1)?);
            if old.is_empty() {
                return Err(invalid("replace", "the text to replace must not be empty"));
            }
            let count = s.matches(&*old).count();
            // Size the result before building it, so a replacement can't blow up memory.
            let len = (s.len() - count * old.len()).saturating_add(count.saturating_mul(new.len()));
            m.reserve_string(len)?;
            V::String(Arc::from(s.replace(&*old, &new)))
        }
        ("split", 1) => {
            let sep = arg(0)?;
            if sep.is_empty() {
                return Err(invalid("split", "the separator must not be empty"));
            }
            m.reserve_list(s.matches(&*sep).count() + 1)?;
            m.reserve_string(s.len())?;
            V::list(s.split(&*sep).map(V::string).collect())
        }
        ("substring", 1 | 2) => {
            let idx = |i: usize| match args.get(i) {
                Some(V::Int(n)) => Ok(*n),
                _ => Err(type_err("substring")),
            };
            let len = s.chars().count();
            let start = idx(0)?;
            let end = if args.len() == 2 { idx(1)? } else { len as i64 };
            let bad = || {
                invalid("substring", format!("range {start}..{end} is outside a string of {len} characters"))
            };
            let (su, eu) =
                (usize::try_from(start).map_err(|_| bad())?, usize::try_from(end).map_err(|_| bad())?);
            if su > eu || eu > len {
                return Err(bad());
            }
            m.string(s.chars().skip(su).take(eu - su).collect())?
        }
        _ => return Err(type_err(f)),
    })
}
