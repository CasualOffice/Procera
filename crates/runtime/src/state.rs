//! Instance state: tokens, splits, variables, status and the trace (docs/04 §2.5, §4.1).
//!
//! The state is a plain value. The interpreter takes one and returns the next; persistence
//! (BPMS-083) stores it, and the trace becomes `instance_events` rows. Every collection is ordered
//! (`BTreeMap`, `Vec` in creation order) so equal histories serialise to equal bytes.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;
use uuid::Uuid;

/// A token id, unique within its instance and allocated in creation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenId(pub u64);

/// A parallel-split instance id, unique within its instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SplitId(pub u64);

impl std::fmt::Display for TokenId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{}", self.0)
    }
}

impl std::fmt::Display for SplitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// Instance status (docs/04 §4.1). `Waiting` is derived after every event: every live token is
/// waiting. `Terminated` covers both `end{result: terminated}` and an instance cancel (the
/// stored state `cancelled` in docs/04 §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Running,
    Waiting,
    Completed,
    Terminated,
    Failed,
}

impl InstanceStatus {
    /// No further events are accepted.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Completed | Self::Terminated | Self::Failed)
    }
}

/// One process instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceState {
    pub instance_id: Uuid,
    pub process_key: String,
    pub process_version: u32,
    pub status: InstanceStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    /// The initiating principal as supplied at start (expression root `initiator`).
    pub initiator: Value,
    /// The expression root `instance`.
    pub instance: Value,
    pub variables: BTreeMap<String, Value>,
    /// Live tokens only. Consumed and cancelled tokens leave the map; the trace keeps them.
    pub tokens: BTreeMap<TokenId, Token>,
    /// Open parallel splits awaiting their join.
    pub splits: BTreeMap<SplitId, Split>,
    /// How many times each node has been entered (loop guard, docs/11 §4).
    pub entries: BTreeMap<String, u32>,
    /// History: one entry per transition, in order.
    pub trace: Vec<TraceEntry>,
    /// Next id for tokens, splits and activations.
    pub next_id: u64,
    /// Accepted events so far (+1 per accepted event, docs/15 §5).
    pub revision: u64,
}

impl InstanceState {
    pub(crate) fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Live tokens currently waiting, with their wait.
    pub fn waits(&self) -> impl Iterator<Item = (&Token, &Wait)> {
        self.tokens.values().filter_map(|t| match &t.state {
            TokenState::Waiting(w) => Some((t, w)),
            _ => None,
        })
    }

    /// Live tokens with an open incident.
    pub fn incidents(&self) -> impl Iterator<Item = (&Token, &Incident)> {
        self.tokens.values().filter_map(|t| match &t.state {
            TokenState::Incident(i) => Some((t, i)),
            _ => None,
        })
    }
}

/// A position in the process graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub id: TokenId,
    pub node: String,
    pub state: TokenState,
    /// The enclosing parallel branches, innermost last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
}

/// Membership of a token in one branch of an open split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub split: SplitId,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TokenState {
    /// Will be advanced by the current (or next, after a budget yield) advance.
    Ready,
    Waiting(Wait),
    /// Stuck until an operator acts (BPMS-088).
    Incident(Incident),
}

/// What a waiting token waits for. Completion events address it by `key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wait {
    pub kind: WaitKind,
    /// The activation key: work item, timer, subscription, connector call, child instance or case.
    pub key: String,
    /// Boundary timers armed for this activation: `(timer key, boundary index)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundary_timers: Vec<(String, usize)>,
    /// The `wait.signal` timeout timer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_timer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    WorkItem,
    Timer,
    Signal,
    Service,
    Subprocess,
    Case,
}

/// Runtime incident codes. `IR-R010` is fixed by docs/04 §4.5; the others are A-23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentCode {
    /// Expression or rule evaluation failed.
    #[serde(rename = "IR-R001")]
    Expression,
    /// A decision condition was not a boolean (docs/11 §6: null never selects a branch).
    #[serde(rename = "IR-R002")]
    NonBoolean,
    /// No edge for the produced outcome and no `unhandled: fail`.
    #[serde(rename = "IR-R003")]
    NoEdge,
    /// A node was entered more often than its `maxIterations` or the instance guard.
    #[serde(rename = "IR-R004")]
    LoopLimit,
    /// A token reached a join outside the split it belongs to.
    #[serde(rename = "IR-R005")]
    Join,
    /// A value had the wrong shape (timer instant, idempotency key).
    #[serde(rename = "IR-R006")]
    BadValue,
    /// Expression budget exhausted (docs/04 §4.5).
    #[serde(rename = "IR-R010")]
    ExprBudget,
}

impl IncidentCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expression => "IR-R001",
            Self::NonBoolean => "IR-R002",
            Self::NoEdge => "IR-R003",
            Self::LoopLimit => "IR-R004",
            Self::Join => "IR-R005",
            Self::BadValue => "IR-R006",
            Self::ExprBudget => "IR-R010",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    pub code: IncidentCode,
    pub message: String,
    pub key: String,
}

/// An open parallel split (docs/04 §2.5 `splits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub split_node: String,
    pub join_node: String,
    pub expected: Vec<String>,
    pub arrived: BTreeSet<String>,
}

/// One history entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEntry {
    pub seq: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub event: TraceEvent,
}

/// What happened. Payloads carry ids, outcomes and variable *names*, never values (docs/11 §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEvent {
    InstanceStarted,
    NodeEntered,
    NodeLeft { outcome: String },
    Waiting { wait: WaitKind, key: String },
    Resumed { key: String },
    BoundaryTriggered { on: String, interrupting: bool },
    Forked { split: SplitId, branches: Vec<String> },
    JoinArrived { split: SplitId, branch: String },
    JoinReleased { split: SplitId },
    VariablesSet { names: Vec<String> },
    TokenConsumed,
    TokenCancelled,
    IncidentRaised { code: IncidentCode },
    Yielded,
    InstanceCompleted,
    InstanceTerminated { reason: String },
    InstanceFailed { outcome: String },
}
