//! Evaluator: semantics of every operator and built-in, null safety, runtime errors.
// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use procera_expr::*;
use serde_json::json;

#[test]
fn integers_and_decimals() {
    assert_eq!(val("1 + 2 * 3"), json!(7));
    assert_eq!(val("7 / 2"), json!(3));
    assert_eq!(val("-7 % 3"), json!(-1));
    assert_eq!(val("7.0 / 2"), json!("3.5"));
    // SP-08 C03: exact decimals, no IEEE doubles.
    assert_eq!(val("0.1 + 0.2 == 0.3"), json!(true));
    assert_eq!(val("sum(prices) == 0.3"), json!(true));
    assert_eq!(val("1 == 1.0"), json!(true));
    assert_eq!(val("qty + total"), json!("15.5"));
    assert_eq!(val("1.0 / 3"), json!("0.3333333333333333333333333333"));
}

#[test]
fn widening_keeps_decimal_semantics() {
    // An integer flowing into a decimal position becomes a decimal, so `/` is exact.
    assert_eq!(val("(flag ? 1 : 2.5) / 2"), json!("0.5"));
    assert_eq!(val("[1, 2.5][0] / 2"), json!("0.5"));
    assert_eq!(val("min(1, 2.5) / 2"), json!("0.5"));
    assert_eq!(val("([1] + [2.5])[0] / 2"), json!("0.5"));
}

#[test]
fn money() {
    // SP-08 C04/C05.
    assert_eq!(val("amount > money(50000, \"INR\")"), json!(true));
    assert_eq!(val("amount.amount > 50000"), json!(true));
    assert_eq!(val("amount * 2"), json!({"amount": "100000.02", "currency": "INR"}));
    assert_eq!(val("amount - amount"), json!({"amount": "0.00", "currency": "INR"}));
    assert_eq!(val("round(amount / 3, 2)"), json!({"amount": "16666.67", "currency": "INR"}));
    assert_eq!(val("amount / amount"), json!("1"));
    assert_eq!(val("amount.currency"), json!("INR"));
    assert_eq!(val("string(amount)"), json!("50000.01 INR"));
    assert_eq!(val("sum([amount, amount]).amount"), json!("100000.02"));
}

#[test]
fn rounding() {
    assert_eq!(val("round(2.5)"), json!("3"));
    assert_eq!(val("round(-2.5)"), json!("-3"));
    assert_eq!(val("round(1.005, 2)"), json!("1.01"));
    assert_eq!(val("floor(-1.5)"), json!("-2"));
    assert_eq!(val("ceil(1.2)"), json!("2"));
    assert_eq!(val("floor(qty)"), json!(3));
    assert_eq!(val("abs(-qty)"), json!(3));
    assert_eq!(err_code("round(total, qty - 10)"), "EXPR-R013");
}

#[test]
fn strings() {
    assert_eq!(val("name + \" x\" + string(qty)"), json!("Laptop x3"));
    assert_eq!(val("name.size()"), json!(6));
    assert_eq!(val("size(\"héllo\")"), json!(5));
    assert_eq!(val("name.lower().upper()"), json!("LAPTOP"));
    assert_eq!(val("\"  a \".trim()"), json!("a"));
    assert_eq!(val("name.contains(\"apt\") && name.startsWith(\"L\") && name.endsWith(\"p\")"), json!(true));
    assert_eq!(val("\"a,b,,c\".split(\",\")"), json!(["a", "b", "", "c"]));
    assert_eq!(val("tags.join(\"/\")"), json!("it/hardware"));
    assert_eq!(val("\"aXbX\".replace(\"X\", \"--\")"), json!("a--b--"));
    assert_eq!(val("\"héllo\".substring(1, 3)"), json!("él"));
    assert_eq!(val("\"héllo\".substring(4)"), json!("o"));
    assert_eq!(val("\"b\" > \"a\""), json!(true));
    assert_eq!(val("severity == \"major\""), json!(true));
    assert_eq!(val("int(\" 42 \") + int(2.9) + int(-2.9)"), json!(42));
    assert_eq!(val("decimal(\"1.50\") + decimal(qty)"), json!("4.50"));
    assert_eq!(val("string(true) + string(1.50)"), json!("true1.50"));
    // SP-08 C07: idempotency keys.
    assert_eq!(val("instance.id + \":\" + initiator.id"), json!("i-9:u-1"));
}

#[test]
fn string_errors() {
    assert_eq!(err_code("\"abc\".substring(2, 1)"), "EXPR-R013");
    assert_eq!(err_code("\"abc\".substring(0, 9)"), "EXPR-R013");
    assert_eq!(err_code("name.split(\"\")"), "EXPR-R013");
    assert_eq!(err_code("name.replace(\"\", \"x\")"), "EXPR-R013");
    assert_eq!(err_code("int(name)"), "EXPR-R013");
    assert_eq!(err_code("decimal(name)"), "EXPR-R013");
}

#[test]
fn lists_and_comprehensions() {
    assert_eq!(val("size(tags) + tags.size()"), json!(4));
    assert_eq!(val("\"it\" in tags"), json!(true));
    assert_eq!(val("tags[1]"), json!("hardware"));
    assert_eq!(val("scores + [1]"), json!([4, 9, 2, 1]));
    assert_eq!(val("scores.filter(s, s > 3)"), json!([4, 9]));
    assert_eq!(val("scores.map(s, s * 2)"), json!([8, 18, 4]));
    assert_eq!(val("scores.all(s, s > 1)"), json!(true));
    assert_eq!(val("scores.exists(s, s > 8)"), json!(true));
    assert_eq!(val("scores.exists_one(s, s > 3)"), json!(false));
    assert_eq!(val("[].all(s, false)"), json!(true));
    assert_eq!(val("[] == []"), json!(true));
    assert_eq!(val("[1, 2] == [1, 2.0]"), json!(true));
    assert_eq!(val("max(scores) - min(scores)"), json!(7));
    assert_eq!(val("avg(scores)"), json!("5"));
    assert_eq!(val("sum([])"), json!(0));
    assert_eq!(val("sum(prices.filter(p, p > 5.0))"), json!("0"));
    // SP-08 C09: list totals.
    assert_eq!(val("sum(items.map(i, i.quantity * i.unit_price))"), json!("40.98"));
    assert_eq!(val("items.map(i, i.description)"), json!(["Mouse", "Dock"]));
    assert_eq!(err_code("tags[5]"), "EXPR-R012");
    assert_eq!(err_code("tags[-1]"), "EXPR-R012");
    assert_eq!(err_code("avg(scores.filter(s, s > 100))"), "EXPR-R014");
    assert_eq!(err_code("max(scores.filter(s, s > 100))"), "EXPR-R014");
}

#[test]
fn null_safety() {
    assert_eq!(val("maybe_qty == null"), json!(true));
    assert_eq!(val("maybe_qty != null ? maybe_qty : -1"), json!(-1));
    assert_eq!(val("nickname == null || nickname.size() > 2"), json!(true));
    // Null-safe navigation through optional principals (SP-08 C06).
    assert_eq!(val("initiator.manager.id"), json!("u-2"));
    assert_eq!(val("initiator.manager.manager.manager.id"), json!(null));
    assert_eq!(val("employee.manager == null"), json!(true));
    assert_eq!(val("initiator.manager.manager.manager == null"), json!(true));
    assert_eq!(val("initiator == initiator"), json!(true));
    assert_eq!(val("employee == initiator"), json!(false));
}

#[test]
fn dates_times_and_durations() {
    // now = 2026-10-11T20:00Z, calendar offset +05:30 → local date 2026-10-12 01:30.
    assert_eq!(val("now()"), json!("2026-10-11T20:00:00Z"));
    assert_eq!(val("today()"), json!("2026-10-12"));
    assert_eq!(val("now().hour()"), json!(1));
    assert_eq!(val("now().minute()"), json!(30));
    assert_eq!(val("date(now())"), json!("2026-10-12"));
    assert_eq!(val("datetime(start_date)"), json!("2026-11-01T18:30:00Z"));
    assert_eq!(val("start_date.weekday()"), json!(1));
    assert_eq!(
        val("start_date.year() * 10000 + start_date.month() * 100 + start_date.day()"),
        json!(20261102)
    );
    assert_eq!(val("start_date + duration(\"P1W\")"), json!("2026-11-09"));
    assert_eq!(val("start_date - date(\"2026-10-31\")"), json!("P2D"));
    assert_eq!(val("(start_date - today()).inDays()"), json!(21));
    assert_eq!(val("instance.started_at"), json!("2026-10-10T02:30:00Z"));
    assert_eq!(val("now() - instance.started_at"), json!("P1DT17H30M"));
    assert_eq!(val("(now() - instance.started_at) > duration(\"P1D\")"), json!(true));
    assert_eq!(val("instance.started_at + wait"), json!("2026-10-12T05:30:00Z"));
    assert_eq!(val("wait * 2"), json!("P4DT6H"));
    assert_eq!(val("-wait"), json!("-P2DT3H"));
    assert_eq!(val("abs(-wait).inHours()"), json!(51));
    assert_eq!(val("wait.inMinutes()"), json!(3060));
    assert_eq!(val("duration(\"PT0S\")"), json!("PT0S"));
    assert_eq!(val("sum([wait, wait])"), json!("P4DT6H"));
    assert_eq!(val("due != null && due > now()"), json!(true));
    assert_eq!(val("string(duration(\"PT90M\"))"), json!("PT1H30M"));
    assert_eq!(err_code("start_date + duration(\"PT3H\")"), "EXPR-R013");
    assert_eq!(err_code("date(name)"), "EXPR-R013");
    assert_eq!(err_code("duration(\"P\" + \"1M\")"), "EXPR-R013");
}

#[test]
fn arithmetic_errors() {
    assert_eq!(err_code("qty / 0"), "EXPR-R010");
    assert_eq!(err_code("qty % 0"), "EXPR-R010");
    assert_eq!(err_code("total / 0"), "EXPR-R010");
    assert_eq!(err_code("amount / 0"), "EXPR-R010");
    assert_eq!(err_code("9223372036854775807 + 1"), "EXPR-R011");
    assert_eq!(err_code("-9223372036854775808 / -1"), "EXPR-R011");
    assert_eq!(err_code("-(-9223372036854775808)"), "EXPR-R011");
    assert_eq!(err_code("abs(-9223372036854775808)"), "EXPR-R011");
    assert_eq!(err_code("decimal(\"79228162514264337593543950335\") * 2"), "EXPR-R011");
    assert_eq!(err_code("wait * 9999999999"), "EXPR-R011");
    assert_eq!(err_code("int(decimal(\"79228162514264337593543950335\"))"), "EXPR-R011");
}

#[test]
fn short_circuit_avoids_errors() {
    assert_eq!(val("false && qty / 0 > 1"), json!(false));
    assert_eq!(val("true || qty / 0 > 1"), json!(true));
    assert_eq!(val("flag ? 1 : qty / 0"), json!(1));
    assert_eq!(val("scores.exists(s, s == 4 || s / 0 > 1)"), json!(true));
}

#[test]
fn bindings_are_validated() {
    let s = schema();
    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("qty".into(), json!("three"));
    let e = Bindings::from_json(&s, &obj).unwrap_err();
    assert_eq!((e.code, e.name.as_str()), ("EXPR-B002", "qty"));

    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("ghost".into(), json!(1));
    assert_eq!(Bindings::from_json(&s, &obj).unwrap_err().code, "EXPR-B001");

    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("amount".into(), json!({"amount": "1", "currency": "USD"}));
    assert_eq!(Bindings::from_json(&s, &obj).unwrap_err().code, "EXPR-B002");

    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("severity".into(), json!("huge"));
    assert_eq!(Bindings::from_json(&s, &obj).unwrap_err().code, "EXPR-B002");

    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("due".into(), json!("2026-10-20T17:00:00"));
    assert_eq!(Bindings::from_json(&s, &obj).unwrap_err().code, "EXPR-B002");

    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("employee".into(), json!({"id": "u", "extra": 1}));
    assert_eq!(Bindings::from_json(&s, &obj).unwrap_err().code, "EXPR-B002");

    // Typed marshalling: a JSON integer bound to a decimal is a decimal (SP-08 C08).
    let mut obj = bindings_json().as_object().unwrap().clone();
    obj.insert("total".into(), json!(5));
    let b = Bindings::from_json(&s, &obj).unwrap();
    assert_eq!(b.get("total"), Some(&Value::Decimal(5.into())));

    // Direct values are widened too.
    let b = Bindings::new(&s, [("total".to_string(), Value::Int(5))].into()).unwrap();
    assert_eq!(b.get("total"), Some(&Value::Decimal(5.into())));
}

#[test]
fn only_read_variables_need_binding() {
    let s = schema();
    let b = Bindings::new(&s, [("qty".to_string(), Value::Int(2))].into()).unwrap();
    let c = compile_ok("qty * 2");
    assert_eq!(evaluate(&c, &b, &env(), &Budget::default()).unwrap(), Value::Int(4));
    // Missing nullable variables are null; missing required ones are an error before evaluating.
    let c = compile_ok("salary == null");
    assert_eq!(evaluate(&c, &b, &env(), &Budget::default()).unwrap(), Value::Bool(true));
    let c = compile_ok("flag || qty > 1");
    let e = evaluate(&c, &b, &env(), &Budget::default()).unwrap_err();
    assert_eq!(e, EvalError::Unbound("flag".into()));
    assert_eq!(e.code(), "EXPR-R015");
}

#[test]
fn value_json_round_trip() {
    let s = schema();
    for name in ["amount", "start_date", "due", "wait", "items", "initiator", "instance", "severity"] {
        let decl = s.get(name).unwrap();
        let v = bindings().get(name).unwrap().clone();
        assert_eq!(Value::from_json(&decl.ty, &v.to_json()).unwrap(), v, "{name}");
    }
}
