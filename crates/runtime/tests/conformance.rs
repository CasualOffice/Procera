//! Runtime conformance suite (docs/15 §4, layout in docs/11 §15): every case under
//! `conformance/runtime/<group>/<case>/` compiles its `process.ir.json`, applies `script.json`,
//! and must reproduce `expected.trace.jsonl` and `expected.final.json` exactly.
//!
//! `PROCERA_BLESS=1 cargo test -p procera-runtime --test conformance` rewrites the expected
//! files; review the diff before committing it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::testkit::MiniEvaluator;
use procera_runtime::{Budget, Command, Event, InstanceState, TimerPurpose, TokenState};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn cases() -> Vec<PathBuf> {
    let root = repo_root().join("conformance/runtime");
    let mut out = Vec::new();
    for group in std::fs::read_dir(&root).unwrap() {
        let group = group.unwrap().path();
        if !group.is_dir() {
            continue;
        }
        for case in std::fs::read_dir(&group).unwrap() {
            let case = case.unwrap().path();
            if case.join("process.ir.json").exists() {
                out.push(case);
            }
        }
    }
    out.sort();
    out
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn harness(case: &Path) -> Harness {
    let mut eval = MiniEvaluator::default();
    let mut budget = Budget::default();
    let bundle_path = case.join("bundle.json");
    if bundle_path.exists() {
        let bundle = read_json(&bundle_path);
        for (k, v) in bundle["rules"].as_object().into_iter().flatten() {
            eval.rules.insert(k.clone(), v.as_str().unwrap().to_owned());
        }
        for (k, v) in bundle["expressions"].as_object().into_iter().flatten() {
            eval.overrides.insert(k.clone(), v.clone());
        }
        if let Some(n) = bundle["budget"]["max_steps"].as_u64() {
            budget.max_steps = u32::try_from(n).unwrap();
        }
    }
    let mut h = Harness::with(program(&read_json(&case.join("process.ir.json"))), eval);
    h.budget = budget;
    h
}

fn obj_vars(v: &Value) -> BTreeMap<String, Value> {
    v.as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
}

/// The activation key at `node`: the live wait, else the last one issued there (stale keys
/// let a script prove that late events are refused).
fn key(h: &Harness, node: &str) -> String {
    if let Some((_, w)) = h.state().waits().find(|(t, _)| t.node == node) {
        return w.key.clone();
    }
    h.commands
        .iter()
        .rev()
        .find_map(|c| match c {
            Command::CreateWorkItem { key, node: n, .. }
            | Command::SubscribeSignal { key, node: n, .. }
            | Command::CallConnector { key, node: n, .. }
            | Command::StartSubprocess { key, node: n, .. }
            | Command::OpenCase { key, node: n, .. } => (n == node).then(|| key.clone()),
            Command::ScheduleTimer { key, node: n, purpose: TimerPurpose::Wait, .. } => {
                (n == node).then(|| key.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no activation at {node}"))
}

/// Applies one script step. `Err` carries the rejection.
fn step(h: &mut Harness, s: &Value) -> Result<(), String> {
    let (op, arg) = s.as_object().and_then(|m| m.iter().next()).expect("one key per step");
    let node = || arg["node"].as_str().unwrap_or_default().to_owned();
    let outcome = || arg["outcome"].as_str().unwrap_or_default().to_owned();
    let event = match op.as_str() {
        "start" => {
            h.start(obj_vars(&arg["variables"]));
            return Ok(());
        }
        "complete" => Event::WorkItemCompleted {
            key: key(h, &node()),
            outcome: outcome(),
            variables: obj_vars(&arg["variables"]),
        },
        "fire_timer" => Event::TimerFired { key: key(h, &node()) },
        "fire_boundary" => {
            Event::TimerFired { key: format!("{}/boundary/{}", key(h, &node()), arg["index"]) }
        }
        "fire_timeout" => Event::TimerFired { key: format!("{}/timeout", key(h, &node())) },
        "signal" => Event::SignalReceived { key: key(h, &node()), variables: obj_vars(&arg["variables"]) },
        "service" => Event::ServiceCompleted {
            key: key(h, &node()),
            outcome: outcome(),
            variables: obj_vars(&arg["variables"]),
        },
        "subprocess" => {
            Event::SubprocessFinished { key: key(h, &node()), outcome: outcome(), variables: BTreeMap::new() }
        }
        "case_closed" => Event::CaseClosed { key: key(h, &node()) },
        "cancel" => Event::Cancel { reason: arg["reason"].as_str().unwrap_or_default().to_owned() },
        "continue" => Event::Continue,
        "advance_clock" => {
            let secs = procera_ir::model::Duration(arg.as_str().unwrap().to_owned()).seconds().unwrap();
            h.now += time::Duration::seconds(i64::try_from(secs).unwrap());
            return Ok(());
        }
        "reject" => {
            // The step must be refused and leave the state unchanged.
            let before = h.state.clone();
            let result = step(h, arg);
            assert_eq!(h.state, before, "a rejected step changed the state");
            return match result {
                Err(_) => Ok(()),
                Ok(()) => Err(format!("expected {arg} to be rejected")),
            };
        }
        other => panic!("unknown script step {other}"),
    };
    h.send(event).map(|_| ()).map_err(|e| e.to_string())
}

fn trace_lines(state: &InstanceState) -> String {
    let mut out = String::new();
    for e in &state.trace {
        let mut v = serde_json::to_value(e).unwrap();
        let m = v.as_object_mut().unwrap();
        m.remove("seq");
        m.remove("at");
        let Some(Value::Object(event)) = m.remove("event") else { panic!("event") };
        m.extend(event);
        out.push_str(&serde_json::to_string(&v).unwrap());
        out.push('\n');
    }
    out
}

fn final_json(h: &Harness) -> Value {
    let s = h.state();
    let tokens: Vec<Value> = s
        .tokens
        .values()
        .map(|t| match &t.state {
            TokenState::Ready => json!({ "node": t.node, "state": "ready" }),
            TokenState::Waiting(w) => json!({ "node": t.node, "state": "waiting", "wait": w.kind }),
            TokenState::Incident(i) => json!({ "node": t.node, "state": "incident", "code": i.code }),
        })
        .collect();
    const KEEP: [&str; 11] = [
        "command",
        "node",
        "kind",
        "purpose",
        "due",
        "topic",
        "result",
        "code",
        "template",
        "correlation",
        "outcomes",
    ];
    let commands: Vec<Value> = h
        .commands
        .iter()
        .map(|c| {
            let v = serde_json::to_value(c).unwrap();
            let m: Map<String, Value> = v
                .as_object()
                .unwrap()
                .iter()
                .filter(|(k, _)| KEEP.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(m)
        })
        .collect();
    json!({
        "status": s.status,
        "revision": s.revision,
        "tokens": tokens,
        "open_splits": s.splits.len(),
        "variables": s.variables,
        "commands": commands,
    })
}

#[test]
fn runtime_conformance_suite() {
    let bless = std::env::var("PROCERA_BLESS").is_ok_and(|v| v == "1");
    let all = cases();
    assert!(all.len() >= 20, "expected the BPMS-082 fixture set, found {}", all.len());
    let mut failures = Vec::new();
    for case in &all {
        let name = case.strip_prefix(repo_root().join("conformance/runtime")).unwrap().display().to_string();
        let mut h = harness(case);
        let script = read_json(&case.join("script.json"));
        for (i, s) in script.as_array().unwrap().iter().enumerate() {
            if let Err(e) = step(&mut h, s) {
                panic!("{name}: step {i} {s}: {e}");
            }
        }
        h.assert_replays();
        let trace = trace_lines(h.state());
        let fin = serde_json::to_string_pretty(&final_json(&h)).unwrap() + "\n";
        let (tp, fp) = (case.join("expected.trace.jsonl"), case.join("expected.final.json"));
        if bless {
            std::fs::write(&tp, &trace).unwrap();
            std::fs::write(&fp, &fin).unwrap();
            continue;
        }
        if std::fs::read_to_string(&tp).ok().as_deref() != Some(trace.as_str()) {
            failures.push(format!("{name}: trace differs:\n{trace}"));
        }
        if std::fs::read_to_string(&fp).ok().map(|t| serde_json::from_str::<Value>(&t).unwrap())
            != Some(final_json(&h))
        {
            failures.push(format!("{name}: final state differs:\n{fin}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
