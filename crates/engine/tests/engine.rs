//! BPMS-083 against real PostgreSQL: persistence and replay, timers firing once, leases and
//! fencing, concurrent claims, cancellation, business calendars and SLA clocks.
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{T0, sla_process, timer_process, world};
use procera_engine::{CalendarSpec, EngineError, put_calendar};
use procera_kernel::{Clock, UuidV7Gen};
use procera_runtime::{Event, InstanceStatus};
use serde_json::json;
use time::{Duration, OffsetDateTime, UtcOffset};

#[tokio::test]
async fn start_persists_state_log_effects_outbox_and_audit_and_replays() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1H", false)]).await else { return };
    let a =
        w.engine.start(w.ctx, w.program("p.timer"), json!({ "id": "u1" }), BTreeMap::new()).await.unwrap();
    assert_eq!((a.revision, a.status), (1, InstanceStatus::Waiting));
    let id = a.instance_id;
    let rec = w.engine.instance(w.ctx, id).await.unwrap();
    assert_eq!((rec.status.as_str(), rec.revision), ("running", 1));
    assert_eq!(w.scalar_i64("SELECT count(*) FROM instance_events WHERE instance_id = $1", id).await, 1);
    assert_eq!(
        w.strings("SELECT command FROM instance_effects WHERE instance_id = $1 ORDER BY command", id).await,
        ["emit_event", "schedule_timer"]
    );
    assert_eq!(
        w.strings("SELECT topic FROM outbox WHERE aggregate_id = $1", id).await,
        ["instance.started.v1"]
    );
    assert_eq!(
        w.strings("SELECT action FROM audit_events WHERE object_id = $1", id).await,
        ["instance.started"]
    );
    let due: OffsetDateTime = {
        let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
        let d = sqlx::query_scalar("SELECT due_at FROM timers WHERE instance_id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        d
    };
    assert_eq!(due, T0 + Duration::hours(1));
    assert_eq!(w.engine.replay(w.ctx, id).await.unwrap(), rec.state);

    // The log is append-only for the app role.
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let denied =
        sqlx::query("DELETE FROM instance_events WHERE instance_id = $1").bind(id).execute(tx.conn()).await;
    assert!(denied.is_err());
    tx.rollback().await.unwrap();

    // Another workspace sees nothing.
    let other = procera_db::WsCtx { workspace_id: uuid::Uuid::now_v7() };
    assert!(matches!(w.engine.instance(other, id).await, Err(EngineError::NotFound(_))));
}

#[tokio::test]
async fn timer_fires_once_and_the_instance_completes_with_lateness_measured() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1H", false)]).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.timer"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let (s1, s2) = (w.scheduler("w1"), w.scheduler("w2"));
    assert!(!s1.run_once().await.unwrap().progressed(), "not due yet");
    w.clock.advance(Duration::hours(1) + Duration::seconds(5));
    let (a, b) = tokio::join!(s1.fire_timers(w.ctx, w.clock.now()), s2.fire_timers(w.ctx, w.clock.now()));
    assert_eq!(a.unwrap().timers_fired + b.unwrap().timers_fired, 1, "two schedulers fire a timer once");
    let p = s1.drain().await.unwrap();
    assert_eq!((p.timers_fired, p.jobs_done), (0, 1));
    let rec = w.engine.instance(w.ctx, id).await.unwrap();
    assert_eq!((rec.status.as_str(), rec.revision), ("completed", 2));
    assert_eq!(w.engine.replay(w.ctx, id).await.unwrap(), rec.state, "replay equals the stored state");
    let late = s1.lateness();
    let late2 = s2.lateness();
    assert_eq!(late.count + late2.count, 1);
    assert_eq!(late.max_ms.max(late2.max_ms), 5_000);
    assert_eq!(
        w.strings("SELECT topic FROM outbox WHERE aggregate_id = $1 ORDER BY created_at, id", id).await,
        ["instance.started.v1", "instance.completed.v1"]
    );
    assert_eq!(
        w.strings("SELECT action FROM audit_events WHERE object_id = $1 ORDER BY seq", id).await,
        ["instance.started", "instance.completed"]
    );
    // Firing again later does nothing.
    w.clock.advance(Duration::hours(5));
    assert!(!s1.drain().await.unwrap().progressed());
}

#[tokio::test]
async fn a_stale_lease_holder_cannot_complete_and_its_effect_is_discarded() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1M", false)]).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.timer"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    w.clock.advance(Duration::minutes(1));
    let (a, b) = (w.scheduler("a"), w.scheduler("b"));
    a.fire_timers(w.ctx, w.clock.now()).await.unwrap();
    let held_a = a.claim(w.ctx, w.clock.now()).await.unwrap();
    assert_eq!(held_a.len(), 1);
    assert!(b.claim(w.ctx, w.clock.now()).await.unwrap().is_empty(), "the lease is held");
    // Worker a stalls past its lease; b reclaims with a newer fence.
    w.clock.advance(Duration::seconds(31));
    let held_b = b.claim(w.ctx, w.clock.now()).await.unwrap();
    assert_eq!(held_b.len(), 1);
    assert_eq!(held_b[0].fence, held_a[0].fence + 1);
    let late = a.execute(w.ctx, &held_a[0]).await.unwrap();
    assert_eq!(late.stale, 1);
    assert_eq!(w.engine.instance(w.ctx, id).await.unwrap().revision, 1, "the stale effect was rolled back");
    let ok = b.execute(w.ctx, &held_b[0]).await.unwrap();
    assert_eq!(ok.jobs_done, 1);
    assert_eq!(w.scalar_i64("SELECT count(*) FROM instance_events WHERE instance_id = $1", id).await, 2);
    assert_eq!(w.engine.instance(w.ctx, id).await.unwrap().status, "completed");
}

#[tokio::test]
async fn concurrent_claims_never_double_run() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1M", false)]).await else { return };
    let mut ids = Vec::new();
    for _ in 0..24 {
        ids.push(
            w.engine
                .start(w.ctx, w.program("p.timer"), json!({}), BTreeMap::new())
                .await
                .unwrap()
                .instance_id,
        );
    }
    w.clock.advance(Duration::minutes(2));
    let fired = w.scheduler("f").fire_timers(w.ctx, w.clock.now()).await.unwrap();
    assert_eq!(fired.timers_fired, 24);
    let ws: Vec<_> = (0..4).map(|i| w.scheduler(&format!("w{i}"))).collect();
    let now = w.clock.now();
    let claims = futures_join(&ws, &w, now).await;
    let all: Vec<_> = claims.iter().flatten().map(|c| c.id).collect();
    let unique: BTreeSet<_> = all.iter().collect();
    assert_eq!((all.len(), unique.len()), (24, 24), "every job claimed exactly once");
    for (s, held) in ws.iter().zip(&claims) {
        for job in held {
            assert_eq!(s.execute(w.ctx, job).await.unwrap().jobs_done, 1);
        }
    }
    for id in ids {
        assert_eq!(w.scalar_i64("SELECT revision FROM process_instances WHERE id = $1", id).await, 2);
    }
}

async fn futures_join(
    ws: &[procera_engine::Scheduler],
    w: &common::World,
    now: OffsetDateTime,
) -> Vec<Vec<procera_engine::jobs::Claimed>> {
    let (a, b, c, d) = tokio::join!(
        ws[0].claim(w.ctx, now),
        ws[1].claim(w.ctx, now),
        ws[2].claim(w.ctx, now),
        ws[3].claim(w.ctx, now)
    );
    vec![a.unwrap(), b.unwrap(), c.unwrap(), d.unwrap()]
}

#[tokio::test]
async fn cancel_withdraws_the_timer_and_revisions_are_checked() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1H", false)]).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.timer"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let cancel = Event::Cancel { reason: "withdrawn".into() };
    let stale = w.engine.advance(w.ctx, id, cancel.clone(), Some(7)).await;
    assert!(matches!(stale, Err(EngineError::Conflict { expected: 7, actual: 1 })));
    let a = w.engine.advance(w.ctx, id, cancel.clone(), Some(1)).await.unwrap();
    assert_eq!(a.status, InstanceStatus::Terminated);
    assert_eq!(w.strings("SELECT state FROM timers WHERE instance_id = $1", id).await, ["cancelled"]);
    assert!(w.engine.advance(w.ctx, id, cancel, None).await.unwrap_err().is_rejection());
    w.clock.advance(Duration::hours(2));
    assert!(!w.scheduler("s").drain().await.unwrap().progressed(), "a cancelled timer never fires");
    assert_eq!(w.engine.instance(w.ctx, id).await.unwrap().status, "cancelled");
}

fn office() -> CalendarSpec {
    let day = vec![["09:00".to_owned(), "18:00".to_owned()]];
    let hours = ["mon", "tue", "wed", "thu", "fri"].iter().map(|d| ((*d).to_owned(), day.clone())).collect();
    CalendarSpec {
        timezone: "+05:30".into(),
        working_hours: hours,
        holidays: vec![time::macros::date!(2026 - 10 - 19)],
    }
}

#[tokio::test]
async fn business_timers_use_the_pinned_calendar_version() {
    let Some(w) = world(vec![timer_process("p.biz", "P1D", true)]).await else { return };
    let missing = w.engine.start(w.ctx, w.program("p.biz"), json!({}), BTreeMap::new()).await;
    assert!(matches!(missing, Err(EngineError::CalendarMissing(_))), "no calendar, no instance");
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let cal =
        put_calendar(&mut tx, w.clock.as_ref(), &UuidV7Gen, "calendar:in-blr@2026.2", "2026.2", &office())
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let id = w.engine.start(w.ctx, w.program("p.biz"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let (due, pinned, tz, rule): (OffsetDateTime, uuid::Uuid, String, String) = sqlx::query_as(
        "SELECT due_at, calendar_version_id, source_tz, rule FROM timers WHERE instance_id = $1",
    )
    .bind(id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    // Friday 15:00 IST + one working day, Monday a holiday: Tuesday 15:00 IST.
    let ist = UtcOffset::from_hms(5, 30, 0).unwrap();
    assert_eq!(due, time::macros::datetime!(2026-10-20 15:00).assume_offset(ist));
    assert_eq!((pinned, tz.as_str()), (cal, "+05:30"));
    assert!(rule.starts_with("business P1D from "));
}

#[tokio::test]
async fn sla_clocks_pause_resume_and_stop_met() {
    let Some(w) = world(vec![sla_process("p.sla", "PT2H", true)]).await else { return };
    let id = w.engine.start(w.ctx, w.program("p.sla"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    // Started and immediately paused while `hold` waits: no breach timer is armed.
    let mut topics = w
        .strings(
            "SELECT topic FROM outbox WHERE aggregate_id = $1 AND topic LIKE 'sla.%' ORDER BY created_at, id",
            id,
        )
        .await;
    assert_eq!(topics, ["sla.started.v1", "sla.paused.v1"]);
    w.clock.advance(Duration::hours(1));
    let s = w.scheduler("s");
    s.drain().await.unwrap();
    // Resumed at T0+1h: the due moves by the hour spent paused.
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let (due, state): (OffsetDateTime, String) = sqlx::query_as(
        "SELECT c.due_at, t.state FROM sla_clocks c JOIN timers t ON t.timer_key = c.clock_key || '/breach' WHERE c.instance_id = $1",
    )
    .bind(id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!((due, state.as_str()), (T0 + Duration::hours(3), "scheduled"));
    w.clock.advance(Duration::minutes(30));
    let st = w.engine.instance(w.ctx, id).await.unwrap().state;
    let key = st.waits().next().map(|(_, wt)| wt.key.clone()).unwrap();
    let done = Event::WorkItemCompleted { key, outcome: "done".into(), variables: BTreeMap::new() };
    let a = w.engine.advance(w.ctx, id, done, None).await.unwrap();
    assert_eq!(a.status, InstanceStatus::Completed);
    topics = w
        .strings(
            "SELECT topic FROM outbox WHERE aggregate_id = $1 AND topic LIKE 'sla.%' ORDER BY created_at, id",
            id,
        )
        .await;
    assert_eq!(topics, ["sla.started.v1", "sla.paused.v1", "sla.resumed.v1", "sla.stopped.v1"]);
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let met: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE topic = 'sla.stopped.v1'")
            .fetch_one(tx.conn())
            .await
            .unwrap();
    let paused: f64 = sqlx::query_scalar("SELECT extract(epoch FROM paused_total)::float8 FROM sla_clocks")
        .fetch_one(tx.conn())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(met["met"], json!(true));
    assert_eq!(paused, 3_600.0);
    assert_eq!(
        w.strings("SELECT state FROM timers WHERE purpose = 'sla' AND instance_id = $1", id).await,
        ["cancelled"]
    );
}

#[tokio::test]
async fn sla_breach_fires_once_and_records_the_breach_actions() {
    let Some(w) = world(vec![sla_process("p.breach", "PT30M", false)]).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.breach"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    w.clock.advance(Duration::minutes(45));
    let p = w.scheduler("s").drain().await.unwrap();
    assert_eq!(p.sla_breached, 1);
    assert!(!w.scheduler("s2").drain().await.unwrap().progressed());
    assert_eq!(
        w.strings("SELECT topic FROM outbox WHERE aggregate_id = $1 AND topic = 'sla.breached.v1'", id)
            .await
            .len(),
        1
    );
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let (state, payload): (String, serde_json::Value) = sqlx::query_as(
        "SELECT state, payload FROM instance_effects WHERE command = 'sla_breach' AND instance_id = $1",
    )
    .bind(id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(state, "pending");
    assert_eq!(payload["actions"][0]["to"], json!("role:process-owner"));
    assert_eq!(
        w.strings("SELECT action FROM audit_events WHERE action = 'sla.breached' AND object_id = $1", id)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn failing_jobs_back_off_then_dead_letter() {
    let Some(w) = world(vec![timer_process("p.timer", "PT1H", false)]).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.timer"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    sqlx::query(
        "INSERT INTO jobs (workspace_id, id, kind, instance_id, payload, run_at, max_attempts, idempotency_key, created_at)
         VALUES ($1, $2, 'advance', $3, '{\"stimulus\": \"garbage\"}', $4, 2, 'broken', $4)",
    )
    .bind(w.ctx.workspace_id)
    .bind(uuid::Uuid::now_v7())
    .bind(id)
    .bind(w.clock.now())
    .execute(tx.conn())
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let s = w.scheduler("s");
    assert_eq!(s.run_once().await.unwrap().jobs_retried, 1);
    assert_eq!(s.run_once().await.unwrap().jobs_retried, 0, "backing off");
    w.clock.advance(Duration::seconds(3));
    assert_eq!(s.run_once().await.unwrap().jobs_dead, 1);
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let (state, attempts, err): (String, i32, serde_json::Value) =
        sqlx::query_as("SELECT state, attempts, last_error FROM jobs WHERE idempotency_key = 'broken'")
            .fetch_one(tx.conn())
            .await
            .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!((state.as_str(), attempts), ("dead", 2));
    assert!(err["error"].as_str().unwrap().contains("bad job payload"));
    assert_eq!(w.engine.instance(w.ctx, id).await.unwrap().revision, 1, "the instance is untouched");
}

#[tokio::test]
async fn a_budget_yield_continues_through_an_advance_job() {
    let doc = json!({
        "schemaVersion": 1,
        "process": { "key": "p.long", "version": 1, "name": { "en": "long" } },
        "variables": [ { "name": "n", "type": "integer", "required": true } ],
        "nodes": [
            { "id": "start", "kind": "start" },
            { "id": "a", "kind": "set", "assign": { "n": "expr:cel: 1" } },
            { "id": "b", "kind": "set", "assign": { "n": "expr:cel: n + 1" } },
            { "id": "c", "kind": "set", "assign": { "n": "expr:cel: n + 1" } },
            { "id": "done", "kind": "end" }
        ],
        "edges": [
            { "id": "e1", "from": "start", "to": "a" },
            { "id": "e2", "from": "a", "to": "b" },
            { "id": "e3", "from": "b", "to": "c" },
            { "id": "e4", "from": "c", "to": "done" }
        ]
    });
    let Some(mut w) = world(vec![]).await else { return };
    let mut exe = common::program(doc);
    exe.budget = procera_runtime::Budget { max_steps: 2, ..Default::default() };
    let mut programs = procera_engine::StaticPrograms::default();
    let r = programs.insert(exe);
    w.engine = procera_engine::Engine::new(
        w.db().clone(),
        w.clock.clone(),
        std::sync::Arc::new(UuidV7Gen),
        std::sync::Arc::new(programs),
    );
    let a = w.engine.start(w.ctx, &r, json!({}), BTreeMap::new()).await.unwrap();
    assert_eq!(a.status, InstanceStatus::Running, "yielded after two steps");
    let p = w.scheduler("s").drain().await.unwrap();
    assert_eq!(p.jobs_done, 2);
    let rec = w.engine.instance(w.ctx, a.instance_id).await.unwrap();
    assert_eq!(rec.status, "completed");
    assert_eq!(rec.state.variables["n"], json!(3));
    assert_eq!(w.engine.replay(w.ctx, a.instance_id).await.unwrap(), rec.state);
}
