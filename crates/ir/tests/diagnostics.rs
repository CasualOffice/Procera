//! One test per diagnostic code: a broken document reports the code at the expected pointer.
//! The valid side of every check is the five reference processes (tests/reference.rs).
// Helpers outside #[test] fns may unwrap too (clippy.toml allows it inside tests).
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use procera_ir::{Code, CompileContext, Resolution, Severity, compile, compile_str};
use serde_json::{Value, json};

fn fin() -> Value {
    reference("fin.purchase-requisition")
}

fn nodes(doc: &mut Value) -> &mut Vec<Value> {
    doc["nodes"].as_array_mut().unwrap()
}

fn edges(doc: &mut Value) -> &mut Vec<Value> {
    doc["edges"].as_array_mut().unwrap()
}

/// `start → x → done`, with `x` handling `outcome`.
fn chain(x: Value, outcome: &str) -> Value {
    json!({
        "schemaVersion": 1,
        "process": { "key": "t", "version": 1, "name": { "en": "T" } },
        "variables": [ { "name": "amount", "type": "decimal", "classification": "internal" } ],
        "nodes": [ { "id": "start", "kind": "start" }, x, { "id": "done", "kind": "end" } ],
        "edges": [
            { "id": "e1", "from": "start", "to": "x" },
            { "id": "e2", "from": "x", "to": "done", "on": outcome }
        ]
    })
}

fn task() -> Value {
    json!({ "id": "x", "kind": "human.task",
            "assignment": { "candidates": [ { "type": "role", "ref": "process-owner" } ] } })
}

#[track_caller]
fn expect_with(doc: &Value, ctx: &CompileContext<'_>, code: Code, pointer: &str) {
    let out = compile(doc, ctx);
    assert!(
        out.diagnostics.iter().any(|d| d.code == code && d.pointer == pointer),
        "expected {code} at '{pointer}', got {:#?}",
        out.diagnostics
    );
    if code.severity() == Severity::Error {
        assert!(out.compiled.is_none(), "errors must block the output");
    }
}

#[track_caller]
fn expect(doc: &Value, code: Code, pointer: &str) {
    expect_with(doc, &CompileContext::default(), code, pointer);
}

#[test]
fn chain_fixture_is_valid() {
    let out = offline(&chain(task(), "submitted"));
    assert_eq!(codes(&out), ["IR-I001"], "{:#?}", out.diagnostics);
}

#[test]
fn e001_start_count() {
    let mut d = fin();
    nodes(&mut d).push(json!({"id": "start2", "kind": "start"}));
    expect(&d, Code::E001, "/nodes/10");
    let mut d = chain(task(), "submitted");
    d["nodes"][0]["kind"] = json!("set");
    expect(&d, Code::E001, "/nodes");
}

#[test]
fn e002_duplicate_node() {
    let mut d = fin();
    nodes(&mut d).push(json!({"id": "revise", "kind": "set", "assign": {}}));
    expect(&d, Code::E002, "/nodes/10/id");
}

#[test]
fn e003_duplicate_edge() {
    let mut d = fin();
    edges(&mut d).push(json!({"id": "e1", "from": "notify_approved", "to": "end_approved"}));
    expect(&d, Code::E003, "/edges/17/id");
}

#[test]
fn e004_missing_endpoint() {
    let mut d = fin();
    edges(&mut d).push(json!({"id": "e99", "from": "start", "to": "ghost"}));
    expect(&d, Code::E004, "/edges/17/to");
}

#[test]
fn e005_unhandled_outcome_and_unhandled_fail() {
    let mut d = fin();
    edges(&mut d).retain(|e| !(e["from"] == "manager_approval" && e["on"] == "changes_requested"));
    expect(&d, Code::E005, "/nodes/2");
    d["nodes"][2]["unhandled"] = json!("fail");
    let out = offline(&d);
    assert!(!codes(&out).contains(&"IR-E005"), "{:#?}", out.diagnostics);
}

#[test]
fn e006_undeclared_outcome() {
    let mut d = fin();
    edges(&mut d).push(json!({"id": "e99", "from": "manager_approval", "to": "end_approved", "on": "maybe"}));
    expect(&d, Code::E006, "/edges/17/on");
}

#[test]
fn e007_decision_default() {
    let mut d = fin();
    edges(&mut d).retain(|e| !(e["from"] == "route" && e["on"] == "default"));
    expect(&d, Code::E007, "/nodes/1");
}

fn parallel(branches: &[&str]) -> Value {
    let mut nodes = vec![
        json!({"id": "start", "kind": "start"}),
        json!({"id": "fork", "kind": "parallel.split", "joinId": "join",
               "branches": branches.iter().map(|b| json!({"name": b})).collect::<Vec<_>>()}),
        json!({"id": "join", "kind": "parallel.join"}),
        json!({"id": "done", "kind": "end"}),
    ];
    let mut edges = vec![
        json!({"id": "e_start", "from": "start", "to": "fork"}),
        json!({"id": "e_join", "from": "join", "to": "done"}),
    ];
    for b in branches {
        nodes.push(json!({"id": format!("t_{b}"), "kind": "set", "assign": {}}));
        edges.push(json!({"id": format!("e_{b}"), "from": "fork", "to": format!("t_{b}"), "on": b}));
        edges.push(json!({"id": format!("e_{b}_j"), "from": format!("t_{b}"), "to": "join"}));
    }
    json!({
        "schemaVersion": 1,
        "process": { "key": "p", "version": 1, "name": { "en": "P" } },
        "variables": [], "nodes": nodes, "edges": edges
    })
}

#[test]
fn e008_parallel_structure() {
    assert_eq!(codes(&offline(&parallel(&["a", "b"]))), ["IR-I001"]);
    // A branch escaping to an end.
    let d = reference("hr.employee-onboarding");
    let mut d2 = d.clone();
    edges(&mut d2).retain(|e| e["from"] != "facilities");
    edges(&mut d2).push(json!({"id": "e90", "from": "facilities", "to": "end_done", "on": "submitted"}));
    let fork = d["nodes"].as_array().unwrap().iter().position(|n| n["id"] == "fork").unwrap();
    expect(&d2, Code::E008, &format!("/nodes/{fork}"));
    // Two branches merging before the join.
    let mut d = parallel(&["a", "b"]);
    edges(&mut d).retain(|e| e["id"] != "e_a_j");
    edges(&mut d).push(json!({"id": "e_cross", "from": "t_a", "to": "t_b"}));
    expect(&d, Code::E008, "/nodes/1");
    // A split without a join, and a join nobody closes.
    let mut d = parallel(&["a", "b"]);
    d["nodes"][1].as_object_mut().unwrap().remove("joinId");
    expect(&d, Code::E008, "/nodes/1");
    expect(&d, Code::E008, "/nodes/2");
}

#[test]
fn e009_unreachable() {
    let mut d = fin();
    nodes(&mut d).push(json!({"id": "orphan", "kind": "end"}));
    expect(&d, Code::E009, "/nodes/10");
}

#[test]
fn e010_no_reachable_end() {
    let mut d = chain(task(), "submitted");
    d["edges"][1]["to"] = json!("x");
    expect(&d, Code::E010, "/nodes");
}

#[test]
fn e011_unbounded_loop() {
    let mut d = chain(json!({"id": "x", "kind": "set", "assign": {}}), "next");
    nodes(&mut d).push(json!({"id": "y", "kind": "decision", "branches": [{"name": "again", "when": "expr:cel: amount > 1"}]}));
    d["edges"][1]["to"] = json!("y");
    edges(&mut d).push(json!({"id": "e3", "from": "y", "to": "x", "on": "again"}));
    edges(&mut d).push(json!({"id": "e4", "from": "y", "to": "done", "on": "default"}));
    expect(&d, Code::E011, "/nodes/1");
    d["nodes"][1]["maxIterations"] = json!(5);
    assert!(!codes(&offline(&d)).contains(&"IR-E011"));
}

#[test]
fn e012_limits() {
    let names: Vec<String> = (0..51).map(|i| format!("b{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    expect(&parallel(&refs), Code::E012, "/nodes/1/branches");
    // Eleven nested splits.
    let mut ns = vec![json!({"id": "start", "kind": "start"}), json!({"id": "done", "kind": "end"})];
    let mut es = vec![
        json!({"id": "e_in", "from": "start", "to": "s0"}),
        json!({"id": "e_out", "from": "j0", "to": "done"}),
    ];
    for k in 0..11 {
        ns.push(json!({"id": format!("s{k}"), "kind": "parallel.split", "joinId": format!("j{k}"), "branches": [{"name": "a"}]}));
        ns.push(json!({"id": format!("j{k}"), "kind": "parallel.join"}));
        let inner = if k == 10 { format!("j{k}") } else { format!("s{}", k + 1) };
        es.push(json!({"id": format!("ea{k}"), "from": format!("s{k}"), "to": inner, "on": "a"}));
        if k < 10 {
            es.push(json!({"id": format!("eb{k}"), "from": format!("j{}", k + 1), "to": format!("j{k}")}));
        }
    }
    let d = json!({"schemaVersion": 1, "process": {"key": "n", "version": 1, "name": {"en": "N"}},
                   "variables": [], "nodes": ns, "edges": es});
    expect(&d, Code::E012, "/nodes/22");
}

#[test]
fn e013_edges_into_start_or_out_of_end() {
    let mut d = fin();
    edges(&mut d).push(json!({"id": "e99", "from": "end_approved", "to": "start"}));
    expect(&d, Code::E013, "/edges/17");
}

#[test]
fn e014_unknown_sod_reference() {
    let mut d = fin();
    d["nodes"][3]["assignment"]["excludeActorsOf"] = json!(["nobody"]);
    expect(&d, Code::E014, "/nodes/3/assignment/excludeActorsOf/0");
}

#[test]
fn e015_unknown_boundary_target() {
    let mut d = fin();
    d["nodes"][2]["boundary"] = json!([{"on": "timeout", "after": "P5D", "to": "ghost"}]);
    expect(&d, Code::E015, "/nodes/2/boundary/0/to");
}

#[test]
fn e016_duplicate_variable() {
    let mut d = fin();
    d["variables"].as_array_mut().unwrap().push(json!({"name": "amount", "type": "decimal"}));
    expect(&d, Code::E016, "/variables/6/name");
}

#[test]
fn e017_outcome_with_two_edges() {
    let mut d = fin();
    edges(&mut d)
        .push(json!({"id": "e99", "from": "manager_approval", "to": "notify_rejected", "on": "approved"}));
    expect(&d, Code::E017, "/edges/17/on");
}

#[test]
fn e018_required_fields() {
    let mut d = reference("it.access-request");
    let i = d["nodes"].as_array().unwrap().iter().position(|n| n["id"] == "provision").unwrap();
    d["nodes"][i].as_object_mut().unwrap().remove("idempotencyKey");
    expect(&d, Code::E018, &format!("/nodes/{i}"));
    let mut d = chain(task(), "submitted");
    d["nodes"][1].as_object_mut().unwrap().remove("assignment");
    expect(&d, Code::E018, "/nodes/1");
    let d = chain(json!({"id": "x", "kind": "decision", "branches": [{"name": "big"}]}), "default");
    expect(&d, Code::E018, "/nodes/1/branches/0");
}

#[test]
fn e019_malformed_expression() {
    let mut d = chain(task(), "submitted");
    d["nodes"][1]["assignment"]["candidates"][0] = json!({"type": "user", "ref": "expr:js: x"});
    expect(&d, Code::E019, "/nodes/1/assignment/candidates/0/ref");
    let d = chain(json!({"id": "x", "kind": "set", "assign": {"amount": "expr:cel: size(amount"}}), "next");
    expect(&d, Code::E019, "/nodes/1/assign/amount");
}

#[test]
fn e020_undeclared_variable() {
    let d = chain(json!({"id": "x", "kind": "set", "assign": {"total": "expr:cel: amount * rate"}}), "next");
    expect(&d, Code::E020, "/nodes/1/assign/total");
    let out = offline(&d);
    let msgs: Vec<_> = out.diagnostics.iter().filter(|x| x.code == Code::E020).map(|x| &x.message).collect();
    assert_eq!(msgs.len(), 2, "{msgs:?}");
}

#[test]
fn e021_restricted_in_notification_or_event_field() {
    let mut d = reference("hr.employee-onboarding");
    let i = d["nodes"].as_array().unwrap().iter().position(|n| n["id"] == "welcome").unwrap();
    d["nodes"][i]["to"]["candidates"][0]["ref"] = json!("expr:cel: salary");
    expect(&d, Code::E021, &format!("/nodes/{i}/to/candidates/0/ref"));
    let mut d = reference("hr.employee-onboarding");
    let i = d["nodes"].as_array().unwrap().iter().position(|n| n["id"] == "laptop_delivered").unwrap();
    d["nodes"][i]["correlation"] = json!("expr:cel: salary");
    expect(&d, Code::E021, &format!("/nodes/{i}/correlation"));
}

#[test]
fn e022_unknown_ref_key() {
    let mut d = fin();
    d["nodes"][5]["form"] = json!("nope");
    expect(&d, Code::E022, "/nodes/5/form");
}

#[test]
fn e023_unresolved_reference() {
    let env = FakeEnv {
        missing_refs: vec![("form:fin.purchase-request@1", Resolution::NotPublished)],
        ..Default::default()
    };
    let ctx = CompileContext { environment: Some(&env), ..Default::default() };
    expect_with(&fin(), &ctx, Code::E023, "/refs/forms/request");
}

#[test]
fn e024_connector_unusable() {
    let env = FakeEnv { disabled_connectors: vec!["connector:http-provisioning"], ..Default::default() };
    let ctx = CompileContext { environment: Some(&env), ..Default::default() };
    expect_with(&reference("it.access-request"), &ctx, Code::E024, "/refs/connectors/provisioning");
}

#[test]
fn e025_unknown_group() {
    let env = FakeEnv { missing_principals: vec!["finance-approvers"], ..Default::default() };
    let ctx = CompileContext { environment: Some(&env), ..Default::default() };
    expect_with(&fin(), &ctx, Code::E025, "/nodes/3/assignment/candidates/0/ref");
}

#[test]
fn environment_checks_pass_and_drop_i001() {
    let env = FakeEnv::default();
    let ctx = CompileContext { environment: Some(&env), ..Default::default() };
    for name in REFERENCE {
        let out = compile(&reference(name), &ctx);
        assert!(out.diagnostics.is_empty(), "{name}: {:#?}", out.diagnostics);
    }
}

#[test]
fn e026_sod_unsatisfiable() {
    let mut d = chain(task(), "submitted");
    d["nodes"][1]["assignment"] =
        json!({"candidates": [{"type": "user", "ref": "expr:cel: initiator"}], "excludeInitiator": true});
    expect(&d, Code::E026, "/nodes/1/assignment");
}

#[test]
fn e027_business_time_without_calendar() {
    let mut d = fin();
    d["refs"].as_object_mut().unwrap().remove("calendars");
    expect(&d, Code::E027, "/nodes/2/due");
    expect(&d, Code::E027, "/sla/0/business");
}

#[test]
fn e028_timer_shape() {
    let d = chain(json!({"id": "x", "kind": "wait.timer", "after": "P1D", "at": "expr:cel: now"}), "fired");
    expect(&d, Code::E028, "/nodes/1");
    let mut d = chain(task(), "submitted");
    d["nodes"][1]["boundary"] = json!([{"on": "timeout", "to": "done"}]);
    expect(&d, Code::E028, "/nodes/1/boundary/0");
    let d = chain(json!({"id": "x", "kind": "wait.timer", "after": "PT0S"}), "fired");
    expect(&d, Code::E028, "/nodes/1/after");
}

#[test]
fn e029_instant_without_offset() {
    let d = chain(
        json!({"id": "x", "kind": "wait.timer", "at": "expr:cel: datetime(\"2026-11-01T09:00:00\")"}),
        "fired",
    );
    expect(&d, Code::E029, "/nodes/1/at");
    let ok = chain(
        json!({"id": "x", "kind": "wait.timer", "at": "expr:cel: datetime(\"2026-11-01T09:00:00+05:30\")"}),
        "fired",
    );
    assert_eq!(codes(&offline(&ok)), ["IR-I001"]);
}

#[test]
fn e030_escalations() {
    let mut d = fin();
    d["nodes"][2]["escalations"] = json!([{"at": "P1D", "action": "reassign"}]);
    expect(&d, Code::E030, "/nodes/2/escalations/0");
    d["nodes"][2]["escalations"] = json!([{"at": "P1D", "action": "outcome", "outcome": "maybe"}]);
    expect(&d, Code::E030, "/nodes/2/escalations/0");
    d["nodes"][2]["escalations"] = json!([{"at": "P1D", "action": "outcome", "outcome": "rejected"}]);
    assert!(!codes(&offline(&d)).contains(&"IR-E030"));
    d["process"]["criticality"] = json!("controlled");
    expect(&d, Code::E030, "/nodes/2/escalations/0");
}

#[test]
fn e031_boundary_on_wrong_kind() {
    let mut d = fin();
    d["nodes"][6]["boundary"] = json!([{"on": "cancel", "to": "end_rejected"}]);
    expect(&d, Code::E031, "/nodes/6/boundary");
}

#[test]
fn e032_sla_unknown_node() {
    let mut d = fin();
    d["sla"][0]["scope"] = json!("node:ghost");
    expect(&d, Code::E032, "/sla/0/scope");
    d["sla"][0]["pauseWhile"] = json!(["wait.signal:revise"]);
    expect(&d, Code::E032, "/sla/0/pauseWhile/0");
}

#[test]
fn e033_duplicate_sla() {
    let mut d = fin();
    let sla = d["sla"][0].clone();
    d["sla"].as_array_mut().unwrap().push(sla);
    expect(&d, Code::E033, "/sla/1/id");
}

#[test]
fn e034_duplicate_branch() {
    let mut d = fin();
    d["nodes"][1]["branches"].as_array_mut().unwrap().push(json!({"name": "cfo"}));
    expect(&d, Code::E034, "/nodes/1/branches/3/name");
}

/// `start → x → done` with typed variables, for the expression type checks.
fn typed(x: Value, outcome: &str) -> Value {
    let mut d = chain(x, outcome);
    d["variables"] = json!([
        { "name": "amount", "type": "decimal", "required": true },
        { "name": "limit", "type": { "money": { "currency": "EUR" } }, "required": true },
        { "name": "fee", "type": { "money": { "currency": "INR" } }, "required": true },
        { "name": "note", "type": "string" },
        { "name": "urgent", "type": "boolean" },
        { "name": "level", "type": { "enum": ["low", "high"] }, "required": true },
        { "name": "salary", "type": "decimal", "classification": "restricted", "required": true }
    ]);
    if d["nodes"][1]["kind"] == "decision" {
        edges(&mut d).push(json!({"id": "e-default", "from": "x", "to": "done", "on": "default"}));
    }
    d
}

fn message(doc: &Value, code: Code, pointer: &str) -> String {
    let out = offline(doc);
    let d = out.diagnostics.iter().find(|d| d.code == code && d.pointer == pointer);
    d.unwrap_or_else(|| panic!("no {code} at {pointer}: {:#?}", out.diagnostics)).message.clone()
}

#[test]
fn e035_type_errors() {
    // Assigning a decimal to a string variable.
    let d = typed(json!({"id": "x", "kind": "set", "assign": {"note": "expr:cel: amount * 2"}}), "next");
    expect(&d, Code::E035, "/nodes/1/assign/note");
    assert!(message(&d, Code::E035, "/nodes/1/assign/note").starts_with("EXPR-E017 at 1:1:"));
    // A non-boolean branch condition.
    let br = |when: &str| json!({"id": "x", "kind": "decision", "branches": [{"name": "a", "when": when}]});
    let d = typed(br("expr:cel: amount + 1"), "a");
    expect(&d, Code::E035, "/nodes/1/branches/0/when");
    // Mixing currencies, ordering across types, a value outside the enum, a bad literal.
    for (src, expr_code) in [
        ("expr:cel: limit + fee > limit", "EXPR-E020"),
        ("expr:cel: amount > 'x'", "EXPR-E014"),
        ("expr:cel: level == 'medium'", "EXPR-E019"),
        ("expr:cel: date('2026-02-30') == date('2026-03-01')", "EXPR-E021"),
    ] {
        let d = typed(br(src), "a");
        let msg = message(&d, Code::E035, "/nodes/1/branches/0/when");
        assert!(msg.starts_with(expr_code), "{src}: {msg}");
    }
    // A timer instant must be a datetime; an idempotency key a string.
    let d = typed(json!({"id": "x", "kind": "wait.timer", "at": "expr:cel: amount"}), "fired");
    expect(&d, Code::E035, "/nodes/1/at");
    // An assignee must be a principal, group or id.
    let mut d = typed(task(), "submitted");
    d["nodes"][1]["assignment"]["candidates"][0] = json!({"type": "user", "ref": "expr:cel: amount"});
    expect(&d, Code::E035, "/nodes/1/assignment/candidates/0/ref");
    // A variable type with no expression type (money without a currency).
    let mut d = typed(task(), "submitted");
    d["variables"][1]["type"] = json!({"money": {}});
    expect(&d, Code::E035, "/variables/1/type");
}

#[test]
fn e036_null_misuse() {
    let br = |when: &str| json!({"id": "x", "kind": "decision", "branches": [{"name": "a", "when": when}]});
    // An optional boolean decides nothing on its own (docs/11 §6).
    let d = typed(br("expr:cel: urgent"), "a");
    expect(&d, Code::E036, "/nodes/1/branches/0/when");
    assert!(message(&d, Code::E036, "/nodes/1/branches/0/when").starts_with("EXPR-E018"));
    // Using an optional value in arithmetic, at its exact position in the source.
    let d = typed(br("expr:cel: amount > 1 && size(note) > 3"), "a");
    let msg = message(&d, Code::E036, "/nodes/1/branches/0/when");
    assert!(msg.starts_with("EXPR-E016 at 1:"), "{msg}");
    // Guards make both fine.
    for ok in ["expr:cel: urgent == true", "expr:cel: note != null && size(note) > 3"] {
        assert_eq!(codes(&offline(&typed(br(ok), "a"))), ["IR-I001"], "{ok}");
    }
}

#[test]
fn restricted_reads_only_fail_in_event_facing_fields() {
    // A decision may read a restricted variable; nothing about it leaves the instance.
    let br = json!({"id": "x", "kind": "decision", "branches": [{"name": "a", "when": "expr:cel: salary > 100.0"}]});
    assert_eq!(codes(&offline(&typed(br, "a"))), ["IR-I001"]);
    // An idempotency key may not.
    let mut d = typed(
        json!({"id": "x", "kind": "service.call", "connector": "c", "action": "a",
               "idempotencyKey": "expr:cel: string(salary)"}),
        "ok",
    );
    d["refs"] = json!({"connectors": {"c": "connector:c"}});
    edges(&mut d).push(json!({"id": "e3", "from": "x", "to": "done", "on": "error"}));
    expect(&d, Code::E021, "/nodes/1/idempotencyKey");
    assert!(message(&d, Code::E021, "/nodes/1/idempotencyKey").starts_with("EXPR-E023 at 1:8:"));
}

#[test]
fn compiled_output_carries_every_checked_expression() {
    let d = typed(
        json!({"id": "x", "kind": "decision", "branches": [
            {"name": "a", "when": "expr:cel: amount > 10"},
            {"name": "b", "when": "expr:cel: level == 'high'"}
        ]}),
        "a",
    );
    let mut d = d;
    edges(&mut d).push(json!({"id": "e3", "from": "x", "to": "done", "on": "b"}));
    let compiled = offline(&d).compiled.unwrap();
    assert_eq!(compiled.expressions.len(), 2);
    let a = compiled.expressions.get("x", "branches/0/when").unwrap();
    assert_eq!(a.source, "amount > 10");
    assert_eq!(a.ty, procera_expr::Type::Bool);
    assert!(compiled.expressions.get("x", "branches/1/when").is_some());
    // The checked forms are derived data: they do not change the checksum.
    let again = offline(&serde_json::to_value(&compiled.ir).unwrap()).compiled.unwrap();
    assert_eq!(again.checksum, compiled.checksum);
    assert_eq!(again.expressions, compiled.expressions);
}

#[test]
fn e100_schema() {
    let mut d = fin();
    d["nodes"][0]["script"] = json!("rm -rf /");
    expect(&d, Code::E100, "/nodes/0");
    let mut d = fin();
    d["schemaVersion"] = json!(2);
    expect(&d, Code::E100, "/schemaVersion");
    let out = compile_str("{ not json", &CompileContext::default());
    assert_eq!(codes(&out), ["IR-E100"]);
}

#[test]
fn w001_readers_without_restriction() {
    let mut d = fin();
    d["variables"][0]["readers"] = json!(["role:hr"]);
    expect(&d, Code::W001, "/variables/0/readers");
    assert!(offline(&d).compiled.is_some(), "warnings do not block");
}

#[test]
fn w002_sod_reference_to_non_human() {
    let mut d = fin();
    d["nodes"][3]["assignment"]["excludeActorsOf"] = json!(["notify_approved"]);
    expect(&d, Code::W002, "/nodes/3/assignment/excludeActorsOf/0");
}

#[test]
fn i001_offline_compile() {
    expect(&fin(), Code::I001, "");
}

#[test]
fn every_code_has_a_fixed_severity_and_unique_wire_form() {
    let mut seen = std::collections::BTreeSet::new();
    for c in Code::ALL {
        assert!(seen.insert(c.as_str()), "{c} twice");
        let prefix = match c.severity() {
            Severity::Error => "IR-E",
            Severity::Warning => "IR-W",
            Severity::Info => "IR-I",
        };
        assert!(c.as_str().starts_with(prefix), "{c}");
    }
}

#[test]
fn docs_11_lists_every_code() {
    let doc = std::fs::read_to_string(repo_root().join("docs/11-WORKFLOW-IR-SPEC.md")).unwrap();
    let section = doc.split("As built (BPMS-080)").nth(1).expect("docs/11 has the as-built section");
    for c in Code::ALL {
        assert!(section.contains(&format!("| {} |", c.as_str())), "docs/11 as-built table lacks {c}");
    }
}
