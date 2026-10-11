//! The production [`Evaluator`]: procera-expr, the typed and budgeted CEL subset (docs/11 §6.1).
//!
//! The compiler stores every expression as a [`CheckedExpr`]; this evaluator never parses. For
//! each evaluation it binds only the variables the expression reads, marshalled from the
//! instance's JSON by their declared types, and evaluates with the injected `now` (the
//! stimulus time) and time-zone offset under the expression [`Budget`]. A budget overrun is
//! [`EvalError::Budget`], which the interpreter raises as incident `IR-R010` (docs/04 §4.5);
//! every other failure is [`EvalError::Failed`] (incident `IR-R001`). Nothing panics.

use crate::port::{EvalError, Evaluator, Scope};
use procera_expr::{Bindings, Budget, CheckOptions, CheckedExpr, EvalEnv, Schema, Type};
use procera_ir::model::Expr;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use time::UtcOffset;

#[derive(Debug, Clone)]
pub struct CelEvaluator {
    /// Expression limits (docs/04 §4.5). Defaults: 10 000 steps, 1 MiB.
    pub budget: Budget,
    /// Offset for `today()` and date parts, resolved by the caller from the process calendar
    /// (A-22 (5)). UTC by default.
    pub tz: UtcOffset,
    rules: BTreeMap<String, CheckedExpr>,
}

impl Default for CelEvaluator {
    fn default() -> Self {
        Self { budget: Budget::default(), tz: UtcOffset::UTC, rules: BTreeMap::new() }
    }
}

impl CelEvaluator {
    pub fn new(budget: Budget, tz: UtcOffset) -> Self {
        Self { budget, tz, rules: BTreeMap::new() }
    }

    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    pub fn with_tz(mut self, tz: UtcOffset) -> Self {
        self.tz = tz;
        self
    }

    /// A rule-table stand-in until rule tables ship (BPMS-087): the decision's output is the
    /// value of this CEL expression, checked against the program's schema.
    pub fn with_rule(
        mut self,
        rule: &str,
        source: &str,
        schema: &Schema,
    ) -> Result<Self, Vec<procera_expr::Diagnostic>> {
        let checked = procera_expr::compile(source, schema, &CheckOptions::default())?;
        self.rules.insert(rule.to_owned(), checked);
        Ok(self)
    }

    fn run(&self, checked: &CheckedExpr, scope: &Scope<'_>) -> Result<Value, EvalError> {
        let mut values = Map::new();
        for name in checked.reads.keys() {
            let Some(decl) = scope.schema.get(name) else {
                return Err(EvalError::Failed(format!("`{name}` is not declared")));
            };
            let json = match scope.variables.get(name) {
                Some(v) => Some(v),
                None if name == "initiator" => Some(scope.initiator),
                None if name == "instance" => Some(scope.instance),
                // Unset: null for an optional variable, EXPR-R015 for a required one.
                None => None,
            };
            if let Some(j) = json {
                values.insert(name.clone(), shape(&decl.ty, j));
            }
        }
        let bindings = Bindings::from_json(scope.schema, &values)
            .map_err(|e| EvalError::Failed(format!("binding {e}")))?;
        let env = EvalEnv::new(scope.now, self.tz);
        match procera_expr::evaluate(checked, &bindings, &env, &self.budget) {
            Ok(v) => Ok(v.to_json()),
            Err(e) if e.is_budget() => Err(EvalError::Budget(e.to_string())),
            Err(e) => Err(EvalError::Failed(e.to_string())),
        }
    }
}

/// Adapts instance JSON to the built-in shapes (A-26): a principal or group given as a bare
/// string is its id; fields the built-in shapes do not expose are dropped; an `instance` without
/// a `display_id` uses its id.
fn shape(ty: &Type, j: &Value) -> Value {
    match (ty, j) {
        (Type::Optional(t), j) => shape(t, j),
        (Type::List(t), Value::Array(items)) => Value::Array(items.iter().map(|x| shape(t, x)).collect()),
        (Type::Principal | Type::Group, Value::String(id)) => serde_json::json!({ "id": id }),
        (Type::Principal | Type::Group | Type::File | Type::Instance, Value::Object(o)) => {
            let mut out = Map::new();
            for (k, ft) in ty.fields().unwrap_or_default() {
                if let Some(v) = o.get(&k) {
                    out.insert(k.clone(), shape(&ft, v));
                }
            }
            if *ty == Type::Instance
                && !out.contains_key("display_id")
                && let Some(id) = o.get("id")
            {
                out.insert("display_id".to_owned(), id.clone());
            }
            Value::Object(out)
        }
        (Type::Record(fields), Value::Object(o)) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), fields.get(k).map_or_else(|| v.clone(), |t| shape(t, v))))
                .collect(),
        ),
        _ => j.clone(),
    }
}

impl Evaluator for CelEvaluator {
    fn eval(
        &self,
        expr: &Expr,
        checked: Option<&CheckedExpr>,
        scope: &Scope<'_>,
    ) -> Result<Value, EvalError> {
        match checked {
            Some(c) => self.run(c, scope),
            None => Err(EvalError::Failed(format!("`{}` has no compiled form", expr.0))),
        }
    }

    fn eval_rule(&self, rule: &str, _: Option<&str>, _: &str, scope: &Scope<'_>) -> Result<Value, EvalError> {
        match self.rules.get(rule) {
            Some(c) => self.run(c, scope),
            None => Err(EvalError::Failed(format!("rule table `{rule}` is not available (BPMS-087)"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shapes_principals_and_instances() {
        let p = shape(&Type::Principal, &json!({ "id": "u1", "manager": "u2", "name": "Ann" }));
        assert_eq!(p, json!({ "id": "u1", "manager": { "id": "u2" } }));
        let i = shape(&Type::Instance, &json!({ "id": "i1", "started_at": "2026-10-11T09:00:00Z" }));
        assert_eq!(i["display_id"], "i1");
        assert_eq!(shape(&Type::optional(Type::Group), &json!("g")), json!({ "id": "g" }));
    }
}
