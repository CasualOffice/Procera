//! BPMS-025: forced RLS is present on every workspace table and actually isolates (SEC-01 at the DB layer).

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
async fn rls_guard_is_clean_after_migrations() -> anyhow::Result<()> {
    let t = db_or_skip!();
    t.db.assert_rls_guard().await?;
    Ok(())
}

#[tokio::test]
async fn guard_catches_an_unprotected_workspace_table() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    sqlx::query("SET ROLE procera_migrator").execute(&mut admin).await?;
    sqlx::query("CREATE TABLE sneaky (workspace_id uuid NOT NULL, secret text)").execute(&mut admin).await?;
    let err = t.db.assert_rls_guard().await.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err.contains("sneaky"), "guard must name the unprotected table: {err}");
    Ok(())
}

#[tokio::test]
async fn workspaces_cannot_see_each_other_and_missing_context_sees_nothing() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let (a, b) = (WsCtx { workspace_id: Uuid::now_v7() }, WsCtx { workspace_id: Uuid::now_v7() });
    for ctx in [a, b] {
        let mut tx = t.db.begin_ws(ctx).await?;
        for i in 0..3 {
            outbox::enqueue(
                &mut tx,
                outbox::OutboxEvent {
                    topic: "t",
                    aggregate_type: "x",
                    aggregate_id: Uuid::now_v7(),
                    aggregate_revision: i,
                    payload: json!({}),
                    trace_id: None,
                },
            )
            .await?;
        }
        tx.commit().await?;
    }
    let mut tx = t.db.begin_ws(a).await?;
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT workspace_id FROM outbox").fetch_all(tx.conn()).await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|(w,)| *w == a.workspace_id));
    // Writing into another workspace is rejected by WITH CHECK.
    let cross = sqlx::query("INSERT INTO outbox (workspace_id, id, topic, aggregate_type, aggregate_id, aggregate_revision, payload) VALUES ($1, $2, 't', 'x', $2, 1, '{}')")
        .bind(b.workspace_id).bind(Uuid::now_v7()).execute(tx.conn()).await;
    assert!(cross.is_err(), "cross-workspace insert must fail");
    tx.rollback().await?;
    // A pooled connection that previously had context, now without it, sees zero rows (NULLIF fix).
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox").fetch_one(t.db.pool()).await?;
    assert_eq!(n, 0);
    // Discovery function reveals only ids.
    let mut ws = t.db.outbox_pending_workspaces(10).await?;
    ws.sort();
    let mut expected = vec![a.workspace_id, b.workspace_id];
    expected.sort();
    assert_eq!(ws, expected);
    // The discovery role can read ids but not payloads (column-level grant).
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    sqlx::query("SET ROLE procera_discovery").execute(&mut admin).await?;
    let payload = sqlx::query("SELECT payload FROM outbox LIMIT 1").execute(&mut admin).await;
    assert!(payload.is_err(), "discovery role must not read payloads");
    Ok(())
}

#[tokio::test]
async fn audit_partitions_are_protected_and_unreachable_directly() -> anyhow::Result<()> {
    let t = db_or_skip!();
    t.db.ensure_audit_partitions(3).await?;
    t.db.assert_rls_guard().await?; // new partitions carry forced RLS too
    let part: String = sqlx::query_scalar("SELECT c.relname::text FROM pg_inherits i JOIN pg_class c ON c.oid = i.inhrelid WHERE i.inhparent = 'audit_events'::regclass LIMIT 1").fetch_one(t.db.pool()).await?;
    let direct = sqlx::query(&format!("SELECT count(*) FROM {part}")).execute(t.db.pool()).await;
    assert!(direct.is_err(), "app role must not read partitions directly");
    Ok(())
}
