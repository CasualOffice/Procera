//! A small stand-in [`Evaluator`] for tests that need fixed answers (overrides) or a token
//! budget. It is not a CEL implementation and ignores the checked form; the production engine
//! is [`crate::CelEvaluator`].
//!
//! Grammar (both `=` and `==` compare; `and`/`&&`, `or`/`||`):
//!
//! ```text
//! expr  := and (or and)*        and := cmp (and cmp)*        cmp := sum (op sum)?
//! sum   := term ((+|-) term)*   term := unary ((*|/) unary)*
//! unary := not(expr) | -unary | number | "string" | 'string' | true | false | null
//!        | path | (expr)        path := ident(.ident)*
//! ```
//!
//! A path resolves against the variables, then the roots `instance`, `initiator` and `now`;
//! a missing value is `null`. Anything outside the grammar can be answered by an override keyed
//! by the exact expression source.

use crate::port::{EvalError, Evaluator, Scope};
use procera_expr::CheckedExpr;
use procera_ir::model::Expr;
use serde_json::{Number, Value};
use std::collections::BTreeMap;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone)]
pub struct MiniEvaluator {
    /// Fixed answers keyed by the full expression source (`expr:feel: …`).
    pub overrides: BTreeMap<String, Value>,
    /// Rule-table stand-ins: rule key → an expression body in this grammar giving the output.
    pub rules: BTreeMap<String, String>,
    /// Maximum tokens per evaluation; more is an [`EvalError::Budget`].
    pub max_tokens: usize,
}

impl Default for MiniEvaluator {
    fn default() -> Self {
        Self { overrides: BTreeMap::new(), rules: BTreeMap::new(), max_tokens: 10_000 }
    }
}

impl MiniEvaluator {
    pub fn with_override(mut self, source: &str, value: Value) -> Self {
        self.overrides.insert(source.to_owned(), value);
        self
    }

    pub fn with_rule(mut self, rule: &str, body: &str) -> Self {
        self.rules.insert(rule.to_owned(), body.to_owned());
        self
    }

    /// Evaluates a body in the grammar above.
    pub fn eval_body(&self, body: &str, scope: &Scope<'_>) -> Result<Value, EvalError> {
        let tokens = lex(body)?;
        if tokens.len() > self.max_tokens {
            return Err(EvalError::Budget(format!("{} tokens > {}", tokens.len(), self.max_tokens)));
        }
        let mut p = Parser { toks: &tokens, pos: 0, scope };
        let v = p.or()?;
        match p.toks.get(p.pos) {
            None => Ok(v),
            Some(t) => Err(fail(format!("unexpected {t:?}"))),
        }
    }
}

impl Evaluator for MiniEvaluator {
    fn eval(&self, expr: &Expr, _: Option<&CheckedExpr>, scope: &Scope<'_>) -> Result<Value, EvalError> {
        if let Some(v) = self.overrides.get(&expr.0) {
            return Ok(v.clone());
        }
        let (_, body) = expr.parse().ok_or_else(|| fail(format!("not an expression: {}", expr.0)))?;
        self.eval_body(body, scope)
    }

    fn eval_rule(&self, rule: &str, _: Option<&str>, _: &str, scope: &Scope<'_>) -> Result<Value, EvalError> {
        match self.rules.get(rule) {
            Some(body) => self.eval_body(body, scope),
            None => Ok(Value::Null),
        }
    }
}

fn fail(msg: String) -> EvalError {
    EvalError::Failed(msg)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
}

fn lex(src: &str) -> Result<Vec<Tok>, EvalError> {
    const OPS: [&str; 16] =
        ["==", "!=", ">=", "<=", "&&", "||", "=", ">", "<", "+", "-", "*", "/", "(", ")", "."];
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '\'' {
            let end =
                chars[i + 1..].iter().position(|x| *x == c).ok_or_else(|| fail("open string".into()))?;
            out.push(Tok::Str(chars[i + 1..i + 1 + end].iter().collect()));
            i += end + 2;
        } else if c.is_ascii_digit() {
            let len = chars[i..].iter().take_while(|x| x.is_ascii_digit() || **x == '.').count();
            let text: String = chars[i..i + len].iter().collect();
            out.push(Tok::Num(text.parse().map_err(|_| fail(format!("bad number {text}")))?));
            i += len;
        } else if c.is_alphabetic() || c == '_' {
            let len = chars[i..].iter().take_while(|x| x.is_alphanumeric() || **x == '_').count();
            out.push(Tok::Ident(chars[i..i + len].iter().collect()));
            i += len;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op =
                OPS.iter().find(|o| rest.starts_with(**o)).ok_or_else(|| fail(format!("unexpected {c}")))?;
            out.push(Tok::Op(op));
            i += op.len();
        }
    }
    Ok(out)
}

struct Parser<'a, 's> {
    toks: &'a [Tok],
    pos: usize,
    scope: &'a Scope<'s>,
}

impl Parser<'_, '_> {
    fn eat_op(&mut self, ops: &[&str]) -> Option<&'static str> {
        match self.toks.get(self.pos) {
            Some(Tok::Op(o)) if ops.contains(o) => {
                self.pos += 1;
                Some(o)
            }
            Some(Tok::Ident(w)) if ops.contains(&w.as_str()) => {
                self.pos += 1;
                if w == "and" { Some("&&") } else { Some("||") }
            }
            _ => None,
        }
    }

    fn or(&mut self) -> Result<Value, EvalError> {
        let mut v = self.and()?;
        while self.eat_op(&["||", "or"]).is_some() {
            let r = self.and()?;
            v = Value::Bool(truth(&v)? || truth(&r)?);
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<Value, EvalError> {
        let mut v = self.cmp()?;
        while self.eat_op(&["&&", "and"]).is_some() {
            let r = self.cmp()?;
            v = Value::Bool(truth(&v)? && truth(&r)?);
        }
        Ok(v)
    }

    fn cmp(&mut self) -> Result<Value, EvalError> {
        let l = self.sum()?;
        let Some(op) = self.eat_op(&["=", "==", "!=", ">", ">=", "<", "<="]) else { return Ok(l) };
        let r = self.sum()?;
        let eq = match (l.as_f64(), r.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => l == r,
        };
        let ord = |f: fn(f64, f64) -> bool| match (l.as_f64(), r.as_f64(), l.as_str(), r.as_str()) {
            (Some(a), Some(b), _, _) => Ok(f(a, b)),
            (_, _, Some(a), Some(b)) => Ok(f(a.cmp(b) as i8 as f64, 0.0)),
            _ => Err(fail(format!("cannot order {l} and {r}"))),
        };
        Ok(Value::Bool(match op {
            "=" | "==" => eq,
            "!=" => !eq,
            ">" => ord(|a, b| a > b)?,
            ">=" => ord(|a, b| a >= b)?,
            "<" => ord(|a, b| a < b)?,
            _ => ord(|a, b| a <= b)?,
        }))
    }

    fn sum(&mut self) -> Result<Value, EvalError> {
        let mut v = self.term()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let r = self.term()?;
            v = match (op, &v, &r) {
                ("+", Value::String(a), b) => Value::String(format!("{a}{}", text(b))),
                ("+", a, Value::String(b)) => Value::String(format!("{}{b}", text(a))),
                _ => arith(op, &v, &r)?,
            };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<Value, EvalError> {
        let mut v = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "/"]) {
            let r = self.unary()?;
            v = arith(op, &v, &r)?;
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<Value, EvalError> {
        if self.eat_op(&["-"]).is_some() {
            return arith("-", &Value::from(0), &self.unary()?);
        }
        let tok = self.toks.get(self.pos).cloned().ok_or_else(|| fail("unexpected end".into()))?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(num(n)),
            Tok::Str(s) => Ok(Value::String(s)),
            Tok::Op("(") => {
                let v = self.or()?;
                self.eat_op(&[")"]).ok_or_else(|| fail("missing )".into()))?;
                Ok(v)
            }
            Tok::Ident(w) if w == "not" => {
                self.eat_op(&["("]).ok_or_else(|| fail("not needs (".into()))?;
                let v = truth(&self.or()?)?;
                self.eat_op(&[")"]).ok_or_else(|| fail("missing )".into()))?;
                Ok(Value::Bool(!v))
            }
            Tok::Ident(w) if w == "true" || w == "false" => Ok(Value::Bool(w == "true")),
            Tok::Ident(w) if w == "null" => Ok(Value::Null),
            Tok::Ident(root) => {
                let mut v = self.root(&root);
                while self.eat_op(&["."]).is_some() {
                    let Some(Tok::Ident(f)) = self.toks.get(self.pos).cloned() else {
                        return Err(fail("expected a field".into()));
                    };
                    self.pos += 1;
                    v = v.get(&f).cloned().unwrap_or(Value::Null);
                }
                Ok(v)
            }
            Tok::Op(o) => Err(fail(format!("unexpected {o}"))),
        }
    }

    fn root(&self, name: &str) -> Value {
        if let Some(v) = self.scope.variables.get(name) {
            return v.clone();
        }
        match name {
            "instance" => self.scope.instance.clone(),
            "initiator" => self.scope.initiator.clone(),
            "now" => Value::String(self.scope.now.format(&Rfc3339).unwrap_or_default()),
            _ => Value::Null,
        }
    }
}

fn truth(v: &Value) -> Result<bool, EvalError> {
    v.as_bool().ok_or_else(|| fail(format!("{v} is not a boolean")))
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::from(n as i64)
    } else {
        Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

fn arith(op: &str, l: &Value, r: &Value) -> Result<Value, EvalError> {
    let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) else {
        return Err(fail(format!("{l} {op} {r} needs numbers")));
    };
    Ok(num(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        _ if b == 0.0 => return Err(fail("division by zero".into())),
        _ => a / b,
    }))
}
