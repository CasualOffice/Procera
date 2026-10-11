//! Expression compilation (docs/11 §6, §6.1, §11 check 7), backed by `procera-expr`.
//!
//! Every expression site in the document is parsed and type-checked against a [`Schema`] built
//! from the declared variables plus the process context (`initiator`, `instance`; `now()` is a
//! function). Each site knows what it needs: a branch condition is a non-null boolean, a `set`
//! assignment has its variable's type, a timer `at` is a datetime, an idempotency key is a string,
//! and an assignee reference is a principal, group or id. Event-facing sites (every `notify`
//! field, `correlation`, `idempotencyKey`) are checked in [`ExprContext::EventFacing`], so reading
//! a `restricted` variable there is an error.
//!
//! The checked forms are kept in [`CompiledExpressions`], keyed by node id and the field's
//! pointer relative to the node, so the runtime evaluates them without parsing again.

use crate::diag::{Code, Diagnostic, escape_pointer};
use crate::model::{self, Assignment, Expr, ExprLang, IrDocument, NodeKind};
use procera_expr::{CheckOptions, CheckedExpr, Classification, ExprContext, Schema, Type};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The checked expressions of one compiled document, and the schema they were checked against.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompiledExpressions {
    /// Declared variables plus the process context (docs/11 §5, §13.10).
    pub schema: Schema,
    /// Node id → field pointer relative to the node (`branches/0/when`, `assign/total`, `at`,
    /// `assignment/candidates/0/ref`, …) → checked expression.
    pub sites: BTreeMap<String, BTreeMap<String, CheckedExpr>>,
}

impl CompiledExpressions {
    /// The checked expression at `field` of node `node`.
    pub fn get(&self, node: &str, field: &str) -> Option<&CheckedExpr> {
        self.sites.get(node).and_then(|f| f.get(field))
    }

    /// Number of compiled sites.
    pub fn len(&self) -> usize {
        self.sites.values().map(BTreeMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every site as `(node, field, expression)`, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &CheckedExpr)> {
        self.sites.iter().flat_map(|(n, f)| f.iter().map(move |(k, e)| (n.as_str(), k.as_str(), e)))
    }
}

/// The site key for `assign/<name>` and `input/<name>` (RFC 6901 escaping of the name).
pub fn field_key(section: &str, name: &str) -> String {
    format!("{section}/{}", escape_pointer(name))
}

/// What a site needs its expression to produce.
#[derive(Debug, Clone)]
enum Want {
    Any,
    Type(Type),
    /// A principal, group or id (string), or a list of them; may be null (the fallback applies).
    Assignee,
}

#[derive(Debug)]
struct Site<'a> {
    index: usize,
    node: &'a str,
    field: String,
    expr: Expr,
    facing: bool,
    want: Want,
}

fn classification(c: Option<model::Classification>) -> Classification {
    match c {
        Some(model::Classification::Public) => Classification::Public,
        Some(model::Classification::Confidential) => Classification::Confidential,
        Some(model::Classification::Restricted) => Classification::Restricted,
        // A-21 (9): no classification counts as not restricted.
        Some(model::Classification::Internal) | None => Classification::Internal,
    }
}

/// The schema expressions are checked against: the process context plus every declared
/// variable whose type maps to an expression type. A variable that does not (a `money` without a
/// currency) is reported as `IR-E035` at its type and left out.
pub fn schema_of(doc: &IrDocument) -> (Schema, Vec<Diagnostic>) {
    let mut schema = Schema::with_process_context();
    let mut diags = Vec::new();
    for (k, v) in doc.variables.iter().enumerate() {
        let ty = serde_json::to_value(&v.ty).map_err(|e| e.to_string()).and_then(|j| Type::from_ir(&j));
        match ty {
            Ok(ty) => {
                schema.declare(&v.name, ty, v.required == Some(true), classification(v.classification));
            }
            Err(e) => diags.push(Diagnostic::new(
                Code::E035,
                format!("/variables/{k}/type"),
                format!("variable '{}' has no expression type: {e}", v.name),
            )),
        }
    }
    (schema, diags)
}

fn sites<'a>(doc: &'a IrDocument, schema: &Schema) -> Vec<Site<'a>> {
    let mut out = Vec::new();
    for (index, n) in doc.nodes.iter().enumerate() {
        let notify = n.kind == NodeKind::Notify;
        let mut push = |field: String, expr: &Expr, facing: bool, want: Want| {
            out.push(Site { index, node: &n.id, field, expr: expr.clone(), facing: facing || notify, want });
        };
        if let Some(e) = &n.at {
            push("at".into(), e, false, Want::Type(Type::DateTime));
        }
        // Correlation keys and idempotency keys are recorded in events and job rows.
        if let Some(e) = &n.correlation {
            push("correlation".into(), e, true, Want::Any);
        }
        if let Some(e) = &n.idempotency_key {
            push("idempotencyKey".into(), e, true, Want::Type(Type::String));
        }
        for (k, e) in n.input.iter().flatten() {
            push(field_key("input", k), e, false, Want::Any);
        }
        for (k, e) in n.assign.iter().flatten() {
            let want = schema.get(k).map_or(Want::Any, |d| Want::Type(d.ty.clone()));
            push(field_key("assign", k), e, false, want);
        }
        for (b, br) in n.branches.iter().flatten().enumerate() {
            if let Some(e) = &br.when {
                push(format!("branches/{b}/when"), e, false, Want::Type(Type::Bool));
            }
        }
        let mut assignee = |base: String, a: &Assignment| {
            for (c, cand) in a.candidates.iter().enumerate() {
                if let Some(expr) = cand.reference.as_expr() {
                    out.push(Site {
                        index,
                        node: &n.id,
                        field: format!("{base}/candidates/{c}/ref"),
                        expr,
                        facing: notify,
                        want: Want::Assignee,
                    });
                }
            }
            if let Some(expr) = a.fallback.as_ref().and_then(|f| f.reference.as_expr()) {
                out.push(Site {
                    index,
                    node: &n.id,
                    field: format!("{base}/fallback/ref"),
                    expr,
                    facing: notify,
                    want: Want::Assignee,
                });
            }
        };
        for (field, a) in n.assignments() {
            assignee(field.to_owned(), a);
        }
        for (k, esc) in n.escalations.iter().flatten().enumerate() {
            if let Some(expr) = esc.to.as_ref().and_then(|t| t.reference.as_expr()) {
                out.push(Site {
                    index,
                    node: &n.id,
                    field: format!("escalations/{k}/to/ref"),
                    expr,
                    facing: notify,
                    want: Want::Assignee,
                });
            }
        }
    }
    out
}

/// IR code for an expression diagnostic (docs/11 §14, check 7).
fn ir_code(c: procera_expr::Code) -> Code {
    use procera_expr::Code as X;
    match c {
        X::Syntax | X::Lexical | X::BadNumber | X::SourceTooLong | X::TooDeep | X::Reserved => Code::E019,
        X::UnknownVariable => Code::E020,
        X::RestrictedInContext => Code::E021,
        X::Nullable | X::NullableCondition => Code::E036,
        _ => Code::E035,
    }
}

fn is_assignee(t: &Type) -> bool {
    match t.non_null() {
        Type::Principal | Type::Group | Type::String | Type::Enum(_) | Type::Never => true,
        Type::List(inner) => matches!(inner.non_null(), Type::Principal | Type::Group | Type::String),
        _ => false,
    }
}

/// Parses and type-checks every expression in `doc`. Returns the sites that compiled and the
/// diagnostics of those that did not, with pointers into the source document.
pub fn compile_expressions(doc: &IrDocument) -> (CompiledExpressions, Vec<Diagnostic>) {
    let (schema, mut diags) = schema_of(doc);
    let mut compiled: BTreeMap<String, BTreeMap<String, CheckedExpr>> = BTreeMap::new();
    for site in sites(doc, &schema) {
        let ptr = format!("/nodes/{}/{}", site.index, site.field);
        let err = |code: Code, msg: String| Diagnostic::new(code, ptr.clone(), msg).at_node(site.node);
        let body = match site.expr.parse() {
            None => {
                diags.push(err(Code::E019, format!("'{}' is not `expr:cel: …`", site.expr.0)));
                continue;
            }
            Some((ExprLang::Feel, _)) => {
                diags.push(err(
                    Code::E019,
                    "language `feel` is not supported: expressions are CEL (`expr:cel: …`, ADR-0010, docs/11 §6.1)"
                        .into(),
                ));
                continue;
            }
            Some((ExprLang::Cel, body)) => body,
        };
        let expected = match &site.want {
            Want::Type(t) => Some(t.clone()),
            Want::Any | Want::Assignee => None,
        };
        let context = if site.facing { ExprContext::EventFacing } else { ExprContext::General };
        match procera_expr::compile(body, &schema, &CheckOptions { expected, context }) {
            Ok(checked) => {
                if matches!(site.want, Want::Assignee) && !is_assignee(&checked.ty) {
                    diags.push(err(
                        Code::E035,
                        format!("an assignee must be a principal, group or id, found {}", checked.ty),
                    ));
                    continue;
                }
                compiled.entry(site.node.to_owned()).or_default().insert(site.field, checked);
            }
            Err(ds) => {
                for d in ds {
                    let (line, col) = d.span.line_col(body);
                    diags.push(err(ir_code(d.code), format!("{} at {line}:{col}: {}", d.code, d.message)));
                }
            }
        }
    }
    (CompiledExpressions { schema, sites: compiled }, diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_expression_code() {
        use procera_expr::Code as X;
        assert_eq!(ir_code(X::Syntax), Code::E019);
        assert_eq!(ir_code(X::UnknownVariable), Code::E020);
        assert_eq!(ir_code(X::RestrictedInContext), Code::E021);
        assert_eq!(ir_code(X::NullableCondition), Code::E036);
        assert_eq!(ir_code(X::CurrencyMismatch), Code::E035);
    }

    #[test]
    fn assignee_types() {
        assert!(is_assignee(&Type::optional(Type::Principal)));
        assert!(is_assignee(&Type::list(Type::Group)));
        assert!(!is_assignee(&Type::Decimal));
    }
}
