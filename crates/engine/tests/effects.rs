//! BPMS-083/085: the effect port runs inside the advance transaction (a failing handler rolls the
//! advance back), user-driven stimuli are audited as the user (A-28 (4)), and a pinned checksum
//! refuses another build of the same program. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{sla_process, timer_process, world};
use procera_db::WsTx;
use procera_engine::{
    Actor, BoxFuture, EffectContext, EffectHandler, Engine, EngineError, FiredTimer, HandlerTimer,
    StaticPrograms, arm_timer, cancel_timers_under,
};
use procera_kernel::UuidV7Gen;
use procera_runtime::{Event, TimerPurpose};
use serde_json::json;
use uuid::Uuid;

/// Records the commands it sees; fails on demand.
#[derive(Debug, Default)]
struct Seen {
    commands: std::sync::Mutex<Vec<String>>,
    fail: AtomicBool,
}

impl EffectHandler for Seen {
    fn apply<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        cx: EffectContext<'a>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            // Inside the advance transaction: the instance row is already visible.
            let n: i64 = sqlx::query_scalar("SELECT count(*) FROM process_instances WHERE id = $1")
                .bind(cx.state.instance_id)
                .fetch_one(tx.conn())
                .await
                .map_err(|e| e.to_string())?;
            assert_eq!(n, 1);
            if self.fail.load(Ordering::SeqCst) {
                return Err("handler refused".into());
            }
            let mut seen = self.commands.lock().unwrap();
            for c in cx.commands {
                let v = serde_json::to_value(c).unwrap();
                seen.push(v["command"].as_str().unwrap().to_owned());
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn effect_handler_runs_in_the_advance_transaction_and_users_are_audited() {
    let Some(mut w) = world(vec![sla_process("p.fx", "P1D", false)]).await else { return };
    let seen = Arc::new(Seen::default());
    w.engine = w.engine.clone().with_effects(seen.clone());
    let actor = Actor { subject_ref: Uuid::now_v7(), ..Actor::default() };
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let a = w
        .engine
        .start_in(&mut tx, w.program("p.fx"), json!({ "id": "u1" }), BTreeMap::new(), Some(actor.clone()))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(seen.commands.lock().unwrap().contains(&"schedule_timer".to_owned()));
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let who: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT actor_kind, actor_subject_ref FROM audit_events WHERE object_id = $1")
            .bind(a.instance_id)
            .fetch_all(tx.conn())
            .await
            .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(who, [("user".to_owned(), Some(actor.subject_ref))]);

    // A failing handler rolls the whole advance back.
    seen.fail.store(true, Ordering::SeqCst);
    let cancel = Event::Cancel { reason: "stop".into() };
    let err = w.engine.advance(w.ctx, a.instance_id, cancel, None).await.unwrap_err();
    assert!(matches!(err, EngineError::Effects(_)), "{err}");
    assert_eq!(w.engine.instance(w.ctx, a.instance_id).await.unwrap().revision, 1);
}

#[tokio::test]
async fn a_pinned_checksum_refuses_another_build() {
    let Some(w) = world(vec![]).await else { return };
    let mut programs = StaticPrograms::default();
    let mut r = programs.insert_pinned(common::program(timer_process("p.pin", "PT1H", false)));
    assert!(r.checksum.is_some());
    let engine = Engine::new(w.db().clone(), w.clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs));
    assert!(engine.start(w.ctx, &r, json!({}), BTreeMap::new()).await.is_ok());
    r.checksum = Some("sha256:other".into());
    let err = engine.start(w.ctx, &r, json!({}), BTreeMap::new()).await.unwrap_err();
    assert!(matches!(err, EngineError::Program(_)), "{err}");
}

/// Arms three handler timers at start (`<instance>/esc/0..2`); records what fires; fails on `/esc/1`.
#[derive(Debug, Default)]
struct Arms {
    fired: std::sync::Mutex<Vec<String>>,
}

impl EffectHandler for Arms {
    fn apply<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        cx: EffectContext<'a>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if cx.before.is_some() {
                return Ok(());
            }
            let iid = cx.state.instance_id;
            let purposes = [TimerPurpose::Reminder, TimerPurpose::Escalation, TimerPurpose::Escalation];
            for (n, purpose) in (1_i64..).zip(purposes) {
                let key = format!("{iid}/esc/{}", n - 1);
                let t = HandlerTimer {
                    key: &key,
                    instance: iid,
                    node: "hold",
                    purpose,
                    due_at: cx.stimulus.at + time::Duration::minutes(10 * n),
                    calendar_version: None,
                    source_tz: None,
                    rule: "test",
                };
                arm_timer(tx, &UuidV7Gen, cx.stimulus.at, t).await.map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    fn fire<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        t: FiredTimer<'a>,
    ) -> BoxFuture<'a, Result<Option<procera_runtime::Stimulus>, String>> {
        Box::pin(async move {
            // A write that must not survive a failure.
            sqlx::query("UPDATE timers SET rule = 'touched' WHERE timer_key = $1")
                .bind(t.key)
                .execute(tx.conn())
                .await
                .map_err(|e| e.to_string())?;
            if t.key.ends_with("/esc/1") {
                return Err("refused".into());
            }
            let n = t.key.rsplit('/').next().unwrap_or_default();
            self.fired.lock().unwrap().push(format!("{}:{n}", t.purpose));
            Ok(None)
        })
    }
}

#[tokio::test]
async fn handler_timers_fire_through_the_handler_once_and_can_be_cancelled() {
    let Some(mut w) = world(vec![sla_process("p.arm", "P1D", false)]).await else { return };
    let arms = Arc::new(Arms::default());
    w.engine = w.engine.clone().with_effects(arms.clone());
    let a = w.engine.start(w.ctx, w.program("p.arm"), json!({ "id": "u1" }), BTreeMap::new()).await.unwrap();
    let sched = w.scheduler("t");
    w.clock.advance(time::Duration::minutes(25));
    sched.run_once().await.unwrap();
    sched.run_once().await.unwrap();
    // The reminder fired once; the failing escalation was skipped and its write rolled back.
    assert_eq!(*arms.fired.lock().unwrap(), ["reminder:0"]);
    let rules = w
        .strings(
            "SELECT rule || ':' || state FROM timers WHERE instance_id = $1 AND purpose IN ('reminder', 'escalation') ORDER BY due_at",
            a.instance_id,
        )
        .await;
    assert_eq!(rules, ["touched:fired", "test:fired", "test:scheduled"]);
    // Cancelling under the prefix stops the rest.
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    assert_eq!(cancel_timers_under(&mut tx, &format!("{}/esc/", a.instance_id)).await.unwrap(), 1);
    tx.commit().await.unwrap();
    w.clock.advance(time::Duration::hours(1));
    sched.run_once().await.unwrap();
    assert_eq!(arms.fired.lock().unwrap().len(), 1);
}
