//! Procera runtime engine (BPMS-083, docs/04 §4.2-§4.4): everything around the pure interpreter
//! that needs a database or a clock.
//!
//! - **Instances** ([`Engine`]): `start` and `advance` run the interpreter and persist its state,
//!   the stimulus log (`instance_events`), the commands as durable effects, the SLA clocks, the
//!   outbox events and the audit record in **one transaction**, with optimistic revision checks.
//!   [`Engine::replay`] rebuilds the state from the log.
//! - **Jobs** ([`jobs`]): leased with a fencing token, retried with backoff, dead-lettered; the
//!   rules are stated and property-tested in [`lease`].
//! - **Timers** ([`Scheduler`]): due timers fire once by enqueuing `timer_fired` stimuli; lateness
//!   is measured for the SLO.
//! - **Calendars** ([`calendar`], [`store`]): business-duration arithmetic over immutable calendar
//!   versions.
//! - **SLA clocks** ([`sla`]): start, pause, resume, stop and breach, emitted as domain events.

pub mod calendar;
mod effects;
pub mod engine;
pub mod jobs;
pub mod lease;
pub mod scheduler;
pub mod sla;
pub mod store;

pub use calendar::{BusinessDuration, Calendar, CalendarError, CalendarSpec};
pub use engine::{
    Actor, Advanced, BoxFuture, EffectContext, EffectHandler, Engine, Executable, InstanceRecord, ProgramRef,
    ProgramSource, StaticPrograms, stored_status,
};
pub use lease::{Lease, Stale};
pub use scheduler::{Lateness, LatenessSnapshot, Pass, Scheduler};
pub use store::{CalendarVersion, calendar_by_id, calendar_by_ref, put_calendar};

use procera_runtime::EventError;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("instance {0} not found")]
    NotFound(Uuid),
    #[error("revision conflict: expected {expected}, found {actual}")]
    Conflict { expected: u64, actual: u64 },
    /// A stale or duplicate stimulus; nothing changed (docs/04 §4.2).
    #[error("stimulus rejected: {0}")]
    Rejected(EventError),
    #[error("program unavailable: {0}")]
    Program(String),
    /// Restricted variables need envelope encryption at rest, which is not built yet (A-28).
    #[error("variable {0} is restricted; restricted instance data is not supported yet")]
    Restricted(String),
    /// The effect handler (work items, notifications) failed; the advance rolls back.
    #[error("effects failed: {0}")]
    Effects(String),
    #[error("calendar {0} not found")]
    CalendarMissing(String),
    #[error(transparent)]
    Calendar(#[from] CalendarError),
    #[error("replay failed: {0}")]
    Replay(String),
    #[error("bad stored state: {0}")]
    State(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Db(#[from] procera_db::DbError),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Audit(#[from] procera_audit::AuditError),
}
