//! Type checker: inferred types, every diagnostic code, null narrowing, reads and
//! classification.
// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use procera_expr::*;

fn ty(src: &str) -> String {
    compile_ok(src).ty.to_string()
}

#[test]
fn infers_types() {
    assert_eq!(ty("qty + 1"), "integer");
    assert_eq!(ty("qty + total"), "decimal");
    assert_eq!(ty("1 / 2"), "integer");
    assert_eq!(ty("1.0 / 2"), "decimal");
    assert_eq!(ty("amount * 2"), "money<INR>");
    assert_eq!(ty("amount / amount"), "decimal");
    assert_eq!(ty("amount.amount"), "decimal");
    assert_eq!(ty("name + \"!\""), "string");
    assert_eq!(ty("severity + \"!\""), "string");
    assert_eq!(ty("start_date + duration(\"P1D\")"), "date");
    assert_eq!(ty("now() - instance.started_at"), "duration");
    assert_eq!(ty("start_date - today()"), "duration");
    assert_eq!(ty("[1, 2.5]"), "list<decimal>");
    assert_eq!(ty("[]"), "list<nothing>");
    assert_eq!(ty("[null, 1]"), "list<integer?>");
    assert_eq!(ty("flag ? 1 : 2.5"), "decimal");
    assert_eq!(ty("flag ? \"x\" : null"), "string?");
    assert_eq!(ty("items.map(i, i.quantity * i.unit_price)"), "list<decimal>");
    assert_eq!(
        ty("items.filter(i, i.quantity > 1)"),
        "list<record{description: string, quantity: integer, unit_price: decimal}>"
    );
    assert_eq!(ty("sum(items.map(i, i.quantity))"), "integer");
    assert_eq!(ty("sum([amount, amount])"), "money<INR>");
    assert_eq!(ty("avg(scores)"), "decimal");
    assert_eq!(ty("max(scores)"), "integer");
    assert_eq!(ty("min(qty, total)"), "decimal");
    assert_eq!(ty("initiator.manager"), "principal?");
    assert_eq!(ty("initiator.manager.manager.id"), "string?");
    assert_eq!(ty("maybe_qty"), "integer?");
    assert_eq!(ty("name.split(\",\")"), "list<string>");
    assert_eq!(ty("round(total, 2)"), "decimal");
    assert_eq!(ty("round(qty)"), "decimal");
    assert_eq!(ty("floor(qty)"), "integer");
    assert_eq!(ty("money(10, \"USD\")"), "money<USD>");
}

#[test]
fn null_narrowing() {
    assert_eq!(ty("maybe_qty != null ? maybe_qty + 1 : 0"), "integer");
    assert_eq!(ty("maybe_qty == null ? 0 : maybe_qty + 1"), "integer");
    assert_eq!(ty("maybe_qty != null && maybe_qty > 2"), "boolean");
    assert_eq!(ty("maybe_qty == null || maybe_qty > 2"), "boolean");
    assert_eq!(ty("!(maybe_qty == null) && maybe_qty > 2"), "boolean");
    assert_eq!(ty("initiator.manager != null ? initiator.manager.id : \"\""), "string");
    assert_eq!(ty("nickname != null && flag && nickname.size() > 2"), "boolean");
    // Without the guard, arithmetic on a nullable value is rejected.
    assert_eq!(codes("maybe_qty + 1"), ["EXPR-E016"]);
    // The guard on the wrong branch doesn't help.
    assert_eq!(codes("maybe_qty == null ? maybe_qty + 1 : 0"), ["EXPR-E016"]);
    assert_eq!(codes("maybe_qty != null || maybe_qty > 2"), ["EXPR-E016"]);
}

#[test]
fn comprehension_variable_hides_outer_narrowing() {
    let mut s = schema();
    s.declare("opt_list", Type::list(Type::optional(Type::Int)), true, Classification::Internal);
    s.declare("x", Type::Int, false, Classification::Internal);
    let r = compile("x != null && opt_list.all(x, x > 1)", &s, &CheckOptions::default());
    assert_eq!(r.unwrap_err()[0].code, Code::Nullable);
}

#[test]
fn diagnostic_unknown_names() {
    let d = compile("amout > 5", &schema(), &CheckOptions::default()).unwrap_err();
    assert_eq!(d[0].code, Code::UnknownVariable);
    assert_eq!((d[0].span.start, d[0].span.end), (0, 5));
    assert_eq!(codes("initiator.nmae"), ["EXPR-E011"]);
    assert_eq!(codes("qty.foo"), ["EXPR-E011"]);
    assert_eq!(codes("frobnicate(1)"), ["EXPR-E012"]);
    assert_eq!(codes("qty.lower()"), ["EXPR-E012"]);
    assert_eq!(codes("contains(name, \"a\")"), ["EXPR-E012"]);
}

#[test]
fn diagnostic_arity_and_arguments() {
    assert_eq!(codes("size(tags, 1)"), ["EXPR-E013"]);
    assert_eq!(codes("now(1)"), ["EXPR-E013"]);
    assert_eq!(codes("name.contains(1)"), ["EXPR-E015"]);
    assert_eq!(codes("sum(tags)"), ["EXPR-E015"]);
    assert_eq!(codes("size(qty)"), ["EXPR-E015"]);
    assert_eq!(codes("min(name, qty)"), ["EXPR-E015"]);
}

#[test]
fn diagnostic_operator_types() {
    assert_eq!(codes("name > 5"), ["EXPR-E014"]);
    assert_eq!(codes("name - \"a\""), ["EXPR-E014"]);
    assert_eq!(codes("!qty"), ["EXPR-E014"]);
    assert_eq!(codes("flag ? 1 : \"x\""), ["EXPR-E014"]);
    assert_eq!(codes("qty ? 1 : 2"), ["EXPR-E014"]);
    assert_eq!(codes("tags[\"a\"]"), ["EXPR-E014"]);
    assert_eq!(codes("qty[0]"), ["EXPR-E014"]);
    assert_eq!(codes("qty in tags"), ["EXPR-E014"]);
    assert_eq!(codes("qty && flag"), ["EXPR-E014"]);
    assert_eq!(codes("start_date + duration(\"P1D\") > now()"), ["EXPR-E014"]);
}

#[test]
fn diagnostic_null_conditions() {
    assert_eq!(codes_with("maybe_flag", &CheckOptions::condition()), ["EXPR-E018"]);
    assert_eq!(codes("maybe_flag ? 1 : 2"), ["EXPR-E018"]);
    assert_eq!(codes("maybe_flag && flag"), ["EXPR-E018"]);
    assert_eq!(codes("tags.all(t, maybe_flag)"), ["EXPR-E018"]);
    // Comparing with a literal makes it total.
    assert_eq!(codes_with("maybe_flag == true", &CheckOptions::condition()), Vec::<&str>::new());
}

#[test]
fn diagnostic_expected_type() {
    assert_eq!(codes_with("qty", &CheckOptions::condition()), ["EXPR-E017"]);
    assert_eq!(codes_with("null", &CheckOptions::condition()), ["EXPR-E017"]);
    assert_eq!(codes_with("total", &CheckOptions::expecting(Type::Int)), ["EXPR-E017"]);
    let c = compile("qty", &schema(), &CheckOptions::expecting(Type::Decimal)).unwrap();
    assert_eq!(c.ty, Type::Decimal);
    assert_eq!(evaluate(&c, &bindings(), &env(), &Budget::default()).unwrap().to_json(), "3");
}

#[test]
fn diagnostic_enum_values() {
    let d = compile("severity == \"majr\"", &schema(), &CheckOptions::default()).unwrap_err();
    assert_eq!(d[0].code, Code::UnknownEnumValue);
    assert_eq!((d[0].span.start, d[0].span.end), (12, 18));
    assert_eq!(codes("\"crit\" != severity"), ["EXPR-E019"]);
    assert_eq!(codes("severity in [\"minor\", \"huge\"]"), ["EXPR-E019"]);
    assert!(codes("severity in [\"minor\", \"major\"]").is_empty());
}

#[test]
fn diagnostic_currency() {
    assert_eq!(codes("amount + money(1, \"USD\")"), ["EXPR-E020"]);
    assert_eq!(codes("amount == money(1, \"USD\")"), ["EXPR-E020"]);
    assert_eq!(codes("money(1, name)"), ["EXPR-E021"]);
    assert_eq!(codes("money(1, \"usd\")"), ["EXPR-E021"]);
}

#[test]
fn diagnostic_literals() {
    assert_eq!(codes("date(\"2026-13-01\")"), ["EXPR-E021"]);
    assert_eq!(codes("datetime(\"2026-01-01T00:00:00\")"), ["EXPR-E021"]);
    assert_eq!(codes("duration(\"P1M\")"), ["EXPR-E021"]);
    assert_eq!(codes("duration(\"P1Y\")"), ["EXPR-E021"]);
    assert_eq!(codes("decimal(\"1,5\")"), ["EXPR-E021"]);
    assert_eq!(codes("int(\"x\")"), ["EXPR-E021"]);
    assert_eq!(codes("round(total, 29)"), ["EXPR-E021"]);
}

#[test]
fn diagnostic_mixed_lists_and_ordering() {
    assert_eq!(codes("[1, \"a\"]"), ["EXPR-E022"]);
    assert_eq!(codes("flag < true"), ["EXPR-E025"]);
    assert_eq!(codes("severity < severity"), ["EXPR-E025"]);
    assert_eq!(codes("employee < initiator"), ["EXPR-E025"]);
}

#[test]
fn restricted_variables_in_event_facing_contexts() {
    let event = CheckOptions { expected: None, context: ExprContext::EventFacing };
    let d = compile("\"Salary: \" + string(salary != null ? salary : 0)", &schema(), &event).unwrap_err();
    assert!(d.iter().all(|d| d.code == Code::RestrictedInContext));
    assert_eq!(d.len(), 2);
    assert!(codes_with("name + \" \" + string(qty)", &event).is_empty());
    // A general context may read it; the classification travels with the result.
    let c = compile("salary != null && salary > 100000", &schema(), &CheckOptions::condition()).unwrap();
    assert_eq!(c.classification(), Classification::Restricted);
}

#[test]
fn reads_and_classification() {
    let c = compile_ok("items.exists(i, i.quantity > qty) && notes != null && tags.size() > 0");
    let reads: Vec<_> = c.reads.keys().map(String::as_str).collect();
    assert_eq!(reads, ["items", "notes", "qty", "tags"]);
    assert_eq!(c.classification(), Classification::Confidential);
    assert_eq!(compile_ok("1 + 1").classification(), Classification::Public);
    // Comprehension variables are not reads.
    assert!(!compile_ok("scores.map(name, name + 1)").reads.contains_key("name"));
}

#[test]
fn diagnostics_are_sorted_and_all_reported() {
    let d = compile("foo + bar", &schema(), &CheckOptions::default()).unwrap_err();
    assert_eq!(d.len(), 2);
    assert!(d[0].span.start < d[1].span.start);
    assert!(d[0].to_string().starts_with("EXPR-E010 at 0..3"));
}

#[test]
fn type_from_ir() {
    use serde_json::json;
    assert_eq!(Type::from_ir(&json!("text")).unwrap(), Type::String);
    assert_eq!(Type::from_ir(&json!({"money": {"currency": "INR"}})).unwrap(), Type::Money("INR".into()));
    assert_eq!(Type::from_ir(&json!({"enum": ["b", "a"]})).unwrap(), Type::enumeration(["a", "b"]));
    assert_eq!(Type::from_ir(&json!({"list": "integer"})).unwrap(), Type::list(Type::Int));
    assert!(Type::from_ir(&json!("float")).is_err());
    assert!(Type::from_ir(&json!({"money": {"currency": "inr"}})).is_err());
    assert!(Type::from_ir(&json!({"enum": []})).is_err());
}

#[test]
fn checked_expr_round_trips_through_json() {
    let c = compile_ok("flag ? qty : total");
    let back: CheckedExpr = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(c, back);
}
