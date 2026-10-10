//! Idempotency keys for mutating commands (docs/08 §2, docs/04 §3 step 9).

use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{DbError, WsTx};

/// What the caller should do with a request carrying an `Idempotency-Key`.
#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    /// First time: execute the command, then call [`complete`] in the same transaction.
    Fresh,
    /// Same key, same request: return the stored response without re-executing.
    Replay { status: i32, response: Value },
    /// Same key, different request body: reject with 409 `idempotency_key_reused`.
    Conflict,
}

pub fn request_hash(method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(method.as_bytes());
    h.update(b"\n");
    h.update(path.as_bytes());
    h.update(b"\n");
    h.update(body);
    h.finalize().to_vec()
}

/// Claims `key` for (`actor`, `operation`) within the current workspace transaction.
/// Keys are kept for 24 h (docs/08 §2).
pub async fn claim(
    tx: &mut WsTx<'_>,
    actor: Uuid,
    operation: &str,
    key: &str,
    hash: &[u8],
) -> Result<Claim, DbError> {
    let ws = tx.ctx.workspace_id;
    let inserted = sqlx::query(
        "INSERT INTO idempotency_keys (workspace_id, actor_id, operation, key, request_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5, now() + interval '24 hours') ON CONFLICT DO NOTHING",
    )
    .bind(ws)
    .bind(actor)
    .bind(operation)
    .bind(key)
    .bind(hash)
    .execute(tx.conn())
    .await?
    .rows_affected();
    if inserted == 1 {
        return Ok(Claim::Fresh);
    }
    let (stored_hash, status, response): (Vec<u8>, Option<i32>, Option<Value>) = sqlx::query_as(
        "SELECT request_hash, status, response FROM idempotency_keys WHERE workspace_id = $1 AND actor_id = $2 AND operation = $3 AND key = $4 FOR UPDATE",
    )
    .bind(ws).bind(actor).bind(operation).bind(key)
    .fetch_one(tx.conn())
    .await?;
    if stored_hash != hash {
        return Ok(Claim::Conflict);
    }
    match (status, response) {
        (Some(status), Some(response)) => Ok(Claim::Replay { status, response }),
        // Claimed by a concurrent request that hasn't committed a response: treat as conflict-in-progress.
        _ => Ok(Claim::Conflict),
    }
}

/// Stores the response for a fresh claim (same transaction as the command's effects).
pub async fn complete(
    tx: &mut WsTx<'_>,
    actor: Uuid,
    operation: &str,
    key: &str,
    status: i32,
    response: &Value,
) -> Result<(), DbError> {
    let ws = tx.ctx.workspace_id;
    sqlx::query("UPDATE idempotency_keys SET status = $5, response = $6 WHERE workspace_id = $1 AND actor_id = $2 AND operation = $3 AND key = $4")
        .bind(ws).bind(actor).bind(operation).bind(key).bind(status).bind(response)
        .execute(tx.conn())
        .await?;
    Ok(())
}
