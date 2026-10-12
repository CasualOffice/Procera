//! Proof tests for the 0.2 evidence gates (docs/23 E2E-R2-SYS-01, -02), against real PostgreSQL:
//!
//! - **Restart durability** (V-06): instances, pending timers and queued or leased jobs live only in
//!   the database. Dropping the engine and scheduler and building fresh ones loses nothing, and
//!   each timer fires exactly once.
//! - **Atomic advance** (V-06): a failing effect handler rolls the whole advance back (state, log,
//!   outbox, audit); the retried advance succeeds once and a third try is a rejected duplicate.
//! - **Lost lease** (PR-07): a worker that claims a job and dies is replaced after the lease
//!   expires; the job completes once and the dead worker's late completion is fenced off.
//! - **Version pinning** (V-04): an instance keeps advancing on the build it started on after a
//!   newer build of the same key is registered; new starts use the new build; records and audit
//!   name the checksum each used.
//!
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{World, program, timer_process, world};
use procera_db::WsTx;
use procera_engine::{
    BoxFuture, EffectContext, EffectHandler, Engine, EngineError, ProgramRef, Scheduler, StaticPrograms,
};
use procera_kernel::{Clock, UuidV7Gen};
use procera_runtime::{Event, InstanceStatus};
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

/// A fresh engine over the same database, as after a process restart: new program registry,
/// no shared memory with the old one.
fn restarted(w: &World, docs: &[Value]) -> Engine {
    let mut programs = StaticPrograms::default();
    for d in docs {
        programs.insert(program(d.clone()));
    }
    Engine::new(w.db().clone(), w.clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs))
}

async fn count(w: &World, sql: &str, id: Uuid) -> i64 {
    w.scalar_i64(sql, id).await
}

#[tokio::test]
async fn sys01_restart_keeps_instances_timers_and_jobs_and_each_timer_fires_once() {
    let docs = vec![timer_process("p.short", "PT1H", false), timer_process("p.long", "P2D", false)];
    let Some(mut w) = world(docs.clone()).await else { return };
    let short = w.engine.start(w.ctx, w.program("p.short"), json!({}), BTreeMap::new()).await.unwrap();
    let long = w.engine.start(w.ctx, w.program("p.long"), json!({}), BTreeMap::new()).await.unwrap();
    // The short timer fires before the "crash": its advance job is queued but never run.
    w.clock.advance(Duration::hours(1));
    let before = w.scheduler("before-crash");
    assert_eq!(before.fire_timers(w.ctx, w.clock.now()).await.unwrap().timers_fired, 1);
    let state_before = w.engine.instance(w.ctx, short.instance_id).await.unwrap();

    // Crash: the engine, its scheduler and the program registry are gone.
    drop(before);
    w.engine = restarted(&w, &docs);
    let after = w.scheduler("after-restart");

    // Everything is still in the database.
    let rec = w.engine.instance(w.ctx, short.instance_id).await.unwrap();
    assert_eq!(rec, state_before, "state survives the restart unchanged");
    assert_eq!(
        w.strings("SELECT state FROM jobs WHERE instance_id = $1", short.instance_id).await,
        ["queued"],
        "the queued advance job survives"
    );
    assert_eq!(
        w.strings("SELECT state FROM timers WHERE instance_id = $1", long.instance_id).await,
        ["scheduled"],
        "the pending timer survives"
    );

    // The new scheduler runs the queued job once.
    let p = after.drain().await.unwrap();
    assert_eq!((p.timers_fired, p.jobs_done), (0, 1));
    assert_eq!(w.engine.instance(w.ctx, short.instance_id).await.unwrap().status, "completed");

    // The pending timer fires exactly once after the restart, even with two schedulers racing.
    w.clock.advance(Duration::days(2));
    let other = Scheduler::new(w.engine.clone(), "second-node");
    let (a, b) =
        tokio::join!(after.fire_timers(w.ctx, w.clock.now()), other.fire_timers(w.ctx, w.clock.now()));
    assert_eq!(a.unwrap().timers_fired + b.unwrap().timers_fired, 1);
    after.drain().await.unwrap();
    assert!(!after.drain().await.unwrap().progressed(), "nothing left to fire");
    for id in [short.instance_id, long.instance_id] {
        let rec = w.engine.instance(w.ctx, id).await.unwrap();
        assert_eq!((rec.status.as_str(), rec.revision), ("completed", 2));
        assert_eq!(
            count(
                &w,
                "SELECT count(*) FROM instance_events WHERE instance_id = $1 AND event_type = 'timer_fired'",
                id
            )
            .await,
            1,
            "one timer_fired stimulus"
        );
        assert_eq!(w.engine.replay(w.ctx, id).await.unwrap(), rec.state);
    }
}

/// Fails while `fail` is set; counts the advances it saw succeed.
#[derive(Debug, Default)]
struct Flaky {
    fail: AtomicBool,
    applied: std::sync::atomic::AtomicUsize,
}

impl EffectHandler for Flaky {
    fn apply<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        cx: EffectContext<'a>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            // A write of its own that must vanish with the advance.
            sqlx::query("UPDATE process_instances SET display_id = display_id || '!' WHERE id = $1")
                .bind(cx.state.instance_id)
                .execute(tx.conn())
                .await
                .map_err(|e| e.to_string())?;
            if self.fail.load(Ordering::SeqCst) {
                return Err("delivery failed".into());
            }
            self.applied.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

/// Row counts that an advance touches, for "nothing changed" checks.
async fn footprint(w: &World, id: Uuid) -> [i64; 5] {
    [
        count(w, "SELECT revision FROM process_instances WHERE id = $1", id).await,
        count(w, "SELECT count(*) FROM instance_events WHERE instance_id = $1", id).await,
        count(w, "SELECT count(*) FROM outbox WHERE aggregate_id = $1", id).await,
        count(w, "SELECT count(*) FROM audit_events WHERE object_id = $1", id).await,
        count(w, "SELECT count(*) FROM timers WHERE instance_id = $1 AND state = 'scheduled'", id).await,
    ]
}

#[tokio::test]
async fn sys01_a_failing_effect_rolls_the_advance_back_and_the_retry_applies_once() {
    let Some(mut w) = world(vec![timer_process("p.fx", "PT1H", false)]).await else { return };
    let flaky = Arc::new(Flaky::default());
    w.engine = w.engine.clone().with_effects(flaky.clone());

    // A start whose effects fail leaves no instance at all.
    flaky.fail.store(true, Ordering::SeqCst);
    let err = w.engine.start(w.ctx, w.program("p.fx"), json!({}), BTreeMap::new()).await.unwrap_err();
    assert!(matches!(err, EngineError::Effects(_)), "{err}");
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM process_instances").fetch_one(tx.conn()).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(n, 0);

    flaky.fail.store(false, Ordering::SeqCst);
    let id = w.engine.start(w.ctx, w.program("p.fx"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let at_start = footprint(&w, id).await;
    assert_eq!(at_start, [1, 1, 1, 1, 1]);

    // The handler fails after the engine wrote the state, the log, the outbox event and the
    // timer cancel: everything rolls back, including the handler's own write.
    flaky.fail.store(true, Ordering::SeqCst);
    let cancel = Event::Cancel { reason: "stop".into() };
    let err = w.engine.advance(w.ctx, id, cancel.clone(), Some(1)).await.unwrap_err();
    assert!(matches!(err, EngineError::Effects(_)), "{err}");
    assert_eq!(footprint(&w, id).await, at_start, "nothing of the failed advance is left");
    assert_eq!(
        w.strings("SELECT display_id FROM process_instances WHERE id = $1", id).await,
        [id.to_string() + "!"]
    );

    // The retry succeeds once; a duplicate is rejected and changes nothing.
    flaky.fail.store(false, Ordering::SeqCst);
    let a = w.engine.advance(w.ctx, id, cancel.clone(), Some(1)).await.unwrap();
    assert_eq!((a.revision, a.status), (2, InstanceStatus::Terminated));
    let after = footprint(&w, id).await;
    assert_eq!(after, [2, 2, 2, 2, 0]);
    assert!(w.engine.advance(w.ctx, id, cancel, None).await.unwrap_err().is_rejection());
    assert_eq!(footprint(&w, id).await, after);
    assert_eq!(flaky.applied.load(Ordering::SeqCst), 2, "the start and the one successful advance");
}

#[tokio::test]
async fn sys01_a_job_whose_worker_died_is_reclaimed_after_the_lease_and_completes_once() {
    let docs = vec![timer_process("p.lease", "PT1M", false)];
    let Some(mut w) = world(docs.clone()).await else { return };
    let id =
        w.engine.start(w.ctx, w.program("p.lease"), json!({}), BTreeMap::new()).await.unwrap().instance_id;
    w.clock.advance(Duration::minutes(1));
    let dying = w.scheduler("dying");
    dying.fire_timers(w.ctx, w.clock.now()).await.unwrap();
    let held = dying.claim(w.ctx, w.clock.now()).await.unwrap();
    assert_eq!(held.len(), 1);

    // The worker dies holding the lease; a restarted node takes over.
    w.engine = restarted(&w, &docs);
    let fresh = w.scheduler("fresh");
    assert!(!fresh.run_once().await.unwrap().progressed(), "the lease still holds");
    w.clock.advance(Duration::seconds(31));
    let p = fresh.run_once().await.unwrap();
    assert_eq!(p.jobs_done, 1, "re-claimed after expiry and run");
    let (fence, attempts, state): (i64, i32, String) = {
        let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
        let r = sqlx::query_as("SELECT fence, attempts, state FROM jobs WHERE instance_id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        r
    };
    assert_eq!((fence, attempts, state.as_str()), (held[0].fence + 1, 2, "done"));

    // The dead worker comes back and tries to finish: fenced off, nothing applied twice.
    let late = dying.execute(w.ctx, &held[0]).await.unwrap();
    assert_eq!((late.stale, late.jobs_done), (1, 0));
    let rec = w.engine.instance(w.ctx, id).await.unwrap();
    assert_eq!((rec.status.as_str(), rec.revision), ("completed", 2));
    assert_eq!(count(&w, "SELECT count(*) FROM instance_events WHERE instance_id = $1", id).await, 2);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM outbox WHERE aggregate_id = $1 AND topic = 'instance.completed.v1'",
            id
        )
        .await,
        1
    );
}

/// start → review (human task) → mark (`path := tag`) → done. Builds with different tags have
/// different checksums under the same key and version.
fn tagged(version: u32, tag: &str) -> Value {
    json!({
        "schemaVersion": 1,
        "process": { "key": "p.pinned", "version": version, "name": { "en": "Pinned" } },
        "variables": [ { "name": "path", "type": "string" } ],
        "nodes": [
            { "id": "start", "kind": "start" },
            { "id": "review", "kind": "human.task",
              "assignment": { "candidates": [ { "type": "role", "ref": "process-owner" } ] },
              "outcomes": ["done"] },
            { "id": "mark", "kind": "set", "assign": { "path": format!("expr:cel: \"{tag}\"") } },
            { "id": "end", "kind": "end" }
        ],
        "edges": [
            { "id": "e1", "from": "start", "to": "review" },
            { "id": "e2", "from": "review", "to": "mark", "on": "done" },
            { "id": "e3", "from": "mark", "to": "end" }
        ]
    })
}

async fn finish_review(engine: &Engine, w: &World, id: Uuid) -> procera_engine::Advanced {
    let st = engine.instance(w.ctx, id).await.unwrap().state;
    let key = st.waits().next().map(|(_, wt)| wt.key.clone()).expect("waiting on review");
    let done = Event::WorkItemCompleted { key, outcome: "done".into(), variables: BTreeMap::new() };
    engine.advance(w.ctx, id, done, None).await.unwrap()
}

async fn audited_checksums(w: &World, id: Uuid) -> Vec<String> {
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let v: Vec<Value> = sqlx::query_scalar("SELECT diff FROM audit_events WHERE object_id = $1 ORDER BY seq")
        .bind(id)
        .fetch_all(tx.conn())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    v.iter().map(|d| d["program"]["checksum"].as_str().unwrap_or("-").to_owned()).collect()
}

#[tokio::test]
async fn sys02_old_instances_finish_on_their_pinned_build_and_new_starts_use_the_new_one() {
    let Some(w) = world(vec![]).await else { return };
    // Build A is current; an instance starts on it.
    let mut programs = StaticPrograms::default();
    let ref_a = programs.insert_pinned(program(tagged(1, "a")));
    let engine_a = Engine::new(w.db().clone(), w.clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs));
    let old = engine_a.start(w.ctx, &ref_a, json!({}), BTreeMap::new()).await.unwrap().instance_id;

    // A new build B of the same key and version is registered (a redeploy that changed the
    // built-in), then version 2. The registry keeps A loadable by its checksum.
    let mut programs = StaticPrograms::default();
    programs.insert_pinned(program(tagged(1, "a")));
    let ref_b = programs.insert_pinned(program(tagged(1, "b")));
    let ref_v2 = programs.insert_pinned(program(tagged(2, "v2")));
    assert_ne!(ref_a.checksum, ref_b.checksum);
    assert_eq!((ref_b.version, ref_v2.version), (1, 2));
    assert_eq!(programs.builds("p.pinned").len(), 3);
    let engine = Engine::new(w.db().clone(), w.clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs));
    w.clock.advance(Duration::minutes(5));
    let new_b = engine.start(w.ctx, &ref_b, json!({}), BTreeMap::new()).await.unwrap().instance_id;
    let new_v2 = engine.start(w.ctx, &ref_v2, json!({}), BTreeMap::new()).await.unwrap().instance_id;

    // The old instance keeps advancing on A, to completion; the new ones run their own builds.
    for (id, tag) in [(old, "a"), (new_b, "b"), (new_v2, "v2")] {
        let a = finish_review(&engine, &w, id).await;
        assert_eq!(a.status, InstanceStatus::Completed);
        let rec = engine.instance(w.ctx, id).await.unwrap();
        assert_eq!(rec.state.variables["path"], json!(tag), "{id} ran build {tag}");
        assert_eq!(engine.replay(w.ctx, id).await.unwrap(), rec.state, "replays on its own build");
    }

    // Records and audit name the build each instance used, on every advance.
    let pinned = |r: &ProgramRef| r.checksum.clone().unwrap();
    for (id, r) in [(old, &ref_a), (new_b, &ref_b), (new_v2, &ref_v2)] {
        let rec = engine.instance(w.ctx, id).await.unwrap();
        assert_eq!(rec.program, r.clone());
        assert_eq!(audited_checksums(&w, id).await, [pinned(r), pinned(r)]);
    }

    // A build nobody registered is refused rather than silently swapped.
    let stray = ProgramRef { checksum: Some("sha256:never-registered".into()), ..ref_a.clone() };
    let err = engine.start(w.ctx, &stray, json!({}), BTreeMap::new()).await.unwrap_err();
    assert!(matches!(err, EngineError::Program(_)), "{err}");
}
