//! Budgets: every limit yields a typed error (incident IR-R010), never a panic or a hang.
// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use procera_expr::*;

fn with(name: &str, v: Value) -> Bindings {
    Bindings::new(&schema(), [(name.to_string(), v)].into()).unwrap()
}

fn run_with(src: &str, b: &Bindings, budget: &Budget) -> Result<Value, EvalError> {
    evaluate(&compile_ok(src), b, &env(), budget)
}

#[test]
fn step_limit() {
    let b = with("scores", Value::list((0..5_000).map(Value::Int).collect()));
    let e = run_with("scores.map(s, s + 1).map(s, s + 1).size()", &b, &Budget::default()).unwrap_err();
    assert_eq!(e, EvalError::StepLimit);
    assert!(e.is_budget());
    assert_eq!(e.incident_code(), Some("IR-R010"));
    assert_eq!(e.code(), "EXPR-R001");
    // The same expression fits a larger budget.
    let big = Budget { max_steps: 100_000, ..Budget::default() };
    assert_eq!(run_with("scores.map(s, s + 1).map(s, s + 1).size()", &b, &big).unwrap(), Value::Int(5_000));
}

#[test]
fn nested_comprehensions_are_bounded_by_steps() {
    // Quadratic work over a 200-item list (the IR maxItems ceiling) stops at the budget.
    let b = with("scores", Value::list((0..200).map(Value::Int).collect()));
    let e = run_with("scores.all(a, scores.all(b, scores.exists(c, a + b == c)))", &b, &Budget::default());
    assert_eq!(e.unwrap_err(), EvalError::StepLimit);
}

#[test]
fn string_limit() {
    let b = with("name", Value::string("x".repeat(40 * 1024)));
    assert_eq!(run_with("name + name", &b, &Budget::default()).unwrap_err(), EvalError::StringTooLong);
    assert_eq!(run_with("name.size()", &b, &Budget::default()).unwrap(), Value::Int(40 * 1024));
    // A replacement is sized before it is built.
    let b = with("name", Value::string("a".repeat(1000)));
    let e = run_with("name.replace(\"a\", name)", &b, &Budget::default()).unwrap_err();
    assert_eq!(e, EvalError::StringTooLong);
    let e = run_with("[name, name].join(name)", &b, &Budget { max_string_len: 2500, ..Budget::default() });
    assert_eq!(e.unwrap_err(), EvalError::StringTooLong);
}

#[test]
fn list_limit() {
    let b = with("scores", Value::list((0..6_000).map(Value::Int).collect()));
    assert_eq!(
        run_with("size(scores + scores)", &b, &Budget::default()).unwrap_err(),
        EvalError::ListTooLong
    );
    let b = with("name", Value::string("a,".repeat(12_000)));
    assert_eq!(
        run_with("name.split(\",\").size()", &b, &Budget::default()).unwrap_err(),
        EvalError::ListTooLong
    );
    let small = Budget { max_list_len: 2, ..Budget::default() };
    assert_eq!(run_with("[1, 2, 3]", &b, &small).unwrap_err(), EvalError::ListTooLong);
}

#[test]
fn allocation_limit() {
    let b = with("name", Value::string("y".repeat(30 * 1024)));
    // Plenty of steps, so the allocation limit is the one that trips.
    let budget = Budget { max_steps: 1_000_000, ..Budget::default() };
    let src = format!("size([{}])", ["name + name"; 20].join(", "));
    assert_eq!(run_with(&src, &b, &budget).unwrap_err(), EvalError::AllocLimit);
    let src = format!("size([{}])", ["name + name"; 10].join(", "));
    assert_eq!(run_with(&src, &b, &budget).unwrap(), Value::Int(10));
}

#[test]
fn depth_limit() {
    let shallow = Budget { max_depth: 3, ..Budget::default() };
    let b = bindings();
    assert_eq!(run_with("1 + 2 + 3 + 4", &b, &shallow).unwrap_err(), EvalError::DepthLimit);
    assert_eq!(run_with("1 + 2", &b, &shallow).unwrap(), Value::Int(3));
}

#[test]
fn usage_is_reported_and_deterministic() {
    let c = compile_ok("sum(items.map(i, i.quantity * i.unit_price)) > 10.0");
    let (v1, u1) = evaluate_metered(&c, &bindings(), &env(), &Budget::default()).unwrap();
    let (v2, u2) = evaluate_metered(&c, &bindings(), &env(), &Budget::default()).unwrap();
    assert_eq!((v1, u1), (v2, u2));
    assert!(u1.steps > 10 && u1.steps < 100, "{u1:?}");
    // Exactly the reported steps suffice; one fewer does not.
    let exact = Budget { max_steps: u1.steps, ..Budget::default() };
    assert!(evaluate(&c, &bindings(), &env(), &exact).is_ok());
    let short = Budget { max_steps: u1.steps - 1, ..Budget::default() };
    assert_eq!(evaluate(&c, &bindings(), &env(), &short).unwrap_err(), EvalError::StepLimit);
}

#[test]
fn defaults_match_the_lld() {
    let b = Budget::default();
    assert_eq!(b.max_steps, 10_000);
    assert_eq!(b.max_alloc_bytes, 1024 * 1024);
    assert_eq!(b.max_string_len, 64 * 1024);
    assert_eq!(MAX_SOURCE_LEN, 4096);
    assert_eq!(MAX_AST_DEPTH, 32);
}
