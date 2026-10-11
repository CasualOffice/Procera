//! Shared test helpers: compile a process, drive an instance with scripted events.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use procera_ir::{CompileContext, Severity, compile};
use procera_runtime::{
    Budget, CelEvaluator, Command, Evaluator, Event, EventError, InstanceState, Interpreter, Program,
    Stimulus, TraceEvent, WaitKind,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use time::OffsetDateTime;
use time::macros::datetime;

pub const T0: OffsetDateTime = datetime!(2026-10-11 09:00 UTC);

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn reference(name: &str) -> Value {
    let path = repo_root().join("conformance/reference").join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// Compiles `source` and fails the test on any error diagnostic.
pub fn program(source: &Value) -> Program {
    let out = compile(source, &CompileContext::default());
    let errors: Vec<_> = out.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "compile errors: {errors:#?}");
    Program::from_compiled(&out.compiled.unwrap()).unwrap()
}

/// A process document from nodes, edges and typed variable names. The variables are
/// `required`, so expressions may use them without null guards (docs/11 §6.1).
pub fn process(nodes: Value, edges: Value, vars: &[(&str, &str)]) -> Value {
    let variables: Vec<Value> =
        vars.iter().map(|(n, t)| json!({ "name": n, "type": t, "required": true })).collect();
    json!({
        "schemaVersion": 1,
        "process": { "key": "test.process", "version": 1, "name": { "en": "Test" } },
        "variables": variables,
        "nodes": nodes,
        "edges": edges,
    })
}

pub fn assignment() -> Value {
    json!({ "candidates": [{ "type": "role", "ref": "process-owner" }] })
}

pub fn task(id: &str) -> Value {
    json!({ "id": id, "kind": "human.task", "assignment": assignment() })
}

pub fn edge(id: &str, from: &str, to: &str, on: &str) -> Value {
    json!({ "id": id, "from": from, "to": to, "on": on })
}

pub fn vars(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

/// Drives one instance and records every accepted stimulus and command.
#[derive(Debug)]
pub struct Harness {
    pub program: Program,
    pub eval: Box<dyn Evaluator>,
    pub budget: Budget,
    pub now: OffsetDateTime,
    pub state: Option<InstanceState>,
    pub stimuli: Vec<Stimulus>,
    pub commands: Vec<Command>,
    pub last: Vec<Command>,
}

impl Harness {
    pub fn new(source: &Value) -> Self {
        Self::with(program(source), CelEvaluator::default())
    }

    /// With a specific evaluator. [`Harness::new`] uses the production [`CelEvaluator`].
    pub fn with(program: Program, eval: impl Evaluator + 'static) -> Self {
        Self {
            program,
            eval: Box::new(eval),
            budget: Budget::default(),
            now: T0,
            state: None,
            stimuli: Vec::new(),
            commands: Vec::new(),
            last: Vec::new(),
        }
    }

    pub fn state(&self) -> &InstanceState {
        self.state.as_ref().expect("started")
    }

    pub fn interp(&self) -> Interpreter<'_> {
        Interpreter::new(&self.program, self.eval.as_ref()).with_budget(self.budget)
    }

    /// Applies an event; on success records it and returns the new commands.
    pub fn send(&mut self, event: Event) -> Result<Vec<Command>, EventError> {
        let stimulus = Stimulus { at: self.now, event };
        let step = match &self.state {
            None => self.interp().start(&stimulus)?,
            Some(s) => self.interp().handle(s, &stimulus)?,
        };
        self.state = Some(step.state);
        self.stimuli.push(stimulus);
        self.commands.extend(step.commands.clone());
        self.last = step.commands.clone();
        Ok(step.commands)
    }

    pub fn start(&mut self, variables: BTreeMap<String, Value>) -> Vec<Command> {
        let id = uuid::Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);
        let initiator = json!({ "id": "u-initiator", "manager": "u-manager" });
        self.send(Event::Start { instance_id: id, initiator, variables }).unwrap()
    }

    /// The activation key of the single token waiting at `node`.
    pub fn key(&self, node: &str) -> String {
        let keys: Vec<String> =
            self.state().waits().filter(|(t, _)| t.node == node).map(|(_, w)| w.key.clone()).collect();
        assert_eq!(keys.len(), 1, "expected one wait at {node}, state: {:#?}", self.state().tokens);
        keys[0].clone()
    }

    pub fn wait_kind(&self, node: &str) -> WaitKind {
        self.state().waits().find(|(t, _)| t.node == node).map(|(_, w)| w.kind).expect("waiting")
    }

    pub fn try_complete(
        &mut self,
        node: &str,
        outcome: &str,
        v: BTreeMap<String, Value>,
    ) -> Result<(), EventError> {
        let key = self.key(node);
        self.send(Event::WorkItemCompleted { key, outcome: outcome.to_owned(), variables: v }).map(|_| ())
    }

    pub fn complete(&mut self, node: &str, outcome: &str) {
        self.try_complete(node, outcome, BTreeMap::new()).unwrap();
    }

    pub fn fire(&mut self, node: &str) {
        let key = self.key(node);
        self.send(Event::TimerFired { key }).unwrap();
    }

    pub fn fire_boundary(&mut self, node: &str, index: usize) {
        let key = format!("{}/boundary/{index}", self.key(node));
        self.send(Event::TimerFired { key }).unwrap();
    }

    pub fn fire_timeout(&mut self, node: &str) {
        let key = format!("{}/timeout", self.key(node));
        self.send(Event::TimerFired { key }).unwrap();
    }

    pub fn signal(&mut self, node: &str) {
        let key = self.key(node);
        self.send(Event::SignalReceived { key, variables: BTreeMap::new() }).unwrap();
    }

    pub fn service(&mut self, node: &str, outcome: &str) {
        let key = self.key(node);
        self.send(Event::ServiceCompleted { key, outcome: outcome.to_owned(), variables: BTreeMap::new() })
            .unwrap();
    }

    pub fn subprocess(&mut self, node: &str, outcome: &str) {
        let key = self.key(node);
        let ev = Event::SubprocessFinished { key, outcome: outcome.to_owned(), variables: BTreeMap::new() };
        self.send(ev).unwrap();
    }

    pub fn case_closed(&mut self, node: &str) {
        let key = self.key(node);
        self.send(Event::CaseClosed { key }).unwrap();
    }

    /// Nodes with a waiting token, sorted.
    pub fn waiting_at(&self) -> Vec<String> {
        let mut v: Vec<String> = self.state().waits().map(|(t, _)| t.node.clone()).collect();
        v.sort();
        v
    }

    /// Node ids entered, in order.
    pub fn path(&self) -> Vec<String> {
        self.state()
            .trace
            .iter()
            .filter(|e| e.event == TraceEvent::NodeEntered)
            .filter_map(|e| e.node.clone())
            .collect()
    }

    /// Replays the recorded stimuli from scratch and checks the result equals the live state.
    pub fn assert_replays(&self) {
        let replayed = self.interp().replay(&self.stimuli).unwrap();
        assert_eq!(&replayed.state, self.state(), "replay diverged");
        assert_eq!(replayed.commands, self.commands, "replayed commands diverged");
    }
}

pub fn commands_named<'a>(cmds: &'a [Command], name: &str) -> Vec<&'a Command> {
    cmds.iter().filter(|c| serde_json::to_value(c).unwrap()["command"] == name).collect()
}
