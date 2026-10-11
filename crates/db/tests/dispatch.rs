//! BPMS-029: the outbox dispatcher — per-aggregate order, at-least-once with backoff, hold-back
//! behind a failed event, isolation across workspaces, and NOTIFY wake-ups.

use std::sync::Mutex;
use std::time::Duration;

use procera_db::dispatch::{Bus, Dispatcher, Message, Pass};
use procera_db::outbox::{self, OutboxEvent};
use procera_db::{Db, WsCtx, test_support};
use serde_json::json;
use uuid::Uuid;

/// Records deliveries; fails any message whose payload has `"fail": true` while `failing` is set.
#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<(Uuid, Uuid, i64)>>,
    failing: Mutex<bool>,
}

impl Bus for Recorder {
    async fn deliver(&self, m: &Message) -> Result<(), String> {
        if m.payload["fail"] == json!(true) && *self.failing.lock().map_err(|e| e.to_string())? {
            return Err("downstream unavailable".into());
        }
        self.seen.lock().map_err(|e| e.to_string())?.push((
            m.workspace_id,
            m.aggregate_id,
            m.aggregate_revision,
        ));
        Ok(())
    }
}

async fn emit(db: &Db, ws: Uuid, agg: Uuid, rev: i64, fail: bool) -> anyhow::Result<()> {
    let mut tx = db.begin_ws(WsCtx { workspace_id: ws }).await?;
    outbox::enqueue(
        &mut tx,
        OutboxEvent {
            topic: "thing.changed.v1",
            aggregate_type: "thing",
            aggregate_id: agg,
            aggregate_revision: rev,
            payload: json!({"fail": fail}),
            trace_id: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn order_backoff_holdback_and_workspaces() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let (wa, wb) = (Uuid::now_v7(), Uuid::now_v7());
    let (x, y, z) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    for rev in 1..=3 {
        emit(&t.db, wa, x, rev, false).await?;
    }
    emit(&t.db, wa, y, 1, true).await?; // will fail
    emit(&t.db, wa, y, 2, false).await?; // must wait behind y#1
    emit(&t.db, wb, z, 1, false).await?;

    let d = Dispatcher::new(t.db.clone(), Recorder::default());
    *d.bus().failing.lock().map_err(|e| anyhow::anyhow!("{e}"))? = true;
    let pass = d.drain().await?;
    assert_eq!(pass, Pass { delivered: 4, failed: 1 });
    let seen = d.bus().seen.lock().map_err(|e| anyhow::anyhow!("{e}"))?.clone();
    let xs: Vec<i64> = seen.iter().filter(|s| s.1 == x).map(|s| s.2).collect();
    assert_eq!(xs, vec![1, 2, 3], "per-aggregate order");
    assert!(seen.iter().any(|s| s.0 == wb && s.1 == z), "other workspaces are not blocked");
    assert!(!seen.iter().any(|s| s.1 == y), "y#2 is held behind the failed y#1");

    // The failed event backs off: nothing is due now, and the error is recorded.
    assert_eq!(d.pending().await?, 0);
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: wa }).await?;
    let (attempts, err): (i32, Option<String>) = sqlx::query_as(
        "SELECT attempts, last_error FROM outbox WHERE aggregate_id = $1 AND aggregate_revision = 1",
    )
    .bind(y)
    .fetch_one(tx.conn())
    .await?;
    assert_eq!((attempts, err.as_deref()), (1, Some("downstream unavailable")));
    // Make it due again (as if the backoff elapsed) and let the downstream recover.
    sqlx::query("UPDATE outbox SET next_attempt_at = now() - interval '1 second' WHERE aggregate_id = $1")
        .bind(y)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    *d.bus().failing.lock().map_err(|e| anyhow::anyhow!("{e}"))? = false;
    assert_eq!(d.drain().await?, Pass { delivered: 2, failed: 0 });
    let ys: Vec<i64> = d
        .bus()
        .seen
        .lock()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .iter()
        .filter(|s| s.1 == y)
        .map(|s| s.2)
        .collect();
    assert_eq!(ys, vec![1, 2]);
    assert_eq!(d.drain().await?, Pass::default(), "everything published exactly once per pass");
    Ok(())
}

#[tokio::test]
async fn notify_wakes_the_running_dispatcher() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let d = std::sync::Arc::new(Dispatcher::new(t.db.clone(), Recorder::default()));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let runner = {
        let d = d.clone();
        tokio::spawn(async move {
            d.run(async {
                let _ = stop_rx.await;
            })
            .await
        })
    };
    let ws = Uuid::now_v7();
    emit(&t.db, ws, Uuid::now_v7(), 1, false).await?;
    let delivered = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !d.bus().seen.lock().map(|s| s.is_empty()).unwrap_or(true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let _ = stop_tx.send(());
    runner.await?;
    assert!(delivered.is_ok(), "the running dispatcher delivered the event");
    Ok(())
}
