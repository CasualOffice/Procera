//! The interpreter: `(program, state, stimulus) → (state', commands)` (docs/04 §4.2).
//!
//! It is a pure function. It reads no clock (the stimulus carries its time), allocates ids from
//! the state's own counter, performs no I/O, and every advance is bounded by [`Budget`].

use crate::command::{Command, InstanceResult};
use crate::event::{Event, Stimulus};
use crate::port::{EvalError, Evaluator, Scope, instance_value};
use crate::program::Program;
use crate::state::{
    Frame, IncidentCode, InstanceState, InstanceStatus, Token, TokenId, TokenState, TraceEntry, TraceEvent,
};
use procera_ir::model::{Expr, Node};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use time::OffsetDateTime;

mod nodes;
mod resume;

/// Bounds on one advance (docs/04 §4.2, docs/11 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Node executions per advance. When it runs out with ready tokens left, the advance yields
    /// with [`Command::ContinueAdvance`]; nothing is lost and nothing hangs.
    pub max_steps: u32,
    /// Entries per node per instance when the node declares no `maxIterations`. A node entered
    /// more often raises incident `IR-R004` instead of looping.
    pub max_node_entries: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self { max_steps: 200, max_node_entries: 1_000 }
    }
}

/// Why a stimulus was rejected. The state is unchanged; a duplicate or stale completion lands
/// here and the caller treats it as a no-op (docs/04 §4.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    #[error("the first event of an instance must be start")]
    NotStart,
    #[error("the instance has already started")]
    AlreadyStarted,
    #[error("the instance is {0:?} and accepts no events")]
    Final(InstanceStatus),
    #[error("no token waits on activation {0}")]
    UnknownKey(String),
    #[error("activation {key} does not accept {event}")]
    WrongEvent { key: String, event: &'static str },
    #[error("outcome {outcome} is not an outcome of node {node}")]
    Outcome { node: String, outcome: String },
    #[error("variable {0} is not declared by the process")]
    UndeclaredVariable(String),
    #[error("no ready token to continue")]
    NothingToContinue,
}

/// The result of an accepted stimulus.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub state: InstanceState,
    /// To execute after the state commits, in order.
    pub commands: Vec<Command>,
}

/// Executes one program. Cheap to construct; holds only references.
#[derive(Debug, Clone, Copy)]
pub struct Interpreter<'a> {
    program: &'a Program,
    evaluator: &'a dyn Evaluator,
    budget: Budget,
}

impl<'a> Interpreter<'a> {
    pub fn new(program: &'a Program, evaluator: &'a dyn Evaluator) -> Self {
        Self { program, evaluator, budget: Budget::default() }
    }

    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Starts an instance from an [`Event::Start`] stimulus.
    pub fn start(&self, stimulus: &Stimulus) -> Result<Step, EventError> {
        let Event::Start { instance_id, initiator, variables } = &stimulus.event else {
            return Err(EventError::NotStart);
        };
        self.check_declared(variables)?;
        let ir = self.program.ir();
        let state = InstanceState {
            instance_id: *instance_id,
            process_key: ir.process.key.clone(),
            process_version: ir.process.version,
            status: InstanceStatus::Running,
            started_at: stimulus.at,
            ended_at: None,
            initiator: initiator.clone(),
            instance: instance_value(*instance_id, stimulus.at),
            variables: variables.clone(),
            tokens: BTreeMap::new(),
            splits: BTreeMap::new(),
            entries: BTreeMap::new(),
            trace: Vec::new(),
            next_id: 0,
            revision: 1,
        };
        let mut run = Run { it: *self, st: state, cmds: Vec::new(), at: stimulus.at };
        run.trace(None, None, TraceEvent::InstanceStarted);
        run.emit("instance.started", json!({}));
        if !variables.is_empty() {
            run.trace(None, None, TraceEvent::VariablesSet { names: variables.keys().cloned().collect() });
        }
        let start = self.program.start().to_owned();
        let id = run.spawn(&start, Vec::new());
        run.enter(id, &start);
        Ok(run.advance())
    }

    /// Applies a stimulus to a running instance.
    pub fn handle(&self, state: &InstanceState, stimulus: &Stimulus) -> Result<Step, EventError> {
        if state.status.is_final() {
            return Err(EventError::Final(state.status));
        }
        let mut run = Run { it: *self, st: state.clone(), cmds: Vec::new(), at: stimulus.at };
        run.apply(&stimulus.event)?;
        run.st.revision += 1;
        Ok(run.advance())
    }

    /// Replays a recorded stimulus sequence from the start. Commands are concatenated.
    pub fn replay<'s>(&self, stimuli: impl IntoIterator<Item = &'s Stimulus>) -> Result<Step, ReplayError> {
        let mut it = stimuli.into_iter();
        let first = it.next().ok_or(ReplayError::Empty)?;
        let mut step = self.start(first).map_err(|e| ReplayError::Rejected { index: 0, error: e })?;
        for (i, s) in it.enumerate() {
            let next =
                self.handle(&step.state, s).map_err(|e| ReplayError::Rejected { index: i + 1, error: e })?;
            step.state = next.state;
            step.commands.extend(next.commands);
        }
        Ok(step)
    }

    fn check_declared(&self, vars: &BTreeMap<String, Value>) -> Result<(), EventError> {
        match vars.keys().find(|k| !self.program.declares(k)) {
            Some(k) => Err(EventError::UndeclaredVariable(k.clone())),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("nothing to replay")]
    Empty,
    #[error("stimulus {index} rejected: {error}")]
    Rejected { index: usize, error: EventError },
}

/// One advance in progress.
struct Run<'a> {
    it: Interpreter<'a>,
    st: InstanceState,
    cmds: Vec<Command>,
    at: OffsetDateTime,
}

impl Run<'_> {
    fn advance(mut self) -> Step {
        let mut steps = 0u32;
        while !self.st.status.is_final() {
            let Some(id) = self.st.tokens.values().find(|t| t.state == TokenState::Ready).map(|t| t.id)
            else {
                break;
            };
            if steps >= self.it.budget.max_steps {
                self.trace(None, None, TraceEvent::Yielded);
                let key = format!("{}/continue/{}", self.st.instance_id, self.st.trace.len());
                self.cmds.push(Command::ContinueAdvance { key });
                break;
            }
            steps += 1;
            self.execute(id);
        }
        if !self.st.status.is_final() {
            if self.st.tokens.is_empty() {
                self.finish(InstanceResult::Completed, String::new());
            } else if self.st.tokens.values().any(|t| !matches!(t.state, TokenState::Waiting(_))) {
                self.st.status = InstanceStatus::Running;
            } else {
                self.st.status = InstanceStatus::Waiting;
            }
        }
        Step { state: self.st, commands: self.cmds }
    }

    fn iid(&self) -> String {
        self.st.instance_id.to_string()
    }

    fn trace(&mut self, token: Option<TokenId>, node: Option<&str>, event: TraceEvent) {
        let seq = self.st.trace.len() as u64 + 1;
        self.st.trace.push(TraceEntry { seq, at: self.at, token, node: node.map(str::to_owned), event });
    }

    fn emit(&mut self, topic: &str, extra: Value) {
        let mut payload = json!({
            "instance_id": self.iid(),
            "process_key": self.st.process_key,
            "process_version": self.st.process_version,
        });
        if let (Some(p), Value::Object(extra)) = (payload.as_object_mut(), extra) {
            p.extend(extra);
        }
        let key = format!("{}/event/{topic}", self.iid());
        self.cmds.push(Command::EmitEvent { key, topic: topic.to_owned(), payload });
    }

    fn node(&self, id: &str) -> Option<Node> {
        self.it.program.node(id).cloned()
    }

    fn token_node(&self, id: TokenId) -> Option<String> {
        self.st.tokens.get(&id).map(|t| t.node.clone())
    }

    /// A new ready token at `node` (not yet entered).
    fn spawn(&mut self, node: &str, frames: Vec<Frame>) -> TokenId {
        let id = TokenId(self.st.alloc());
        self.st.tokens.insert(id, Token { id, node: node.to_owned(), state: TokenState::Ready, frames });
        id
    }

    /// Moves a token onto `node`, counting the entry against the loop guard.
    fn enter(&mut self, id: TokenId, node: &str) {
        if let Some(t) = self.st.tokens.get_mut(&id) {
            t.node = node.to_owned();
            t.state = TokenState::Ready;
        }
        self.trace(Some(id), Some(node), TraceEvent::NodeEntered);
        let count = self.st.entries.entry(node.to_owned()).or_insert(0);
        *count += 1;
        let count = *count;
        let limit = self.node(node).and_then(|n| n.max_iterations).unwrap_or(self.it.budget.max_node_entries);
        if count > limit {
            self.incident(id, IncidentCode::LoopLimit, format!("entered {count} times, limit {limit}"));
        }
    }

    /// Leaves the token's node on `outcome` and enters the target.
    fn leave(&mut self, id: TokenId, outcome: &str) {
        let Some(node_id) = self.token_node(id) else { return };
        self.trace(Some(id), Some(&node_id), TraceEvent::NodeLeft { outcome: outcome.to_owned() });
        match self.it.program.target(&node_id, outcome).map(str::to_owned) {
            Some(to) => self.enter(id, &to),
            None if self.node(&node_id).is_some_and(|n| n.unhandled.is_some()) => {
                self.trace(
                    Some(id),
                    Some(&node_id),
                    TraceEvent::InstanceFailed { outcome: outcome.to_owned() },
                );
                self.finish(InstanceResult::Failed, format!("{node_id}:{outcome}"));
            }
            None => self.incident(id, IncidentCode::NoEdge, format!("no edge for outcome {outcome}")),
        }
    }

    fn incident(&mut self, id: TokenId, code: IncidentCode, message: String) {
        let Some(node) = self.token_node(id) else { return };
        let key = format!("{}/incident/{}", self.iid(), self.st.alloc());
        if let Some(t) = self.st.tokens.get_mut(&id) {
            t.state = TokenState::Incident(crate::state::Incident {
                code,
                message: message.clone(),
                key: key.clone(),
            });
        }
        self.trace(Some(id), Some(&node), TraceEvent::IncidentRaised { code });
        self.cmds.push(Command::RaiseIncident { key, token: id, node, code, message });
    }

    fn eval_failed(&mut self, id: TokenId, e: EvalError) {
        let code = match e {
            EvalError::Budget(_) => IncidentCode::ExprBudget,
            EvalError::Failed(_) => IncidentCode::Expression,
        };
        self.incident(id, code, e.to_string());
    }

    /// Evaluates the expression at `field` of `node`, with the compiler's checked form.
    fn eval(&self, node: &str, field: &str, expr: &Expr) -> Result<Value, EvalError> {
        self.it.evaluator.eval(expr, self.it.program.expression(node, field), &self.scope())
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            variables: &self.st.variables,
            instance: &self.st.instance,
            initiator: &self.st.initiator,
            now: self.at,
            schema: self.it.program.schema(),
        }
    }

    /// Ends the instance. Every live token is cancelled together with its open activations.
    fn finish(&mut self, result: InstanceResult, reason: String) {
        let tokens: Vec<Token> = std::mem::take(&mut self.st.tokens).into_values().collect();
        for t in tokens {
            if let TokenState::Waiting(w) = &t.state {
                self.cancel_wait(w);
            }
            if result != InstanceResult::Completed {
                self.trace(Some(t.id), Some(&t.node), TraceEvent::TokenCancelled);
            }
        }
        self.st.splits.clear();
        self.st.ended_at = Some(self.at);
        let (status, topic) = match result {
            InstanceResult::Completed => (InstanceStatus::Completed, "instance.completed"),
            InstanceResult::Terminated => (InstanceStatus::Terminated, "instance.cancelled"),
            InstanceResult::Failed => (InstanceStatus::Failed, "instance.failed"),
        };
        self.st.status = status;
        match result {
            InstanceResult::Completed => self.trace(None, None, TraceEvent::InstanceCompleted),
            InstanceResult::Terminated => {
                self.trace(None, None, TraceEvent::InstanceTerminated { reason: reason.clone() })
            }
            InstanceResult::Failed => {}
        }
        let extra = if reason.is_empty() { json!({}) } else { json!({ "reason": reason }) };
        self.emit(topic, extra);
        let key = format!("{}/complete", self.iid());
        self.cmds.push(Command::CompleteInstance { key, result });
    }
}
