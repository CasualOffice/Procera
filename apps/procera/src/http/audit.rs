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
use serde_json::json;
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
        .route("/audit/exports", post(export))
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
    let (kind, ids) = if object_type == "artifact" {
        let versions: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM artifact_versions WHERE artifact_id = $1 ORDER BY version_no")
                .bind(id)
                .fetch_all(tx.conn())
                .await
                .map_err(internal)?;
        ("artifact_version".to_owned(), versions)
    } else {
        (object_type, vec![id])
    };
    let after = explorer::parse_cursor(q.cursor.as_deref())?;
    let page = explorer::timeline(&mut tx, &kind, &ids, own, after).await?;
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
