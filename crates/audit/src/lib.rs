//! Audit trail (BPMS-026).
//!
//! - **Per-workspace hash chain**: `hash_n = SHA-256(hash_{n-1} ‖ JCS(event_n))` (RFC 8785).
//! - **Same transaction as the business change** via [`append`] on a [`WsTx`] (ADR-0005).
//! - **High-volume categories** (`data_access`) go through [`enqueue`] + [`seal_pending`] (SP-10).
//! - **Append-only in the database**: no UPDATE/DELETE grant and an immutability trigger (AUD-07).
//! - **[`verify`]** recomputes the chain from stored columns and reports the first broken sequence.
//! - **[`explorer`]** reads the chain for people: filters, timelines, saved queries, exports (BPMS-064).

pub mod explorer;

use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

pub const HASH_VERSION: i16 = 1;
const GENESIS: [u8; 32] = [0u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Service,
    System,
    Guest,
    BreakGlass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Auth,
    Access,
    Config,
    Content,
    Runtime,
    File,
    DataAccess,
    Export,
    Admin,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Allowed,
    Denied,
    Error,
}

fn as_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// One accountability record (FR-T01). `diff` must already be redacted for restricted values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub actor_subject_ref: Option<Uuid>,
    pub actor_kind: ActorKind,
    pub on_behalf_of: Option<Uuid>,
    pub session_id_hash: Option<Vec<u8>>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub category: Category,
    pub action: String,
    pub object_type: Option<String>,
    pub object_id: Option<Uuid>,
    pub result: Outcome,
    pub reason: Option<String>,
    pub policy_ids: Vec<String>,
    pub diff: Option<Value>,
    pub trace_id: Option<String>,
}

impl AuditEvent {
    /// Minimal constructor; set the optional fields directly.
    pub fn new(
        actor: Option<Uuid>,
        actor_kind: ActorKind,
        category: Category,
        action: &str,
        result: Outcome,
    ) -> Self {
        Self {
            actor_subject_ref: actor,
            actor_kind,
            on_behalf_of: None,
            session_id_hash: None,
            ip: None,
            user_agent: None,
            category,
            action: action.to_owned(),
            object_type: None,
            object_id: None,
            result,
            reason: None,
            policy_ids: Vec::new(),
            diff: None,
            trace_id: None,
        }
    }
    pub fn object(mut self, kind: &str, id: Uuid) -> Self {
        self.object_type = Some(kind.to_owned());
        self.object_id = Some(id);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("canonicalisation failed: {0}")]
    Canonical(String),
}

impl From<serde_json::Error> for AuditError {
    fn from(e: serde_json::Error) -> Self {
        Self::Canonical(e.to_string())
    }
}

/// The exact fields that are hashed. Changing this requires a new HASH_VERSION (verifier keeps old ones).
fn canonical_bytes(
    ws: Uuid,
    seq: i64,
    id: Uuid,
    occurred_us: i64,
    e: &AuditEvent,
) -> Result<Vec<u8>, AuditError> {
    let doc = json!({
        "v": HASH_VERSION, "workspace_id": ws, "seq": seq, "id": id, "occurred_at_us": occurred_us,
        "actor_subject_ref": e.actor_subject_ref, "actor_kind": as_str(&e.actor_kind), "on_behalf_of": e.on_behalf_of,
        "session_id_hash": e.session_id_hash.as_ref().map(|h| h.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        "ip": e.ip, "user_agent": e.user_agent, "category": as_str(&e.category), "action": e.action,
        "object_type": e.object_type, "object_id": e.object_id, "result": as_str(&e.result), "reason": e.reason,
        "policy_ids": e.policy_ids, "diff": e.diff, "trace_id": e.trace_id,
    });
    serde_jcs::to_vec(&doc).map_err(|e| AuditError::Canonical(e.to_string()))
}

fn chain(prev: &[u8], canonical: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(canonical);
    h.finalize().to_vec()
}

/// Truncate to microseconds so the stored timestamptz round-trips exactly.
fn micros(t: OffsetDateTime) -> (OffsetDateTime, i64) {
    let us = (t.unix_timestamp_nanos() / 1_000) as i64;
    let t = OffsetDateTime::from_unix_timestamp_nanos(i128::from(us) * 1_000).unwrap_or(t);
    (t, us)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended {
    pub seq: i64,
    pub hash: Vec<u8>,
}

async fn lock_head(tx: &mut WsTx<'_>) -> Result<(i64, Vec<u8>), AuditError> {
    let ws = tx.ctx.workspace_id;
    sqlx::query("INSERT INTO audit_heads (workspace_id, last_seq, last_hash) VALUES ($1, 0, $2) ON CONFLICT DO NOTHING")
        .bind(ws).bind(&GENESIS[..]).execute(tx.conn()).await?;
    Ok(sqlx::query_as("SELECT last_seq, last_hash FROM audit_heads WHERE workspace_id = $1 FOR UPDATE")
        .bind(ws)
        .fetch_one(tx.conn())
        .await?)
}

async fn insert(
    tx: &mut WsTx<'_>,
    seq: i64,
    id: Uuid,
    at: OffsetDateTime,
    prev: &[u8],
    hash: &[u8],
    e: &AuditEvent,
) -> Result<(), AuditError> {
    let ws = tx.ctx.workspace_id;
    sqlx::query(
        "INSERT INTO audit_events (workspace_id, seq, id, occurred_at, actor_subject_ref, actor_kind, on_behalf_of, session_id_hash, ip, user_agent,
                                   category, action, object_type, object_id, result, reason, policy_ids, diff, trace_id, hash_v, prev_hash, hash)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::inet,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
    )
    .bind(ws).bind(seq).bind(id).bind(at).bind(e.actor_subject_ref).bind(as_str(&e.actor_kind)).bind(e.on_behalf_of)
    .bind(&e.session_id_hash).bind(&e.ip).bind(&e.user_agent).bind(as_str(&e.category)).bind(&e.action)
    .bind(&e.object_type).bind(e.object_id).bind(as_str(&e.result)).bind(&e.reason).bind(&e.policy_ids)
    .bind(&e.diff).bind(&e.trace_id).bind(HASH_VERSION).bind(prev).bind(hash)
    .execute(tx.conn())
    .await?;
    Ok(())
}

/// Appends `event` to the current workspace's chain inside the caller's transaction.
pub async fn append(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    event: &AuditEvent,
) -> Result<Appended, AuditError> {
    let (last_seq, prev) = lock_head(tx).await?;
    let seq = last_seq + 1;
    let id = ids.new_id();
    let (at, us) = micros(clock.now());
    let hash = chain(&prev, &canonical_bytes(tx.ctx.workspace_id, seq, id, us, event)?);
    insert(tx, seq, id, at, &prev, &hash, event).await?;
    sqlx::query("UPDATE audit_heads SET last_seq = $2, last_hash = $3 WHERE workspace_id = $1")
        .bind(tx.ctx.workspace_id)
        .bind(seq)
        .bind(&hash)
        .execute(tx.conn())
        .await?;
    Ok(Appended { seq, hash })
}

/// Buffers a high-volume event (no chain lock); [`seal_pending`] chains it shortly after.
pub async fn enqueue(tx: &mut WsTx<'_>, clock: &dyn Clock, event: &AuditEvent) -> Result<(), AuditError> {
    let (_, us) = micros(clock.now());
    let mut v = serde_json::to_value(event)?;
    v["occurred_at_us"] = json!(us);
    sqlx::query("INSERT INTO audit_pending (workspace_id, event) VALUES ($1, $2)")
        .bind(tx.ctx.workspace_id)
        .bind(v)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Moves up to `max` pending events of `ctx` into the chain in one transaction. Returns the count.
pub async fn seal_pending(db: &Db, ctx: WsCtx, ids: &dyn IdGen, max: i64) -> Result<usize, AuditError> {
    let mut tx = db.begin_ws(ctx).await?;
    let rows: Vec<(i64, Value)> =
        sqlx::query_as("SELECT id, event FROM audit_pending ORDER BY id LIMIT $1 FOR UPDATE SKIP LOCKED")
            .bind(max)
            .fetch_all(tx.conn())
            .await?;
    if rows.is_empty() {
        tx.rollback().await?;
        return Ok(0);
    }
    let (mut seq, mut prev) = lock_head(&mut tx).await?;
    let mut done = Vec::with_capacity(rows.len());
    for (pid, mut v) in rows {
        let us = v["occurred_at_us"].as_i64().unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("occurred_at_us");
        }
        let event: AuditEvent = serde_json::from_value(v)?;
        seq += 1;
        let id = ids.new_id();
        let at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(us) * 1_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let hash = chain(&prev, &canonical_bytes(ctx.workspace_id, seq, id, us, &event)?);
        insert(&mut tx, seq, id, at, &prev, &hash, &event).await?;
        prev = hash;
        done.push(pid);
    }
    sqlx::query("DELETE FROM audit_pending WHERE id = ANY($1)").bind(&done).execute(tx.conn()).await?;
    sqlx::query("UPDATE audit_heads SET last_seq = $2, last_hash = $3 WHERE workspace_id = $1")
        .bind(ctx.workspace_id)
        .bind(seq)
        .bind(&prev)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    Ok(done.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked: u64,
    /// First sequence whose hash, link or numbering does not verify.
    pub first_broken: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct Row {
    seq: i64,
    id: Uuid,
    occurred_at: OffsetDateTime,
    actor_subject_ref: Option<Uuid>,
    actor_kind: String,
    on_behalf_of: Option<Uuid>,
    session_id_hash: Option<Vec<u8>>,
    ip: Option<String>,
    user_agent: Option<String>,
    category: String,
    action: String,
    object_type: Option<String>,
    object_id: Option<Uuid>,
    result: String,
    reason: Option<String>,
    policy_ids: Vec<String>,
    diff: Option<Value>,
    trace_id: Option<String>,
    prev_hash: Vec<u8>,
    hash: Vec<u8>,
}

fn parse<T: for<'de> Deserialize<'de>>(s: &str) -> Option<T> {
    serde_json::from_value(Value::String(s.to_owned())).ok()
}

/// Recomputes the workspace chain from stored columns (AUD-05 logic; the offline CLI reuses it).
pub async fn verify(db: &Db, ctx: WsCtx) -> Result<VerifyReport, AuditError> {
    let mut tx = db.begin_ws(ctx).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT seq, id, occurred_at, actor_subject_ref, actor_kind, on_behalf_of, session_id_hash, host(ip) AS ip, user_agent, category, action,
                object_type, object_id, result, reason, policy_ids, diff, trace_id, prev_hash, hash
         FROM audit_events ORDER BY seq",
    )
    .fetch_all(tx.conn())
    .await?;
    tx.rollback().await?;
    let mut prev = GENESIS.to_vec();
    for (i, r) in rows.iter().enumerate() {
        let expected_seq = i as i64 + 1;
        let event = AuditEvent {
            actor_subject_ref: r.actor_subject_ref,
            actor_kind: parse(&r.actor_kind).unwrap_or(ActorKind::System),
            on_behalf_of: r.on_behalf_of,
            session_id_hash: r.session_id_hash.clone(),
            ip: r.ip.clone(),
            user_agent: r.user_agent.clone(),
            category: parse(&r.category).unwrap_or(Category::System),
            action: r.action.clone(),
            object_type: r.object_type.clone(),
            object_id: r.object_id,
            result: parse(&r.result).unwrap_or(Outcome::Error),
            reason: r.reason.clone(),
            policy_ids: r.policy_ids.clone(),
            diff: r.diff.clone(),
            trace_id: r.trace_id.clone(),
        };
        let (_, us) = micros(r.occurred_at);
        let ok = r.seq == expected_seq
            && r.prev_hash == prev
            && chain(&prev, &canonical_bytes(ctx.workspace_id, r.seq, r.id, us, &event)?) == r.hash;
        if !ok {
            return Ok(VerifyReport { checked: i as u64, first_broken: Some(r.seq) });
        }
        prev = r.hash.clone();
    }
    Ok(VerifyReport { checked: rows.len() as u64, first_broken: None })
}
