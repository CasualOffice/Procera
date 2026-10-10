//! SP-01..SP-06: native runtime spikes (docs/01 §8, ADR-0015). Throwaway code; see docs/spikes/SP-01-06.
//! Usage: PG_URL=postgres://postgres@127.0.0.1:55432/postgres cargo run --release -p sp01-runtime [sp01|sp02|...|all]

mod bpmn;
mod engine;
mod workers;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use engine::{DecideResult, Engine};
use rand::Rng;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
use workers::{Stats, supervise};

fn approval_process() -> Value {
    json!({"schemaVersion":1,"process":{"key":"sp.approval","version":1,"name":{"en":"Approval"}},"variables":[],
      "nodes":[{"id":"start","kind":"start"},{"id":"approval","kind":"human.approval"},
               {"id":"provision","kind":"service.call","connector":"x","action":"y"},
               {"id":"end_ok","kind":"end"},{"id":"end_rej","kind":"end"}],
      "edges":[{"id":"e1","from":"start","to":"approval"},{"id":"e2","from":"approval","to":"provision","on":"approved"},
               {"id":"e3","from":"approval","to":"end_rej","on":"rejected"},{"id":"e4","from":"approval","to":"end_rej","on":"changes_requested"},
               {"id":"e5","from":"provision","to":"end_ok","on":"ok"},{"id":"e6","from":"provision","to":"end_rej","on":"error"}]})
}

fn parallel_process() -> Value {
    json!({"schemaVersion":1,"process":{"key":"sp.parallel","version":1,"name":{"en":"Parallel"}},"variables":[],
      "nodes":[{"id":"start","kind":"start"},
               {"id":"fork","kind":"parallel.split","joinId":"join","branches":[{"name":"a"},{"name":"b"},{"name":"c"}]},
               {"id":"task_a","kind":"human.task","escalations":[{"at":"PT30M","action":"reassign","to":{"type":"role","ref":"owner"}}]},
               {"id":"wait_b","kind":"wait.timer","after":"PT1H"},
               {"id":"task_c","kind":"human.task"},
               {"id":"join","kind":"parallel.join"},{"id":"end","kind":"end"}],
      "edges":[{"id":"e1","from":"start","to":"fork"},{"id":"e2","from":"fork","to":"task_a","on":"a"},{"id":"e3","from":"fork","to":"wait_b","on":"b"},
               {"id":"e4","from":"fork","to":"task_c","on":"c"},{"id":"e5","from":"task_a","to":"join","on":"submitted"},
               {"id":"e6","from":"wait_b","to":"join","on":"fired"},{"id":"e7","from":"task_c","to":"join","on":"submitted"},{"id":"e8","from":"join","to":"end"}]})
}

fn signal_process() -> Value {
    json!({"schemaVersion":1,"process":{"key":"sp.signal","version":1,"name":{"en":"Signal"}},"variables":[],
      "nodes":[{"id":"start","kind":"start"},{"id":"wait","kind":"wait.signal"},{"id":"end","kind":"end"}],
      "edges":[{"id":"e1","from":"start","to":"wait"},{"id":"e2","from":"wait","to":"end","on":"received"}]})
}

fn dormant_v1() -> Value {
    json!({"schemaVersion":1,"process":{"key":"sp.dormant","version":1,"name":{"en":"Dormant v1"}},"variables":[],
      "nodes":[{"id":"start","kind":"start"},{"id":"wait","kind":"wait.timer","after":"P30D"},{"id":"review","kind":"human.task"},{"id":"end","kind":"end"}],
      "edges":[{"id":"e1","from":"start","to":"wait"},{"id":"e2","from":"wait","to":"review","on":"fired"},{"id":"e3","from":"review","to":"end","on":"submitted"}]})
}

fn dormant_v2() -> Value {
    json!({"schemaVersion":1,"process":{"key":"sp.dormant","version":2,"name":{"en":"Dormant v2"}},"variables":[],
      "nodes":[{"id":"start","kind":"start"},{"id":"wait","kind":"wait.timer","after":"P30D"},{"id":"end","kind":"end"}],
      "edges":[{"id":"e1","from":"start","to":"wait"},{"id":"e2","from":"wait","to":"end","on":"fired"}]})
}

async fn wait_until(what: &str, timeout: Duration, mut cond: impl AsyncFnMut() -> Result<bool>) -> Result<Duration> {
    let t = Instant::now();
    while !cond().await? {
        ensure!(t.elapsed() < timeout, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(t.elapsed())
}

async fn scalar(e: &Engine, sql: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).fetch_one(&e.pool).await?)
}

async fn reset(e: &Engine) -> Result<()> {
    sqlx::raw_sql(include_str!("schema.sql")).execute(&e.pool).await?;
    e.clear_cache();
    Ok(())
}

/// Invariants that must hold after every scenario.
async fn invariants(e: &Engine) -> Result<()> {
    ensure!(scalar(e, "SELECT count(*) FROM rt.instances i WHERE i.revision <> (SELECT count(*) FROM rt.events v WHERE v.instance_id = i.id)").await? == 0,
        "event sequences must be gap-free (max seq = count)");
    ensure!(scalar(e, "SELECT count(*) FROM rt.instances WHERE state IN ('completed','cancelled') AND EXISTS (SELECT 1 FROM rt.tokens t WHERE t.instance_id = rt.instances.id AND t.state IN ('enter','waiting','resume','arrived'))").await? == 0,
        "finished instances must not have live tokens");
    ensure!(scalar(e, "SELECT count(*) FROM rt.splits WHERE released AND cardinality(arrived) <> expected").await? == 0, "joins release only when complete");
    ensure!(scalar(e, "SELECT count(*) FROM (SELECT instance_id, payload->>'split' s FROM rt.events WHERE kind = 'join.released' GROUP BY 1, 2 HAVING count(*) > 1) x").await? == 0,
        "each join releases exactly once");
    ensure!(scalar(e, "SELECT count(*) FROM rt.instances WHERE state = 'failed'").await? == 0, "no failed instances");
    Ok(())
}

async fn sp01(e: &Engine) -> Result<()> {
    println!("\n## SP-01 + SP-04a — 10 000 waiting approvals, chaos workers, idempotent side effects");
    reset(e).await?;
    e.publish(1, &approval_process()).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats::default());
    // 16 workers; 3 % crash probability per job (mid-tx and after-effect); a hard kill every 150 ms.
    let sup = supervise(e.clone(), 16, 2, 0.03, Some(Duration::from_millis(150)), stop.clone(), stats.clone());
    let n = 10_000usize;
    let t = Instant::now();
    let mut hs = vec![];
    for chunk in 0..32 {
        let e = e.clone();
        hs.push(tokio::spawn(async move {
            for _ in 0..n / 32 + usize::from(chunk < n % 32) { e.start(1, json!({})).await?; }
            anyhow::Ok(())
        }));
    }
    for h in hs { h.await??; }
    let waited = wait_until("all instances waiting at approval", Duration::from_secs(300), async || Ok(scalar(e, "SELECT count(*) FROM rt.work_items WHERE state = 'offered'").await? == n as i64)).await?;
    println!("started {n} instances; all reached the approval in {:?} (incl. {:?} waiting)", t.elapsed(), waited);

    // Restart ALL workers while instances wait (SP-01 "restart all workers").
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    stop.store(false, Ordering::SeqCst);
    let sup = supervise(e.clone(), 16, 2, 0.03, Some(Duration::from_millis(150)), stop.clone(), stats.clone());
    println!("killed and restarted all workers");

    let items: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM rt.work_items WHERE state = 'offered'").fetch_all(&e.pool).await?;
    let t = Instant::now();
    let mut hs = vec![];
    for (i, part) in items.chunks(items.len().div_ceil(32)).enumerate() {
        let (e, part) = (e.clone(), part.to_vec());
        hs.push(tokio::spawn(async move {
            for wi in part {
                let outcome = if rand::rng().random_bool(0.7) { "approved" } else { "rejected" };
                let r = e.decide(wi, outcome, &format!("approver-{i}"), &format!("decide-{wi}")).await?;
                ensure!(r == DecideResult::Committed, "unexpected {r:?}");
            }
            anyhow::Ok(())
        }));
    }
    for h in hs { h.await??; }
    let done = wait_until("all instances completed", Duration::from_secs(600), async || Ok(scalar(e, "SELECT count(*) FROM rt.instances WHERE state = 'completed'").await? == n as i64)).await?;
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    let approved = scalar(e, "SELECT count(*) FROM rt.work_items WHERE outcome = 'approved'").await?;
    let effects = scalar(e, "SELECT count(*) FROM rt.effects").await?;
    let attempts = scalar(e, "SELECT count(*) FROM rt.effect_attempts").await?;
    let dup = scalar(e, "SELECT count(*) FROM (SELECT instance_id FROM rt.effects GROUP BY 1 HAVING count(*) > 1) x").await?;
    let stuck = scalar(e, "SELECT count(*) FROM rt.jobs WHERE state IN ('queued','leased')").await?;
    println!("decided {} work items; all {n} instances completed {:?} after the last decision ({:?} total)", items.len(), done, t.elapsed());
    println!("crashes injected: {} · lost leases (fencing rejected stale workers): {} · jobs done: {}",
        stats.crashes.load(Ordering::Relaxed), stats.lost_leases.load(Ordering::Relaxed), stats.jobs_done.load(Ordering::Relaxed));
    println!("approved {approved} → external effects applied {effects} (attempts {attempts}, i.e. {} retries absorbed by idempotency keys); instances with >1 effect: {dup}; stuck jobs: {stuck}",
        attempts - effects);
    ensure!(effects == approved && dup == 0 && stuck == 0, "SP-01/SP-04a failed");
    invariants(e).await?;
    println!("✅ SP-01 passed: 0 lost transitions, 0 duplicate effects, invariants hold");
    Ok(())
}

async fn sp02(e: &Engine) -> Result<()> {
    println!("\n## SP-02 — double-approval race (1 000 work items × 2 concurrent deciders)");
    reset(e).await?;
    e.publish(1, &approval_process()).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats::default());
    let sup = supervise(e.clone(), 8, 1, 0.0, None, stop.clone(), stats.clone());
    for _ in 0..1000 { e.start(1, json!({})).await?; }
    wait_until("offered", Duration::from_secs(120), async || Ok(scalar(e, "SELECT count(*) FROM rt.work_items WHERE state = 'offered'").await? == 1000)).await?;
    let items: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM rt.work_items").fetch_all(&e.pool).await?;
    let mut hs = vec![];
    for wi in items.clone() {
        for (actor, outcome) in [("alice", "approved"), ("bob", "rejected")] {
            let e = e.clone();
            hs.push(tokio::spawn(async move { e.decide(wi, outcome, actor, &format!("{actor}-{wi}")).await }));
        }
    }
    let (mut committed, mut already) = (0, 0);
    for h in hs {
        match h.await?? { DecideResult::Committed => committed += 1, DecideResult::AlreadyDecided => already += 1, DecideResult::Replayed => {} }
    }
    // Replay with the same key must return the stored result, not a second transition.
    let replay = e.decide(items[0], "approved", "alice", &format!("alice-{}", items[0])).await?;
    wait_until("completed", Duration::from_secs(120), async || Ok(scalar(e, "SELECT count(*) FROM rt.instances WHERE state = 'completed'").await? == 1000)).await?;
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    let transitions = scalar(e, "SELECT count(*) FROM rt.events WHERE kind = 'work.completed'").await?;
    println!("committed {committed} · rejected as already decided {already} · work.completed events {transitions} · replay of a used key → {replay:?}");
    ensure!(committed == 1000 && already == 1000 && transitions == 1000, "SP-02 failed");
    invariants(e).await?;
    println!("✅ SP-02 passed: exactly one accepted transition per work item");
    Ok(())
}

async fn sp03(e: &Engine) -> Result<()> {
    println!("\n## SP-03 — parallel join, timers, escalation, cancellation (500 + 100 instances)");
    reset(e).await?;
    e.publish(1, &parallel_process()).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats::default());
    let sup = supervise(e.clone(), 12, 4, 0.02, Some(Duration::from_millis(300)), stop.clone(), stats.clone());
    let mut ids = vec![];
    for _ in 0..600 { ids.push(e.start(1, json!({})).await?); }
    wait_until("branches waiting", Duration::from_secs(120), async || Ok(scalar(e, "SELECT count(*) FROM rt.work_items WHERE state = 'offered'").await? == 1200)).await?;

    // Escalation: 31 minutes later every task_a is reassigned (timer purpose 'escalate').
    e.advance_clock(31 * 60).await?;
    wait_until("escalations", Duration::from_secs(120), async || Ok(scalar(e, "SELECT count(*) FROM rt.work_items WHERE node_id = 'task_a' AND assignee = 'escalated'").await? == 600)).await?;
    println!("escalation fired for all 600 task_a items after +31 min");

    // Cancel 100 instances mid-parallel.
    for id in &ids[500..] { e.cancel(*id).await?; }

    // Race: complete task_a and task_c concurrently AND fire the 1 h timers at the same moment.
    let a_c: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM rt.work_items WHERE state = 'offered' ORDER BY random()").fetch_all(&e.pool).await?;
    let clock = { let e = e.clone(); tokio::spawn(async move { e.advance_clock(30 * 60).await }) };
    let mut hs = vec![];
    for part in a_c.chunks(a_c.len().div_ceil(24)) {
        let (e, part) = (e.clone(), part.to_vec());
        hs.push(tokio::spawn(async move { for wi in part { e.decide(wi, "submitted", "u", &format!("k-{wi}")).await?; } anyhow::Ok(()) }));
    }
    clock.await??;
    for h in hs { h.await??; }
    wait_until("completed", Duration::from_secs(300), async || Ok(scalar(e, "SELECT count(*) FROM rt.instances WHERE state = 'completed'").await? == 500)).await?;
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    let releases = scalar(e, "SELECT count(*) FROM rt.events WHERE kind = 'join.released'").await?;
    let cancelled_live = scalar(e, "SELECT count(*) FROM rt.tokens t JOIN rt.instances i ON i.id = t.instance_id WHERE i.state = 'cancelled' AND t.state IN ('enter','waiting','resume','arrived')").await?;
    let cancelled_timers = scalar(e, "SELECT count(*) FROM rt.timers t JOIN rt.instances i ON i.id = t.instance_id WHERE i.state = 'cancelled' AND t.state = 'fired' AND t.purpose = 'wait'").await?;
    let late_decision = sqlx::query_scalar::<_, Uuid>("SELECT w.id FROM rt.work_items w JOIN rt.instances i ON i.id = w.instance_id WHERE i.state = 'cancelled' LIMIT 1").fetch_one(&e.pool).await?;
    let late = e.decide(late_decision, "submitted", "u", "late-1").await?;
    println!("join released {releases} times for 500 instances · cancelled instances: live tokens {cancelled_live}, wait timers fired {cancelled_timers}, late decision → {late:?}");
    println!("crashes injected {} · timers fired {}", stats.crashes.load(Ordering::Relaxed), stats.timers_fired.load(Ordering::Relaxed));
    ensure!(releases == 500 && cancelled_live == 0 && cancelled_timers == 0 && late == DecideResult::AlreadyDecided, "SP-03 failed");
    invariants(e).await?;
    println!("✅ SP-03 passed: join releases exactly once under races; escalation and cancellation correct");
    Ok(())
}

async fn sp04(e: &Engine) -> Result<()> {
    println!("\n## SP-04b — duplicate webhook deliveries (1 000 instances × 3 concurrent deliveries)");
    reset(e).await?;
    e.publish(1, &signal_process()).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats::default());
    let sup = supervise(e.clone(), 8, 1, 0.0, None, stop.clone(), stats.clone());
    let mut ids = vec![];
    for _ in 0..1000 { ids.push(e.start(1, json!({})).await?); }
    wait_until("waiting", Duration::from_secs(60), async || Ok(scalar(e, "SELECT count(*) FROM rt.tokens WHERE node_id = 'wait' AND state = 'waiting'").await? == 1000)).await?;
    let mut hs = vec![];
    for id in &ids {
        for _ in 0..3 {
            let (e, id) = (e.clone(), *id);
            hs.push(tokio::spawn(async move { e.deliver_signal(id, &format!("msg-{id}")).await }));
        }
    }
    let mut accepted = 0;
    for h in hs { if h.await?? { accepted += 1; } }
    wait_until("completed", Duration::from_secs(60), async || Ok(scalar(e, "SELECT count(*) FROM rt.instances WHERE state = 'completed'").await? == 1000)).await?;
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    let received = scalar(e, "SELECT count(*) FROM rt.events WHERE kind = 'signal.received'").await?;
    println!("3 000 deliveries → {accepted} accepted, {received} signal.received events, 1 000 completed");
    ensure!(accepted == 1000 && received == 1000, "SP-04b failed");
    invariants(e).await?;
    println!("✅ SP-04 passed: duplicates produce no duplicate effect");
    Ok(())
}

async fn sp05(e: &Engine) -> Result<()> {
    println!("\n## SP-05 — 30-day dormant instances survive a definition change (pinned version)");
    reset(e).await?;
    e.publish(1, &dormant_v1()).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(Stats::default());
    let sup = supervise(e.clone(), 4, 2, 0.0, None, stop.clone(), stats.clone());
    for _ in 0..100 { e.start(1, json!({})).await?; }
    e.publish(2, &dormant_v2()).await?; // v2 removes the review step
    for _ in 0..100 { e.start(2, json!({})).await?; }
    wait_until("timers scheduled", Duration::from_secs(60), async || Ok(scalar(e, "SELECT count(*) FROM rt.timers").await? == 200)).await?;
    e.advance_clock(30 * 86400 + 60).await?;
    wait_until("v2 completed and v1 at review", Duration::from_secs(120), async || Ok(
        scalar(e, "SELECT count(*) FROM rt.instances WHERE def_version = 2 AND state = 'completed'").await? == 100
        && scalar(e, "SELECT count(*) FROM rt.work_items w JOIN rt.instances i ON i.id = w.instance_id WHERE i.def_version = 1 AND w.node_id = 'review'").await? == 100)).await?;
    stop.store(true, Ordering::SeqCst);
    sup.await?;
    println!("after +30 days: 100 v1 instances are at 'review' (their pinned graph); 100 v2 instances completed (new graph)");
    invariants(e).await?;
    println!("✅ SP-05 passed: instances follow the version they started on");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("PG_URL").unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/postgres".into());
    let pool = PgPoolOptions::new().max_connections(80).acquire_timeout(Duration::from_secs(60)).connect(&url).await?;
    let e = Engine::new(pool);
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let all = which == "all";
    if all || which == "sp06" { bpmn::sp06()?; }
    if all || which == "sp02" { sp02(&e).await?; }
    if all || which == "sp04" { sp04(&e).await?; }
    if all || which == "sp05" { sp05(&e).await?; }
    if all || which == "sp03" { sp03(&e).await?; }
    if all || which == "sp01" { sp01(&e).await?; }
    Ok(())
}
