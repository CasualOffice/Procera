//! Outbox dispatcher (BPMS-029, docs/04 §2.11, ADR-0005): delivers committed outbox events to a
//! [`Bus`] at least once, in order per aggregate.
//!
//! - Discovery lists workspaces with due events (ids only); each workspace is processed under its
//!   own RLS context and a transaction-scoped advisory lock, so one dispatcher drains a workspace at
//!   a time and per-aggregate order holds across replicas.
//! - A batch is partitioned by aggregate; partitions are delivered sequentially. A failed delivery
//!   stops its partition, backs the event off exponentially (2^attempts s, at most 5 min) and holds
//!   later events of that aggregate behind it.
//! - The `outbox_notify` trigger wakes [`Dispatcher::run`]; it also polls every second because a
//!   NOTIFY can be lost.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;

use crate::{Db, DbError, WsCtx};

/// A delivered event (docs/08 §6 envelope).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Message {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub topic: String,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub aggregate_revision: i64,
    pub payload: Value,
    pub trace_id: Option<String>,
    pub attempts: i32,
}

/// Where events go. Delivery must be idempotent on `Message::id` (consumers use the inbox).
pub trait Bus: Send + Sync {
    fn deliver(&self, msg: &Message) -> impl Future<Output = Result<(), String>> + Send;
}

/// A bus without consumers: events are acknowledged and dropped (until consumers exist, e.g. BPMS-048).
#[derive(Debug, Clone, Default)]
pub struct NullBus;

impl Bus for NullBus {
    async fn deliver(&self, _msg: &Message) -> Result<(), String> {
        Ok(())
    }
}

/// Result of one pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pass {
    pub delivered: usize,
    pub failed: usize,
}

pub struct Dispatcher<B: Bus> {
    db: Db,
    bus: B,
    batch: i64,
}

impl<B: Bus> std::fmt::Debug for Dispatcher<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher").field("batch", &self.batch).finish_non_exhaustive()
    }
}

const CHANNEL: &str = "procera_outbox";

/// Due and deliverable now: unpublished, not backing off, not held behind an earlier event of the
/// same aggregate that is backing off.
const DELIVERABLE: &str =
    "o.published_at IS NULL AND (o.next_attempt_at IS NULL OR o.next_attempt_at <= now())
   AND NOT EXISTS (SELECT 1 FROM outbox e WHERE e.aggregate_id = o.aggregate_id AND e.published_at IS NULL
                   AND e.next_attempt_at > now() AND (e.created_at, e.id) < (o.created_at, o.id))";

impl<B: Bus> Dispatcher<B> {
    pub fn new(db: Db, bus: B) -> Self {
        Self { db, bus, batch: 500 }
    }

    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Delivers what is due in every workspace once.
    pub async fn run_once(&self) -> Result<Pass, DbError> {
        let mut pass = Pass::default();
        for ws in self.db.outbox_pending_workspaces(1000).await? {
            let p = self.drain_workspace(ws).await?;
            pass.delivered += p.delivered;
            pass.failed += p.failed;
        }
        Ok(pass)
    }

    /// Runs passes until nothing more is delivered (tests and `/__test/drain`).
    pub async fn drain(&self) -> Result<Pass, DbError> {
        let mut total = Pass::default();
        loop {
            let p = self.run_once().await?;
            total.delivered += p.delivered;
            total.failed += p.failed;
            if p.delivered == 0 {
                return Ok(total);
            }
        }
    }

    async fn drain_workspace(&self, ws: Uuid) -> Result<Pass, DbError> {
        let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
        let locked: bool = sqlx::query_scalar(
            "SELECT pg_try_advisory_xact_lock(hashtextextended('procera_outbox:' || $1::text, 0))",
        )
        .bind(ws)
        .fetch_one(tx.conn())
        .await?;
        if !locked {
            tx.rollback().await?;
            return Ok(Pass::default()); // another dispatcher owns this workspace right now
        }
        // Due events, oldest first, skipping aggregates held behind an earlier event in backoff.
        let msgs: Vec<Message> = sqlx::query_as(&format!(
            "SELECT o.workspace_id, o.id, o.topic, o.aggregate_type, o.aggregate_id, o.aggregate_revision, o.payload, o.trace_id, o.attempts
             FROM outbox o WHERE {DELIVERABLE} ORDER BY o.created_at, o.id LIMIT $1 FOR UPDATE SKIP LOCKED"
        ))
        .bind(self.batch)
        .fetch_all(tx.conn())
        .await?;
        let mut partitions: BTreeMap<Uuid, Vec<&Message>> = BTreeMap::new();
        let mut order = vec![];
        for m in &msgs {
            if !partitions.contains_key(&m.aggregate_id) {
                order.push(m.aggregate_id);
            }
            partitions.entry(m.aggregate_id).or_default().push(m);
        }
        let mut pass = Pass::default();
        let mut published = vec![];
        for agg in order {
            for m in partitions.get(&agg).into_iter().flatten() {
                match self.bus.deliver(m).await {
                    Ok(()) => published.push(m.id),
                    Err(e) => {
                        pass.failed += 1;
                        let backoff =
                            2_i64.saturating_pow(u32::try_from(m.attempts.clamp(0, 9)).unwrap_or(9)).min(300);
                        tracing::warn!(topic = %m.topic, id = %m.id, attempts = m.attempts + 1, error = %e, "outbox delivery failed; backing off");
                        sqlx::query("UPDATE outbox SET attempts = attempts + 1, last_error = left($2, 500), next_attempt_at = now() + make_interval(secs => $3) WHERE id = $1")
                            .bind(m.id).bind(&e).bind(backoff as f64).execute(tx.conn()).await?;
                        break; // keep order: later events of this aggregate wait
                    }
                }
            }
        }
        if !published.is_empty() {
            sqlx::query("UPDATE outbox SET published_at = now(), attempts = attempts + 1, last_error = NULL, next_attempt_at = NULL WHERE id = ANY($1)")
                .bind(&published)
                .execute(tx.conn())
                .await?;
        }
        pass.delivered = published.len();
        tx.commit().await?;
        Ok(pass)
    }

    /// Runs until `shutdown` resolves: drains, then waits for a NOTIFY or the 1 s poll.
    pub async fn run(&self, shutdown: impl Future<Output = ()>) {
        tokio::pin!(shutdown);
        let mut listener = match sqlx::postgres::PgListener::connect_with(self.db.pool()).await {
            Ok(mut l) => match l.listen(CHANNEL).await {
                Ok(()) => Some(l),
                Err(e) => {
                    tracing::warn!(error = %e, "outbox LISTEN failed; polling only");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "outbox listener unavailable; polling only");
                None
            }
        };
        loop {
            if let Err(e) = self.drain().await {
                tracing::error!(error = %e, "outbox dispatch pass failed");
            }
            let wake = async {
                match listener.as_mut() {
                    Some(l) => {
                        let _ = tokio::time::timeout(Duration::from_secs(1), l.recv()).await;
                    }
                    None => tokio::time::sleep(Duration::from_secs(1)).await,
                }
            };
            tokio::select! {
                () = &mut shutdown => return,
                () = wake => {}
            }
        }
    }

    /// Unpublished events that are due (all workspaces), for health and `/__test/drain`.
    pub async fn pending(&self) -> Result<usize, DbError> {
        let mut n = 0;
        for ws in self.db.outbox_pending_workspaces(1000).await? {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let c: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM outbox o WHERE {DELIVERABLE}"))
                .fetch_one(tx.conn())
                .await?;
            tx.rollback().await?;
            n += usize::try_from(c).unwrap_or(0);
        }
        Ok(n)
    }
}
