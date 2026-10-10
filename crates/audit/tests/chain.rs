//! BPMS-026: chain correctness, concurrency, sealing, immutability and tamper detection.

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append, enqueue, seal_pending, verify};
use procera_db::{WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
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

fn ev(i: u32) -> AuditEvent {
    let mut e = AuditEvent::new(
        Some(Uuid::now_v7()),
        ActorKind::User,
        Category::Content,
        "artifact.updated",
        Outcome::Allowed,
    )
    .object("artifact", Uuid::now_v7());
    e.ip = Some("10.1.2.3".into());
    e.policy_ids = vec!["role:hr-designer".into()];
    e.diff = Some(json!({"title": {"before": format!("v{i}"), "after": format!("v{}", i + 1)}}));
    e
}

#[tokio::test]
async fn chain_appends_and_verifies_and_workspaces_are_independent() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let (a, b) = (WsCtx { workspace_id: Uuid::now_v7() }, WsCtx { workspace_id: Uuid::now_v7() });
    for ctx in [a, b] {
        let mut tx = t.db.begin_ws(ctx).await?;
        for i in 0..20 {
            let r = append(&mut tx, &SystemClock, &UuidV7Gen, &ev(i)).await?;
            assert_eq!(r.seq, i64::from(i) + 1);
        }
        tx.commit().await?;
    }
    assert_eq!(verify(&t.db, a).await?.first_broken, None);
    assert_eq!(verify(&t.db, b).await?.checked, 20);
    Ok(())
}

#[tokio::test]
async fn concurrent_appenders_produce_a_gap_free_chain() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let ctx = WsCtx { workspace_id: Uuid::now_v7() };
    let mut hs = vec![];
    for w in 0..8 {
        let db = t.db.clone();
        hs.push(tokio::spawn(async move {
            for i in 0..25 {
                let mut tx = db.begin_ws(ctx).await?;
                append(&mut tx, &SystemClock, &UuidV7Gen, &ev(w * 100 + i)).await?;
                tx.commit().await?;
            }
            anyhow::Ok(())
        }));
    }
    for h in hs {
        h.await??;
    }
    let r = verify(&t.db, ctx).await?;
    assert_eq!((r.checked, r.first_broken), (200, None));
    Ok(())
}

#[tokio::test]
async fn pending_events_are_sealed_into_the_same_chain() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let ctx = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ctx).await?;
    append(&mut tx, &SystemClock, &UuidV7Gen, &ev(0)).await?;
    for _ in 0..300 {
        let mut e = AuditEvent::new(
            Some(Uuid::now_v7()),
            ActorKind::User,
            Category::DataAccess,
            "field.read",
            Outcome::Allowed,
        )
        .object("request", Uuid::now_v7());
        e.reason = Some("salary".into());
        enqueue(&mut tx, &SystemClock, &e).await?;
    }
    tx.commit().await?;
    let mut sealed = 0;
    loop {
        let n = seal_pending(&t.db, ctx, &UuidV7Gen, 128).await?;
        if n == 0 {
            break;
        }
        sealed += n;
    }
    assert_eq!(sealed, 300);
    let r = verify(&t.db, ctx).await?;
    assert_eq!((r.checked, r.first_broken), (301, None));
    Ok(())
}

#[tokio::test]
async fn app_role_cannot_rewrite_history_and_tampering_is_detected() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let ctx = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ctx).await?;
    for i in 0..10 {
        append(&mut tx, &SystemClock, &UuidV7Gen, &ev(i)).await?;
    }
    tx.commit().await?;

    let mut tx = t.db.begin_ws(ctx).await?;
    let upd = sqlx::query("UPDATE audit_events SET action = 'nothing.happened' WHERE seq = 5")
        .execute(tx.conn())
        .await;
    assert!(upd.is_err(), "app role must not UPDATE audit events");
    tx.rollback().await?;
    let mut tx = t.db.begin_ws(ctx).await?;
    let del = sqlx::query("DELETE FROM audit_events WHERE seq = 5").execute(tx.conn()).await;
    assert!(del.is_err(), "app role must not DELETE audit events");
    tx.rollback().await?;

    // A superuser bypassing every control (trigger disabled) still cannot hide the change from the verifier.
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let part: String = sqlx::query_scalar(
        "SELECT tableoid::regclass::text FROM audit_events WHERE workspace_id = $1 AND seq = 5",
    )
    .bind(ctx.workspace_id)
    .fetch_one(&mut admin)
    .await?;
    sqlx::query(&format!("ALTER TABLE {part} DISABLE TRIGGER USER")).execute(&mut admin).await?;
    sqlx::query("UPDATE audit_events SET result = 'denied' WHERE workspace_id = $1 AND seq = 5")
        .bind(ctx.workspace_id)
        .execute(&mut admin)
        .await?;
    let r = verify(&t.db, ctx).await?;
    assert_eq!(r.first_broken, Some(5));
    Ok(())
}
