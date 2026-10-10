//! BPMS-029: idempotency claims and the outbox/inbox protocol.

use procera_db::idempotency::{self, Claim};
use procera_db::{WsCtx, outbox, test_support};
use serde_json::json;
use uuid::Uuid;

macro_rules! db_or_skip {
    () => {
        match test_support::fresh().await {
            Some(r) => r?,
            None => return Ok(()),
        }
    };
}

#[tokio::test]
async fn idempotency_fresh_replay_conflict() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let ctx = WsCtx { workspace_id: Uuid::now_v7() };
    let actor = Uuid::now_v7();
    let h1 = idempotency::request_hash("POST", "/api/v1/requests", br#"{"a":1}"#);
    let h2 = idempotency::request_hash("POST", "/api/v1/requests", br#"{"a":2}"#);

    let mut tx = t.db.begin_ws(ctx).await?;
    assert_eq!(
        idempotency::claim(&mut tx, actor, "requests.start", "key-00000001", &h1).await?,
        Claim::Fresh
    );
    idempotency::complete(&mut tx, actor, "requests.start", "key-00000001", 201, &json!({"id": "r1"}))
        .await?;
    tx.commit().await?;

    let mut tx = t.db.begin_ws(ctx).await?;
    assert_eq!(
        idempotency::claim(&mut tx, actor, "requests.start", "key-00000001", &h1).await?,
        Claim::Replay { status: 201, response: json!({"id": "r1"}) }
    );
    assert_eq!(
        idempotency::claim(&mut tx, actor, "requests.start", "key-00000001", &h2).await?,
        Claim::Conflict
    );
    tx.rollback().await?;

    // Keys are per workspace: the same key in another workspace is fresh.
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    assert_eq!(
        idempotency::claim(&mut tx, actor, "requests.start", "key-00000001", &h1).await?,
        Claim::Fresh
    );
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn outbox_claim_publish_and_inbox_dedup() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let ctx = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ctx).await?;
    let agg = Uuid::now_v7();
    for rev in 1..=5 {
        outbox::enqueue(
            &mut tx,
            outbox::OutboxEvent {
                topic: "instance.started.v1",
                aggregate_type: "instance",
                aggregate_id: agg,
                aggregate_revision: rev,
                payload: json!({"rev": rev}),
                trace_id: Some("t"),
            },
        )
        .await?;
    }
    tx.commit().await?;

    // Two dispatchers claim concurrently: SKIP LOCKED gives disjoint batches.
    let mut d1 = t.db.begin_ws(ctx).await?;
    let mut d2 = t.db.begin_ws(ctx).await?;
    let b1 = outbox::claim_batch(&mut d1, 3).await?;
    let b2 = outbox::claim_batch(&mut d2, 3).await?;
    assert_eq!(b1.len() + b2.len(), 5);
    assert!(b1.iter().all(|x| b2.iter().all(|y| x.id != y.id)));
    assert!(
        b1.windows(2).all(|w| w[0].aggregate_revision < w[1].aggregate_revision),
        "per-aggregate order kept"
    );
    outbox::mark_published(&mut d1, &b1.iter().map(|p| p.id).collect::<Vec<_>>()).await?;
    outbox::mark_published(&mut d2, &b2.iter().map(|p| p.id).collect::<Vec<_>>()).await?;
    d1.commit().await?;
    d2.commit().await?;
    assert!(t.db.outbox_pending_workspaces(10).await?.is_empty());

    // Consumer dedup: second delivery of the same message is a no-op.
    let msg = b1[0].id;
    let mut c = t.db.begin_ws(ctx).await?;
    assert!(outbox::mark_consumed(&mut c, "notifications", msg).await?);
    assert!(!outbox::mark_consumed(&mut c, "notifications", msg).await?);
    c.commit().await?;
    Ok(())
}
