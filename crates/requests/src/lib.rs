//! Requests and approvals (Procera 0.2; BPMS-085 work items, BPMS-101 forms subset, BPMS-103
//! request API; docs/04 §2.6, §4.6, docs/08 "Requests"/"Work", docs/12 §10).
//!
//! - Request types are built in ([`catalogue`]): compiled at boot, pinned by checksum.
//! - Starting validates the start form ([`forms`]), shapes the initiator (and principal fields)
//!   from the directory, never from client input, and runs [`procera_engine::Engine::start_in`]
//!   in the caller's transaction with the user as the runtime audit actor.
//! - The engine's effect port ([`handler::Effects`]) creates work items with resolved candidates
//!   ([`resolve`]), cancels them, sends notifications and keeps the request status current.
//! - A decision re-checks candidacy and SoD under the instance lock, so exactly one decision wins.
//!
//! Permissions are checked by the caller (HTTP layer, Cedar); this crate enforces candidacy, SoD,
//! the lifecycle and "involved" visibility facts.

pub mod attach;
pub mod catalogue;
pub mod comments;
pub mod delegation;
mod escalate;
pub mod forms;
pub mod handler;
pub mod incidents;
mod offer;
pub mod policy;
pub mod preview;
pub mod resolve;
mod service;
mod views;
mod why;
pub mod workload;

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_db::Db;
use procera_engine::{CalendarSpec, Engine};
use procera_kernel::{Clock, IdGen};
use procera_members::Members;
use procera_notify::Notify;
use uuid::Uuid;

pub use attach::{Attachment, DecisionExtras};
pub use catalogue::{Catalogue, Field, Form, RequestType};
pub use comments::{Comment, Conversation, NewComment};
pub use delegation::{Away, OutOfOffice, SetOutOfOffice};
pub use forms::FieldError;
pub use incidents::Incident;
pub use offer::Offer;
pub use policy::PolicyRef;
pub use preview::Preview;
pub use service::{Scope, WorkState};
pub use views::{
    Decided, Reassigned, Release, RequestDetail, RequestSummary, Started, Step, TimelineEntry, WhyLine,
    WorkView,
};
pub use workload::{Reach, Workload};

/// The calendar every built-in names (`refs.calendars.default`), seeded per workspace on first use.
pub const DEFAULT_CALENDAR: &str = "calendar:default@1";

/// Monday to Friday, 09:00–17:00 UTC, no holidays, until workspace calendars are configurable.
pub fn default_calendar() -> CalendarSpec {
    let day = vec![["09:00".to_owned(), "17:00".to_owned()]];
    let working_hours: BTreeMap<String, Vec<[String; 2]>> =
        ["mon", "tue", "wed", "thu", "fri"].iter().map(|d| ((*d).to_owned(), day.clone())).collect();
    CalendarSpec { timezone: "+00:00".into(), working_hours, holidays: vec![] }
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("not found")]
    NotFound,
    #[error("unknown request type {0}")]
    UnknownType(String),
    #[error("invalid input")]
    Invalid(Vec<FieldError>),
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// The caller is excluded from this step by separation of duties (docs/12 §10).
    #[error("separation of duties: {0}")]
    SoD(String),
    #[error("someone already decided this")]
    AlreadyDecided,
    #[error("stale revision; current is {current}")]
    Stale { current: i64 },
    #[error("{0}")]
    Conflict(String),
    #[error("internal error: {0}")]
    Internal(String),
}

macro_rules! internal_from {
    ($($t:ty),*) => {$(
        impl From<$t> for RequestError {
            fn from(e: $t) -> Self {
                Self::Internal(e.to_string())
            }
        }
    )*};
}
internal_from!(
    procera_db::DbError,
    sqlx::Error,
    procera_audit::AuditError,
    procera_members::MembersError,
    serde_json::Error,
    procera_engine::EngineError,
    procera_settings::SettingsError
);

/// Who acts: a membership, its audit subject and the client it acts from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl Actor {
    pub(crate) fn runtime(&self) -> procera_engine::Actor {
        procera_engine::Actor {
            subject_ref: self.subject_ref,
            ip: self.ip.clone(),
            user_agent: self.user_agent.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Requests {
    pub(crate) db: Db,
    pub(crate) engine: Engine,
    pub(crate) catalogue: Arc<Catalogue>,
    pub(crate) members: Members,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdGen>,
    pub(crate) effects: handler::Effects,
}

impl std::fmt::Debug for Requests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Requests")
    }
}

impl Requests {
    /// Compiles the built-ins (fails fast) and wires the engine with the request effect port.
    pub fn new(
        db: Db,
        members: Members,
        notify: Notify,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        public_url: &str,
    ) -> Result<Self, String> {
        let catalogue = Arc::new(Catalogue::builtin()?);
        let effects = handler::Effects {
            catalogue: catalogue.clone(),
            notify,
            ids: ids.clone(),
            clock: clock.clone(),
            public_url: public_url.trim_end_matches('/').to_owned(),
        };
        // The built-in programs first, then processes published from the library (BPMS-084).
        let programs: Arc<dyn procera_engine::ProgramSource> = Arc::new(procera_processes::Chain(vec![
            catalogue.programs(),
            Arc::new(procera_processes::DbPrograms::new(db.clone())),
        ]));
        let engine = Engine::new(db.clone(), clock.clone(), ids.clone(), programs)
            .with_effects(Arc::new(effects.clone()));
        Ok(Self { db, engine, catalogue, members, clock, ids, effects })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    pub fn db(&self) -> &Db {
        &self.db
    }
}
