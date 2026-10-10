//! Spike interpreter for IR v1 on Postgres (docs/04 §4). Deliberately small: enough node kinds to
//! test the hard properties (durability, races, joins, timers, idempotency, version pinning).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

pub type Tx<'a> = Transaction<'a, Postgres>;

#[derive(Debug)]
pub struct Ir {
    nodes: HashMap<String, Value>,
    edges: Vec<(String, String, String)>, // from, to, on
}

impl Ir {
    pub fn parse(doc: &Value) -> Result<Self> {
        let nodes = doc["nodes"]
            .as_array()
            .ok_or_else(|| anyhow!("nodes"))?
            .iter()
            .map(|n| (n["id"].as_str().unwrap_or_default().to_owned(), n.clone()))
            .collect();
        let edges = doc["edges"]
            .as_array()
            .ok_or_else(|| anyhow!("edges"))?
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap_or_default().to_owned(),
                    e["to"].as_str().unwrap_or_default().to_owned(),
                    e["on"].as_str().unwrap_or("next").to_owned(),
                )
            })
            .collect();
        Ok(Self { nodes, edges })
    }
    fn node(&self, id: &str) -> Result<&Value> {
        self.nodes.get(id).ok_or_else(|| anyhow!("unknown node {id}"))
    }
    fn kind(&self, id: &str) -> Result<&str> {
        Ok(self.node(id)?["kind"].as_str().unwrap_or_default())
    }
    fn next(&self, from: &str, on: &str) -> Option<&str> {
        self.edges.iter().find(|(f, _, o)| f == from && o == on).map(|(_, t, _)| t.as_str())
    }
    fn outgoing(&self, from: &str) -> Vec<(&str, &str)> {
        self.edges.iter().filter(|(f, _, _)| f == from).map(|(_, t, o)| (t.as_str(), o.as_str())).collect()
    }
    pub fn start_node(&self) -> Result<&str> {
        self.nodes
            .iter()
            .find(|(_, n)| n["kind"] == "start")
            .map(|(k, _)| k.as_str())
            .ok_or_else(|| anyhow!("no start"))
    }
}

/// ISO-8601 duration subset (W, D, T H/M/S) → seconds.
pub fn duration_secs(s: &str) -> Result<i64> {
    let rest = s.strip_prefix('P').ok_or_else(|| anyhow!("bad duration {s}"))?;
    let (d, t) = rest.split_once('T').unwrap_or((rest, ""));
    let mut total = 0i64;
    for (part, date) in [(d, true), (t, false)] {
        let mut num = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() {
                num.push(c);
                continue;
            }
            let n: i64 = num.parse()?;
            num.clear();
            total += match (c, date) {
                ('W', true) => n * 7 * 86400,
                ('D', true) => n * 86400,
                ('H', false) => n * 3600,
                ('M', false) => n * 60,
                ('S', false) => n,
                _ => bail!("bad duration {s}"),
            };
        }
    }
    Ok(total)
}

/// Tiny condition evaluator for decisions in the spike: `name = "v"`, `name == "v"`, or a boolean variable.
fn eval_condition(expr: &str, vars: &Value) -> bool {
    let body = expr.splitn(3, ':').nth(2).unwrap_or(expr).trim();
    if let Some((l, r)) = body.split_once("==").or_else(|| body.split_once('=')) {
        let v = r.trim().trim_matches('"');
        return vars[l.trim()].as_str() == Some(v);
    }
    vars[body].as_bool().unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct Engine {
    pub pool: PgPool,
    defs: Arc<RwLock<HashMap<i32, Arc<Ir>>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecideResult {
    Committed,
    Replayed,
    AlreadyDecided,
}

impl Engine {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, defs: Arc::default() }
    }

    pub fn clear_cache(&self) {
        if let Ok(mut m) = self.defs.write() { m.clear(); }
    }

    pub async fn publish(&self, version: i32, ir: &Value) -> Result<()> {
        sqlx::query("INSERT INTO rt.definitions VALUES ($1, $2)").bind(version).bind(ir).execute(&self.pool).await?;
        Ok(())
    }

    async fn ir(&self, tx: &mut Tx<'_>, version: i32) -> Result<Arc<Ir>> {
        if let Some(ir) = self.defs.read().map_err(|_| anyhow!("lock"))?.get(&version) {
            return Ok(ir.clone());
        }
        let doc: Value = sqlx::query_scalar("SELECT ir FROM rt.definitions WHERE version = $1").bind(version).fetch_one(&mut **tx).await?;
        let ir = Arc::new(Ir::parse(&doc)?);
        self.defs.write().map_err(|_| anyhow!("lock"))?.insert(version, ir.clone());
        Ok(ir)
    }

    async fn event(tx: &mut Tx<'_>, instance: Uuid, kind: &str, node: Option<&str>, payload: Value) -> Result<i64> {
        let seq: i64 = sqlx::query_scalar("UPDATE rt.instances SET revision = revision + 1 WHERE id = $1 RETURNING revision")
            .bind(instance)
            .fetch_one(&mut **tx)
            .await?;
        sqlx::query("INSERT INTO rt.events VALUES ($1, $2, $3, $4, $5)")
            .bind(instance)
            .bind(seq)
            .bind(kind)
            .bind(node)
            .bind(payload)
            .execute(&mut **tx)
            .await?;
        Ok(seq)
    }

    async fn enqueue_advance(tx: &mut Tx<'_>, instance: Uuid, key: &str) -> Result<()> {
        sqlx::query("INSERT INTO rt.jobs (id, kind, instance_id, idem_key) VALUES ($1, 'advance', $2, $3) ON CONFLICT (idem_key) DO NOTHING")
            .bind(Uuid::now_v7())
            .bind(instance)
            .bind(key)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn now(tx: &mut Tx<'_>) -> Result<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>> {
        Ok(sqlx::query_scalar("SELECT now FROM rt.clock").fetch_one(&mut **tx).await?)
    }

    pub async fn advance_clock(&self, secs: i64) -> Result<()> {
        sqlx::query("UPDATE rt.clock SET now = now + make_interval(secs => $1)").bind(secs as f64).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn start(&self, version: i32, vars: Value) -> Result<Uuid> {
        let mut tx = self.pool.begin().await?;
        let id = Uuid::now_v7();
        let ir = self.ir(&mut tx, version).await?;
        sqlx::query("INSERT INTO rt.instances (id, def_version, state) VALUES ($1, $2, 'running')").bind(id).bind(version).execute(&mut *tx).await?;
        sqlx::query("UPDATE rt.instances SET vars = $2 WHERE id = $1").bind(id).bind(&vars).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO rt.tokens (id, instance_id, node_id, state) VALUES ($1, $2, $3, 'enter')")
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(ir.start_node()?)
            .execute(&mut *tx)
            .await?;
        Self::event(&mut tx, id, "instance.started", None, json!({"version": version})).await?;
        Self::enqueue_advance(&mut tx, id, &format!("adv:start:{id}")).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// The advance loop (docs/04 §4.2). Caller owns the transaction; the instance row lock serialises writers.
    pub async fn advance(&self, tx: &mut Tx<'_>, instance: Uuid) -> Result<()> {
        let row = sqlx::query("SELECT state, def_version, vars FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).fetch_one(&mut **tx).await?;
        let state: String = row.get(0);
        if state != "running" {
            return Ok(());
        }
        let ir = self.ir(tx, row.get(1)).await?;
        let vars: Value = row.get(2);
        for _ in 0..500 {
            let tok = sqlx::query("SELECT id, node_id, state, outcome, split_id, branch FROM rt.tokens WHERE instance_id = $1 AND state IN ('enter','resume') ORDER BY seq LIMIT 1")
                .bind(instance)
                .fetch_optional(&mut **tx)
                .await?;
            let Some(tok) = tok else { break };
            let (tid, node, tstate): (Uuid, String, String) = (tok.get(0), tok.get(1), tok.get(2));
            let outcome: Option<String> = tok.get(3);
            let split_id: Option<Uuid> = tok.get(4);
            let branch: Option<String> = tok.get(5);
            if tstate == "resume" {
                let on = outcome.unwrap_or_else(|| "next".into());
                let Some(to) = ir.next(&node, &on) else {
                    sqlx::query("UPDATE rt.instances SET state = 'failed' WHERE id = $1").bind(instance).execute(&mut **tx).await?;
                    Self::event(tx, instance, "incident", Some(&node), json!({"error": format!("no edge for outcome {on}")})).await?;
                    return Ok(());
                };
                sqlx::query("UPDATE rt.tokens SET node_id = $2, state = 'enter', outcome = NULL WHERE id = $1").bind(tid).bind(to).execute(&mut **tx).await?;
                continue;
            }
            let kind = ir.kind(&node)?.to_owned();
            let n = ir.node(&node)?.clone();
            match kind.as_str() {
                "start" | "set" | "notify" | "case.open" => {
                    if kind == "notify" {
                        sqlx::query("INSERT INTO rt.outbox (instance_id, topic, payload) VALUES ($1, 'notify', $2)").bind(instance).bind(json!({"node": node})).execute(&mut **tx).await?;
                    }
                    let out = if kind == "case.open" { if n["waitForClose"].as_bool() == Some(true) { "closed" } else { "opened" } } else { "next" };
                    sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = $2 WHERE id = $1").bind(tid).bind(out).execute(&mut **tx).await?;
                }
                "decision" => {
                    let chosen = n["branches"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|b| b["when"].as_str().is_some_and(|w| eval_condition(w, &vars)))
                        .and_then(|b| b["name"].as_str())
                        .unwrap_or("default")
                        .to_owned();
                    Self::event(tx, instance, "decision.evaluated", Some(&node), json!({"branch": chosen})).await?;
                    sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = $2 WHERE id = $1").bind(tid).bind(chosen).execute(&mut **tx).await?;
                }
                "end" => {
                    sqlx::query("UPDATE rt.tokens SET state = 'done' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                    if n["result"] == "terminated" {
                        self.cancel_live(tx, instance).await?;
                    }
                }
                "human.task" | "human.approval" | "human.sign" | "human.acknowledge" => {
                    let wi = Uuid::now_v7();
                    sqlx::query("INSERT INTO rt.work_items (id, instance_id, token_id, node_id, state, assignee) VALUES ($1, $2, $3, $4, 'offered', 'original')")
                        .bind(wi).bind(instance).bind(tid).bind(&node).execute(&mut **tx).await?;
                    sqlx::query("UPDATE rt.tokens SET state = 'waiting' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                    for esc in n["escalations"].as_array().into_iter().flatten() {
                        if esc["action"] == "reassign" {
                            let now = Self::now(tx).await?;
                            let due = now + chrono::TimeDelta::seconds(duration_secs(esc["at"].as_str().unwrap_or("PT0S"))?);
                            sqlx::query("INSERT INTO rt.timers (id, instance_id, token_id, purpose, due_at, payload) VALUES ($1, $2, $3, 'escalate', $4, $5)")
                                .bind(Uuid::now_v7()).bind(instance).bind(tid).bind(due).bind(json!({"work_item": wi})).execute(&mut **tx).await?;
                        }
                    }
                    Self::event(tx, instance, "work.offered", Some(&node), json!({"work_item": wi})).await?;
                }
                "wait.timer" => {
                    let now = Self::now(tx).await?;
                    let due = now + chrono::TimeDelta::seconds(duration_secs(n["after"].as_str().unwrap_or("PT0S"))?);
                    sqlx::query("INSERT INTO rt.timers (id, instance_id, token_id, purpose, due_at) VALUES ($1, $2, $3, 'wait', $4)")
                        .bind(Uuid::now_v7()).bind(instance).bind(tid).bind(due).execute(&mut **tx).await?;
                    sqlx::query("UPDATE rt.tokens SET state = 'waiting' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                }
                "wait.signal" => {
                    sqlx::query("UPDATE rt.tokens SET state = 'waiting' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                }
                "service.call" => {
                    let seq = Self::event(tx, instance, "service.requested", Some(&node), json!({})).await?;
                    let key = format!("svc:{instance}:{seq}");
                    sqlx::query("INSERT INTO rt.jobs (id, kind, instance_id, token_id, idem_key, payload) VALUES ($1, 'service_call', $2, $3, $4, $5)")
                        .bind(Uuid::now_v7()).bind(instance).bind(tid).bind(&key).bind(json!({"node": node})).execute(&mut **tx).await?;
                    sqlx::query("UPDATE rt.tokens SET state = 'waiting' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                }
                "parallel.split" => {
                    let outs = ir.outgoing(&node);
                    let sid = Uuid::now_v7();
                    sqlx::query("INSERT INTO rt.splits (id, instance_id, join_node, expected, parent_split) VALUES ($1, $2, $3, $4, $5)")
                        .bind(sid).bind(instance).bind(n["joinId"].as_str().unwrap_or_default()).bind(outs.len() as i32).bind(split_id).execute(&mut **tx).await?;
                    for (to, on) in outs {
                        sqlx::query("INSERT INTO rt.tokens (id, instance_id, node_id, state, split_id, branch) VALUES ($1, $2, $3, 'enter', $4, $5)")
                            .bind(Uuid::now_v7()).bind(instance).bind(to).bind(sid).bind(on).execute(&mut **tx).await?;
                    }
                    sqlx::query("UPDATE rt.tokens SET state = 'done' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                }
                "parallel.join" => {
                    let sid = split_id.ok_or_else(|| anyhow!("join without split"))?;
                    let s = sqlx::query("UPDATE rt.splits SET arrived = CASE WHEN $2 = ANY(arrived) THEN arrived ELSE array_append(arrived, $2) END WHERE id = $1 RETURNING cardinality(arrived), expected, released, parent_split")
                        .bind(sid).bind(branch.clone().unwrap_or_default()).fetch_one(&mut **tx).await?;
                    let (arrived, expected, released, parent): (i32, i32, bool, Option<Uuid>) = (s.get(0), s.get(1), s.get(2), s.get(3));
                    sqlx::query("UPDATE rt.tokens SET state = 'arrived' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                    if arrived == expected && !released {
                        sqlx::query("UPDATE rt.splits SET released = true WHERE id = $1").bind(sid).execute(&mut **tx).await?;
                        sqlx::query("UPDATE rt.tokens SET state = 'done' WHERE split_id = $1 AND state = 'arrived'").bind(sid).execute(&mut **tx).await?;
                        sqlx::query("INSERT INTO rt.tokens (id, instance_id, node_id, state, outcome, split_id) VALUES ($1, $2, $3, 'resume', 'next', $4)")
                            .bind(Uuid::now_v7()).bind(instance).bind(&node).bind(parent).execute(&mut **tx).await?;
                        Self::event(tx, instance, "join.released", Some(&node), json!({"split": sid})).await?;
                    }
                }
                "subprocess.call" => {
                    sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = 'completed' WHERE id = $1").bind(tid).execute(&mut **tx).await?;
                }
                other => bail!("spike interpreter does not support {other}"),
            }
        }
        let live: i64 = sqlx::query_scalar("SELECT count(*) FROM rt.tokens WHERE instance_id = $1 AND state IN ('enter','waiting','resume','arrived')")
            .bind(instance).fetch_one(&mut **tx).await?;
        if live == 0 {
            sqlx::query("UPDATE rt.instances SET state = 'completed', ended_at = now() WHERE id = $1").bind(instance).execute(&mut **tx).await?;
            Self::event(tx, instance, "instance.completed", None, json!({})).await?;
            sqlx::query("INSERT INTO rt.outbox (instance_id, topic, payload) VALUES ($1, 'instance.completed', '{}')").bind(instance).execute(&mut **tx).await?;
        }
        Ok(())
    }

    async fn cancel_live(&self, tx: &mut Tx<'_>, instance: Uuid) -> Result<()> {
        sqlx::query("UPDATE rt.tokens SET state = 'cancelled' WHERE instance_id = $1 AND state IN ('enter','waiting','resume','arrived')").bind(instance).execute(&mut **tx).await?;
        sqlx::query("UPDATE rt.work_items SET state = 'cancelled' WHERE instance_id = $1 AND state = 'offered'").bind(instance).execute(&mut **tx).await?;
        sqlx::query("UPDATE rt.timers SET state = 'cancelled' WHERE instance_id = $1 AND state IN ('scheduled','leased')").bind(instance).execute(&mut **tx).await?;
        sqlx::query("UPDATE rt.jobs SET state = 'cancelled' WHERE instance_id = $1 AND state IN ('queued','leased') AND kind <> 'advance'").bind(instance).execute(&mut **tx).await?;
        Ok(())
    }

    pub async fn cancel(&self, instance: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let state: String = sqlx::query_scalar("SELECT state FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).fetch_one(&mut *tx).await?;
        if state == "running" {
            self.cancel_live(&mut tx, instance).await?;
            sqlx::query("UPDATE rt.instances SET state = 'cancelled', ended_at = now() WHERE id = $1").bind(instance).execute(&mut *tx).await?;
            Self::event(&mut tx, instance, "instance.cancelled", None, json!({})).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Decide a work item (docs/04 §4.3): idempotency key, instance lock, state check, resume token.
    pub async fn decide(&self, work_item: Uuid, outcome: &str, actor: &str, idem: &str) -> Result<DecideResult> {
        let mut tx = self.pool.begin().await?;
        let hash = format!("{work_item}:{outcome}");
        let inserted = sqlx::query("INSERT INTO rt.idempotency (actor, key, request_hash) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(actor).bind(idem).bind(&hash).execute(&mut *tx).await?.rows_affected();
        if inserted == 0 {
            let prev: String = sqlx::query_scalar("SELECT request_hash FROM rt.idempotency WHERE actor = $1 AND key = $2").bind(actor).bind(idem).fetch_one(&mut *tx).await?;
            tx.rollback().await?;
            if prev == hash { return Ok(DecideResult::Replayed) }
            bail!("idempotency key reused with a different request");
        }
        let instance: Uuid = sqlx::query_scalar("SELECT instance_id FROM rt.work_items WHERE id = $1").bind(work_item).fetch_one(&mut *tx).await?;
        let inst_state: String = sqlx::query_scalar("SELECT state FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).fetch_one(&mut *tx).await?;
        let wi = sqlx::query("SELECT state, token_id, node_id FROM rt.work_items WHERE id = $1 FOR UPDATE").bind(work_item).fetch_one(&mut *tx).await?;
        let (wstate, token, node): (String, Uuid, String) = (wi.get(0), wi.get(1), wi.get(2));
        if wstate != "offered" || inst_state != "running" {
            tx.rollback().await?;
            return Ok(DecideResult::AlreadyDecided);
        }
        sqlx::query("UPDATE rt.work_items SET state = 'completed', outcome = $2, revision = revision + 1 WHERE id = $1").bind(work_item).bind(outcome).execute(&mut *tx).await?;
        sqlx::query("UPDATE rt.tokens SET state = 'resume', outcome = $2 WHERE id = $1 AND state = 'waiting'").bind(token).bind(outcome).execute(&mut *tx).await?;
        Self::event(&mut tx, instance, "work.completed", Some(&node), json!({"work_item": work_item, "outcome": outcome, "actor": actor})).await?;
        Self::enqueue_advance(&mut tx, instance, &format!("adv:wi:{work_item}")).await?;
        sqlx::query("UPDATE rt.idempotency SET response = $3 WHERE actor = $1 AND key = $2").bind(actor).bind(idem).bind(json!({"ok": true})).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(DecideResult::Committed)
    }

    /// Inbound webhook / signal with at-least-once delivery, deduplicated by message id (inbox).
    pub async fn deliver_signal(&self, instance: Uuid, message_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let fresh = sqlx::query("INSERT INTO rt.inbox_dedup VALUES ('signals', $1) ON CONFLICT DO NOTHING").bind(message_id).execute(&mut *tx).await?.rows_affected();
        if fresh == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query("SELECT 1 FROM rt.instances WHERE id = $1 FOR UPDATE").bind(instance).execute(&mut *tx).await?;
        let moved = sqlx::query("UPDATE rt.tokens t SET state = 'resume', outcome = 'received' FROM rt.definitions d, rt.instances i WHERE t.instance_id = $1 AND i.id = t.instance_id AND d.version = i.def_version AND t.state = 'waiting' AND EXISTS (SELECT 1 FROM jsonb_array_elements(d.ir->'nodes') n WHERE n->>'id' = t.node_id AND n->>'kind' = 'wait.signal')")
            .bind(instance).execute(&mut *tx).await?.rows_affected();
        if moved > 0 {
            Self::event(&mut tx, instance, "signal.received", None, json!({"message": message_id})).await?;
            Self::enqueue_advance(&mut tx, instance, &format!("adv:sig:{message_id}")).await?;
        }
        tx.commit().await?;
        Ok(moved > 0)
    }
}
