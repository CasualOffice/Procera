//! SLA clocks (docs/11 §9, docs/04 §2.12, FR-E12): start, pause, resume, stop and breach.
//!
//! The clocks are **level-triggered** from the instance state (A-29):
//! - an `instance` SLA runs while the instance is not final;
//! - a node SLA (`node:<id>`) runs while a live token sits on that node;
//! - a running SLA is paused while a live token sits on a `pauseWhile` node (an optional
//!   `<kind>:` prefix is ignored).
//!
//! After every accepted stimulus the phase before and after is compared, and the difference is a
//! list of transitions. Each transition updates `sla_clocks`, arms or cancels the breach timer,
//! and emits `sla.{started,paused,resumed,stopped}` so attainment can be recomputed from events.
//! A due breach timer marks the clock breached, emits `sla.breached` and records the SLA's breach
//! actions as a pending effect (notification routing arrives with its item).

use std::collections::BTreeSet;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use procera_db::outbox::{self, OutboxEvent};
use procera_ir::model::Duration as IrDuration;
use procera_runtime::{InstanceState, Program};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::calendar::BusinessDuration;
use crate::effects::{self, NewTimer};
use crate::engine::{Engine, Executable};
use crate::{EngineError, store};

/// One SLA definition, ready for phase computation.
#[derive(Debug, Clone, PartialEq)]
pub struct SlaSpec {
    pub id: String,
    pub node: Option<String>,
    pub target: String,
    pub business: bool,
    pub pause: BTreeSet<String>,
    pub breach: Value,
}

impl SlaSpec {
    pub fn of(program: &Program) -> Vec<Self> {
        program
            .ir()
            .sla
            .iter()
            .map(|s| SlaSpec {
                id: s.id.clone(),
                node: match s.scope_node() {
                    Some(n) => Some(n.to_owned()),
                    None if s.scope == "instance" => None,
                    None => Some(s.scope.clone()),
                },
                target: s.target.0.clone(),
                business: s.business == Some(true),
                pause: s
                    .pause_while
                    .iter()
                    .flatten()
                    .map(|p| p.rsplit_once(':').map_or(p.as_str(), |(_, n)| n).to_owned())
                    .collect(),
                breach: serde_json::to_value(&s.breach).unwrap_or(Value::Null),
            })
            .collect()
    }

    pub fn phase(&self, state: Option<&InstanceState>) -> Phase {
        let Some(st) = state else { return Phase::Inactive };
        let active = match &self.node {
            None => !st.status.is_final(),
            Some(n) => st.tokens.values().any(|t| &t.node == n),
        };
        if !active {
            Phase::Inactive
        } else if st.tokens.values().any(|t| self.pause.contains(&t.node)) {
            Phase::Paused
        } else {
            Phase::Running
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Inactive,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Start,
    Pause,
    Resume,
    Stop,
}

/// The transitions that move a clock from `before` to `after`.
pub fn transitions(before: Phase, after: Phase) -> &'static [Transition] {
    use Phase::*;
    match (before, after) {
        (Inactive, Running) => &[Transition::Start],
        (Inactive, Paused) => &[Transition::Start, Transition::Pause],
        (Running, Paused) => &[Transition::Pause],
        (Paused, Running) => &[Transition::Resume],
        (Running | Paused, Inactive) => &[Transition::Stop],
        _ => &[],
    }
}

fn breach_key(clock_key: &str) -> String {
    format!("{clock_key}/breach")
}

async fn emit(
    tx: &mut WsTx<'_>,
    topic: &str,
    instance: Uuid,
    revision: i64,
    payload: Value,
) -> Result<(), EngineError> {
    let ev = OutboxEvent {
        topic,
        aggregate_type: "process_instance",
        aggregate_id: instance,
        aggregate_revision: revision,
        payload,
        trace_id: None,
    };
    outbox::enqueue(tx, ev).await?;
    Ok(())
}

type OpenClock = (String, Option<OffsetDateTime>, OffsetDateTime, bool, Option<Uuid>, Option<OffsetDateTime>);

/// Moves every SLA clock of the instance from the `before` state to `after`.
pub(crate) async fn sync(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    exe: &Executable,
    before: Option<&InstanceState>,
    after: &InstanceState,
    at: OffsetDateTime,
) -> Result<(), EngineError> {
    let iid = after.instance_id;
    let revision = i64::try_from(after.revision).unwrap_or(i64::MAX);
    for spec in SlaSpec::of(&exe.program) {
        for tr in transitions(spec.phase(before), spec.phase(Some(after))) {
            let base =
                json!({ "instance_id": iid, "sla": spec.id, "node": spec.node, "at": at.unix_timestamp() });
            match tr {
                Transition::Start => start(eng, tx, exe, &spec, iid, revision, at).await?,
                Transition::Pause => {
                    let key: Option<(String,)> = sqlx::query_as(
                        "UPDATE sla_clocks SET paused_since = $3
                         WHERE instance_id = $1 AND sla_key = $2 AND stopped_at IS NULL AND paused_since IS NULL RETURNING clock_key",
                    )
                    .bind(iid)
                    .bind(&spec.id)
                    .bind(at)
                    .fetch_optional(tx.conn())
                    .await?;
                    if let Some((k,)) = key {
                        effects::cancel_timer(tx, &breach_key(&k)).await?;
                        emit(tx, "sla.paused.v1", iid, revision, base).await?;
                    }
                }
                Transition::Resume => {
                    let open: Option<OpenClock> = sqlx::query_as(
                        "SELECT clock_key, paused_since, due_at, business, calendar_version_id, breached_at FROM sla_clocks
                         WHERE instance_id = $1 AND sla_key = $2 AND stopped_at IS NULL AND paused_since IS NOT NULL FOR UPDATE",
                    )
                    .bind(iid)
                    .bind(&spec.id)
                    .fetch_optional(tx.conn())
                    .await?;
                    let Some((k, Some(since), due, business, cal, breached)) = open else { continue };
                    let new_due = match (business, cal) {
                        (true, Some(cid)) => {
                            let c = store::calendar_by_id(tx, cid).await?.calendar;
                            c.add_working(at, c.working_between(since, due))?
                        }
                        _ => due + (at - since),
                    };
                    sqlx::query(
                        "UPDATE sla_clocks SET paused_total = paused_total + make_interval(secs => $2), paused_since = NULL, due_at = $3
                         WHERE clock_key = $1",
                    )
                    .bind(&k)
                    .bind((at - since).as_seconds_f64())
                    .bind(new_due)
                    .execute(tx.conn())
                    .await?;
                    if breached.is_none() {
                        sqlx::query(
                            "UPDATE timers SET due_at = $2, state = 'scheduled' WHERE timer_key = $1 AND state = 'cancelled'",
                        )
                        .bind(breach_key(&k))
                        .bind(new_due)
                        .execute(tx.conn())
                        .await?;
                    }
                    emit(tx, "sla.resumed.v1", iid, revision, base).await?;
                }
                Transition::Stop => {
                    let row: Option<(String, Option<OffsetDateTime>)> = sqlx::query_as(
                        "UPDATE sla_clocks SET stopped_at = $3,
                                paused_total = paused_total + COALESCE($3 - paused_since, interval '0'), paused_since = NULL
                         WHERE instance_id = $1 AND sla_key = $2 AND stopped_at IS NULL RETURNING clock_key, breached_at",
                    )
                    .bind(iid)
                    .bind(&spec.id)
                    .bind(at)
                    .fetch_optional(tx.conn())
                    .await?;
                    if let Some((k, breached)) = row {
                        effects::cancel_timer(tx, &breach_key(&k)).await?;
                        let mut p = base;
                        p["met"] = json!(breached.is_none());
                        emit(tx, "sla.stopped.v1", iid, revision, p).await?;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn start(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    exe: &Executable,
    spec: &SlaSpec,
    iid: Uuid,
    revision: i64,
    at: OffsetDateTime,
) -> Result<(), EngineError> {
    let clock_key = format!("{iid}/sla/{}/{revision}", spec.id);
    let secs = IrDuration(spec.target.clone())
        .seconds()
        .ok_or_else(|| EngineError::State(format!("bad SLA target {}", spec.target)))?;
    let (due, cal) = if spec.business {
        let cref = exe.calendar_ref(None).ok_or_else(|| EngineError::CalendarMissing("(none)".into()))?;
        let cal = store::calendar_by_ref(tx, &cref).await?;
        (cal.calendar.add(at, BusinessDuration::parse(&spec.target)?)?, Some(cal))
    } else {
        (at + Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX)), None)
    };
    sqlx::query(
        "INSERT INTO sla_clocks (workspace_id, id, clock_key, instance_id, sla_key, node_id, target, target_text, business,
                                 calendar_version_id, breach, started_at, due_at)
         VALUES ($1, $2, $3, $4, $5, $6, make_interval(secs => $7), $8, $9, $10, $11, $12, $13)
         ON CONFLICT (workspace_id, clock_key) DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(eng.ids.new_id())
    .bind(&clock_key)
    .bind(iid)
    .bind(&spec.id)
    .bind(&spec.node)
    .bind(secs as f64)
    .bind(&spec.target)
    .bind(spec.business)
    .bind(cal.as_ref().map(|c| c.id))
    .bind(&spec.breach)
    .bind(at)
    .bind(due)
    .execute(tx.conn())
    .await?;
    let rule = if spec.business { format!("business {} from {at}", spec.target) } else { "after".to_owned() };
    let timer = NewTimer {
        key: &breach_key(&clock_key),
        instance: iid,
        token: None,
        node: spec.node.as_deref(),
        purpose: "sla",
        due_at: due,
        calendar_version: cal.as_ref().map(|c| c.id),
        source_tz: cal.as_ref().map(|c| c.timezone.as_str()),
        rule: &rule,
    };
    effects::insert_timer(eng, tx, timer).await?;
    let payload = json!({ "instance_id": iid, "sla": spec.id, "node": spec.node, "at": at.unix_timestamp(),
                          "due_at": due.unix_timestamp(), "business": spec.business });
    emit(tx, "sla.started.v1", iid, revision, payload).await
}

/// A due SLA breach timer fired: mark the clock breached once, emit `sla.breached`, record the
/// breach actions as a pending effect, and audit it. Returns whether the clock was breached.
pub(crate) async fn breach(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    timer_key: &str,
    at: OffsetDateTime,
) -> Result<bool, EngineError> {
    let Some(clock_key) = timer_key.strip_suffix("/breach") else { return Ok(false) };
    let row: Option<(Uuid, String, Option<String>, Value, OffsetDateTime)> = sqlx::query_as(
        "UPDATE sla_clocks SET breached_at = $2 WHERE clock_key = $1 AND stopped_at IS NULL AND breached_at IS NULL
         RETURNING instance_id, sla_key, node_id, breach, due_at",
    )
    .bind(clock_key)
    .bind(at)
    .fetch_optional(tx.conn())
    .await?;
    let Some((iid, sla, node, actions, due)) = row else { return Ok(false) };
    let (revision,): (i64,) = sqlx::query_as("SELECT revision FROM process_instances WHERE id = $1")
        .bind(iid)
        .fetch_one(tx.conn())
        .await?;
    let payload = json!({ "instance_id": iid, "sla": sla, "node": node, "due_at": due.unix_timestamp(), "at": at.unix_timestamp() });
    emit(tx, "sla.breached.v1", iid, revision, payload.clone()).await?;
    let mut effect = payload;
    effect["actions"] = actions;
    effects::record(tx, iid, timer_key, "sla_breach", effect, true, eng.clock.now()).await?;
    let mut ev =
        AuditEvent::new(None, ActorKind::System, Category::Runtime, "sla.breached", Outcome::Allowed)
            .object("process_instance", iid);
    ev.diff = Some(json!({ "sla": sla, "node": node }));
    append(tx, eng.clock.as_ref(), eng.ids.as_ref(), &ev).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn phase() -> impl Strategy<Value = Phase> {
        prop_oneof![Just(Phase::Inactive), Just(Phase::Running), Just(Phase::Paused)]
    }

    /// A clock model: (open, paused).
    fn step(mut clock: (bool, bool), tr: Transition) -> Result<(bool, bool), String> {
        match tr {
            Transition::Start if !clock.0 => clock = (true, false),
            Transition::Pause if clock.0 && !clock.1 => clock.1 = true,
            Transition::Resume if clock.0 && clock.1 => clock.1 = false,
            Transition::Stop if clock.0 => clock = (false, false),
            _ => return Err(format!("{tr:?} is invalid in {clock:?}")),
        }
        Ok(clock)
    }

    proptest! {
        /// Any sequence of phases yields only valid transitions, and the clock always ends in the
        /// target phase: open iff active, paused iff paused.
        #[test]
        fn transitions_track_the_phase(phases in prop::collection::vec(phase(), 1..30)) {
            let mut clock = (false, false);
            let mut prev = Phase::Inactive;
            for p in phases {
                for tr in transitions(prev, p) {
                    clock = step(clock, *tr).map_err(TestCaseError::fail)?;
                }
                prop_assert_eq!(clock, (p != Phase::Inactive, p == Phase::Paused));
                prev = p;
            }
        }
    }
}
