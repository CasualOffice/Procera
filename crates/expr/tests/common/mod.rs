#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use procera_expr::*;
use serde_json::json;
use time::macros::{datetime, offset};

pub fn schema() -> Schema {
    use Classification::*;
    let mut s = Schema::with_process_context();
    let item = Type::from_ir(
        &json!({"record": {"description": "string", "quantity": "integer", "unit_price": "decimal"}}),
    )
    .unwrap();
    s.declare("amount", Type::Money("INR".into()), true, Internal)
        .declare("total", Type::Decimal, true, Internal)
        .declare("qty", Type::Int, true, Internal)
        .declare("maybe_qty", Type::Int, false, Internal)
        .declare("name", Type::String, true, Internal)
        .declare("nickname", Type::String, false, Internal)
        .declare("severity", Type::enumeration(["minor", "major", "critical"]), true, Internal)
        .declare("flag", Type::Bool, true, Internal)
        .declare("maybe_flag", Type::Bool, false, Internal)
        .declare("start_date", Type::Date, true, Internal)
        .declare("due", Type::DateTime, false, Internal)
        .declare("wait", Type::Duration, true, Internal)
        .declare("salary", Type::Decimal, false, Restricted)
        .declare("notes", Type::String, false, Confidential)
        .declare("tags", Type::list(Type::String), true, Public)
        .declare("scores", Type::list(Type::Int), true, Internal)
        .declare("prices", Type::list(Type::Decimal), true, Internal)
        .declare("items", Type::list(item), true, Internal)
        .declare("employee", Type::Principal, true, Internal);
    s
}

pub fn bindings_json() -> serde_json::Value {
    json!({
        "initiator": {"id": "u-1", "email": "asha@example.test", "department": "finance",
                      "manager": {"id": "u-2", "manager": {"id": "u-3"}}},
        "instance": {"id": "i-9", "display_id": "PR-0042", "started_at": "2026-10-10T08:00:00+05:30"},
        "amount": {"amount": "50000.01", "currency": "INR"},
        "total": 12.5,
        "qty": 3,
        "name": "Laptop",
        "severity": "major",
        "flag": true,
        "start_date": "2026-11-02",
        "due": "2026-10-20T17:00:00Z",
        "wait": "P2DT3H",
        "salary": "120000.00",
        "tags": ["it", "hardware"],
        "scores": [4, 9, 2],
        "prices": ["0.1", "0.2"],
        "items": [
            {"description": "Mouse", "quantity": 2, "unit_price": "10.49"},
            {"description": "Dock", "quantity": 1, "unit_price": "20.00"}
        ],
        "employee": {"id": "u-7"}
    })
}

pub fn bindings() -> Bindings {
    Bindings::from_json(&schema(), bindings_json().as_object().unwrap()).unwrap()
}

pub fn env() -> EvalEnv {
    EvalEnv::new(datetime!(2026-10-11 20:00 UTC), offset!(+5:30))
}

pub fn compile_ok(src: &str) -> CheckedExpr {
    compile(src, &schema(), &CheckOptions::default()).unwrap_or_else(|d| panic!("{src}: {d:?}"))
}

pub fn run(src: &str) -> Result<Value, EvalError> {
    evaluate(&compile_ok(src), &bindings(), &env(), &Budget::default())
}

/// Evaluate and render as JSON for compact assertions.
pub fn val(src: &str) -> serde_json::Value {
    run(src).unwrap_or_else(|e| panic!("{src}: {e}")).to_json()
}

pub fn codes(src: &str) -> Vec<&'static str> {
    codes_with(src, &CheckOptions::default())
}

pub fn codes_with(src: &str, opts: &CheckOptions) -> Vec<&'static str> {
    match compile(src, &schema(), opts) {
        Ok(_) => vec![],
        Err(d) => d.iter().map(|d| d.code.as_str()).collect(),
    }
}

pub fn err_code(src: &str) -> &'static str {
    run(src).expect_err(src).code()
}
