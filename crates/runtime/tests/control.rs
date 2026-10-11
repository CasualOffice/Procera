//! Terminate, cancel, boundaries, unhandled outcomes, budgets and the state's wire form.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::{Budget, Command, Event, EventError, InstanceState, InstanceStatus, TraceEvent};
use serde_json::json;
use std::collections::BTreeSet;

fn terminate_in_branch() -> serde_json::Value {
    process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "fork", "kind": "parallel.split", "joinId": "join", "branches": [{ "name": "a" }, { "name": "b" }] },
            { "id": "check", "kind": "human.approval", "assignment": assignment() },
            { "id": "slow", "kind": "wait.timer", "after": "P3D", "boundary": [] },
            task("tb"),
            { "id": "join", "kind": "parallel.join" },
            { "id": "done", "kind": "end" },
            { "id": "kill", "kind": "end", "result": "terminated" }
        ]),
        json!([
            edge("e1", "start", "fork", "next"),
            edge("e2", "fork", "check", "a"),
            edge("e3", "fork", "tb", "b"),
            edge("e4", "check", "join", "approved"),
            edge("e5", "check", "kill", "rejected"),
            edge("e6", "check", "join", "changes_requested"),
            edge("e7", "tb", "slow", "submitted"),
            edge("e8", "slow", "join", "fired"),
            edge("e9", "join", "done", "next"),
        ]),
        &[],
    )
}

#[test]
fn terminate_inside_a_branch_cancels_every_token() {
    let mut h = Harness::new(&terminate_in_branch());
    h.start(vars(&[]));
    h.complete("tb", "submitted");
    assert_eq!(h.waiting_at(), ["check", "slow"]);
    let timer = h.key("slow");
    h.complete("check", "rejected");
    let s = h.state();
    assert_eq!(s.status, InstanceStatus::Terminated);
    assert!(s.tokens.is_empty() && s.splits.is_empty());
    assert!(h.last.iter().any(|c| matches!(c, Command::CancelTimer { key } if *key == timer)));
    assert!(matches!(
        h.last.last(),
        Some(Command::CompleteInstance { result: procera_runtime::InstanceResult::Terminated, .. })
    ));
    // Nothing is accepted afterwards, not even the cancelled timer.
    let late = h.send(Event::TimerFired { key: timer });
    assert_eq!(late, Err(EventError::Final(InstanceStatus::Terminated)));
    h.assert_replays();
}

#[test]
fn cancel_terminates_and_withdraws_open_work() {
    let mut h = Harness::new(&terminate_in_branch());
    h.start(vars(&[]));
    let cmds = h.send(Event::Cancel { reason: "duplicate".into() }).unwrap();
    assert_eq!(h.state().status, InstanceStatus::Terminated);
    assert_eq!(commands_named(&cmds, "cancel_work_item").len(), 2);
    let cancelled = h.state().trace.iter().filter(|e| e.event == TraceEvent::TokenCancelled).count();
    assert_eq!(cancelled, 2);
}

fn boundary_process(interrupting: bool) -> serde_json::Value {
    process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "approve", "kind": "human.approval", "assignment": assignment(),
              "boundary": [{ "on": "timeout", "after": "P5D", "interrupting": interrupting, "to": "chase" }] },
            { "id": "chase", "kind": "notify", "to": assignment(), "template": "chase" },
            { "id": "chased", "kind": "end" },
            { "id": "done", "kind": "end" }
        ]),
        json!([
            edge("e1", "start", "approve", "next"),
            edge("e2", "approve", "done", "approved"),
            edge("e3", "approve", "done", "rejected"),
            edge("e4", "approve", "done", "changes_requested"),
            edge("e5", "chase", "chased", "next"),
        ]),
        &[],
    )
}

#[test]
fn interrupting_boundary_cancels_the_work_item_and_moves_the_token() {
    let mut h = Harness::new(&boundary_process(true));
    let cmds = h.start(vars(&[]));
    assert_eq!(commands_named(&cmds, "schedule_timer").len(), 1);
    let work = h.key("approve");
    h.fire_boundary("approve", 0);
    assert!(h.last.iter().any(|c| matches!(c, Command::CancelWorkItem { key } if *key == work)));
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert!(h.path().ends_with(&["chase".to_owned(), "chased".to_owned()]));
    h.assert_replays();
}

#[test]
fn non_interrupting_boundary_spawns_a_token_and_keeps_waiting() {
    let mut h = Harness::new(&boundary_process(false));
    h.start(vars(&[]));
    h.fire_boundary("approve", 0);
    assert_eq!(h.waiting_at(), ["approve"]);
    assert_eq!(commands_named(&h.last, "notify").len(), 1);
    assert_eq!(h.state().status, InstanceStatus::Waiting);
    // The boundary timer is spent: completing the work item cancels nothing more.
    h.complete("approve", "approved");
    assert!(commands_named(&h.last, "cancel_timer").is_empty());
    assert_eq!(h.state().status, InstanceStatus::Completed);
    h.assert_replays();
}

#[test]
fn completing_before_the_boundary_disarms_it() {
    let mut h = Harness::new(&boundary_process(true));
    h.start(vars(&[]));
    let timer = format!("{}/boundary/0", h.key("approve"));
    h.complete("approve", "approved");
    assert!(h.last.iter().any(|c| matches!(c, Command::CancelTimer { key } if *key == timer)));
}

#[test]
fn unhandled_fail_outcome_fails_the_instance() {
    let source = process(
        json!([
            { "id": "start", "kind": "start" },
            { "id": "approve", "kind": "human.approval", "assignment": assignment(), "unhandled": "fail" },
            { "id": "done", "kind": "end" }
        ]),
        json!([edge("e1", "start", "approve", "next"), edge("e2", "approve", "done", "approved")]),
        &[],
    );
    let mut h = Harness::new(&source);
    h.start(vars(&[]));
    h.complete("approve", "rejected");
    assert_eq!(h.state().status, InstanceStatus::Failed);
    assert!(commands_named(&h.last, "complete_instance").len() == 1);
    h.assert_replays();
}

fn long_chain(n: usize) -> serde_json::Value {
    let mut nodes = vec![json!({ "id": "start", "kind": "start" })];
    let mut edges = Vec::new();
    let mut prev = "start".to_owned();
    for i in 0..n {
        let id = format!("s{i:03}");
        nodes.push(json!({ "id": id, "kind": "set", "assign": { "n": "expr:cel: n + 1" } }));
        edges.push(edge(&format!("e{i:03}"), &prev, &id, "next"));
        prev = id;
    }
    nodes.push(json!({ "id": "done", "kind": "end" }));
    edges.push(edge("e_end", &prev, "done", "next"));
    process(json!(nodes), json!(edges), &[("n", "integer")])
}

#[test]
fn step_budget_yields_and_continue_resumes() {
    let mut h = Harness::new(&long_chain(10));
    h.budget = Budget { max_steps: 4, ..Budget::default() };
    let cmds = h.start(vars(&[("n", json!(0))]));
    assert!(matches!(cmds.last(), Some(Command::ContinueAdvance { .. })));
    assert_eq!(h.state().status, InstanceStatus::Running);
    let mut rounds = 0;
    while h.state().status == InstanceStatus::Running {
        h.send(Event::Continue).unwrap();
        rounds += 1;
        assert!(rounds < 10, "no progress");
    }
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(h.state().variables["n"], json!(10));
    // 12 node executions at 4 per advance: start + 2 continues.
    assert_eq!(rounds, 2);
    assert_eq!(h.send(Event::Continue), Err(EventError::Final(InstanceStatus::Completed)));
    h.assert_replays();
}

#[test]
fn continue_without_ready_tokens_is_rejected() {
    let mut h = Harness::new(&boundary_process(true));
    h.start(vars(&[]));
    assert_eq!(h.send(Event::Continue), Err(EventError::NothingToContinue));
    assert_eq!(
        h.send(Event::Start { instance_id: uuid::Uuid::nil(), initiator: json!({}), variables: vars(&[]) }),
        Err(EventError::AlreadyStarted)
    );
}

#[test]
fn state_round_trips_through_json() {
    let mut h = Harness::new(&terminate_in_branch());
    h.start(vars(&[]));
    h.complete("tb", "submitted");
    let text = serde_json::to_string(h.state()).unwrap();
    let back: InstanceState = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, h.state());
    let cmds = serde_json::to_string(&h.commands).unwrap();
    let back: Vec<Command> = serde_json::from_str(&cmds).unwrap();
    assert_eq!(back, h.commands);
}

#[test]
fn idempotency_keys_are_unique_and_deterministic() {
    let run = || {
        let mut h = Harness::new(&terminate_in_branch());
        h.start(vars(&[]));
        h.complete("tb", "submitted");
        h.complete("check", "approved");
        h.fire("slow");
        h.commands
    };
    let a = run();
    assert_eq!(a, run());
    let creating: Vec<&str> = a
        .iter()
        .filter(|c| !matches!(c, Command::CancelTimer { .. } | Command::CancelWorkItem { .. }))
        .map(Command::key)
        .collect();
    let unique: BTreeSet<&str> = creating.iter().copied().collect();
    assert_eq!(unique.len(), creating.len(), "{creating:#?}");
}
