//! Response shapes of the request and work API (docs/08 "Requests", "Work").

use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::catalogue::Field;

/// `{id, name}` of a member.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Started {
    pub id: Uuid,
    pub display_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestSummary {
    pub id: Uuid,
    pub display_id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub type_name: String,
    pub title: String,
    /// `in_progress`, `changes_requested`, `approved`, `rejected` or `cancelled`.
    pub status: String,
    pub current_step: Option<String>,
    /// Names of the people the open steps are offered to.
    pub assignees: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub initiator: Person,
}

/// One human step of a request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub id: Uuid,
    pub node: String,
    pub name: String,
    pub kind: String,
    /// `offered`, `claimed`, `completed` or `cancelled`.
    pub state: String,
    pub assignees: Vec<String>,
    /// Nobody could be resolved: an admin must step in.
    pub unassigned: bool,
    pub decided_by: Option<String>,
    pub outcome: Option<String>,
    pub comment: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub decided_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub due_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimelineEntry {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// `submitted`, `decided`, `reminded`, `escalated`, `reassigned`, `cancelled`, `timer`, `completed`.
    pub kind: String,
    pub text: String,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sla {
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
    pub breached: bool,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestDetail {
    #[serde(flatten)]
    pub summary: RequestSummary,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    /// The start-form fields, for labels and types.
    pub fields: Vec<Field>,
    /// Current values by field name; principals as `{id, name}`.
    pub input: serde_json::Map<String, Value>,
    pub steps: Vec<Step>,
    pub timeline: Vec<TimelineEntry>,
    pub sla: Option<Sla>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkRequest {
    pub id: Uuid,
    pub display_id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub type_name: String,
    pub title: String,
    pub initiator: Person,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkView {
    pub id: Uuid,
    pub request: WorkRequest,
    pub step: String,
    pub kind: String,
    pub state: String,
    pub outcomes: Vec<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub due_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// The `If-Match` revision for a decision.
    pub revision: i64,
    /// Fields the decider may (re)fill, with their current values in `values`.
    pub fields: Vec<Field>,
    pub values: serde_json::Map<String, Value>,
    pub unassigned: bool,
    /// Open and past its due date (by the server clock).
    pub overdue: bool,
    /// Names of the people the step is offered to.
    pub assignees: Vec<String>,
    pub outcome: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

/// The result of an admin reassignment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reassigned {
    pub id: Uuid,
    pub revision: i64,
    pub assignees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decided {
    pub id: Uuid,
    pub revision: i64,
    pub outcome: String,
    pub request: Started,
}
