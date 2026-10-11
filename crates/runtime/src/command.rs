//! Effects as data. The interpreter never performs a side effect: it returns commands, and the
//! application layer executes them after the state commits (docs/04 §4.2, ADR-0005). Every
//! command carries an idempotency `key` derived only from the instance id and its history, so
//! replaying the same events yields the same keys.

use crate::state::{IncidentCode, TokenId};
use procera_ir::model::{Assignment, Channel, Due, Escalation, Quorum, Signature};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// Offer a work item for a `human.*` node (BPMS-085 resolves the assignment, quorum and
    /// escalations). Completion arrives as [`crate::Event::WorkItemCompleted`] with `key`.
    CreateWorkItem {
        key: String,
        token: TokenId,
        node: String,
        kind: WorkKind,
        assignment: Option<Assignment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<String>,
        /// Allowed outcomes, as the node declares them.
        outcomes: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        due: Option<Due>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        escalations: Vec<Escalation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quorum: Option<Quorum>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<Signature>,
        /// `human.sign` document / `human.acknowledge` subject.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
    },
    CancelWorkItem {
        key: String,
    },
    /// Arm a timer (BPMS-083 persists and fires it). Firing arrives as
    /// [`crate::Event::TimerFired`] with `key`.
    ScheduleTimer {
        key: String,
        token: TokenId,
        node: String,
        purpose: TimerPurpose,
        due: TimerDue,
    },
    CancelTimer {
        key: String,
    },
    /// Subscribe to a correlated message for `wait.signal`.
    SubscribeSignal {
        key: String,
        token: TokenId,
        node: String,
        correlation: Value,
    },
    CancelSubscription {
        key: String,
    },
    /// Invoke a connector action after commit (docs/11 §3 `service.call`).
    CallConnector {
        key: String,
        token: TokenId,
        node: String,
        connector: String,
        action: String,
        input: BTreeMap<String, Value>,
        /// The evaluated `idempotencyKey` expression handed to the connector.
        connector_key: String,
    },
    /// Fire-and-forget notification (`notify`).
    Notify {
        key: String,
        node: String,
        to: Option<Assignment>,
        template: Option<String>,
        channels: Vec<Channel>,
    },
    /// Start a pinned child instance (`subprocess.call`).
    StartSubprocess {
        key: String,
        token: TokenId,
        node: String,
        process: String,
    },
    CancelSubprocess {
        key: String,
    },
    /// Open a case (`case.open`).
    OpenCase {
        key: String,
        token: TokenId,
        node: String,
        case_type: Option<String>,
        wait_for_close: bool,
    },
    /// Raise an incident on a stuck token (BPMS-088 owns the lifecycle).
    RaiseIncident {
        key: String,
        token: TokenId,
        node: String,
        code: IncidentCode,
        message: String,
    },
    /// Publish a domain event (docs/04 §9) through the outbox.
    EmitEvent {
        key: String,
        topic: String,
        payload: Value,
    },
    /// The instance reached a final status.
    CompleteInstance {
        key: String,
        result: InstanceResult,
    },
    /// The step budget ran out with ready tokens left: enqueue another advance (docs/04 §4.2).
    ContinueAdvance {
        key: String,
    },
}

impl Command {
    pub fn key(&self) -> &str {
        match self {
            Self::CreateWorkItem { key, .. }
            | Self::CancelWorkItem { key }
            | Self::ScheduleTimer { key, .. }
            | Self::CancelTimer { key }
            | Self::SubscribeSignal { key, .. }
            | Self::CancelSubscription { key }
            | Self::CallConnector { key, .. }
            | Self::Notify { key, .. }
            | Self::StartSubprocess { key, .. }
            | Self::CancelSubprocess { key }
            | Self::OpenCase { key, .. }
            | Self::RaiseIncident { key, .. }
            | Self::EmitEvent { key, .. }
            | Self::CompleteInstance { key, .. }
            | Self::ContinueAdvance { key } => key,
        }
    }
}

/// Work-item kinds (docs/04 §2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Task,
    Approval,
    Sign,
    Acknowledge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerPurpose {
    /// `wait.timer`.
    Wait,
    /// A `timeout` boundary on a waiting node.
    Boundary,
    /// The `timeout` of a `wait.signal`.
    SignalTimeout,
}

/// When a timer is due. Calendar arithmetic is the scheduler's job (BPMS-083), so business
/// durations travel unresolved with their start instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimerDue {
    At {
        #[serde(with = "time::serde::rfc3339")]
        at: OffsetDateTime,
    },
    Business {
        #[serde(with = "time::serde::rfc3339")]
        from: OffsetDateTime,
        duration: String,
        calendar: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceResult {
    Completed,
    Terminated,
    Failed,
}
