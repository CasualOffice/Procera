//! Job workers and timer scheduler with leases + fencing (docs/04 §4.3–4.4), plus chaos injection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use rand::Rng;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::engine::Engine;

#[derive(Debug, Default)]
pub struct Stats {
    pub jobs_done: AtomicU64,
    pub lost_leases: AtomicU64,
    pub crashes: AtomicU64,
    pub timers_fired: AtomicU64,
}

const LEASE: &str = "1.5 seconds";

/// Runs until `stop` is set or a simulated crash kills it (returns Err("crash")).
pub async fn job_worker(engine: Engine, me: String, crash_p: f64, stop: Arc<AtomicBool>, stats: Arc<Stats>) -> Result<()> {
    while !stop.load(Ordering::Relaxed) {
        let claimed = sqlx::query(&format!(
            "UPDATE rt.jobs SET state = 'leased', lease_owner = $1, lease_until = now() + interval '{LEASE}', fence = fence + 1, attempts = attempts + 1
             WHERE id = (SELECT id FROM rt.jobs WHERE (state = 'queued' AND run_at <= now()) OR (state = 'leased' AND lease_until < now())
                         ORDER BY run_at LIMIT 1 FOR UPDATE SKIP LOCKED)
             RETURNING id, kind, instance_id, token_id, fence, idem_key"
        ))
        .bind(&me)
        .fetch_optional(&engine.pool)
        .await?;
        let Some(job) = claimed else {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        };
        let (id, kind, instance, token, fence, key): (Uuid, String, Uuid, Option<Uuid>, i64, String) =
            (job.get(0), job.get(1), job.get(2), job.get(3), job.get(4), job.get(5));
        let crash = || rand::rng().random_bool(crash_p);

        if kind == "service_call" {
            // 1) external side effect, honouring the idempotency key (the external system dedupes).
            sqlx::query("INSERT INTO rt.effect_attempts (idem_key) VALUES ($1)").bind(&key).execute(&engine.pool).await?;
            sqlx::query("INSERT INTO rt.effects (idem_key, instance_id) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(&key).bind(instance).execute(&engine.pool).await?;
            // 2) crash window: effect applied, completion not recorded.
            if crash() {
                stats.crashes.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!("crash");
            }
        }
        let mut tx = engine.pool.begin().await?;
        if kind == "service_call" {
            sqlx::query("SELECT 1 FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).execute(&mut *tx).await?;
            sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = 'ok' WHERE id = $1 AND state = 'waiting'").bind(token).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO rt.jobs (id, kind, instance_id, idem_key) VALUES ($1, 'advance', $2, $3) ON CONFLICT (idem_key) DO NOTHING")
                .bind(Uuid::now_v7()).bind(instance).bind(format!("adv:svc:{key}")).execute(&mut *tx).await?;
        } else {
            engine.advance(&mut tx, instance).await?;
        }
        // Crash mid-transaction: nothing commits; the lease expires and another worker retries.
        if crash() {
            stats.crashes.fetch_add(1, Ordering::Relaxed);
            drop(tx);
            anyhow::bail!("crash");
        }
        let fenced = sqlx::query("UPDATE rt.jobs SET state = 'done' WHERE id = $1 AND fence = $2 AND lease_owner = $3")
            .bind(id).bind(fence).bind(&me).execute(&mut *tx).await?.rows_affected();
        if fenced == 0 {
            stats.lost_leases.fetch_add(1, Ordering::Relaxed);
            tx.rollback().await?; // a newer claimant owns this job now
            continue;
        }
        tx.commit().await?;
        stats.jobs_done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

/// Fires due timers against the controllable clock (rt.clock).
pub async fn timer_worker(engine: Engine, _me: String, crash_p: f64, stop: Arc<AtomicBool>, stats: Arc<Stats>) -> Result<()> {
    while !stop.load(Ordering::Relaxed) {
        let claimed = sqlx::query(&format!(
            "UPDATE rt.timers SET state = 'leased', lease_until = now() + interval '{LEASE}', fence = fence + 1
             WHERE id = (SELECT id FROM rt.timers WHERE ((state = 'scheduled') OR (state = 'leased' AND lease_until < now())) AND due_at <= (SELECT now FROM rt.clock)
                         ORDER BY due_at LIMIT 1 FOR UPDATE SKIP LOCKED)
             RETURNING id, instance_id, token_id, purpose, fence, payload"
        ))
        .fetch_optional(&engine.pool)
        .await?;
        let Some(t) = claimed else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        let (id, instance, token, purpose, fence, payload): (Uuid, Uuid, Uuid, String, i64, serde_json::Value) =
            (t.get(0), t.get(1), t.get(2), t.get(3), t.get(4), t.get(5));
        let mut tx = engine.pool.begin().await?;
        let state: String = sqlx::query_scalar("SELECT state FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).fetch_one(&mut *tx).await?;
        if state == "running" {
            if purpose == "wait" {
                sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = 'fired' WHERE id = $1 AND state = 'waiting'").bind(token).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO rt.jobs (id, kind, instance_id, idem_key) VALUES ($1, 'advance', $2, $3) ON CONFLICT (idem_key) DO NOTHING")
                    .bind(Uuid::now_v7()).bind(instance).bind(format!("adv:timer:{id}")).execute(&mut *tx).await?;
            } else if purpose == "escalate" {
                let wi: Uuid = serde_json::from_value(payload["work_item"].clone())?;
                sqlx::query("UPDATE rt.work_items SET assignee = 'escalated', revision = revision + 1 WHERE id = $1 AND state = 'offered'").bind(wi).execute(&mut *tx).await?;
                sqlx::query("UPDATE rt.instances SET revision = revision + 1 WHERE id = $1").bind(instance).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO rt.events SELECT $1, revision, 'work.escalated', NULL, $2 FROM rt.instances WHERE id = $1")
                    .bind(instance).bind(json!({"work_item": wi})).execute(&mut *tx).await?;
            }
        }
        if rand::rng().random_bool(crash_p) {
            stats.crashes.fetch_add(1, Ordering::Relaxed);
            drop(tx);
            anyhow::bail!("crash");
        }
        let fenced = sqlx::query("UPDATE rt.timers SET state = 'fired' WHERE id = $1 AND fence = $2").bind(id).bind(fence).execute(&mut *tx).await?.rows_affected();
        if fenced == 0 {
            stats.lost_leases.fetch_add(1, Ordering::Relaxed);
            tx.rollback().await?;
            continue;
        }
        tx.commit().await?;
        stats.timers_fired.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

/// Keeps `n` job workers and `t` timer workers alive, respawning after simulated crashes,
/// and (optionally) aborting random workers to emulate process kills.
pub fn supervise(engine: Engine, n: usize, t: usize, crash_p: f64, kill_every: Option<Duration>, stop: Arc<AtomicBool>, stats: Arc<Stats>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut g = 0u64;
        let spawn = |g: &mut u64, timer: bool| {
            *g += 1;
            let me = format!("{}-{g}", if timer { "timer" } else { "worker" });
            let (e, s, st) = (engine.clone(), stop.clone(), stats.clone());
            if timer { tokio::spawn(timer_worker(e, me, crash_p, s, st)) } else { tokio::spawn(job_worker(e, me, crash_p, s, st)) }
        };
        let mut jobs: Vec<_> = (0..n).map(|_| spawn(&mut g, false)).collect();
        let mut timers: Vec<_> = (0..t).map(|_| spawn(&mut g, true)).collect();
        let mut last_kill = tokio::time::Instant::now();
        while !stop.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if let Some(every) = kill_every
                && last_kill.elapsed() >= every
                && !jobs.is_empty()
            {
                let i = rand::rng().random_range(0..jobs.len());
                jobs[i].abort(); // hard kill: may interrupt anywhere, incl. mid-transaction
                stats.crashes.fetch_add(1, Ordering::Relaxed);
                last_kill = tokio::time::Instant::now();
            }
            for h in jobs.iter_mut() { if h.is_finished() { *h = spawn(&mut g, false); } }
            for h in timers.iter_mut() { if h.is_finished() { *h = spawn(&mut g, true); } }
        }
        for h in jobs.into_iter().chain(timers) { h.abort(); }
    })
}
