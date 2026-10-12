//! The scheduler worker (docs/04 §4.4): fire due timers, then run due jobs.
//!
//! [`Scheduler::run_once`] is one pass over every workspace with due work (found through the
//! `discovery.runtime_due_workspaces` function, ids only). Each workspace is processed under its
//! own RLS context:
//!
//! 1. **Timers.** Due timers are claimed `FOR UPDATE SKIP LOCKED` (batches of 500) and, in one
//!    transaction, marked `fired` and turned into their effect: an `advance` job carrying a
//!    `timer_fired` stimulus (idempotency key `fired:<timer key>`), or an SLA breach. Lateness
//!    (`fired_at - due_at`) goes into the [`Lateness`] histogram.
//! 2. **Jobs.** Claimable jobs are leased with a new fence and committed. Each job then runs in
//!    its own transaction, together with its fenced completion: a stale holder's completion is
//!    refused and its effect rolled back. Failures retry with backoff, then dead-letter.
//!
//! The time is the injected `Clock`, so tests drive it with a manual clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use procera_db::WsCtx;
use procera_runtime::{Event, Stimulus};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::engine::{Engine, FiredTimer};
use crate::jobs::{self, ADVANCE, Claimed};
use crate::lease::JobState;
use crate::{EngineError, sla};

/// Upper bounds (ms) of the lateness buckets; the last bucket is unbounded.
pub const LATENESS_BOUNDS_MS: [u64; 6] = [100, 1_000, 5_000, 15_000, 60_000, 300_000];
/// The timer SLO: fired within 60 s of due (docs/04 §4.4, docs/06).
pub const LATENESS_SLO: Duration = Duration::seconds(60);

/// Timer lateness histogram, exported for the SLO metric (99.99 % within 60 s).
#[derive(Debug, Default)]
pub struct Lateness {
    buckets: [AtomicU64; 7],
    count: AtomicU64,
    within_slo: AtomicU64,
    max_ms: AtomicU64,
}

/// A point-in-time copy of [`Lateness`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LatenessSnapshot {
    /// `(upper bound in ms, count)`; `None` is the unbounded bucket. Counts are per bucket, not
    /// cumulative.
    pub buckets: Vec<(Option<u64>, u64)>,
    pub count: u64,
    pub within_slo: u64,
    pub max_ms: u64,
}

impl Lateness {
    pub fn record(&self, late: Duration) {
        let ms = u64::try_from(late.whole_milliseconds().max(0)).unwrap_or(u64::MAX);
        let i = LATENESS_BOUNDS_MS.iter().position(|b| ms <= *b).unwrap_or(LATENESS_BOUNDS_MS.len());
        self.buckets[i].fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        if late <= LATENESS_SLO {
            self.within_slo.fetch_add(1, Ordering::Relaxed);
        }
        self.max_ms.fetch_max(ms, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> LatenessSnapshot {
        let mut buckets: Vec<(Option<u64>, u64)> = LATENESS_BOUNDS_MS
            .iter()
            .enumerate()
            .map(|(i, b)| (Some(*b), self.buckets[i].load(Ordering::Relaxed)))
            .collect();
        buckets.push((None, self.buckets[LATENESS_BOUNDS_MS.len()].load(Ordering::Relaxed)));
        LatenessSnapshot {
            buckets,
            count: self.count.load(Ordering::Relaxed),
            within_slo: self.within_slo.load(Ordering::Relaxed),
            max_ms: self.max_ms.load(Ordering::Relaxed),
        }
    }
}

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pass {
    pub timers_fired: usize,
    pub sla_breached: usize,
    pub jobs_done: usize,
    /// Jobs whose stimulus the instance refused (stale or duplicate): done as no-ops.
    pub jobs_rejected: usize,
    pub jobs_retried: usize,
    pub jobs_dead: usize,
    /// Completions refused because the lease had moved to a newer claimant.
    pub stale: usize,
}

impl Pass {
    fn add(&mut self, o: Pass) {
        self.timers_fired += o.timers_fired;
        self.sla_breached += o.sla_breached;
        self.jobs_done += o.jobs_done;
        self.jobs_rejected += o.jobs_rejected;
        self.jobs_retried += o.jobs_retried;
        self.jobs_dead += o.jobs_dead;
        self.stale += o.stale;
    }

    pub fn progressed(&self) -> bool {
        self.timers_fired + self.jobs_done + self.jobs_rejected > 0
    }
}

#[derive(Debug, Clone)]
pub struct Scheduler {
    engine: Engine,
    owner: String,
    batch: i64,
    lateness: Arc<Lateness>,
}

impl Scheduler {
    /// `owner` names this worker in `jobs.lease_owner` (host and process, for operators).
    pub fn new(engine: Engine, owner: impl Into<String>) -> Self {
        Self { engine, owner: owner.into(), batch: 500, lateness: Arc::default() }
    }

    pub fn lateness(&self) -> LatenessSnapshot {
        self.lateness.snapshot()
    }

    /// One pass over every workspace with due timers or jobs.
    pub async fn run_once(&self) -> Result<Pass, EngineError> {
        let now = self.engine.clock.now();
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.runtime_due_workspaces($1, 1000)")
                .bind(now)
                .fetch_all(self.engine.db.pool())
                .await?;
        let mut pass = Pass::default();
        for ws in workspaces {
            let ctx = WsCtx { workspace_id: ws };
            pass.add(self.fire_timers(ctx, now).await?);
            let claimed = self.claim(ctx, now).await?;
            for job in &claimed {
                pass.add(self.execute(ctx, job).await?);
            }
        }
        Ok(pass)
    }

    /// Runs passes until one makes no progress (tests and `/__test/drain`).
    pub async fn drain(&self) -> Result<Pass, EngineError> {
        let mut total = Pass::default();
        for _ in 0..1_000 {
            let p = self.run_once().await?;
            total.add(p);
            if !p.progressed() {
                break;
            }
        }
        Ok(total)
    }

    /// Fires the due timers of one workspace.
    pub async fn fire_timers(&self, ctx: WsCtx, now: OffsetDateTime) -> Result<Pass, EngineError> {
        let eng = &self.engine;
        let mut tx = eng.db.begin_ws(ctx).await?;
        let due: Vec<(Uuid, String, Option<Uuid>, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, timer_key, instance_id, purpose, due_at FROM timers
             WHERE state = 'scheduled' AND due_at <= $1 ORDER BY due_at, id LIMIT $2 FOR UPDATE SKIP LOCKED",
        )
        .bind(now)
        .bind(self.batch)
        .fetch_all(tx.conn())
        .await?;
        let mut pass = Pass::default();
        for (id, key, instance, purpose, due_at) in due {
            sqlx::query("UPDATE timers SET state = 'fired', fired_at = $2 WHERE id = $1")
                .bind(id)
                .bind(now)
                .execute(tx.conn())
                .await?;
            self.lateness.record(now - due_at);
            pass.timers_fired += 1;
            match (purpose.as_str(), instance) {
                ("sla", _) => {
                    if sla::breach(eng, &mut tx, &key, now).await? {
                        pass.sla_breached += 1;
                    }
                }
                ("reminder" | "escalation", _) => {
                    self.fire_handler(&mut tx, (&key, instance, &purpose, due_at), now).await?;
                }
                (_, Some(iid)) => {
                    let stimulus = Stimulus { at: now, event: Event::TimerFired { key: key.clone() } };
                    jobs::enqueue_advance(eng, &mut tx, iid, &format!("fired:{key}"), &stimulus, now).await?;
                }
                _ => {}
            }
        }
        tx.commit().await?;
        if pass.timers_fired > 0 {
            tracing::debug!(workspace = %ctx.workspace_id, fired = pass.timers_fired, "timers fired");
        }
        Ok(pass)
    }

    /// Hands a reminder or escalation timer to the effect handler under a savepoint: a failing
    /// handler is logged and rolled back alone, so one bad timer never blocks the others.
    async fn fire_handler(
        &self,
        tx: &mut procera_db::WsTx<'_>,
        (key, instance, purpose, due_at): (&str, Option<Uuid>, &str, OffsetDateTime),
        now: OffsetDateTime,
    ) -> Result<(), EngineError> {
        let eng = &self.engine;
        let Some(h) = &eng.effects else { return Ok(()) };
        sqlx::query("SAVEPOINT handler_timer").execute(tx.conn()).await?;
        let fired = FiredTimer { key, instance, purpose, due_at, at: now };
        match h.fire(tx, fired).await {
            Ok(stimulus) => {
                sqlx::query("RELEASE SAVEPOINT handler_timer").execute(tx.conn()).await?;
                if let (Some(stimulus), Some(iid)) = (stimulus, instance) {
                    jobs::enqueue_advance(eng, tx, iid, &format!("fired:{key}"), &stimulus, now).await?;
                }
            }
            Err(e) => {
                sqlx::query("ROLLBACK TO SAVEPOINT handler_timer").execute(tx.conn()).await?;
                tracing::error!(timer = key, error = %e, "handler timer failed; skipped");
            }
        }
        Ok(())
    }

    /// Claims due jobs of one workspace under a lease (committed before they run).
    pub async fn claim(&self, ctx: WsCtx, now: OffsetDateTime) -> Result<Vec<Claimed>, EngineError> {
        let mut tx = self.engine.db.begin_ws(ctx).await?;
        let claimed = jobs::claim(&mut tx, now, &self.owner, self.batch).await?;
        tx.commit().await?;
        Ok(claimed)
    }

    /// Runs one claimed job: its effect and its fenced completion commit together, or not at all.
    pub async fn execute(&self, ctx: WsCtx, job: &Claimed) -> Result<Pass, EngineError> {
        let eng = &self.engine;
        let mut pass = Pass::default();
        let mut tx = eng.db.begin_ws(ctx).await?;
        let outcome = match (job.kind.as_str(), job.instance_id) {
            (ADVANCE, Some(iid)) => match serde_json::from_value::<Stimulus>(job.payload["stimulus"].clone())
            {
                Ok(stimulus) => eng.apply_in(&mut tx, iid, &stimulus, None).await.map(|_| ()),
                Err(e) => Err(EngineError::State(format!("bad job payload: {e}"))),
            },
            (kind, _) => Err(EngineError::State(format!("unknown job kind {kind}"))),
        };
        let now = eng.clock.now();
        match outcome {
            Ok(()) | Err(EngineError::Rejected(_)) => {
                let note = outcome.as_ref().err().map(|e| json!({ "rejected": e.to_string() }));
                match jobs::complete(&mut tx, job, now, note).await? {
                    Ok(()) => {
                        tx.commit().await?;
                        if outcome.is_ok() { pass.jobs_done += 1 } else { pass.jobs_rejected += 1 }
                    }
                    Err(stale) => {
                        tx.rollback().await?;
                        tracing::warn!(job = %job.id, fence = stale.held, "stale lease; effect discarded");
                        pass.stale += 1;
                    }
                }
            }
            Err(e) => {
                tx.rollback().await?;
                let mut tx = eng.db.begin_ws(ctx).await?;
                match jobs::fail(&mut tx, job, now, &e.to_string()).await? {
                    Ok(JobState::Dead) => {
                        tracing::error!(job = %job.id, error = %e, "job dead-lettered");
                        pass.jobs_dead += 1;
                    }
                    Ok(_) => {
                        tracing::warn!(job = %job.id, attempt = job.attempts, error = %e, "job failed; retrying");
                        pass.jobs_retried += 1;
                    }
                    Err(_) => pass.stale += 1,
                }
                tx.commit().await?;
            }
        }
        Ok(pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lateness_buckets_and_slo() {
        let l = Lateness::default();
        for ms in [0, 50, 900, 61_000, 400_000] {
            l.record(Duration::milliseconds(ms));
        }
        l.record(Duration::seconds(-3)); // early clocks count as on time
        let s = l.snapshot();
        assert_eq!(s.count, 6);
        assert_eq!(s.within_slo, 4);
        assert_eq!(s.max_ms, 400_000);
        assert_eq!(s.buckets[0], (Some(100), 3));
        assert_eq!(s.buckets[1], (Some(1_000), 1));
        assert_eq!(s.buckets[5], (Some(300_000), 1));
        assert_eq!(s.buckets[6], (None, 1));
    }
}
