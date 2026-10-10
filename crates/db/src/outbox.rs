//! Transactional outbox (ADR-0005): events are written in the same transaction as the state change.

use serde_json::Value;
use uuid::Uuid;

use crate::{DbError, WsTx};

/// A domain event to publish after commit (docs/08 §6 envelope fields).
#[derive(Debug, Clone)]
pub struct OutboxEvent<'a> {
    pub topic: &'a str,
    pub aggregate_type: &'a str,
    pub aggregate_id: Uuid,
    pub aggregate_revision: i64,
    pub payload: Value,
    pub trace_id: Option<&'a str>,
}

/// Inserts the event; returns its id. Never include restricted values in `payload` (docs/04 §9).
pub async fn enqueue(tx: &mut WsTx<'_>, ev: OutboxEvent<'_>) -> Result<Uuid, DbError> {
    let id = Uuid::now_v7();
    let ws = tx.ctx.workspace_id;
    sqlx::query(
        "INSERT INTO outbox (workspace_id, id, topic, aggregate_type, aggregate_id, aggregate_revision, payload, trace_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(ws).bind(id).bind(ev.topic).bind(ev.aggregate_type).bind(ev.aggregate_id).bind(ev.aggregate_revision).bind(ev.payload).bind(ev.trace_id)
    .execute(tx.conn())
    .await?;
    Ok(id)
}

/// Records that `consumer` processed `message_id`, in the consumer's transaction.
/// Returns `false` if it was already processed (duplicate delivery — skip the effect).
pub async fn mark_consumed(tx: &mut WsTx<'_>, consumer: &str, message_id: Uuid) -> Result<bool, DbError> {
    let ws = tx.ctx.workspace_id;
    let n = sqlx::query("INSERT INTO inbox_dedup (workspace_id, consumer, message_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(ws).bind(consumer).bind(message_id).execute(tx.conn()).await?.rows_affected();
    Ok(n == 1)
}

/// A claimed batch row for the dispatcher.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Pending {
    pub id: Uuid,
    pub topic: String,
    pub aggregate_id: Uuid,
    pub aggregate_revision: i64,
    pub payload: Value,
}

/// Claims up to `limit` unpublished events of the current workspace, oldest first (SKIP LOCKED).
pub async fn claim_batch(tx: &mut WsTx<'_>, limit: i64) -> Result<Vec<Pending>, DbError> {
    Ok(sqlx::query_as(
        "SELECT id, topic, aggregate_id, aggregate_revision, payload FROM outbox
         WHERE published_at IS NULL ORDER BY created_at, id LIMIT $1 FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .fetch_all(tx.conn())
    .await?)
}

pub async fn mark_published(tx: &mut WsTx<'_>, ids: &[Uuid]) -> Result<u64, DbError> {
    Ok(sqlx::query("UPDATE outbox SET published_at = now(), attempts = attempts + 1 WHERE id = ANY($1)")
        .bind(ids)
        .execute(tx.conn())
        .await?
        .rows_affected())
}
