//! The `jobs` table: leased with fencing, retried with backoff, dead-lettered (docs/04 §2.5,
//! §4.3). The SQL here implements the rules of [`crate::lease`].

use procera_db::WsTx;
use procera_runtime::Stimulus;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::EngineError;
use crate::engine::Engine;
use crate::lease::{JobState, LEASE, Stale, backoff};

/// Job kinds. More arrive with their items (connector calls, notifications, exports).
pub const ADVANCE: &str = "advance";

/// Enqueues an `advance` job carrying a stamped stimulus. The idempotency key makes a duplicate
/// enqueue (same timer, same continue key) a no-op. Returns whether a row was inserted.
pub async fn enqueue_advance(
    eng: &Engine,
    tx: &mut WsTx<'_>,
    instance: Uuid,
    idempotency_key: &str,
    stimulus: &Stimulus,
    run_at: OffsetDateTime,
) -> Result<bool, EngineError> {
    let n = sqlx::query(
        "INSERT INTO jobs (workspace_id, id, kind, instance_id, payload, run_at, idempotency_key, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (workspace_id, kind, idempotency_key) DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(eng.ids.new_id())
    .bind(ADVANCE)
    .bind(instance)
    .bind(json!({ "stimulus": stimulus }))
    .bind(run_at)
    .bind(idempotency_key)
    .bind(eng.clock.now())
    .execute(tx.conn())
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// A job held under a lease. Only the holder of `fence` may finish it.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Claimed {
    pub id: Uuid,
    pub kind: String,
    pub instance_id: Option<Uuid>,
    pub payload: Value,
    pub fence: i64,
    pub attempts: i32,
}

/// Dead-letters jobs whose last attempt lost its lease, then claims up to `limit` claimable jobs
/// of the transaction's workspace (`FOR UPDATE SKIP LOCKED`, so concurrent claimers never get the
/// same job). The caller commits the claim before running the jobs.
pub async fn claim(
    tx: &mut WsTx<'_>,
    now: OffsetDateTime,
    owner: &str,
    limit: i64,
) -> Result<Vec<Claimed>, EngineError> {
    sqlx::query(
        "UPDATE jobs SET state = 'dead', lease_until = NULL, finished_at = $1,
                last_error = jsonb_build_object('error', 'lease expired on the last attempt')
         WHERE state = 'leased' AND lease_until < $1 AND attempts >= max_attempts",
    )
    .bind(now)
    .execute(tx.conn())
    .await?;
    Ok(sqlx::query_as(
        "WITH c AS (
           SELECT workspace_id, id FROM jobs
           WHERE (state = 'queued' AND run_at <= $1) OR (state = 'leased' AND lease_until < $1 AND attempts < max_attempts)
           ORDER BY run_at, id LIMIT $2 FOR UPDATE SKIP LOCKED)
         UPDATE jobs j SET state = 'leased', lease_owner = $3, lease_until = $4, fence = j.fence + 1, attempts = j.attempts + 1
         FROM c WHERE j.workspace_id = c.workspace_id AND j.id = c.id
         RETURNING j.id, j.kind, j.instance_id, j.payload, j.fence, j.attempts",
    )
    .bind(now)
    .bind(limit)
    .bind(owner)
    .bind(now + LEASE)
    .fetch_all(tx.conn())
    .await?)
}

/// Marks the job done in the caller's transaction, which also holds the job's effect. A stale
/// fence returns [`Stale`]; the caller must then roll back so the effect is discarded.
pub async fn complete(
    tx: &mut WsTx<'_>,
    job: &Claimed,
    now: OffsetDateTime,
    note: Option<Value>,
) -> Result<Result<(), Stale>, EngineError> {
    let n = sqlx::query(
        "UPDATE jobs SET state = 'done', finished_at = $3, lease_until = NULL, last_error = $4
         WHERE id = $1 AND fence = $2 AND state = 'leased'",
    )
    .bind(job.id)
    .bind(job.fence)
    .bind(now)
    .bind(note)
    .execute(tx.conn())
    .await?
    .rows_affected();
    Ok(if n == 1 { Ok(()) } else { Err(Stale { held: job.fence }) })
}

/// Records a failed attempt: queued again after [`backoff`], or dead after `max_attempts`.
pub async fn fail(
    tx: &mut WsTx<'_>,
    job: &Claimed,
    now: OffsetDateTime,
    error: &str,
) -> Result<Result<JobState, Stale>, EngineError> {
    let row: Option<(String,)> = sqlx::query_as(
        "UPDATE jobs SET state = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'queued' END,
                run_at = $3, lease_until = NULL, lease_owner = NULL,
                last_error = jsonb_build_object('error', left($4, 500), 'attempt', attempts),
                finished_at = CASE WHEN attempts >= max_attempts THEN $5 END
         WHERE id = $1 AND fence = $2 AND state = 'leased' RETURNING state",
    )
    .bind(job.id)
    .bind(job.fence)
    .bind(now + backoff(job.attempts))
    .bind(error)
    .bind(now)
    .fetch_optional(tx.conn())
    .await?;
    Ok(match row {
        Some((s,)) if s == "dead" => Ok(JobState::Dead),
        Some(_) => Ok(JobState::Queued),
        None => Err(Stale { held: job.fence }),
    })
}
