//! Property tests (docs/15 §5) over generated block-structured processes driven by random
//! event sequences: determinism and replay equality, budget termination and equivalence,
//! join correctness, terminate kills every token, and activation bookkeeping.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::testkit::MiniEvaluator;
use procera_runtime::{
    Budget, Command, Event, EventError, InstanceState, InstanceStatus, SplitId, Stimulus, TraceEvent,
    WaitKind,
};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
enum Block {
    Task,
    Approval,
    Timer,
    Set,
    Service,
    Signal,
    Kill,
    Decision(Vec<Block>, Vec<Block>),
    Parallel(Vec<Vec<Block>>),
}

fn block() -> impl Strategy<Value = Block> {
    let leaf = prop_oneof![
        3 => Just(Block::Task),
        2 => Just(Block::Approval),
        1 => Just(Block::Timer),
        1 => Just(Block::Set),
        1 => Just(Block::Service),
        1 => Just(Block::Signal),
        1 => Just(Block::Kill),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            (prop::collection::vec(inner.clone(), 0..3), prop::collection::vec(inner.clone(), 0..3))
                .prop_map(|(a, b)| Block::Decision(a, b)),
            prop::collection::vec(prop::collection::vec(inner, 1..3), 2..4).prop_map(Block::Parallel),
        ]
    })
}

/// Builds IR from blocks. `exits` are the dangling `(node, outcome)` pairs to wire next.
#[derive(Default)]
struct Builder {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    n: usize,
    flags: usize,
}

impl Builder {
    fn id(&mut self, prefix: &str) -> String {
        self.n += 1;
        format!("{prefix}{}", self.n)
    }

    fn wire(&mut self, from: Vec<(String, String)>, to: &str) {
        for (f, on) in from {
            let id = self.id("e");
            self.edges.push(edge(&id, &f, to, &on));
        }
    }

    fn node(&mut self, node: Value, incoming: Vec<(String, String)>) -> String {
        let id = node["id"].as_str().unwrap().to_owned();
        self.wire(incoming, &id);
        self.nodes.push(node);
        id
    }

    fn flag(&mut self) -> String {
        self.flags += 1;
        format!("f{}", self.flags)
    }

    fn chain(&mut self, blocks: &[Block], mut exits: Vec<(String, String)>) -> Vec<(String, String)> {
        for b in blocks {
            exits = self.block(b, exits);
        }
        exits
    }

    fn block(&mut self, b: &Block, inc: Vec<(String, String)>) -> Vec<(String, String)> {
        let out = |id: &str, outs: &[&str]| outs.iter().map(|o| (id.to_owned(), (*o).to_owned())).collect();
        match b {
            Block::Task => {
                let id = self.id("task");
                let id =
                    self.node(json!({ "id": id, "kind": "human.task", "assignment": assignment() }), inc);
                out(&id, &["submitted"])
            }
            Block::Approval => {
                let id = self.id("appr");
                let id =
                    self.node(json!({ "id": id, "kind": "human.approval", "assignment": assignment() }), inc);
                out(&id, &["approved", "rejected", "changes_requested"])
            }
            Block::Timer => {
                let id = self.id("timer");
                let id = self.node(json!({ "id": id, "kind": "wait.timer", "after": "PT1H" }), inc);
                out(&id, &["fired"])
            }
            Block::Set => {
                let id = self.id("set");
                let node =
                    json!({ "id": id, "kind": "set", "assign": { "counter": "expr:cel: counter + 1" } });
                let id = self.node(node, inc);
                out(&id, &["next"])
            }
            Block::Service => {
                let id = self.id("svc");
                let node = json!({ "id": id, "kind": "service.call", "connector": "svc", "action": "run",
                    "idempotencyKey": format!("expr:cel: instance.id + '{id}'") });
                let id = self.node(node, inc);
                out(&id, &["ok", "error"])
            }
            Block::Signal => {
                let id = self.id("sig");
                let node = json!({ "id": id, "kind": "wait.signal", "correlation": "expr:cel: instance.id",
                    "timeout": "PT1H" });
                let id = self.node(node, inc);
                out(&id, &["received", "timeout"])
            }
            Block::Kill => {
                let flag = self.flag();
                let id = self.id("kill");
                let end = self.id("term");
                let d = json!({ "id": id, "kind": "decision",
                    "branches": [{ "name": "kill", "when": format!("expr:cel: {flag}") }] });
                let id = self.node(d, inc);
                self.node(json!({ "id": end, "kind": "end", "result": "terminated" }), out(&id, &["kill"]));
                out(&id, &["default"])
            }
            Block::Decision(a, b) => {
                let flag = self.flag();
                let id = self.id("dec");
                let d = json!({ "id": id, "kind": "decision",
                    "branches": [{ "name": "yes", "when": format!("expr:cel: {flag}") }] });
                let id = self.node(d, inc);
                let mut exits = self.chain(a, out(&id, &["yes"]));
                exits.extend(self.chain(b, out(&id, &["default"])));
                exits
            }
            Block::Parallel(branches) => {
                let split = self.id("fork");
                let join = self.id("join");
                let names: Vec<Value> =
                    (0..branches.len()).map(|i| json!({ "name": format!("b{i}") })).collect();
                let s = json!({ "id": split, "kind": "parallel.split", "joinId": join, "branches": names });
                let split = self.node(s, inc);
                let mut into_join = Vec::new();
                for (i, br) in branches.iter().enumerate() {
                    into_join.extend(self.chain(br, vec![(split.clone(), format!("b{i}"))]));
                }
                let join = self.node(json!({ "id": join, "kind": "parallel.join" }), into_join);
                out(&join, &["next"])
            }
        }
    }
}

fn build(blocks: &[Block]) -> (Value, usize) {
    let mut b = Builder::default();
    b.nodes.push(json!({ "id": "start", "kind": "start" }));
    let exits = b.chain(blocks, vec![("start".to_owned(), "next".to_owned())]);
    b.node(json!({ "id": "done", "kind": "end" }), exits);
    let mut vars: Vec<(String, String)> =
        (1..=b.flags).map(|i| (format!("f{i}"), "boolean".to_owned())).collect();
    vars.push(("counter".into(), "integer".into()));
    let var_refs: Vec<(&str, &str)> = vars.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut doc = process(json!(b.nodes), json!(b.edges), &var_refs);
    doc["refs"] = json!({ "connectors": { "svc": "connector:svc" } });
    (doc, b.flags)
}

/// One scripted choice: which waiting token, which outcome, and a flag to set on completion.
type Choice = (u8, u8, u8, bool);

fn choices() -> impl Strategy<Value = Vec<Choice>> {
    prop::collection::vec((any::<u8>(), any::<u8>(), any::<u8>(), any::<bool>()), 0..40)
}

/// Picks the next event for `state`, or `None` when nothing waits.
fn next_event(h: &Harness, flags: usize, (pick, outcome, flag, val): Choice) -> Option<Event> {
    let waits: Vec<_> = h.state().waits().map(|(t, w)| (t.node.clone(), w.clone())).collect();
    if waits.is_empty() || h.state().status.is_final() {
        return None;
    }
    let (node, w) = &waits[pick as usize % waits.len()];
    let o = outcome as usize;
    Some(match w.kind {
        WaitKind::WorkItem => {
            let outs = h.program.node(node).unwrap().outcomes();
            let mut variables = BTreeMap::new();
            if flags > 0 {
                variables.insert(format!("f{}", 1 + flag as usize % flags), json!(val));
            }
            Event::WorkItemCompleted { key: w.key.clone(), outcome: outs[o % outs.len()].clone(), variables }
        }
        WaitKind::Timer => Event::TimerFired { key: w.key.clone() },
        WaitKind::Signal if o.is_multiple_of(2) => {
            Event::SignalReceived { key: w.key.clone(), variables: BTreeMap::new() }
        }
        WaitKind::Signal => Event::TimerFired { key: w.timeout_timer.clone().unwrap() },
        WaitKind::Service => Event::ServiceCompleted {
            key: w.key.clone(),
            outcome: if o.is_multiple_of(2) { "ok" } else { "error" }.into(),
            variables: BTreeMap::new(),
        },
        WaitKind::Subprocess | WaitKind::Case => unreachable!("not generated"),
    })
}

fn start_vars(flags: usize, seed: u64) -> BTreeMap<String, Value> {
    let mut v: BTreeMap<String, Value> =
        (1..=flags).map(|i| (format!("f{i}"), json!(seed >> (i % 64) & 1 == 1))).collect();
    v.insert("counter".into(), json!(0));
    v
}

/// Runs a script. With `roundtrip`, the state goes through JSON between events. `Continue` is
/// sent whenever an advance yields.
fn run(doc: &Value, flags: usize, seed: u64, script: &[Choice], budget: Budget, roundtrip: bool) -> Harness {
    let mut h = Harness::with(program(doc), MiniEvaluator::default());
    h.budget = budget;
    h.start(start_vars(flags, seed));
    let mut pending: Vec<Choice> = script.to_vec();
    pending.reverse();
    let mut guard = 0;
    loop {
        guard += 1;
        assert!(guard < 10_000, "driver did not terminate");
        if roundtrip {
            let text = serde_json::to_string(h.state()).unwrap();
            h.state = Some(serde_json::from_str::<InstanceState>(&text).unwrap());
        }
        if matches!(h.last.last(), Some(Command::ContinueAdvance { .. })) {
            h.send(Event::Continue).unwrap();
            continue;
        }
        let Some(choice) = pending.pop() else { break };
        let Some(ev) = next_event(&h, flags, choice) else { break };
        h.send(ev).unwrap();
    }
    h
}

fn without_continue(cmds: &[Command]) -> Vec<Command> {
    cmds.iter().filter(|c| !matches!(c, Command::ContinueAdvance { .. })).cloned().collect()
}

/// Every activation the instance opened is resumed, cancelled, fired, or still open.
fn assert_bookkeeping(h: &Harness) {
    let s = h.state();
    let mut opened = BTreeSet::new();
    let mut closed = BTreeSet::new();
    for c in &h.commands {
        match c {
            Command::CreateWorkItem { key, .. }
            | Command::ScheduleTimer { key, .. }
            | Command::SubscribeSignal { key, .. } => {
                opened.insert(key.clone());
            }
            Command::CancelWorkItem { key }
            | Command::CancelTimer { key }
            | Command::CancelSubscription { key } => {
                closed.insert(key.clone());
            }
            _ => {}
        }
    }
    for e in &s.trace {
        if let TraceEvent::Resumed { key } = &e.event {
            closed.insert(key.clone());
        }
    }
    for st in &h.stimuli {
        if let Event::TimerFired { key } = &st.event {
            closed.insert(key.clone());
        }
    }
    let mut live = BTreeSet::new();
    for (_, w) in s.waits() {
        if w.kind != WaitKind::Service {
            live.insert(w.key.clone());
        }
        live.extend(w.timeout_timer.clone());
    }
    let open: BTreeSet<String> = opened.difference(&closed).cloned().collect();
    assert_eq!(open, live, "open activations must be exactly the live waits");
}

/// A join releases at most once, only after every branch arrived; a completed instance released
/// every split it forked.
fn assert_joins(s: &InstanceState) {
    let mut branches: BTreeMap<SplitId, usize> = BTreeMap::new();
    let mut arrived: BTreeMap<SplitId, BTreeSet<String>> = BTreeMap::new();
    let mut released: BTreeMap<SplitId, usize> = BTreeMap::new();
    for e in &s.trace {
        match &e.event {
            TraceEvent::Forked { split, branches: b } => {
                branches.insert(*split, b.len());
            }
            TraceEvent::JoinArrived { split, branch } => {
                assert_eq!(released.get(split), None, "arrival after release");
                assert!(arrived.entry(*split).or_default().insert(branch.clone()), "branch arrived twice");
            }
            TraceEvent::JoinReleased { split } => {
                assert_eq!(
                    arrived.get(split).map(BTreeSet::len),
                    branches.get(split).copied(),
                    "early release"
                );
                *released.entry(*split).or_default() += 1;
            }
            _ => {}
        }
    }
    assert!(released.values().all(|n| *n == 1));
    if s.status == InstanceStatus::Completed {
        assert_eq!(released.len(), branches.len(), "a completed instance left a split open");
    }
    for t in s.tokens.values() {
        for f in &t.frames {
            assert!(s.splits.contains_key(&f.split), "token in a released split");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    #[test]
    fn determinism_replay_and_wire_roundtrip(blocks in prop::collection::vec(block(), 1..5), seed: u64, script in choices()) {
        let (doc, flags) = build(&blocks);
        let a = run(&doc, flags, seed, &script, Budget::default(), false);
        let b = run(&doc, flags, seed, &script, Budget::default(), false);
        prop_assert_eq!(a.state(), b.state());
        prop_assert_eq!(&a.commands, &b.commands);
        a.assert_replays();
        let c = run(&doc, flags, seed, &script, Budget::default(), true);
        prop_assert_eq!(a.state(), c.state());
        prop_assert!(a.state().incidents().next().is_none(), "{:?}", a.state().incidents().collect::<Vec<_>>());
        assert_bookkeeping(&a);
        assert_joins(a.state());
    }

    #[test]
    fn small_budgets_terminate_and_match_the_unbounded_run(
        blocks in prop::collection::vec(block(), 1..5), seed: u64, script in choices(), steps in 1u32..4,
    ) {
        let (doc, flags) = build(&blocks);
        let full = run(&doc, flags, seed, &script, Budget::default(), false);
        let small = run(&doc, flags, seed, &script, Budget { max_steps: steps, ..Budget::default() }, false);
        let (f, s) = (full.state(), small.state());
        prop_assert_eq!(f.status, s.status);
        prop_assert_eq!(&f.variables, &s.variables);
        prop_assert_eq!(&f.tokens, &s.tokens);
        prop_assert_eq!(&f.splits, &s.splits);
        prop_assert_eq!(without_continue(&full.commands), without_continue(&small.commands));
        let yields = s.trace.iter().filter(|e| e.event == TraceEvent::Yielded).count();
        let entered = s.trace.iter().filter(|e| e.event == TraceEvent::NodeEntered).count();
        prop_assert!(yields <= entered, "every yield follows real progress");
        small.assert_replays();
        assert_joins(s);
    }

    #[test]
    fn terminate_kills_every_token_and_refuses_later_events(
        branches in prop::collection::vec(prop::collection::vec(block(), 1..3), 2..4), seed: u64, script in choices(),
    ) {
        // A parallel block whose first branch terminates once flag f1 is set.
        let mut first = vec![Block::Task, Block::Kill];
        first.extend(branches[0].clone());
        let mut all = vec![first];
        all.extend(branches[1..].iter().cloned());
        let (doc, flags) = build(&[Block::Parallel(all)]);
        let h = run(&doc, flags, seed | 1, &script, Budget::default(), false);
        let s = h.state();
        if s.status == InstanceStatus::Terminated {
            prop_assert!(s.tokens.is_empty());
            prop_assert!(s.splits.is_empty());
            let cancelled = s.trace.iter().filter(|e| e.event == TraceEvent::TokenCancelled).count();
            let consumed_at_kill = s.trace.iter().filter(|e| e.event == TraceEvent::TokenConsumed).count();
            prop_assert!(cancelled + consumed_at_kill >= 1);
            let late = Stimulus { at: h.now, event: Event::Continue };
            prop_assert_eq!(h.interp().handle(s, &late), Err(EventError::Final(InstanceStatus::Terminated)));
        }
        assert_bookkeeping(&h);
        assert_joins(s);
    }
}
