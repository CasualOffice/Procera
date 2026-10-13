//! Audit explorer API (BPMS-064, FR-T03/T04, docs/08 "Audit", docs/05 §3.19).
//!
//! - Reads need `audit:view` (the whole workspace chain) or `audit:view_own` (only events where the
//!   caller is the actor); verification needs `audit:view`.
//! - Saved queries are private to the member and audited.
//! - Exports need `audit:view` + `audit:export` and a recent authentication (docs/12 §11), and are
//!   themselves audited (`audit.exported`, FR-T01).
//! - Reading the explorer is not logged per query: audit events hold no restricted values (diffs are
//!   redacted when written), and `data_access` events are for restricted/confidential reads
//!   (docs/06 §7, docs/24 §3.5). Exports, which take data out, are always recorded.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_audit::explorer::{self, EXPORT_ROW_CAP, ExplorerError, ExportFormat, Filter, MAX_PAGE, Who};
use procera_db::WsTx;
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audit/events", get(events))
        .route("/audit/objects/{object_type}/{id}/timeline", get(object_timeline))
        .route("/audit/verify", get(verify_chain))
        .route("/audit/saved-queries", get(list_saved).post(save))
        .route("/audit/saved-queries/{id}", delete(delete_saved))
        .route("/audit/exports", get(list_exports).post(export))
        .route("/audit/exports/{id}", get(get_export))
        .route("/audit/query", post(run_query))
}

impl From<ExplorerError> for ApiError {
    fn from(e: ExplorerError) -> Self {
        match e {
            ExplorerError::NotFound => Self::not_found(),
            ExplorerError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            ExplorerError::Conflict(m) => Self::new(409, "conflict", "Conflict").detail(m),
            ExplorerError::Internal(m) => {
                tracing::error!(error = %m, "audit explorer internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "audit api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn who(me: &WsMember, headers: &HeaderMap) -> Who {
    let c = client_info(headers);
    Who {
        membership: me.actor.membership,
        subject_ref: me.actor.subject_ref,
        ip: c.ip,
        user_agent: c.user_agent,
    }
}

/// `None` = the whole workspace chain (`audit:view`); `Some(subject)` = only the caller's own
/// events (`audit:view_own`).
async fn read_scope(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>) -> Result<Option<Uuid>, ApiError> {
    let access = members(s)?.access();
    if access.check(tx, me.actor.membership, "audit:view", &Resource::Workspace).await?.allowed {
        return Ok(None);
    }
    if access.check(tx, me.actor.membership, "audit:view_own", &Resource::Workspace).await?.allowed {
        return Ok(Some(me.actor.subject_ref));
    }
    Err(ApiError::new(403, "forbidden", "You do not have permission for this action").detail("audit:view"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
    /// Actor subject reference.
    #[serde(default)]
    actor: Option<Uuid>,
    /// Membership id, resolved to its subject (alternative to `actor`).
    #[serde(default)]
    member: Option<Uuid>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    object_type: Option<String>,
    #[serde(default)]
    object_id: Option<Uuid>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default, rename = "page[size]")]
    size: Option<i64>,
    #[serde(default, rename = "page[cursor]")]
    cursor: Option<String>,
}

/// `GET /audit/events`: audit events filtered by time, `actor` or `member`, action, category, object and
/// result, in keyset pages (`page[size]`, `page[cursor]`). `audit:view` sees the workspace chain;
/// `audit:view_own` sees only the caller's own events.
async fn events(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<EventsQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let own = read_scope(&s, &me, &mut tx).await?;
    let actor = match (q.actor, q.member) {
        (Some(_), Some(_)) => {
            return Err(ExplorerError::Invalid("use either actor or member, not both".into()).into());
        }
        (None, Some(membership)) => match explorer::member_subject(&mut tx, membership).await? {
            Some(subject) => Some(subject),
            None => return Ok(Json(json!({"items": [], "next_cursor": null})).into_response()),
        },
        (actor, None) => actor,
    };
    let f = Filter {
        from: q.from,
        to: q.to,
        actor,
        action: q.action,
        category: q.category,
        object_type: q.object_type,
        object_id: q.object_id,
        result: q.result,
    };
    let cursor = explorer::parse_cursor(q.cursor.as_deref())?;
    let page = explorer::query(&mut tx, &f, own, cursor, q.size.unwrap_or(50).clamp(1, MAX_PAGE)).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(page).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimelineQuery {
    #[serde(default, rename = "page[cursor]")]
    cursor: Option<String>,
}

/// Every event for one object, oldest first. `artifact` spans all of the artifact's versions
/// (the library records lifecycle events per version).
async fn object_timeline(
    State(s): State<AppState>,
    me: WsMember,
    Path((object_type, id)): Path<(String, Uuid)>,
    Query(q): Query<TimelineQuery>,
) -> Result<Response, ApiError> {
    if object_type.is_empty() || object_type.len() > 100 {
        return Err(ExplorerError::Invalid("object type must be 1-100 characters".into()).into());
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let own = read_scope(&s, &me, &mut tx).await?;
    let after = explorer::parse_cursor(q.cursor.as_deref())?;
    let page = if object_type == "artifact" {
        // An artifact's history: its own events (governance, change requests are separate objects)
        // and every event on its versions.
        let versions: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM artifact_versions WHERE artifact_id = $1 ORDER BY version_no")
                .bind(id)
                .fetch_all(tx.conn())
                .await
                .map_err(internal)?;
        let own_id = [id];
        explorer::timeline_of(&mut tx, &[("artifact", &own_id), ("artifact_version", &versions)], own, after)
            .await?
    } else {
        explorer::timeline(&mut tx, &object_type, &[id], own, after).await?
    };
    tx.rollback().await.map_err(internal)?;
    Ok(Json(page).into_response())
}

/// Recomputes the workspace chain and reports the first sequence that does not verify.
async fn verify_chain(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "audit:view", Resource::Workspace).await?;
    tx.rollback().await.map_err(internal)?;
    let r = procera_audit::verify(m.db(), me.ctx).await.map_err(internal)?;
    Ok(Json(json!({"ok": r.first_broken.is_none(), "checked": r.checked, "first_broken": r.first_broken}))
        .into_response())
}

/// `GET /audit/saved-queries`: the caller's saved audit queries. Needs `audit:view` or `audit:view_own`.
async fn list_saved(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    read_scope(&s, &me, &mut tx).await?;
    let items = explorer::saved_queries(&mut tx, me.actor.membership).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveReq {
    name: String,
    #[serde(default)]
    filters: Filter,
}

/// `POST /audit/saved-queries`: saves a named set of audit `filters` for the caller; 201. Needs `audit:view`
/// or `audit:view_own`.
async fn save(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<SaveReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    read_scope(&s, &me, &mut tx).await?;
    let ids = procera_kernel::UuidV7Gen;
    let q = explorer::save_query(&mut tx, s.clock.as_ref(), &ids, &who(&me, &headers), &r.name, &r.filters)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(q)).into_response())
}

/// `DELETE /audit/saved-queries/{id}`: deletes one of the caller's saved audit queries; 204 (404 if not theirs).
/// Needs `audit:view` or `audit:view_own`.
async fn delete_saved(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    read_scope(&s, &me, &mut tx).await?;
    explorer::delete_saved_query(
        &mut tx,
        s.clock.as_ref(),
        &procera_kernel::UuidV7Gen,
        &who(&me, &headers),
        id,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportReq {
    #[serde(default = "csv")]
    format: ExportFormat,
    #[serde(default)]
    filters: Filter,
    /// Run as a background job (no 10 000-row cap); returns 202 with the job.
    #[serde(default)]
    background: bool,
}

fn csv() -> ExportFormat {
    ExportFormat::Csv
}

/// Synchronous export of up to 10 000 events (newest first). `Procera-Export-Truncated: true`
/// means more matched; narrow the filters. Larger exports move to the export job later.
async fn export(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<ExportReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "audit:view", Resource::Workspace).await?;
    me.require(m, &mut tx, "audit:export", Resource::Workspace).await?;
    me.require_step_up(&s, &mut tx).await?;
    if r.background {
        r.filters.validate()?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO audit_export_jobs (workspace_id, id, requested_by, subject_ref, own_only, filters, format, created_at) VALUES ($1, $2, $3, $4, false, $5, $6, $7)")
            .bind(tx.ctx.workspace_id).bind(id).bind(me.actor.membership).bind(me.actor.subject_ref)
            .bind(serde_json::to_value(&r.filters).map_err(internal)?)
            .bind(if r.format == ExportFormat::Jsonl { "jsonl" } else { "csv" }).bind(s.clock.now())
            .execute(tx.conn()).await.map_err(internal)?;
        m.audit_object(
            &mut tx,
            Some(me.actor.subject_ref),
            "audit.export_requested",
            "audit_export",
            id,
            json!({"format": r.format, "filters": r.filters}),
            &super::auth::client_info(&headers),
        )
        .await?;
        tx.commit().await.map_err(internal)?;
        return Ok((StatusCode::ACCEPTED, Json(json!({"id": id, "state": "queued"}))).into_response());
    }
    let out = explorer::export(
        &mut tx,
        s.clock.as_ref(),
        &procera_kernel::UuidV7Gen,
        &who(&me, &headers),
        &r.filters,
        r.format,
        EXPORT_ROW_CAP,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    let (ctype, ext) = match r.format {
        ExportFormat::Csv => ("text/csv; charset=utf-8", "csv"),
        ExportFormat::Jsonl => ("application/x-ndjson", "jsonl"),
    };
    let stamp = s.clock.now().unix_timestamp();
    let mut res = (StatusCode::OK, out.body).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"audit-{stamp}.{ext}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = HeaderValue::from_str(&out.rows.to_string()) {
        h.insert("procera-export-rows", v);
    }
    h.insert(
        "procera-export-truncated",
        HeaderValue::from_static(if out.truncated { "true" } else { "false" }),
    );
    Ok(res)
}

// ---- BPMS-064: background export jobs and the query language ----

type JobRow = (
    Uuid,
    String,
    String,
    Option<i64>,
    Option<Uuid>,
    Option<String>,
    time::OffsetDateTime,
    Option<time::OffsetDateTime>,
);

fn job_json(j: &JobRow) -> Value {
    let fmt = |t: &time::OffsetDateTime| {
        t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
    };
    json!({"id": j.0, "format": j.1, "state": j.2, "rows": j.3, "file_id": j.4, "error": j.5, "created_at": fmt(&j.6), "finished_at": j.7.as_ref().map(fmt)})
}

/// The caller's background exports, newest first.
async fn list_exports(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "audit:export", Resource::Workspace).await?;
    let rows: Vec<JobRow> = sqlx::query_as("SELECT id, format, state, rows, file_id, error, created_at, finished_at FROM audit_export_jobs WHERE requested_by = $1 ORDER BY created_at DESC LIMIT 50")
        .bind(me.actor.membership).fetch_all(tx.conn()).await.map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": rows.iter().map(job_json).collect::<Vec<_>>() })))
}

/// `GET /audit/exports/{id}`: the state of one of the caller's background audit exports (rows, file,
/// error). Needs `audit:export`; 404 for anyone else's.
async fn get_export(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "audit:export", Resource::Workspace).await?;
    let row: Option<JobRow> = sqlx::query_as("SELECT id, format, state, rows, file_id, error, created_at, finished_at FROM audit_export_jobs WHERE id = $1 AND requested_by = $2")
        .bind(id).bind(me.actor.membership).fetch_optional(tx.conn()).await.map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    row.map(|r| Json(job_json(&r))).ok_or_else(ApiError::not_found)
}

/// `POST /audit/query` (docs/08 §7.5): AND/OR filter tree, sorted on the chain, keyset pages.
async fn run_query(
    State(s): State<AppState>,
    me: WsMember,
    Json(q): Json<procera_audit::query::QueryReq>,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let own = read_scope(&s, &me, &mut tx).await?;
    let page = procera_audit::query::run(&mut tx, &q, own, s.clock.now()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(serde_json::to_value(page).map_err(internal)?))
}
