//! The production evaluator inside the interpreter (BPMS-081 with BPMS-082): decisions route by
//! real CEL, `now()`/`today()` come from the stimulus and the injected offset, and an expression
//! budget overrun is incident IR-R010 at every expression site, never a panic (docs/04 §4.5).
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::{CelEvaluator, ExprBudget, InstanceStatus};
use serde_json::{Value, json};
use time::macros::{datetime, offset};

fn routing_process() -> Value {
    let mut doc = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "d", "kind": "decision", "branches": [
                { "name": "exact", "when": "expr:cel: price.amount * 3 == 0.3" },
                { "name": "tagged", "when": "expr:cel: tags.exists(t, t.startsWith('urg')) && size(tags) > 1" },
                { "name": "weekend", "when": "expr:cel: today().weekday() >= 6" },
                { "name": "manager", "when": "expr:cel: initiator.manager != null && initiator.manager.id == 'u-manager'" }
            ]},
            task("exact_t"), task("tagged_t"), task("weekend_t"), task("manager_t"), task("other_t"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "d", "next"),
            edge("e2", "d", "exact_t", "exact"),
            edge("e3", "d", "tagged_t", "tagged"),
            edge("e4", "d", "weekend_t", "weekend"),
            edge("e5", "d", "manager_t", "manager"),
            edge("e6", "d", "other_t", "default"),
            edge("e7", "exact_t", "done", "submitted"),
            edge("e8", "tagged_t", "done", "submitted"),
            edge("e9", "weekend_t", "done", "submitted"),
            edge("e10", "manager_t", "done", "submitted"),
            edge("e11", "other_t", "done", "submitted"),
        ]),
        &[],
    );
    doc["variables"] = json!([
        { "name": "price", "type": { "money": { "currency": "EUR" } }, "required": true },
        { "name": "tags", "type": { "list": "string" }, "required": true }
    ]);
    doc
}

fn route(h: &mut Harness, price: &str, tags: Value) -> Vec<String> {
    h.start(vars(&[("price", json!({ "amount": price, "currency": "EUR" })), ("tags", tags)]));
    h.waiting_at()
}

#[test]
fn decisions_route_by_real_cel() {
    let doc = routing_process();
    // 0.1 * 3 is exactly 0.3: decimals, no floating point.
    assert_eq!(route(&mut Harness::new(&doc), "0.1", json!([])), ["exact_t"]);
    assert_eq!(route(&mut Harness::new(&doc), "0.2", json!(["urgent", "x"])), ["tagged_t"]);
    // T0 is Sunday 2026-10-11 09:00 UTC.
    assert_eq!(route(&mut Harness::new(&doc), "0.2", json!(["urgent"])), ["weekend_t"]);
    // On Monday the initiator's manager decides.
    let mut h = Harness::new(&doc);
    h.now = datetime!(2026-10-12 09:00 UTC);
    assert_eq!(route(&mut h, "0.2", json!([])), ["manager_t"]);
    h.assert_replays();
    // The same instant is still Sunday at the injected -10:00 offset.
    let mut h = Harness::with(program(&doc), CelEvaluator::default().with_tz(offset!(-10:00)));
    h.now = datetime!(2026-10-12 09:00 UTC);
    assert_eq!(route(&mut h, "0.2", json!([])), ["weekend_t"]);
    h.assert_replays();
}

#[test]
fn set_assigns_typed_values_that_later_expressions_read_back() {
    let mut doc = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "calc", "kind": "set", "assign": {
                "total": "expr:cel: price * 3",
                "due": "expr:cel: now() + duration('P2D')"
            }},
            { "id": "d", "kind": "decision", "branches": [
                { "name": "big", "when": "expr:cel: total != null && due != null && total.amount > 1.0 && due > now()" }
            ]},
            task("big_t"), task("small_t"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "calc", "next"),
            edge("e2", "calc", "d", "next"),
            edge("e3", "d", "big_t", "big"),
            edge("e4", "d", "small_t", "default"),
            edge("e5", "big_t", "done", "submitted"),
            edge("e6", "small_t", "done", "submitted"),
        ]),
        &[],
    );
    doc["variables"] = json!([
        { "name": "price", "type": { "money": { "currency": "EUR" } }, "required": true },
        { "name": "total", "type": { "money": { "currency": "EUR" } } },
        { "name": "due", "type": "datetime" }
    ]);
    let mut h = Harness::new(&doc);
    h.start(vars(&[("price", json!({ "amount": "0.40", "currency": "EUR" }))]));
    assert_eq!(h.state().variables["total"], json!({ "amount": "1.20", "currency": "EUR" }));
    assert_eq!(h.state().variables["due"], json!("2026-10-13T09:00:00Z"));
    assert_eq!(h.waiting_at(), ["big_t"]);
    h.assert_replays();
}

#[test]
fn expression_budget_exhaustion_is_an_incident_at_every_site() {
    let tiny = || CelEvaluator::default().with_budget(ExprBudget { max_steps: 1, ..ExprBudget::default() });
    let set = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "calc", "kind": "set", "assign": { "n": "expr:cel: n + 1" } },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "calc", "next"), edge("e2", "calc", "done", "next")]),
        &[("n", "integer")],
    );
    let timer = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "wait", "kind": "wait.timer", "at": "expr:cel: when + duration('PT1H')" },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "wait", "next"), edge("e2", "wait", "done", "fired")]),
        &[("when", "datetime")],
    );
    let signal = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "sig", "kind": "wait.signal", "correlation": "expr:cel: 'order-' + order" },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "sig", "next"), edge("e2", "sig", "done", "received")]),
        &[("order", "string")],
    );
    let cases = [
        (set, vars(&[("n", json!(1))])),
        (timer, vars(&[("when", json!("2026-12-01T10:00:00Z"))])),
        (signal, vars(&[("order", json!("PO-1"))])),
    ];
    for (doc, start) in cases {
        let mut h = Harness::with(program(&doc), tiny());
        h.start(start);
        let (_, incident) = h.state().incidents().next().expect("an incident, not a panic");
        assert_eq!(incident.code.as_str(), "IR-R010");
        assert!(incident.message.contains("EXPR-R001"), "{}", incident.message);
        assert_eq!(h.state().status, InstanceStatus::Running);
        h.assert_replays();
        // The default budget runs the same instance to its wait or end.
        let mut h = Harness::new(&doc);
        h.start(start_of(&h));
        assert!(h.state().incidents().next().is_none());
    }
}

/// Start variables for the budget cases, read off the declared variables.
fn start_of(h: &Harness) -> std::collections::BTreeMap<String, Value> {
    let mut v = std::collections::BTreeMap::new();
    for var in &h.program.ir().variables {
        let value = match var.name.as_str() {
            "n" => json!(1),
            "when" => json!("2026-12-01T10:00:00Z"),
            _ => json!("PO-1"),
        };
        v.insert(var.name.clone(), value);
    }
    v
}
