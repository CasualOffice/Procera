//! Budgeted, pure evaluator. No I/O, no clock (`now()` comes from [`EvalEnv`]), no randomness;
//! every loop is bounded by the size of a list and charged to the step budget.

use crate::ast::{BinaryOp, Expr, ExprKind, Literal, MacroKind, UnaryOp};
use crate::builtins;
use crate::check::CheckedExpr;
use crate::types::{Schema, Type};
use crate::value::Value;
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use time::{OffsetDateTime, UtcOffset};

/// Evaluation limits (docs/04 §4.5). Exceeding any of them is a typed [`EvalError`] that the
/// runtime raises as incident `IR-R010`; it is never a panic or a hang.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Evaluation steps: one per AST node visited plus one per element or 64 bytes processed.
    pub max_steps: u64,
    /// Bytes of intermediate strings and lists allocated, cumulative.
    pub max_alloc_bytes: u64,
    /// Longest string an expression may build, in bytes (docs/11 §5: 64 KB per value).
    pub max_string_len: usize,
    /// Longest list an expression may build.
    pub max_list_len: usize,
    /// Evaluator recursion depth (parsed ASTs are already capped at 32 levels).
    pub max_depth: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            max_steps: 10_000,
            max_alloc_bytes: 1 << 20,
            max_string_len: 64 * 1024,
            max_list_len: 10_000,
            max_depth: 64,
        }
    }
}

/// Everything an evaluation may depend on besides the variables. Injected so evaluation is
/// reproducible: `now` is the triggering event's recorded timestamp (docs/11 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalEnv {
    pub now: OffsetDateTime,
    /// Offset used for `today()` and date parts of datetimes (the process calendar's offset at
    /// `now`, resolved by the caller).
    pub tz: UtcOffset,
}

impl EvalEnv {
    pub fn new(now: OffsetDateTime, tz: UtcOffset) -> Self {
        EvalEnv { now: now.to_offset(UtcOffset::UTC), tz }
    }

    pub fn utc(now: OffsetDateTime) -> Self {
        Self::new(now, UtcOffset::UTC)
    }
}

/// Runtime failure. Budget variants are incidents `IR-R010`; the rest are data problems
/// (division by zero, overflow…) also surfaced as incidents with their code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    StepLimit,
    AllocLimit,
    StringTooLong,
    ListTooLong,
    DepthLimit,
    DivisionByZero,
    Overflow,
    IndexOutOfBounds {
        index: i64,
        len: usize,
    },
    InvalidArgument {
        function: String,
        message: String,
    },
    EmptyList {
        function: String,
    },
    /// A required variable the expression reads was not bound.
    Unbound(String),
    /// Internal: impossible for checked expressions with conforming bindings (property-tested).
    Type(String),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::StepLimit => "EXPR-R001",
            EvalError::AllocLimit => "EXPR-R002",
            EvalError::StringTooLong => "EXPR-R003",
            EvalError::ListTooLong => "EXPR-R004",
            EvalError::DepthLimit => "EXPR-R005",
            EvalError::DivisionByZero => "EXPR-R010",
            EvalError::Overflow => "EXPR-R011",
            EvalError::IndexOutOfBounds { .. } => "EXPR-R012",
            EvalError::InvalidArgument { .. } => "EXPR-R013",
            EvalError::EmptyList { .. } => "EXPR-R014",
            EvalError::Unbound(_) => "EXPR-R015",
            EvalError::Type(_) => "EXPR-R090",
        }
    }

    pub fn is_budget(&self) -> bool {
        matches!(
            self,
            EvalError::StepLimit
                | EvalError::AllocLimit
                | EvalError::StringTooLong
                | EvalError::ListTooLong
                | EvalError::DepthLimit
        )
    }

    /// The runtime incident code for budget exhaustion (docs/04 §4.5).
    pub fn incident_code(&self) -> Option<&'static str> {
        self.is_budget().then_some("IR-R010")
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            EvalError::StepLimit => f.write_str("evaluation step budget exhausted"),
            EvalError::AllocLimit => f.write_str("evaluation memory budget exhausted"),
            EvalError::StringTooLong => f.write_str("string longer than the budget allows"),
            EvalError::ListTooLong => f.write_str("list longer than the budget allows"),
            EvalError::DepthLimit => f.write_str("evaluation nested too deeply"),
            EvalError::DivisionByZero => f.write_str("division by zero"),
            EvalError::Overflow => f.write_str("number or time out of range"),
            EvalError::IndexOutOfBounds { index, len } => write!(f, "index {index} outside list of {len}"),
            EvalError::InvalidArgument { function, message } => write!(f, "{function}: {message}"),
            EvalError::EmptyList { function } => write!(f, "{function} of an empty list"),
            EvalError::Unbound(n) => write!(f, "variable `{n}` is not bound"),
            EvalError::Type(m) => write!(f, "internal type error: {m}"),
        }
    }
}

impl std::error::Error for EvalError {}

/// Problem with the values supplied for evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingError {
    /// `EXPR-B001` unknown variable, `EXPR-B002` value doesn't match the declared type.
    pub code: &'static str,
    pub name: String,
    pub message: String,
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} `{}`: {}", self.code, self.name, self.message)
    }
}

impl std::error::Error for BindingError {}

/// Variable values, validated against the schema. Only the variables an expression reads
/// need to be bound (see [`CheckedExpr::reads`]), so restricted values can stay sealed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    values: BTreeMap<String, Value>,
    required: BTreeMap<String, bool>,
}

impl Bindings {
    pub fn new(schema: &Schema, values: BTreeMap<String, Value>) -> Result<Self, BindingError> {
        let mut out = BTreeMap::new();
        for (name, v) in values {
            let Some(decl) = schema.get(&name) else {
                return Err(BindingError { code: "EXPR-B001", name, message: "not declared".into() });
            };
            if !v.conforms(&decl.ty) {
                return Err(BindingError {
                    code: "EXPR-B002",
                    message: format!("value does not match {}", decl.ty),
                    name,
                });
            }
            out.insert(name, widen(v, &decl.ty));
        }
        let required = schema.iter().map(|(n, d)| (n.clone(), !d.ty.is_nullable())).collect();
        Ok(Bindings { values: out, required })
    }

    /// Typed marshalling from a JSON object (SP-08 C08).
    pub fn from_json(
        schema: &Schema,
        obj: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, BindingError> {
        let mut values = BTreeMap::new();
        for (name, j) in obj {
            let Some(decl) = schema.get(name) else {
                return Err(BindingError {
                    code: "EXPR-B001",
                    name: name.clone(),
                    message: "not declared".into(),
                });
            };
            let v = Value::from_json(&decl.ty, j).map_err(|message| BindingError {
                code: "EXPR-B002",
                name: name.clone(),
                message,
            })?;
            values.insert(name.clone(), v);
        }
        Bindings::new(schema, values)
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }
}

/// Resources used by one evaluation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub steps: u64,
    pub alloc_bytes: u64,
}

/// Evaluate a checked expression.
pub fn evaluate(
    expr: &CheckedExpr,
    bindings: &Bindings,
    env: &EvalEnv,
    budget: &Budget,
) -> Result<Value, EvalError> {
    evaluate_metered(expr, bindings, env, budget).map(|(v, _)| v)
}

/// Evaluate and report resource usage.
pub fn evaluate_metered(
    expr: &CheckedExpr,
    bindings: &Bindings,
    env: &EvalEnv,
    budget: &Budget,
) -> Result<(Value, Usage), EvalError> {
    for name in expr.reads.keys() {
        if bindings.get(name).is_none() && bindings.required.get(name).copied().unwrap_or(true) {
            return Err(EvalError::Unbound(name.clone()));
        }
    }
    let mut ev = Evaluator { bindings, env, m: Meter::new(budget), locals: Vec::new(), depth: 0 };
    let v = ev.eval(&expr.ast)?;
    Ok((v, Usage { steps: ev.m.steps, alloc_bytes: ev.m.alloc }))
}

/// Step and allocation accounting shared with the built-in functions.
pub(crate) struct Meter<'b> {
    pub(crate) budget: &'b Budget,
    steps: u64,
    alloc: u64,
}

impl<'b> Meter<'b> {
    fn new(budget: &'b Budget) -> Self {
        Meter { budget, steps: 0, alloc: 0 }
    }

    pub(crate) fn step(&mut self, n: u64) -> Result<(), EvalError> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.budget.max_steps { Err(EvalError::StepLimit) } else { Ok(()) }
    }

    fn alloc(&mut self, bytes: u64) -> Result<(), EvalError> {
        self.alloc = self.alloc.saturating_add(bytes);
        if self.alloc > self.budget.max_alloc_bytes { Err(EvalError::AllocLimit) } else { Ok(()) }
    }

    /// Check a string length *before* building it.
    pub(crate) fn reserve_string(&mut self, len: usize) -> Result<(), EvalError> {
        if len > self.budget.max_string_len {
            return Err(EvalError::StringTooLong);
        }
        self.step(len as u64 / 64)?;
        self.alloc(len as u64)
    }

    pub(crate) fn reserve_list(&mut self, len: usize) -> Result<(), EvalError> {
        if len > self.budget.max_list_len {
            return Err(EvalError::ListTooLong);
        }
        self.step(len as u64)?;
        self.alloc(len as u64 * 16)
    }

    pub(crate) fn string(&mut self, s: String) -> Result<Value, EvalError> {
        self.reserve_string(s.len())?;
        Ok(Value::String(Arc::from(s)))
    }
}

struct Evaluator<'a, 'b> {
    bindings: &'a Bindings,
    env: &'a EvalEnv,
    m: Meter<'b>,
    locals: Vec<(String, Value)>,
    depth: u32,
}

pub(crate) fn type_err(what: &str) -> EvalError {
    EvalError::Type(what.to_string())
}

impl Evaluator<'_, '_> {
    fn eval(&mut self, e: &Expr) -> Result<Value, EvalError> {
        self.m.step(1)?;
        self.depth += 1;
        if self.depth > self.m.budget.max_depth {
            return Err(EvalError::DepthLimit);
        }
        let out = self.eval_inner(e);
        self.depth -= 1;
        out
    }

    fn eval_inner(&mut self, e: &Expr) -> Result<Value, EvalError> {
        match &e.kind {
            ExprKind::Lit(l) => Ok(match l {
                Literal::Null => Value::Null,
                Literal::Bool(b) => Value::Bool(*b),
                Literal::Int(i) => Value::Int(*i),
                Literal::Decimal(d) => Value::Decimal(*d),
                Literal::String(s) => Value::string(s.as_str()),
            }),
            ExprKind::Ident(name) => {
                if let Some((_, v)) = self.locals.iter().rev().find(|(n, _)| n == name) {
                    return Ok(v.clone());
                }
                match self.bindings.get(name) {
                    Some(v) => Ok(v.clone()),
                    None if !self.bindings.required.get(name).copied().unwrap_or(true) => Ok(Value::Null),
                    None => Err(EvalError::Unbound(name.clone())),
                }
            }
            ExprKind::Member { target, field } => match self.eval(target)? {
                Value::Null => Ok(Value::Null),
                Value::Record(fields) => Ok(fields.get(field).cloned().unwrap_or(Value::Null)),
                Value::Money { amount, currency } => match field.as_str() {
                    "amount" => Ok(Value::Decimal(amount)),
                    "currency" => Ok(Value::String(currency)),
                    _ => Err(type_err("unknown money field")),
                },
                _ => Err(type_err("field access on a non-record")),
            },
            ExprKind::Index { target, index } => {
                let (t, i) = (self.eval(target)?, self.eval(index)?);
                match (t, i) {
                    (Value::List(items), Value::Int(i)) => usize::try_from(i)
                        .ok()
                        .and_then(|u| items.get(u).cloned())
                        .ok_or(EvalError::IndexOutOfBounds { index: i, len: items.len() }),
                    _ => Err(type_err("index")),
                }
            }
            ExprKind::Unary { op, operand } => {
                let v = self.eval(operand)?;
                match (op, v) {
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (UnaryOp::Neg, Value::Int(i)) => {
                        i.checked_neg().map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (UnaryOp::Neg, Value::Decimal(d)) => Ok(Value::Decimal(-d)),
                    (UnaryOp::Neg, Value::Money { amount, currency }) => {
                        Ok(Value::Money { amount: -amount, currency })
                    }
                    (UnaryOp::Neg, Value::Duration(d)) => {
                        d.checked_neg().map(Value::Duration).ok_or(EvalError::Overflow)
                    }
                    _ => Err(type_err("unary operand")),
                }
            }
            ExprKind::Binary { op: BinaryOp::And, lhs, rhs } => match self.eval(lhs)? {
                Value::Bool(false) => Ok(Value::Bool(false)),
                Value::Bool(true) => self.boolean(rhs),
                _ => Err(type_err("&&")),
            },
            ExprKind::Binary { op: BinaryOp::Or, lhs, rhs } => match self.eval(lhs)? {
                Value::Bool(true) => Ok(Value::Bool(true)),
                Value::Bool(false) => self.boolean(rhs),
                _ => Err(type_err("||")),
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, b) = (self.eval(lhs)?, self.eval(rhs)?);
                self.binary(*op, a, b)
            }
            ExprKind::Cond { cond, then, otherwise } => match self.eval(cond)? {
                Value::Bool(true) => self.eval(then),
                Value::Bool(false) => self.eval(otherwise),
                _ => Err(type_err("condition")),
            },
            ExprKind::List(items) => {
                self.m.reserve_list(items.len())?;
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it)?);
                }
                Ok(Value::list(out))
            }
            ExprKind::Macro { kind, target, var, body, .. } => self.comprehension(*kind, target, var, body),
            ExprKind::Call { target, name, args, ret, .. } => {
                let recv = match target {
                    Some(t) => Some(self.eval(t)?),
                    None => None,
                };
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a)?);
                }
                builtins::call(&mut self.m, self.env, name, recv, vals, ret.as_ref())
            }
            ExprKind::Widen { inner, to } => {
                let v = self.eval(inner)?;
                if let Value::List(items) = &v {
                    self.m.step(items.len() as u64)?;
                }
                Ok(widen(v, to))
            }
        }
    }

    fn boolean(&mut self, e: &Expr) -> Result<Value, EvalError> {
        match self.eval(e)? {
            v @ Value::Bool(_) => Ok(v),
            _ => Err(type_err("boolean operand")),
        }
    }

    fn comprehension(
        &mut self,
        kind: MacroKind,
        target: &Expr,
        var: &str,
        body: &Expr,
    ) -> Result<Value, EvalError> {
        let Value::List(items) = self.eval(target)? else { return Err(type_err("comprehension target")) };
        let mut kept = Vec::new();
        let mut count = 0usize;
        for item in items.iter() {
            self.m.step(1)?;
            self.locals.push((var.to_string(), item.clone()));
            let r = self.eval(body);
            self.locals.pop();
            let r = r?;
            match (kind, r) {
                (MacroKind::Map, v) => {
                    self.m.reserve_list(1)?;
                    kept.push(v);
                }
                (MacroKind::All, Value::Bool(false)) => return Ok(Value::Bool(false)),
                (MacroKind::Exists, Value::Bool(true)) => return Ok(Value::Bool(true)),
                (MacroKind::ExistsOne, Value::Bool(b)) => count += usize::from(b),
                (MacroKind::Filter, Value::Bool(b)) => {
                    if b {
                        self.m.reserve_list(1)?;
                        kept.push(item.clone());
                    }
                }
                (MacroKind::All | MacroKind::Exists, Value::Bool(_)) => {}
                _ => return Err(type_err("predicate")),
            }
        }
        Ok(match kind {
            MacroKind::All => Value::Bool(true),
            MacroKind::Exists => Value::Bool(false),
            MacroKind::ExistsOne => Value::Bool(count == 1),
            MacroKind::Map | MacroKind::Filter => {
                if kept.len() > self.m.budget.max_list_len {
                    return Err(EvalError::ListTooLong);
                }
                Value::list(kept)
            }
        })
    }

    fn binary(&mut self, op: BinaryOp, a: Value, b: Value) -> Result<Value, EvalError> {
        match op {
            BinaryOp::Eq => Ok(Value::Bool(equals(&mut self.m, &a, &b)?)),
            BinaryOp::Ne => Ok(Value::Bool(!equals(&mut self.m, &a, &b)?)),
            BinaryOp::Lt => Ok(Value::Bool(compare(&a, &b)? == Ordering::Less)),
            BinaryOp::Le => Ok(Value::Bool(compare(&a, &b)? != Ordering::Greater)),
            BinaryOp::Gt => Ok(Value::Bool(compare(&a, &b)? == Ordering::Greater)),
            BinaryOp::Ge => Ok(Value::Bool(compare(&a, &b)? != Ordering::Less)),
            BinaryOp::In => {
                let Value::List(items) = b else { return Err(type_err("in")) };
                for it in items.iter() {
                    if equals(&mut self.m, &a, it)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            _ => arith(&mut self.m, op, a, b),
        }
    }
}

/// Convert integers to decimals where `to` expects decimals (see `ExprKind::Widen`).
pub(crate) fn widen(v: Value, to: &Type) -> Value {
    match (v, to) {
        (Value::Null, _) => Value::Null,
        (v, Type::Optional(t)) => widen(v, t),
        (Value::Int(i), Type::Decimal) => Value::Decimal(Decimal::from(i)),
        (Value::List(items), Type::List(t)) if has_decimal(t) => {
            Value::list(items.iter().map(|v| widen(v.clone(), t)).collect())
        }
        (Value::Record(fields), Type::Record(ft)) if ft.values().any(has_decimal) => Value::Record(Arc::new(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), ft.get(k).map_or_else(|| v.clone(), |t| widen(v.clone(), t))))
                .collect(),
        )),
        (v, _) => v,
    }
}

fn has_decimal(t: &Type) -> bool {
    match t.non_null() {
        Type::Decimal => true,
        Type::List(e) => has_decimal(e),
        Type::Record(f) => f.values().any(has_decimal),
        _ => false,
    }
}

pub(crate) fn num(v: &Value) -> Option<Decimal> {
    match v {
        Value::Int(i) => Some(Decimal::from(*i)),
        Value::Decimal(d) => Some(*d),
        _ => None,
    }
}

/// Deep equality; integers and decimals compare by value.
pub(crate) fn equals(m: &mut Meter<'_>, a: &Value, b: &Value) -> Result<bool, EvalError> {
    m.step(1)?;
    Ok(match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Int(_) | Value::Decimal(_), Value::Int(_) | Value::Decimal(_)) => num(a) == num(b),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => {
            m.step(x.len().min(y.len()) as u64 / 64)?;
            x == y
        }
        (Value::Date(x), Value::Date(y)) => x == y,
        (Value::DateTime(x), Value::DateTime(y)) => x == y,
        (Value::Duration(x), Value::Duration(y)) => x == y,
        (Value::Money { amount: x, currency: c }, Value::Money { amount: y, currency: d }) => {
            c == d && x == y
        }
        (Value::List(x), Value::List(y)) => {
            if x.len() != y.len() {
                return Ok(false);
            }
            for (p, q) in x.iter().zip(y.iter()) {
                if !equals(m, p, q)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::Record(x), Value::Record(y)) => {
            for k in x.keys().chain(y.keys()) {
                let p = x.get(k).unwrap_or(&Value::Null);
                let q = y.get(k).unwrap_or(&Value::Null);
                if !equals(m, p, q)? {
                    return Ok(false);
                }
            }
            true
        }
        _ => return Err(type_err("==")),
    })
}

pub(crate) fn compare(a: &Value, b: &Value) -> Result<Ordering, EvalError> {
    Ok(match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Int(_) | Value::Decimal(_), Value::Int(_) | Value::Decimal(_)) => num(a).cmp(&num(b)),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Date(x), Value::Date(y)) => x.cmp(y),
        (Value::DateTime(x), Value::DateTime(y)) => x.cmp(y),
        (Value::Duration(x), Value::Duration(y)) => x.cmp(y),
        (Value::Money { amount: x, currency: c }, Value::Money { amount: y, currency: d }) if c == d => {
            x.cmp(y)
        }
        _ => return Err(type_err("ordering")),
    })
}

fn dec_op(op: BinaryOp, x: Decimal, y: Decimal) -> Result<Decimal, EvalError> {
    if matches!(op, BinaryOp::Div | BinaryOp::Rem) && y.is_zero() {
        return Err(EvalError::DivisionByZero);
    }
    match op {
        BinaryOp::Add => x.checked_add(y),
        BinaryOp::Sub => x.checked_sub(y),
        BinaryOp::Mul => x.checked_mul(y),
        // Quotients drop trailing zeros (`1.0 / 2` is `0.5`); other results keep their scale.
        BinaryOp::Div => x.checked_div(y).map(|q| q.normalize()),
        BinaryOp::Rem => x.checked_rem(y),
        _ => return Err(type_err("decimal operator")),
    }
    .ok_or(EvalError::Overflow)
}

/// Arithmetic on non-null values; mirrors `check::arith`.
pub(crate) fn arith(m: &mut Meter<'_>, op: BinaryOp, a: Value, b: Value) -> Result<Value, EvalError> {
    use BinaryOp::*;
    use Value as V;
    Ok(match (op, a, b) {
        (op, V::Int(x), V::Int(y)) => {
            if matches!(op, Div | Rem) && y == 0 {
                return Err(EvalError::DivisionByZero);
            }
            let r = match op {
                Add => x.checked_add(y),
                Sub => x.checked_sub(y),
                Mul => x.checked_mul(y),
                Div => x.checked_div(y),
                Rem => x.checked_rem(y),
                _ => return Err(type_err("integer operator")),
            };
            V::Int(r.ok_or(EvalError::Overflow)?)
        }
        (op, a @ (V::Int(_) | V::Decimal(_)), b @ (V::Int(_) | V::Decimal(_))) => {
            let (x, y) =
                (num(&a).ok_or_else(|| type_err("number"))?, num(&b).ok_or_else(|| type_err("number"))?);
            V::Decimal(dec_op(op, x, y)?)
        }
        (Add, V::String(x), V::String(y)) => {
            m.reserve_string(x.len() + y.len())?;
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(&x);
            s.push_str(&y);
            V::String(Arc::from(s))
        }
        (Add, V::List(x), V::List(y)) => {
            m.reserve_list(x.len() + y.len())?;
            V::list(x.iter().chain(y.iter()).cloned().collect())
        }
        (op @ (Add | Sub), V::Money { amount: x, currency: c }, V::Money { amount: y, currency: d })
            if c == d =>
        {
            V::Money { amount: dec_op(op, x, y)?, currency: c }
        }
        (Div, V::Money { amount: x, currency: c }, V::Money { amount: y, currency: d }) if c == d => {
            V::Decimal(dec_op(Div, x, y)?)
        }
        (op @ (Mul | Div), V::Money { amount, currency }, n @ (V::Int(_) | V::Decimal(_))) => {
            V::Money { amount: dec_op(op, amount, num(&n).ok_or_else(|| type_err("number"))?)?, currency }
        }
        (Mul, n @ (V::Int(_) | V::Decimal(_)), V::Money { amount, currency }) => {
            V::Money { amount: dec_op(Mul, num(&n).ok_or_else(|| type_err("number"))?, amount)?, currency }
        }
        (Add, V::DateTime(t), V::Duration(d)) | (Add, V::Duration(d), V::DateTime(t)) => {
            V::DateTime(t.checked_add(d).ok_or(EvalError::Overflow)?)
        }
        (Sub, V::DateTime(t), V::Duration(d)) => V::DateTime(t.checked_sub(d).ok_or(EvalError::Overflow)?),
        (Sub, V::DateTime(x), V::DateTime(y)) => V::Duration(x - y),
        (Add, V::Date(t), V::Duration(d)) | (Add, V::Duration(d), V::Date(t)) => {
            V::Date(add_days(t, d, false)?)
        }
        (Sub, V::Date(t), V::Duration(d)) => V::Date(add_days(t, d, true)?),
        (Sub, V::Date(x), V::Date(y)) => V::Duration(x - y),
        (Add, V::Duration(x), V::Duration(y)) => V::Duration(x.checked_add(y).ok_or(EvalError::Overflow)?),
        (Sub, V::Duration(x), V::Duration(y)) => V::Duration(x.checked_sub(y).ok_or(EvalError::Overflow)?),
        (Mul, V::Duration(d), V::Int(n)) | (Mul, V::Int(n), V::Duration(d)) => {
            let n = i32::try_from(n).map_err(|_| EvalError::Overflow)?;
            V::Duration(d.checked_mul(n).ok_or(EvalError::Overflow)?)
        }
        _ => return Err(type_err("operator")),
    })
}

fn add_days(date: time::Date, d: time::Duration, negate: bool) -> Result<time::Date, EvalError> {
    if d.whole_seconds() % 86_400 != 0 || d.subsec_nanoseconds() != 0 {
        return Err(EvalError::InvalidArgument {
            function: "date arithmetic".into(),
            message: "only whole days can be added to a date; use datetimes for hours".into(),
        });
    }
    let d = if negate { d.checked_neg().ok_or(EvalError::Overflow)? } else { d };
    date.checked_add(d).ok_or(EvalError::Overflow)
}
