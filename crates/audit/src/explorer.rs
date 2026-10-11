//! Audit explorer (BPMS-064, FR-T03/T04, docs/05 §3.19): filtered event queries with keyset
//! pagination, object timelines, per-member saved queries and capped exports.
//!
//! - Queries are built with bound parameters only (`QueryBuilder`) and run under forced RLS, so a
//!   caller only ever sees its own workspace chain.
//! - `own` restricts results to events where the given subject is the actor (`audit:view_own`).
//! - Actor names are resolved only for members of the workspace (a subject from elsewhere, such as
//!   a system administrator, keeps its pseudonymous reference).
//! - Saving or deleting a query, and every export, appends an audit event in the same transaction.
//!
//! Permissions and step-up are checked by the caller (HTTP layer).

use procera_db::{DbError, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::{ActorKind, AuditError, AuditEvent, Category, Outcome, append};

pub const MAX_PAGE: i64 = 200;
pub const TIMELINE_PAGE: i64 = 500;
/// Synchronous exports stop here and say so (`truncated`); larger exports become jobs later.
pub const EXPORT_ROW_CAP: i64 = 10_000;
pub const MAX_SAVED_QUERIES: i64 = 100;

const CATEGORIES: &[&str] =
    &["auth", "access", "config", "content", "runtime", "file", "data_access", "export", "admin", "system"];
const RESULTS: &[&str] = &["allowed", "denied", "error"];

#[derive(Debug, thiserror::Error)]
pub enum ExplorerError {
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for ExplorerError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<DbError> for ExplorerError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<AuditError> for ExplorerError {
    fn from(e: AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}

/// Explorer filters. Also the stored shape of a saved query, so unknown fields are rejected.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    /// Inclusive lower bound on `occurred_at`.
    #[serde(default, with = "time::serde::rfc3339::option", skip_serializing_if = "Option::is_none")]
    pub from: Option<OffsetDateTime>,
    /// Exclusive upper bound on `occurred_at`.
    #[serde(default, with = "time::serde::rfc3339::option", skip_serializing_if = "Option::is_none")]
    pub to: Option<OffsetDateTime>,
    /// Actor subject reference (pseudonymous id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Uuid>,
    /// Action prefix, e.g. `member.` or `version.published`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

impl Filter {
    pub fn validate(&self) -> Result<(), ExplorerError> {
        if let Some(c) = &self.category
            && !CATEGORIES.contains(&c.as_str())
        {
            return Err(ExplorerError::Invalid(format!("category must be one of {}", CATEGORIES.join(", "))));
        }
        if let Some(r) = &self.result
            && !RESULTS.contains(&r.as_str())
        {
            return Err(ExplorerError::Invalid(format!("result must be one of {}", RESULTS.join(", "))));
        }
        for (name, v) in [("action", &self.action), ("object_type", &self.object_type)] {
            if v.as_ref().is_some_and(|s| s.is_empty() || s.len() > 100 || s.chars().any(char::is_control)) {
                return Err(ExplorerError::Invalid(format!("{name} must be 1-100 characters")));
            }
        }
        if let (Some(f), Some(t)) = (self.from, self.to)
            && f >= t
        {
            return Err(ExplorerError::Invalid("from must be before to".into()));
        }
        Ok(())
    }
}

/// One audit event as the explorer shows it (diffs are redacted at write time).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventView {
    pub seq: i64,
    pub id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
    pub actor_subject_ref: Option<Uuid>,
    pub actor_kind: String,
    /// Display name when the actor is (or was) a member of this workspace.
    pub actor_name: Option<String>,
    pub on_behalf_of: Option<Uuid>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub category: String,
    pub action: String,
    pub object_type: Option<String>,
    pub object_id: Option<Uuid>,
    pub result: String,
    pub reason: Option<String>,
    pub policy_ids: Vec<String>,
    pub diff: Option<Value>,
    pub trace_id: Option<String>,
    /// Hex chain hash of this event.
    pub hash: String,
}

#[derive(Debug, sqlx::FromRow)]
struct Raw {
    seq: i64,
    id: Uuid,
    occurred_at: OffsetDateTime,
    actor_subject_ref: Option<Uuid>,
    actor_kind: String,
    actor_name: Option<String>,
    on_behalf_of: Option<Uuid>,
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
    hash: Vec<u8>,
}

impl From<Raw> for EventView {
    fn from(r: Raw) -> Self {
        Self {
            seq: r.seq,
            id: r.id,
            occurred_at: r.occurred_at,
            actor_subject_ref: r.actor_subject_ref,
            actor_kind: r.actor_kind,
            actor_name: r.actor_name,
            on_behalf_of: r.on_behalf_of,
            ip: r.ip,
            user_agent: r.user_agent,
            category: r.category,
            action: r.action,
            object_type: r.object_type,
            object_id: r.object_id,
            result: r.result,
            reason: r.reason,
            policy_ids: r.policy_ids,
            diff: r.diff,
            trace_id: r.trace_id,
            hash: r.hash.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}

const COLS: &str = "SELECT e.seq, e.id, e.occurred_at, e.actor_subject_ref, e.actor_kind,
    (SELECT u.display_name FROM users u JOIN memberships m ON m.user_id = u.id WHERE u.subject_ref = e.actor_subject_ref LIMIT 1) AS actor_name,
    e.on_behalf_of, host(e.ip) AS ip, e.user_agent, e.category, e.action, e.object_type, e.object_id, e.result,
    e.reason, e.policy_ids, e.diff, e.trace_id, e.hash
  FROM audit_events e WHERE true";

fn like_prefix(p: &str) -> String {
    format!("{}%", p.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

/// A page of events, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventPage {
    pub items: Vec<EventView>,
    /// Pass back as the cursor for the next (older) page; `None` on the last page.
    pub next_cursor: Option<String>,
}

pub fn parse_cursor(c: Option<&str>) -> Result<Option<i64>, ExplorerError> {
    match c.map(str::trim).filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) => c
            .parse::<i64>()
            .ok()
            .filter(|s| *s > 0)
            .map(Some)
            .ok_or_else(|| ExplorerError::Invalid("invalid cursor".into())),
    }
}

/// Events matching `f`, newest first, keyset-paginated on the chain sequence.
pub async fn query(
    tx: &mut WsTx<'_>,
    f: &Filter,
    own: Option<Uuid>,
    before_seq: Option<i64>,
    limit: i64,
) -> Result<EventPage, ExplorerError> {
    f.validate()?;
    let limit = limit.clamp(1, EXPORT_ROW_CAP);
    let mut qb = QueryBuilder::<Postgres>::new(COLS);
    if let Some(v) = f.from {
        qb.push(" AND e.occurred_at >= ").push_bind(v);
    }
    if let Some(v) = f.to {
        qb.push(" AND e.occurred_at < ").push_bind(v);
    }
    if let Some(v) = f.actor {
        qb.push(" AND e.actor_subject_ref = ").push_bind(v);
    }
    if let Some(v) = own {
        qb.push(" AND e.actor_subject_ref = ").push_bind(v);
    }
    if let Some(v) = &f.action {
        qb.push(" AND e.action LIKE ").push_bind(like_prefix(v));
    }
    if let Some(v) = &f.category {
        qb.push(" AND e.category = ").push_bind(v.clone());
    }
    if let Some(v) = &f.object_type {
        qb.push(" AND e.object_type = ").push_bind(v.clone());
    }
    if let Some(v) = f.object_id {
        qb.push(" AND e.object_id = ").push_bind(v);
    }
    if let Some(v) = &f.result {
        qb.push(" AND e.result = ").push_bind(v.clone());
    }
    if let Some(v) = before_seq {
        qb.push(" AND e.seq < ").push_bind(v);
    }
    qb.push(" ORDER BY e.seq DESC LIMIT ").push_bind(limit + 1);
    let mut rows: Vec<Raw> = qb.build_query_as().fetch_all(tx.conn()).await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = if more { rows.last().map(|r| r.seq.to_string()) } else { None };
    Ok(EventPage { items: rows.into_iter().map(EventView::from).collect(), next_cursor })
}

/// Every event about the given objects (one logical object may span several ids, e.g. an
/// artifact's versions), oldest first, keyset-paginated with `after_seq`.
pub async fn timeline(
    tx: &mut WsTx<'_>,
    object_type: &str,
    object_ids: &[Uuid],
    own: Option<Uuid>,
    after_seq: Option<i64>,
) -> Result<EventPage, ExplorerError> {
    if object_ids.is_empty() {
        return Ok(EventPage { items: vec![], next_cursor: None });
    }
    let mut qb = QueryBuilder::<Postgres>::new(COLS);
    qb.push(" AND e.object_type = ").push_bind(object_type.to_owned());
    qb.push(" AND e.object_id = ANY(").push_bind(object_ids.to_vec()).push(")");
    if let Some(v) = own {
        qb.push(" AND e.actor_subject_ref = ").push_bind(v);
    }
    if let Some(v) = after_seq {
        qb.push(" AND e.seq > ").push_bind(v);
    }
    qb.push(" ORDER BY e.seq ASC LIMIT ").push_bind(TIMELINE_PAGE + 1);
    let mut rows: Vec<Raw> = qb.build_query_as().fetch_all(tx.conn()).await?;
    let more = rows.len() as i64 > TIMELINE_PAGE;
    rows.truncate(TIMELINE_PAGE as usize);
    let next_cursor = if more { rows.last().map(|r| r.seq.to_string()) } else { None };
    Ok(EventPage { items: rows.into_iter().map(EventView::from).collect(), next_cursor })
}

/// The audit subject of a membership in this workspace (for "everything this member did").
pub async fn member_subject(tx: &mut WsTx<'_>, membership: Uuid) -> Result<Option<Uuid>, ExplorerError> {
    Ok(sqlx::query_scalar(
        "SELECT u.subject_ref FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
    )
    .bind(membership)
    .fetch_optional(tx.conn())
    .await?)
}

/// Who acts on the explorer, for its own audit events.
#[derive(Debug, Clone, Default)]
pub struct Who {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl Who {
    fn event(&self, category: Category, action: &str) -> AuditEvent {
        let mut ev =
            AuditEvent::new(Some(self.subject_ref), ActorKind::User, category, action, Outcome::Allowed);
        ev.ip = self.ip.clone();
        ev.user_agent = self.user_agent.clone();
        ev
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedQuery {
    pub id: Uuid,
    pub name: String,
    pub filters: Filter,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

type SavedRow = (Uuid, String, Value, OffsetDateTime);

fn saved(r: SavedRow) -> Result<SavedQuery, ExplorerError> {
    let filters = serde_json::from_value(r.2).map_err(|e| ExplorerError::Internal(e.to_string()))?;
    Ok(SavedQuery { id: r.0, name: r.1, filters, created_at: r.3 })
}

/// The member's own saved queries, by name.
pub async fn saved_queries(tx: &mut WsTx<'_>, membership: Uuid) -> Result<Vec<SavedQuery>, ExplorerError> {
    let rows: Vec<SavedRow> = sqlx::query_as(
        "SELECT id, name, filters, created_at FROM audit_saved_queries WHERE membership_id = $1 ORDER BY lower(name), id",
    )
    .bind(membership)
    .fetch_all(tx.conn())
    .await?;
    rows.into_iter().map(saved).collect()
}

/// Saves a named query for the member (names are unique per member, case-insensitively).
pub async fn save_query(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    who: &Who,
    name: &str,
    filters: &Filter,
) -> Result<SavedQuery, ExplorerError> {
    filters.validate()?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err(ExplorerError::Invalid("name must be 1-120 characters".into()));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_saved_queries WHERE membership_id = $1")
        .bind(who.membership)
        .fetch_one(tx.conn())
        .await?;
    if count >= MAX_SAVED_QUERIES {
        return Err(ExplorerError::Conflict(format!("at most {MAX_SAVED_QUERIES} saved queries per member")));
    }
    // Checked first so the usual duplicate doesn't abort the caller's transaction; the unique index
    // still decides races.
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM audit_saved_queries WHERE membership_id = $1 AND lower(name) = lower($2))",
    )
    .bind(who.membership)
    .bind(name)
    .fetch_one(tx.conn())
    .await?;
    if taken {
        return Err(ExplorerError::Conflict("a saved query with this name exists".into()));
    }
    let (id, now) = (ids.new_id(), clock.now());
    let doc = serde_json::to_value(filters).map_err(|e| ExplorerError::Internal(e.to_string()))?;
    let inserted = sqlx::query("INSERT INTO audit_saved_queries (workspace_id, id, membership_id, name, filters, created_at) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(tx.ctx.workspace_id).bind(id).bind(who.membership).bind(name).bind(&doc).bind(now)
        .execute(tx.conn()).await;
    if let Err(e) = inserted {
        return Err(match e.as_database_error().and_then(|d| d.code()) {
            Some(c) if c == "23505" => ExplorerError::Conflict("a saved query with this name exists".into()),
            _ => e.into(),
        });
    }
    let mut ev = who.event(Category::Admin, "audit.query_saved").object("audit_saved_query", id);
    ev.diff = Some(json!({"name": name, "filters": doc}));
    append(tx, clock, ids, &ev).await?;
    let r: SavedRow =
        sqlx::query_as("SELECT id, name, filters, created_at FROM audit_saved_queries WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
    saved(r)
}

/// Deletes one of the member's own saved queries (someone else's is not found).
pub async fn delete_saved_query(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    who: &Who,
    id: Uuid,
) -> Result<(), ExplorerError> {
    let name: Option<String> = sqlx::query_scalar(
        "DELETE FROM audit_saved_queries WHERE id = $1 AND membership_id = $2 RETURNING name",
    )
    .bind(id)
    .bind(who.membership)
    .fetch_optional(tx.conn())
    .await?;
    let name = name.ok_or(ExplorerError::NotFound)?;
    let mut ev = who.event(Category::Admin, "audit.query_deleted").object("audit_saved_query", id);
    ev.diff = Some(json!({"name": name}));
    append(tx, clock, ids, &ev).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Jsonl,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub body: String,
    pub rows: usize,
    /// More events matched than the row cap; narrow the filters.
    pub truncated: bool,
}

const CSV_HEADER: &[&str] = &[
    "seq",
    "occurred_at",
    "actor_subject_ref",
    "actor_name",
    "actor_kind",
    "on_behalf_of",
    "ip",
    "user_agent",
    "category",
    "action",
    "object_type",
    "object_id",
    "result",
    "reason",
    "policy_ids",
    "diff",
    "trace_id",
    "hash",
];

/// One CSV cell (RFC 4180 quoting). Cells that a spreadsheet would evaluate as a formula are
/// prefixed with an apostrophe (CSV injection, OWASP).
pub fn csv_cell(v: &str) -> String {
    let v = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{v}") } else { v.to_owned() };
    if v.contains([',', '"', '\n', '\r']) || v.starts_with(' ') || v.ends_with(' ') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

fn csv_row(e: &EventView) -> String {
    let opt = |u: &Option<Uuid>| u.map(|u| u.to_string()).unwrap_or_default();
    let s = |v: &Option<String>| v.clone().unwrap_or_default();
    let cells = [
        e.seq.to_string(),
        e.occurred_at.format(&Rfc3339).unwrap_or_default(),
        opt(&e.actor_subject_ref),
        s(&e.actor_name),
        e.actor_kind.clone(),
        opt(&e.on_behalf_of),
        s(&e.ip),
        s(&e.user_agent),
        e.category.clone(),
        e.action.clone(),
        s(&e.object_type),
        opt(&e.object_id),
        e.result.clone(),
        s(&e.reason),
        e.policy_ids.join(";"),
        e.diff.as_ref().map(Value::to_string).unwrap_or_default(),
        s(&e.trace_id),
        e.hash.clone(),
    ];
    cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",")
}

/// Exports the events matching `f` (newest first, at most `cap`) and records the export in the
/// audit trail (FR-T01 "exports") in the caller's transaction.
pub async fn export(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    who: &Who,
    f: &Filter,
    format: ExportFormat,
    cap: i64,
) -> Result<Export, ExplorerError> {
    let page = query(tx, f, None, None, cap).await?;
    let truncated = page.next_cursor.is_some();
    let mut body = String::new();
    match format {
        ExportFormat::Csv => {
            body.push_str(&CSV_HEADER.join(","));
            body.push_str("\r\n");
            for e in &page.items {
                body.push_str(&csv_row(e));
                body.push_str("\r\n");
            }
        }
        ExportFormat::Jsonl => {
            for e in &page.items {
                body.push_str(&serde_json::to_string(e).map_err(|e| ExplorerError::Internal(e.to_string()))?);
                body.push('\n');
            }
        }
    }
    let mut ev = who.event(Category::Export, "audit.exported");
    ev.diff = Some(json!({
        "format": format,
        "rows": page.items.len(),
        "truncated": truncated,
        "filters": serde_json::to_value(f).map_err(|e| ExplorerError::Internal(e.to_string()))?,
    }));
    append(tx, clock, ids, &ev).await?;
    Ok(Export { body, rows: page.items.len(), truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_are_quoted_and_formula_safe() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_cell("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(csv_cell("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn filters_validate_and_reject_unknown_fields() {
        assert!(Filter { category: Some("auth".into()), ..Filter::default() }.validate().is_ok());
        assert!(Filter { category: Some("nope".into()), ..Filter::default() }.validate().is_err());
        assert!(Filter { result: Some("maybe".into()), ..Filter::default() }.validate().is_err());
        assert!(Filter { action: Some(String::new()), ..Filter::default() }.validate().is_err());
        let t = OffsetDateTime::UNIX_EPOCH;
        assert!(Filter { from: Some(t), to: Some(t), ..Filter::default() }.validate().is_err());
        assert!(serde_json::from_value::<Filter>(json!({"category": "auth", "extra": 1})).is_err());
        let f: Filter = serde_json::from_value(json!({"from": "2026-10-01T00:00:00Z", "action": "member."}))
            .unwrap_or_default();
        assert_eq!(f.action.as_deref(), Some("member."));
        assert_eq!(
            serde_json::to_value(&f).ok(),
            Some(json!({"from": "2026-10-01T00:00:00Z", "action": "member."}))
        );
    }

    #[test]
    fn cursors_and_prefixes() {
        assert_eq!(parse_cursor(None).ok(), Some(None));
        assert_eq!(parse_cursor(Some("42")).ok(), Some(Some(42)));
        assert!(parse_cursor(Some("-1")).is_err());
        assert!(parse_cursor(Some("abc")).is_err());
        assert_eq!(like_prefix("a_b%"), "a\\_b\\%%");
    }
}
