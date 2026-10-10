//! SP-10: hash-chained audit log at scale (docs/04 §2.10, docs/14 AUD-01/07). Throwaway spike.
//! Needs a scratch Postgres at PG_URL. Creates schema `sp10`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rand::Rng;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

fn pct(mut v: Vec<Duration>, p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p) as usize]
}

/// Canonical bytes: serde_json with sorted keys (objects are BTreeMap-backed when built via json!
/// with the default feature set) — a stand-in for RFC 8785 JCS in the real implementation.
fn canonical(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_default()
}

fn chain_hash(prev: &[u8], event: &Value) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(canonical(event));
    h.finalize().to_vec()
}

fn event(ws: Uuid, seq: i64, actor: Uuid) -> Value {
    json!({"workspace_id": ws, "seq": seq, "actor": actor, "category": "content", "action": "artifact.updated",
           "object_type": "artifact", "object_id": Uuid::now_v7(), "result": "allowed"})
}

async fn setup(admin: &PgPool) -> Result<()> {
    for s in [
        "DROP SCHEMA IF EXISTS sp10 CASCADE",
        "CREATE SCHEMA sp10",
        "CREATE TABLE sp10.audit_events (workspace_id uuid NOT NULL, seq bigint NOT NULL, occurred_at timestamptz NOT NULL, actor uuid, category text NOT NULL, action text NOT NULL, object_type text, object_id uuid, result text NOT NULL, payload jsonb NOT NULL, prev_hash bytea NOT NULL, hash bytea NOT NULL, PRIMARY KEY (workspace_id, seq, occurred_at)) PARTITION BY RANGE (occurred_at)",
        "CREATE TABLE sp10.audit_heads (workspace_id uuid PRIMARY KEY, last_seq bigint NOT NULL, last_hash bytea NOT NULL)",
        "CREATE UNLOGGED TABLE sp10.audit_pending (id bigserial PRIMARY KEY, workspace_id uuid NOT NULL, payload jsonb NOT NULL, occurred_at timestamptz NOT NULL DEFAULT now())",
    ] {
        sqlx::query(s).execute(admin).await?;
    }
    // Monthly partitions covering the last 12 months and the next one.
    for m in -12..=1i32 {
        let sql = format!(
            "CREATE TABLE sp10.audit_events_m{} PARTITION OF sp10.audit_events FOR VALUES FROM (date_trunc('month', now()) + interval '{} month') TO (date_trunc('month', now()) + interval '{} month')",
            m + 12, m, m + 1
        );
        sqlx::query(&sql).execute(admin).await?;
    }
    Ok(())
}

/// Append one event in its own transaction, serialised by the per-workspace head lock.
async fn append_one(pool: &PgPool, ws: Uuid, actor: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let head: Option<(i64, Vec<u8>)> = sqlx::query_as("SELECT last_seq, last_hash FROM sp10.audit_heads WHERE workspace_id = $1 FOR UPDATE").bind(ws).fetch_optional(&mut *tx).await?;
    let (seq, prev) = head.map(|(s, h)| (s + 1, h)).unwrap_or((1, vec![0u8; 32]));
    let ev = event(ws, seq, actor);
    let hash = chain_hash(&prev, &ev);
    sqlx::query("INSERT INTO sp10.audit_events VALUES ($1,$2,now(),$3,'content','artifact.updated','artifact',$4,'allowed',$5,$6,$7)")
        .bind(ws).bind(seq).bind(actor).bind(Uuid::now_v7()).bind(&ev).bind(&prev).bind(&hash).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO sp10.audit_heads VALUES ($1,$2,$3) ON CONFLICT (workspace_id) DO UPDATE SET last_seq = EXCLUDED.last_seq, last_hash = EXCLUDED.last_hash")
        .bind(ws).bind(seq).bind(&hash).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Batched sealer: move up to `batch` pending events into the chain in one transaction.
async fn seal_batch(pool: &PgPool, ws: Uuid, batch: i64) -> Result<usize> {
    let mut tx = pool.begin().await?;
    let pending: Vec<(i64, Value)> = sqlx::query_as("SELECT id, payload FROM sp10.audit_pending WHERE workspace_id = $1 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED").bind(ws).bind(batch).fetch_all(&mut *tx).await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let head: Option<(i64, Vec<u8>)> = sqlx::query_as("SELECT last_seq, last_hash FROM sp10.audit_heads WHERE workspace_id = $1 FOR UPDATE").bind(ws).fetch_optional(&mut *tx).await?;
    let (mut seq, mut prev) = head.unwrap_or((0, vec![0u8; 32]));
    let (mut seqs, mut prevs, mut hashes, mut payloads, mut ids) = (vec![], vec![], vec![], vec![], vec![]);
    for (id, mut p) in pending {
        seq += 1;
        p["seq"] = json!(seq);
        let h = chain_hash(&prev, &p);
        seqs.push(seq);
        prevs.push(prev.clone());
        hashes.push(h.clone());
        payloads.push(p);
        ids.push(id);
        prev = h;
    }
    sqlx::query("INSERT INTO sp10.audit_events SELECT $1, s, now(), (p->>'actor')::uuid, 'data_access', 'field.read', 'request', (p->>'object_id')::uuid, 'allowed', p, ph, h FROM unnest($2::bigint[], $3::jsonb[], $4::bytea[], $5::bytea[]) AS t(s, p, ph, h)")
        .bind(ws).bind(&seqs).bind(&payloads).bind(&prevs).bind(&hashes).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM sp10.audit_pending WHERE id = ANY($1)").bind(&ids).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO sp10.audit_heads VALUES ($1,$2,$3) ON CONFLICT (workspace_id) DO UPDATE SET last_seq = EXCLUDED.last_seq, last_hash = EXCLUDED.last_hash")
        .bind(ws).bind(seq).bind(&prev).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ids.len())
}

async fn verify(pool: &PgPool, ws: Uuid) -> Result<(usize, Option<i64>)> {
    let rows: Vec<(i64, Value, Vec<u8>, Vec<u8>)> = sqlx::query_as("SELECT seq, payload, prev_hash, hash FROM sp10.audit_events WHERE workspace_id = $1 ORDER BY seq").bind(ws).fetch_all(pool).await?;
    let mut prev = vec![0u8; 32];
    for (seq, p, ph, h) in &rows {
        if *ph != prev || chain_hash(&prev, p) != *h {
            return Ok((rows.len(), Some(*seq)));
        }
        prev = h.clone();
    }
    Ok((rows.len(), None))
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("PG_URL").unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/postgres".into());
    let total: i64 = std::env::var("SP10_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000_000);
    let pool = PgPoolOptions::new().max_connections(32).connect(&url).await?;
    if std::env::var("SP10_QUERIES_ONLY").is_ok() {
        return queries_only(&pool).await;
    }
    setup(&pool).await?;
    let ws_hot = Uuid::now_v7();
    let actor = Uuid::now_v7();

    println!("## T1 single writer, one event per transaction");
    let n = 5_000;
    let t = Instant::now();
    for _ in 0..n {
        append_one(&pool, ws_hot, actor).await?;
    }
    let el = t.elapsed();
    println!("{n} events in {el:?} → {:.0} events/s", n as f64 / el.as_secs_f64());

    println!("\n## T2 16 concurrent writers, same workspace, one event per transaction");
    let per = 500;
    let lat = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let t = Instant::now();
    let mut hs = vec![];
    for _ in 0..16 {
        let pool = pool.clone();
        let lat = lat.clone();
        hs.push(tokio::spawn(async move {
            for _ in 0..per {
                let s = Instant::now();
                append_one(&pool, ws_hot, actor).await?;
                lat.lock().await.push(s.elapsed());
            }
            anyhow::Ok(())
        }));
    }
    for h in hs { h.await??; }
    let el = t.elapsed();
    let lat = lat.lock().await.clone();
    println!("{} events in {el:?} → {:.0} events/s; append latency P50 {:?} P95 {:?} P99 {:?}", 16 * per, (16 * per) as f64 / el.as_secs_f64(), pct(lat.clone(), 0.5), pct(lat.clone(), 0.95), pct(lat, 0.99));

    println!("\n## T3 batched sealer (16 producers → pending, 1 sealer, batch 1000)");
    let producers_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let produce_n = 50_000;
    let t = Instant::now();
    let mut hs = vec![];
    for _ in 0..16 {
        let pool = pool.clone();
        hs.push(tokio::spawn(async move {
            for _ in 0..produce_n / 16 {
                let p = json!({"actor": actor, "object_id": Uuid::now_v7(), "field": "salary", "action": "field.read"});
                sqlx::query("INSERT INTO sp10.audit_pending (workspace_id, payload) VALUES ($1, $2)").bind(ws_hot).bind(p).execute(&pool).await?;
            }
            anyhow::Ok(())
        }));
    }
    let sealer = {
        let pool = pool.clone();
        let done = producers_done.clone();
        tokio::spawn(async move {
            let mut sealed = 0usize;
            loop {
                let k = seal_batch(&pool, ws_hot, 1000).await?;
                sealed += k;
                if k == 0 {
                    if done.load(std::sync::atomic::Ordering::SeqCst) { break; }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
            anyhow::Ok(sealed)
        })
    };
    for h in hs { h.await??; }
    let produced_in = t.elapsed();
    producers_done.store(true, std::sync::atomic::Ordering::SeqCst);
    let sealed = sealer.await??;
    let el = t.elapsed();
    println!("produced {} pending in {produced_in:?} ({:.0}/s); sealed {sealed} into the chain by {el:?} → {:.0} events/s end-to-end",
        produce_n / 16 * 16, (produce_n / 16 * 16) as f64 / produced_in.as_secs_f64(), sealed as f64 / el.as_secs_f64());

    println!("\n## T5 verify + tamper detection");
    let t = Instant::now();
    let (count, broken) = verify(&pool, ws_hot).await?;
    println!("verified {count} chained events in {:?} ({:.0}/s); first broken seq: {broken:?}", t.elapsed(), count as f64 / t.elapsed().as_secs_f64());
    sqlx::query("UPDATE sp10.audit_events SET payload = jsonb_set(payload, '{result}', '\"denied\"') WHERE workspace_id = $1 AND seq = 4242").bind(ws_hot).execute(&pool).await?;
    let (_, broken) = verify(&pool, ws_hot).await?;
    println!("after editing seq 4242 directly in the DB: verifier reports first broken seq = {broken:?}");

    println!("\n## T4 explorer queries over {total} events");
    let t = Instant::now();
    // Bulk-load filler events for a large workspace (hashes are not chained; only query shape matters here).
    let ws_big = Uuid::now_v7();
    let actors: Vec<Uuid> = (0..2000).map(|_| Uuid::now_v7()).collect();
    let chunk = 1_000_000i64;
    let mut done = 0;
    while done < total {
        let k = chunk.min(total - done);
        sqlx::query(
            "INSERT INTO sp10.audit_events
             SELECT $1, $2 + g, now() - ((g * 31) % (360*24*3600) || ' seconds')::interval,
                    ($3::uuid[])[1 + (g % 2000)],
                    (ARRAY['auth','access','config','content','runtime','file','data_access','export'])[1 + (g % 8)],
                    (ARRAY['login','update','publish','decide','download','read','grant','revoke'])[1 + ((g / 8) % 8)],
                    (ARRAY['request','artifact','file','member','role'])[1 + (g % 5)],
                    md5(((g % 50000))::text)::uuid, 'allowed', '{}'::jsonb, '\\x00'::bytea, '\\x00'::bytea
             FROM generate_series(1, $4) g",
        )
        .bind(ws_big).bind(done).bind(&actors).bind(k).execute(&pool).await?;
        done += k;
    }
    println!("loaded {total} rows in {:?}", t.elapsed());
    let t = Instant::now();
    for s in [
        "CREATE INDEX ON sp10.audit_events (workspace_id, occurred_at DESC)",
        "CREATE INDEX ON sp10.audit_events (workspace_id, actor, occurred_at DESC)",
        "CREATE INDEX ON sp10.audit_events (workspace_id, object_type, object_id, occurred_at DESC)",
        "CREATE INDEX ON sp10.audit_events (workspace_id, category, action, occurred_at DESC)",
        "ANALYZE sp10.audit_events",
    ] {
        sqlx::query(s).execute(&pool).await?;
    }
    println!("built 4 indexes + analyze in {:?}", t.elapsed());
    let size: String = sqlx::query_scalar("SELECT pg_size_pretty(sum(pg_total_relation_size(inhrelid))) FROM pg_inherits WHERE inhparent = 'sp10.audit_events'::regclass").fetch_one(&pool).await?;
    println!("table + indexes size: {size}");

    run_queries(&pool, ws_big, &actors).await
}

async fn queries_only(pool: &PgPool) -> Result<()> {
    let ws_big: Uuid = sqlx::query_scalar("SELECT workspace_id FROM sp10.audit_events GROUP BY workspace_id ORDER BY count(*) DESC LIMIT 1").fetch_one(pool).await?;
    let actors: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT actor FROM sp10.audit_events WHERE workspace_id = $1 AND seq <= 5000").bind(ws_big).fetch_all(pool).await?;
    run_queries(pool, ws_big, &actors).await
}

async fn run_queries(pool: &PgPool, ws_big: Uuid, actors: &[Uuid]) -> Result<()> {
    let pool = pool.clone();
    let queries: [(&str, &str); 4] = [
        ("Q1 actor, last 30 days, newest 100", "SELECT seq FROM sp10.audit_events WHERE workspace_id=$1 AND actor=$2 AND occurred_at > now() - interval '30 days' ORDER BY occurred_at DESC LIMIT 100"),
        ("Q2 object timeline", "SELECT seq FROM sp10.audit_events WHERE workspace_id=$1 AND object_type='request' AND object_id=$2 ORDER BY occurred_at DESC LIMIT 100"),
        ("Q3 category+action, last 7 days, newest 100", "SELECT seq FROM sp10.audit_events WHERE workspace_id=$1 AND category='data_access' AND action='read' AND occurred_at > now() - interval '7 days' ORDER BY occurred_at DESC LIMIT 100"),
        ("Q4 latest page, keyset cursor 90 days back", "SELECT seq FROM sp10.audit_events WHERE workspace_id=$1 AND occurred_at < now() - interval '90 days' ORDER BY occurred_at DESC LIMIT 100"),
    ];
    let objects: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT object_id FROM sp10.audit_events WHERE workspace_id=$1 AND object_type='request' LIMIT 200").bind(ws_big).fetch_all(&pool).await?;
    for (name, sql) in queries {
        let mut lat = vec![];
        for i in 0..200 {
            let a = actors[rand::rng().random_range(0..actors.len())];
            let o = objects[i % objects.len().max(1)];
            let t = Instant::now();
            let q = sqlx::query_scalar::<_, i64>(sql).bind(ws_big);
            let q = if sql.contains("object_id=$2") { q.bind(o) } else if sql.contains("$2") { q.bind(a) } else { q };
            let _rows: Vec<i64> = q.fetch_all(&pool).await?;
            lat.push(t.elapsed());
        }
        println!("{name}: P50 {:?} P95 {:?}", pct(lat.clone(), 0.5), pct(lat, 0.95));
    }
    Ok(())
}
