//! One test (or a few) per node kind and gateway (BPMS-082).
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::testkit::MiniEvaluator;
use procera_runtime::{Command, InstanceResult, InstanceStatus, TimerDue, WaitKind, WorkKind};
use serde_json::json;

fn seq(nodes: serde_json::Value, edges: serde_json::Value) -> Harness {
    Harness::new(&process(nodes, edges, &[("flag", "boolean"), ("n", "integer"), ("m", "integer")]))
}

#[test]
fn start_to_end_completes_with_events() {
    let mut h = seq(
        json!([{ "id": "start", "kind": "start" }, { "id": "done", "kind": "end" }]),
        json!([edge("e1", "start", "done", "next")]),
    );
    let cmds = h.start(vars(&[]));
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert!(h.state().tokens.is_empty());
    assert!(h.state().ended_at.is_some());
    let topics: Vec<_> = cmds
        .iter()
        .filter_map(|c| match c {
            Command::EmitEvent { topic, .. } => Some(topic.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(topics, ["instance.started", "instance.completed"]);
    assert!(matches!(cmds.last(), Some(Command::CompleteInstance { result: InstanceResult::Completed, .. })));
    assert_eq!(h.path(), ["start", "done"]);
    h.assert_replays();
}

#[test]
fn set_assigns_against_the_previous_values() {
    let mut h = seq(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "calc", "kind": "set", "assign": { "n": "expr:cel: m + 1", "m": "expr:cel: n * 10" } },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "calc", "next"), edge("e2", "calc", "done", "next")]),
    );
    h.start(vars(&[("n", json!(2)), ("m", json!(5))]));
    assert_eq!(h.state().variables["n"], json!(6));
    assert_eq!(h.state().variables["m"], json!(20));
}

fn decision_process() -> serde_json::Value {
    process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "d", "kind": "decision", "branches": [
                { "name": "big", "when": "expr:cel: n > 100" },
                { "name": "flagged", "when": "expr:cel: flag" }
            ]},
            task("big_t"), task("flag_t"), task("default_t"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "d", "next"),
            edge("e2", "d", "big_t", "big"),
            edge("e3", "d", "flag_t", "flagged"),
            edge("e4", "d", "default_t", "default"),
            edge("e5", "big_t", "done", "submitted"),
            edge("e6", "flag_t", "done", "submitted"),
            edge("e7", "default_t", "done", "submitted"),
        ]),
        &[("n", "integer"), ("flag", "boolean")],
    )
}

#[test]
fn decision_takes_the_first_true_branch_in_order() {
    let mut h = Harness::new(&decision_process());
    h.start(vars(&[("n", json!(500)), ("flag", json!(true))]));
    assert_eq!(h.waiting_at(), ["big_t"]);
    let mut h = Harness::new(&decision_process());
    h.start(vars(&[("n", json!(5)), ("flag", json!(true))]));
    assert_eq!(h.waiting_at(), ["flag_t"]);
}

#[test]
fn decision_falls_back_to_default() {
    let mut h = Harness::new(&decision_process());
    h.start(vars(&[("n", json!(5)), ("flag", json!(false))]));
    assert_eq!(h.waiting_at(), ["default_t"]);
    h.complete("default_t", "submitted");
    assert_eq!(h.state().status, InstanceStatus::Completed);
}

#[test]
fn decision_non_boolean_condition_is_an_incident() {
    let mut h = Harness::new(&decision_process());
    // `flag` is unset (null) and `n` is small, so `flag` yields null.
    h.start(vars(&[("n", json!(5))]));
    let (_, incident) = h.state().incidents().next().unwrap();
    assert_eq!(incident.code.as_str(), "IR-R002");
    assert_eq!(h.state().status, InstanceStatus::Running);
    assert!(commands_named(&h.last, "raise_incident").len() == 1);
}

#[test]
fn decision_evaluation_failures_are_incidents() {
    // `n > 100` with n a string cannot be ordered: IR-R001.
    let mut h = Harness::new(&decision_process());
    h.start(vars(&[("n", json!("x")), ("flag", json!(false))]));
    assert_eq!(h.state().incidents().next().unwrap().1.code.as_str(), "IR-R001");
    // A budget overrun is IR-R010 (docs/04 §4.5).
    let eval = MiniEvaluator { max_tokens: 1, ..MiniEvaluator::default() };
    let mut h = Harness::with(program(&decision_process()), eval);
    h.start(vars(&[("n", json!(5))]));
    assert_eq!(h.state().incidents().next().unwrap().1.code.as_str(), "IR-R010");
}

#[test]
fn rule_table_decision_takes_the_branch_named_by_the_output() {
    let source = reference("fin.purchase-requisition");
    for (level, expected) in
        [("cfo", "cfo_approval"), ("manager", "manager_approval"), ("nope", "finance_approval")]
    {
        let eval = MiniEvaluator::default().with_rule("approver", "approval_level");
        let mut h = Harness::with(program(&source), eval);
        h.start(vars(&[("approval_level", json!(level))]));
        assert_eq!(h.waiting_at(), [expected], "{level}");
    }
    // No rule row (null) takes the default branch.
    let mut h = Harness::new(&source);
    h.start(vars(&[]));
    assert_eq!(h.waiting_at(), ["finance_approval"]);
}

fn parallel_process() -> serde_json::Value {
    process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "fork", "kind": "parallel.split", "joinId": "join",
              "branches": [{ "name": "a" }, { "name": "b" }, { "name": "c" }] },
            task("ta"), task("tb"), task("tc"),
            { "id": "join", "kind": "parallel.join" },
            task("after"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "fork", "next"),
            edge("e2", "fork", "ta", "a"),
            edge("e3", "fork", "tb", "b"),
            edge("e4", "fork", "tc", "c"),
            edge("e5", "ta", "join", "submitted"),
            edge("e6", "tb", "join", "submitted"),
            edge("e7", "tc", "join", "submitted"),
            edge("e8", "join", "after", "next"),
            edge("e9", "after", "done", "submitted"),
        ]),
        &[],
    )
}

#[test]
fn parallel_split_waits_for_every_branch_then_releases_once() {
    let mut h = Harness::new(&parallel_process());
    h.start(vars(&[]));
    assert_eq!(h.waiting_at(), ["ta", "tb", "tc"]);
    assert_eq!(h.state().splits.len(), 1);
    h.complete("tb", "submitted");
    h.complete("ta", "submitted");
    assert_eq!(h.waiting_at(), ["tc"]);
    h.complete("tc", "submitted");
    assert_eq!(h.waiting_at(), ["after"]);
    assert!(h.state().splits.is_empty());
    let releases = h
        .state()
        .trace
        .iter()
        .filter(|e| matches!(e.event, procera_runtime::TraceEvent::JoinReleased { .. }))
        .count();
    assert_eq!(releases, 1);
    assert!(h.state().tokens.values().all(|t| t.frames.is_empty()));
    h.complete("after", "submitted");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();
}

#[test]
fn nested_parallel_blocks_join_inside_out() {
    let source = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "outer", "kind": "parallel.split", "joinId": "outer_join",
              "branches": [{ "name": "x" }, { "name": "y" }] },
            { "id": "inner", "kind": "parallel.split", "joinId": "inner_join",
              "branches": [{ "name": "p" }, { "name": "q" }] },
            task("tp"), task("tq"), task("ty"),
            { "id": "inner_join", "kind": "parallel.join" },
            { "id": "outer_join", "kind": "parallel.join" },
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "outer", "next"),
            edge("e2", "outer", "inner", "x"),
            edge("e3", "outer", "ty", "y"),
            edge("e4", "inner", "tp", "p"),
            edge("e5", "inner", "tq", "q"),
            edge("e6", "tp", "inner_join", "submitted"),
            edge("e7", "tq", "inner_join", "submitted"),
            edge("e8", "inner_join", "outer_join", "next"),
            edge("e9", "ty", "outer_join", "submitted"),
            edge("e10", "outer_join", "done", "next"),
        ]),
        &[],
    );
    let mut h = Harness::new(&source);
    h.start(vars(&[]));
    assert_eq!(h.waiting_at(), ["tp", "tq", "ty"]);
    assert_eq!(h.state().splits.len(), 2);
    h.complete("ty", "submitted");
    h.complete("tp", "submitted");
    assert_eq!(h.state().status, InstanceStatus::Waiting);
    h.complete("tq", "submitted");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();
}

#[test]
fn human_nodes_create_work_items_and_validate_completions() {
    let source = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "review", "kind": "human.approval", "assignment": assignment(), "quorum": "all",
              "signature": "strong", "due": { "after": "P2D" } },
            { "id": "fix", "kind": "human.task", "assignment": assignment(), "outcomes": ["fixed", "gave_up"],
              "maxIterations": 3 },
            { "id": "ok", "kind": "end" }, { "id": "no", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "review", "next"),
            edge("e2", "review", "ok", "approved"),
            edge("e3", "review", "no", "rejected"),
            edge("e4", "review", "fix", "changes_requested"),
            edge("e5", "fix", "review", "fixed"),
            edge("e6", "fix", "no", "gave_up"),
        ]),
        &[("note", "string")],
    );
    let mut h = Harness::new(&source);
    let cmds = h.start(vars(&[]));
    let Some(Command::CreateWorkItem { kind, outcomes, quorum, due, .. }) =
        cmds.iter().find(|c| matches!(c, Command::CreateWorkItem { .. }))
    else {
        panic!("no work item: {cmds:#?}")
    };
    assert_eq!(*kind, WorkKind::Approval);
    assert_eq!(outcomes, &["approved", "changes_requested", "rejected"]);
    assert!(quorum.is_some() && due.is_some());
    assert_eq!(h.wait_kind("review"), WaitKind::WorkItem);
    // An outcome the node cannot produce, or an undeclared variable, is rejected.
    assert!(h.try_complete("review", "submitted", vars(&[])).is_err());
    assert!(h.try_complete("review", "approved", vars(&[("ghost", json!(1))])).is_err());
    let stale = h.key("review");
    h.try_complete("review", "changes_requested", vars(&[("note", json!("fix it"))])).unwrap();
    assert_eq!(h.state().variables["note"], json!("fix it"));
    // A duplicate completion of the same work item is a no-op error.
    let dup = h.send(procera_runtime::Event::WorkItemCompleted {
        key: stale,
        outcome: "approved".into(),
        variables: vars(&[]),
    });
    assert!(matches!(dup, Err(procera_runtime::EventError::UnknownKey(_))));
    h.complete("fix", "fixed");
    h.complete("review", "approved");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();
}

#[test]
fn loop_beyond_max_iterations_raises_an_incident() {
    let source = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "review", "kind": "human.approval", "assignment": assignment() },
            { "id": "fix", "kind": "human.task", "assignment": assignment(), "maxIterations": 2 },
            { "id": "ok", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "review", "next"),
            edge("e2", "review", "ok", "approved"),
            edge("e3", "review", "ok", "rejected"),
            edge("e4", "review", "fix", "changes_requested"),
            edge("e5", "fix", "review", "submitted"),
        ]),
        &[],
    );
    let mut h = Harness::new(&source);
    h.start(vars(&[]));
    for _ in 0..2 {
        h.complete("review", "changes_requested");
        h.complete("fix", "submitted");
    }
    h.complete("review", "changes_requested");
    let (token, incident) = h.state().incidents().next().unwrap();
    assert_eq!((token.node.as_str(), incident.code.as_str()), ("fix", "IR-R004"));
    assert_eq!(h.state().status, InstanceStatus::Running);
}

fn timer_process(timer: serde_json::Value) -> serde_json::Value {
    let mut nodes =
        vec![json!({ "id": "start", "kind": "start" }), timer, json!({ "id": "done", "kind": "end" })];
    nodes[1]["id"] = json!("wait");
    nodes[1]["kind"] = json!("wait.timer");
    let mut doc = process(
        json!(nodes),
        json!([edge("e1", "start", "wait", "next"), edge("e2", "wait", "done", "fired")]),
        &[("when", "datetime")],
    );
    doc["refs"] = json!({ "calendars": { "default": "calendar:in-blr@1" } });
    doc
}

#[test]
fn wait_timer_schedules_and_resumes_on_fire() {
    let mut h = Harness::new(&timer_process(json!({ "after": "PT1H" })));
    let cmds = h.start(vars(&[]));
    let due = cmds.iter().find_map(|c| match c {
        Command::ScheduleTimer { due, .. } => Some(due.clone()),
        _ => None,
    });
    assert_eq!(due, Some(TimerDue::At { at: T0 + time::Duration::hours(1) }));
    assert_eq!(h.wait_kind("wait"), WaitKind::Timer);
    h.now += time::Duration::hours(1);
    h.fire("wait");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();
}

#[test]
fn wait_timer_business_and_absolute_forms() {
    let mut h = Harness::new(&timer_process(json!({ "after": "P2D", "business": true })));
    let cmds = h.start(vars(&[]));
    assert!(cmds.iter().any(|c| matches!(c, Command::ScheduleTimer {
        due: TimerDue::Business { calendar: Some(cal), duration, .. }, ..
    } if cal == "calendar:in-blr@1" && duration == "P2D")));

    let mut h = Harness::new(&timer_process(json!({ "at": "expr:cel: when" })));
    let cmds = h.start(vars(&[("when", json!("2026-12-01T10:00:00+05:30"))]));
    let at = time::macros::datetime!(2026-12-01 10:00 +05:30);
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::ScheduleTimer { due: TimerDue::At { at: a }, .. } if *a == at))
    );

    // A value that is not an instant is an incident, not a panic.
    let mut h = Harness::new(&timer_process(json!({ "at": "expr:cel: when" })));
    h.start(vars(&[("when", json!("tomorrow"))]));
    assert_eq!(h.state().incidents().next().unwrap().1.code.as_str(), "IR-R006");
}

fn signal_process() -> serde_json::Value {
    process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "sig", "kind": "wait.signal", "correlation": "expr:cel: order_id", "timeout": "P1D" },
            task("late"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "sig", "next"),
            edge("e2", "sig", "done", "received"),
            edge("e3", "sig", "late", "timeout"),
            edge("e4", "late", "done", "submitted"),
        ]),
        &[("order_id", "string")],
    )
}

#[test]
fn wait_signal_subscribes_with_correlation_and_times_out() {
    let mut h = Harness::new(&signal_process());
    let cmds = h.start(vars(&[("order_id", json!("PO-7"))]));
    assert!(
        cmds.iter().any(
            |c| matches!(c, Command::SubscribeSignal { correlation, .. } if correlation == &json!("PO-7"))
        )
    );
    assert_eq!(commands_named(&cmds, "schedule_timer").len(), 1);
    h.signal("sig");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(commands_named(&h.last, "cancel_timer").len(), 1);

    let mut h = Harness::new(&signal_process());
    h.start(vars(&[("order_id", json!("PO-8"))]));
    h.fire_timeout("sig");
    assert_eq!(commands_named(&h.last, "cancel_subscription").len(), 1);
    assert_eq!(h.waiting_at(), ["late"]);
    h.assert_replays();
}

fn service_process() -> serde_json::Value {
    let mut doc = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "call", "kind": "service.call", "connector": "crm", "action": "grant",
              "input": { "who": "expr:cel: user" }, "idempotencyKey": "expr:cel: instance.id + \":grant\"",
              "boundary": [{ "on": "error:LIMIT", "to": "manual" }] },
            task("manual"), task("retry"),
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "call", "next"),
            edge("e2", "call", "done", "ok"),
            edge("e3", "call", "retry", "error"),
            edge("e4", "manual", "done", "submitted"),
            edge("e5", "retry", "done", "submitted"),
        ]),
        &[("user", "string")],
    );
    doc["refs"] = json!({ "connectors": { "crm": "connector:crm" } });
    doc
}

#[test]
fn service_call_emits_a_connector_command_and_routes_results() {
    let mut h = Harness::new(&service_process());
    let cmds = h.start(vars(&[("user", json!("u-1"))]));
    let Some(Command::CallConnector { connector_key, input, connector, .. }) =
        cmds.iter().find(|c| matches!(c, Command::CallConnector { .. }))
    else {
        panic!("{cmds:#?}")
    };
    assert_eq!(connector, "crm");
    assert_eq!(input["who"], json!("u-1"));
    assert!(connector_key.ends_with(":grant") && connector_key.len() > 10);
    h.service("call", "ok");
    assert_eq!(h.state().status, InstanceStatus::Completed);

    for (outcome, next) in [("error", "retry"), ("error:LIMIT", "manual"), ("error:OTHER", "retry")] {
        let mut h = Harness::new(&service_process());
        h.start(vars(&[("user", json!("u-1"))]));
        h.service("call", outcome);
        assert_eq!(h.waiting_at(), [next], "{outcome}");
        h.assert_replays();
    }
    let mut h = Harness::new(&service_process());
    h.start(vars(&[]));
    let key = h.key("call");
    let bad = h.send(procera_runtime::Event::ServiceCompleted {
        key,
        outcome: "maybe".into(),
        variables: vars(&[]),
    });
    assert!(bad.is_err());
}

#[test]
fn notify_fires_and_moves_on() {
    let mut h = seq(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "tell", "kind": "notify", "to": assignment(), "template": "hello", "channels": ["email"] },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "tell", "next"), edge("e2", "tell", "done", "next")]),
    );
    let cmds = h.start(vars(&[]));
    assert_eq!(commands_named(&cmds, "notify").len(), 1);
    assert_eq!(h.state().status, InstanceStatus::Completed);
}

fn subprocess_and_case() -> serde_json::Value {
    let mut doc = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "open", "kind": "case.open", "caseType": "incident", "waitForClose": false },
            { "id": "child", "kind": "subprocess.call", "process": "child" },
            { "id": "track", "kind": "case.open", "caseType": "incident", "waitForClose": true },
            { "id": "done", "kind": "end" },
            { "id": "failed", "kind": "end", "result": "terminated" }
        ]),
        json!([
            edge("e1", "start", "open", "next"),
            edge("e2", "open", "child", "opened"),
            edge("e3", "child", "track", "completed"),
            edge("e4", "child", "failed", "failed"),
            edge("e5", "child", "failed", "cancelled"),
            edge("e6", "track", "done", "closed"),
        ]),
        &[],
    );
    doc["refs"] = json!({ "processes": { "child": "process:child@1" } });
    doc
}

#[test]
fn subprocess_and_case_open() {
    let mut h = Harness::new(&subprocess_and_case());
    let cmds = h.start(vars(&[]));
    assert_eq!(commands_named(&cmds, "open_case").len(), 1);
    assert_eq!(commands_named(&cmds, "start_subprocess").len(), 1);
    assert_eq!(h.wait_kind("child"), WaitKind::Subprocess);
    h.subprocess("child", "completed");
    assert_eq!(h.wait_kind("track"), WaitKind::Case);
    h.case_closed("track");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();

    let mut h = Harness::new(&subprocess_and_case());
    h.start(vars(&[]));
    h.subprocess("child", "failed");
    assert_eq!(h.state().status, InstanceStatus::Terminated);
}
