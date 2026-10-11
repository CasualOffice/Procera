//! Stimuli: the only way anything enters an instance (docs/11 §1.1).
//!
//! Time and ids come from the injected [`Clock`] and [`IdGen`] **once**, when the application
//! stamps a new stimulus. The stamped stimulus is what gets recorded, so replaying the recorded
//! sequence needs neither port and reproduces the state exactly.

use procera_kernel::{Clock, IdGen};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// An event with its recorded time. `at` is `now` for every expression it triggers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stimulus {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub event: Event,
}

impl Stimulus {
    /// Stamps an event with the clock's current time.
    pub fn stamp(clock: &dyn Clock, event: Event) -> Self {
        Self { at: clock.now(), event }
    }

    /// A new instance: the id comes from `ids`, the time from `clock`.
    pub fn start(
        clock: &dyn Clock,
        ids: &dyn IdGen,
        initiator: Value,
        variables: BTreeMap<String, Value>,
    ) -> Self {
        Self::stamp(clock, Event::Start { instance_id: ids.new_id(), initiator, variables })
    }
}

/// Everything an instance reacts to. Completion events name the activation `key` that the
/// matching command carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Start a new instance with its start-form variables.
    Start {
        instance_id: Uuid,
        initiator: Value,
        #[serde(default)]
        variables: BTreeMap<String, Value>,
    },
    /// A work item finished with an outcome; `variables` are its form bindings.
    WorkItemCompleted {
        key: String,
        outcome: String,
        #[serde(default)]
        variables: BTreeMap<String, Value>,
    },
    /// A timer armed by `ScheduleTimer` fired.
    TimerFired { key: String },
    /// A correlated message arrived for a `SubscribeSignal`.
    SignalReceived {
        key: String,
        #[serde(default)]
        variables: BTreeMap<String, Value>,
    },
    /// A connector call finished: `ok`, `error` or a typed `error:<CODE>`.
    ServiceCompleted {
        key: String,
        outcome: String,
        #[serde(default)]
        variables: BTreeMap<String, Value>,
    },
    /// A child instance finished: `completed`, `failed` or `cancelled`.
    SubprocessFinished {
        key: String,
        outcome: String,
        #[serde(default)]
        variables: BTreeMap<String, Value>,
    },
    /// A case opened with `waitForClose: true` was closed.
    CaseClosed { key: String },
    /// A user or admin cancelled the instance.
    Cancel { reason: String },
    /// Resume after a step-budget yield (`ContinueAdvance`).
    Continue,
}
