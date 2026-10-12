//! Commands become durable effects in the advance transaction (docs/04 §4.2, docs/11 §15).
//!
//! Every command is first recorded in `instance_effects` under its deterministic key; a command
//! whose key is already recorded is skipped, so an effect is never applied twice even if the
//! same commands are produced again. Then:
//!
//! | Command | Effect |
//! |---|---|
//! | `schedule_timer` | a `timers` row; business durations resolved with the pinned calendar version |
//! | `cancel_timer` | the scheduled timer is cancelled |
//! | `continue_advance` | an `advance` job carrying a `continue` stimulus |
//! | `emit_event` | an outbox event `<topic>.v1` on the instance aggregate |
//! | `complete_instance` | recorded (the status is already in the state) |
//! | everything else | recorded as `pending` for the item that owns it (A-28) |

use procera_db::WsTx;
use procera_db::outbox::{self, OutboxEvent};
use procera_kernel::IdGen;
use procera_runtime::{Command, Event, InstanceState, Stimulus, TimerDue, TimerPurpose};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::calendar::BusinessDuration;
use crate::engine::{Engine, Executable};
use crate::{EngineError, jobs, store};

/// Records an effect once; `false` when the key was already recorded.
pub(crate) async fn record(
    tx: &mut WsTx<'_>,
    instance: Uuid,
    key: &str,
    command: &str,
    payload: Value,
    pending: bool,
    now: OffsetDateTime,
) -> Result<bool, EngineError> {
    let n = sqlx::query(
        "INSERT INTO instance_effects (workspace_id, effect_key, command, instance_id, payload, state, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(key)
    .bind(command)
    .bind(instance)
    .bind(payload)
    .bind(if pending { "pending" } else { "applied" })
    .bind(now)
    .execute(tx.conn())
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// A timer to insert.
pub(crate) struct NewTimer<'a> {
    pub key: &'a str,
    pub instance: Uuid,
    pub token: Option<u64>,
    pub node: Option<&'a str>,
    pub purpose: &'a str,
    pub due_at: OffsetDateTime,
    pub calendar_version: Option<Uuid>,
    pub source_tz: Option<&'a str>,
    pub rule: &'a str,
}

pub(crate) async fn insert_timer(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    t: NewTimer<'_>,
) -> Result<(), EngineError> {
    sqlx::query(
        "INSERT INTO timers (workspace_id, id, timer_key, instance_id, token_id, node_id, purpose, due_at,
                             calendar_version_id, source_tz, rule, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT (workspace_id, timer_key) DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(eng.ids.new_id())
    .bind(t.key)
    .bind(t.instance)
    .bind(t.token.and_then(|v| i64::try_from(v).ok()))
    .bind(t.node)
    .bind(t.purpose)
    .bind(t.due_at)
    .bind(t.calendar_version)
    .bind(t.source_tz)
    .bind(t.rule)
    .bind(eng.clock.now())
    .execute(tx.conn())
    .await?;
    Ok(())
}

/// A timer armed by an [`crate::EffectHandler`] rather than by a runtime command: work-item
/// reminders and escalations (BPMS-085). It fires through [`crate::EffectHandler::fire`], not as a
/// `timer_fired` stimulus, because the interpreter does not know it.
#[derive(Debug, Clone, Copy)]
pub struct HandlerTimer<'a> {
    /// Unique per workspace; by convention `<work key>/esc/<n>`.
    pub key: &'a str,
    pub instance: Uuid,
    pub node: &'a str,
    pub purpose: TimerPurpose,
    pub due_at: OffsetDateTime,
    /// The pinned calendar version when `due_at` is a business duration.
    pub calendar_version: Option<Uuid>,
    pub source_tz: Option<&'a str>,
    pub rule: &'a str,
}

/// Arms a handler timer once (an existing key is left as it is).
pub async fn arm_timer(
    tx: &mut WsTx<'_>,
    ids: &dyn IdGen,
    now: OffsetDateTime,
    t: HandlerTimer<'_>,
) -> Result<(), EngineError> {
    sqlx::query(
        "INSERT INTO timers (workspace_id, id, timer_key, instance_id, node_id, purpose, due_at,
                             calendar_version_id, source_tz, rule, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (workspace_id, timer_key) DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(ids.new_id())
    .bind(t.key)
    .bind(t.instance)
    .bind(t.node)
    .bind(t.purpose.as_str())
    .bind(t.due_at)
    .bind(t.calendar_version)
    .bind(t.source_tz)
    .bind(t.rule)
    .bind(now)
    .execute(tx.conn())
    .await?;
    Ok(())
}

/// Cancels every scheduled timer whose key starts with `prefix` (a work item's reminders and
/// escalations when it completes or is cancelled). Returns how many were cancelled.
pub async fn cancel_timers_under(tx: &mut WsTx<'_>, prefix: &str) -> Result<u64, EngineError> {
    Ok(sqlx::query(
        "UPDATE timers SET state = 'cancelled' WHERE starts_with(timer_key, $1) AND state = 'scheduled'",
    )
    .bind(prefix)
    .execute(tx.conn())
    .await?
    .rows_affected())
}

pub(crate) async fn cancel_timer(tx: &mut WsTx<'_>, key: &str) -> Result<(), EngineError> {
    sqlx::query("UPDATE timers SET state = 'cancelled' WHERE timer_key = $1 AND state = 'scheduled'")
        .bind(key)
        .execute(tx.conn())
        .await?;
    Ok(())
}

pub(crate) async fn apply(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    exe: &Executable,
    st: &InstanceState,
    commands: &[Command],
) -> Result<(), EngineError> {
    let iid = st.instance_id;
    let now = eng.clock.now();
    for cmd in commands {
        let payload = serde_json::to_value(cmd)?;
        let name = payload["command"].as_str().unwrap_or("unknown").to_owned();
        let owned_here = matches!(
            cmd,
            Command::ScheduleTimer { .. }
                | Command::CancelTimer { .. }
                | Command::ContinueAdvance { .. }
                | Command::EmitEvent { .. }
                | Command::CompleteInstance { .. }
        );
        if !record(tx, iid, cmd.key(), &name, payload, !owned_here, now).await? {
            continue;
        }
        match cmd {
            Command::ScheduleTimer { key, token, node, purpose, due } => {
                let purpose = purpose.as_str();
                let base = NewTimer {
                    key,
                    instance: iid,
                    token: Some(token.0),
                    node: Some(node),
                    purpose,
                    due_at: now,
                    calendar_version: None,
                    source_tz: None,
                    rule: "",
                };
                match due {
                    TimerDue::At { at } => {
                        insert_timer(eng, tx, NewTimer { due_at: *at, rule: "at", ..base }).await?;
                    }
                    TimerDue::Business { from, duration, calendar } => {
                        let cref = exe
                            .calendar_ref(calendar.as_deref())
                            .ok_or_else(|| EngineError::CalendarMissing("(none)".into()))?;
                        let cal = store::calendar_by_ref(tx, &cref).await?;
                        let due_at = cal.calendar.add(*from, BusinessDuration::parse(duration)?)?;
                        let rule = format!("business {duration} from {from}");
                        let t = NewTimer {
                            due_at,
                            calendar_version: Some(cal.id),
                            source_tz: Some(&cal.timezone),
                            rule: &rule,
                            ..base
                        };
                        insert_timer(eng, tx, t).await?;
                    }
                }
            }
            Command::CancelTimer { key } => cancel_timer(tx, key).await?,
            Command::ContinueAdvance { key } => {
                let stimulus = Stimulus { at: now, event: Event::Continue };
                jobs::enqueue_advance(eng, tx, iid, key, &stimulus, now).await?;
            }
            Command::EmitEvent { topic, payload, .. } => {
                let topic = format!("{topic}.v1");
                let ev = OutboxEvent {
                    topic: &topic,
                    aggregate_type: "process_instance",
                    aggregate_id: iid,
                    aggregate_revision: i64::try_from(st.revision).unwrap_or(i64::MAX),
                    payload: payload.clone(),
                    trace_id: None,
                };
                outbox::enqueue(tx, ev).await?;
            }
            _ => {}
        }
    }
    Ok(())
}
