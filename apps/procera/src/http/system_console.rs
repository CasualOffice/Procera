//! System console reads and quota defaults (BPMS-065; FR-S04, S08, S09; docs/05 §3.20, docs/08 §5).
//!
//! - Platform health: database, cache, outbox backlog and dispatcher lag, mail queue, version and
//!   schema migrations. Totals and timestamps only.
//! - System audit: the system-plane chain (workspace lifecycle, system roles, platform settings,
//!   sign-ins) through the shared explorer. Workspace chains are not reachable from here.
//! - Quota defaults: the bundle each configuration profile starts with (docs/20 §3).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use procera_audit::explorer::{self, Filter, MAX_PAGE};
use procera_platform::Quotas;
use procera_platform::authz::SystemAction;
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::system::{SystemUser, etag, if_match, platform};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/quota-defaults", get(quota_defaults))
        .route("/quota-defaults/{profile}", put(set_quota_defaults))
        .route("/health", get(health))
        .route("/system-audit/events", get(audit_events))
        .route("/system-audit/verify", get(audit_verify))
}

/// The outbox counts as lagging when its oldest pending event is older than this.
pub const OUTBOX_LAG_DEGRADED_SECONDS: i64 = 300;

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "system console internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn db(s: &AppState) -> Result<&procera_db::Db, ApiError> {
    s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

/// `GET /quota-defaults`: the quota bundle each configuration profile starts with. Any system role.
async fn quota_defaults(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    Ok(Json(json!({"items": platform(&s)?.quota_defaults().await?})).into_response())
}

/// `PUT /quota-defaults/{profile}`: replaces one profile's default quotas for new workspaces.
/// Needs `system.owner` or `system.admin` and a recent sign-in; `If-Match` honoured (412).
async fn set_quota_defaults(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(profile): Path<String>,
    Json(q): Json<Quotas>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfig)?;
    u.require_step_up(&s)?;
    let d = platform(&s)?
        .set_quota_defaults(u.user_id, &profile, if_match(&headers), &q, &client_info(&headers))
        .await?;
    let revision = d.revision;
    Ok(etag((StatusCode::OK, Json(d)).into_response(), revision))
}

fn age_seconds(now: OffsetDateTime, at: Option<OffsetDateTime>) -> i64 {
    at.map(|t| (now - t).whole_seconds().max(0)).unwrap_or(0)
}

/// Platform health (FR-S08). `status` is `ok`, `degraded` (something needs a look) or `down`.
async fn health(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    let now = s.clock.now();
    let p = platform(&s)?;
    let started = std::time::Instant::now();
    let db_ok = db(&s)?.ping().await;
    let latency_ms = started.elapsed().as_millis() as u64;
    let cache_ok = matches!(s.cache.health().await, procera_cache::Health::Ok);
    let (queues, migrations) =
        if db_ok { (Some(p.queues().await?), Some(p.migrations().await?)) } else { (None, None) };
    let expected = procera_db::MIGRATOR.iter().count() as i64;
    let expected_latest = procera_db::MIGRATOR.iter().map(|m| m.version).max();

    let q = queues.unwrap_or_default();
    let lag = age_seconds(now, q.outbox_oldest_at);
    let outbox_ok = lag <= OUTBOX_LAG_DEGRADED_SECONDS && s.dispatcher.is_some();
    let mail_ok = q.mail_failed == 0 && age_seconds(now, q.mail_oldest_at) <= OUTBOX_LAG_DEGRADED_SECONDS;
    let migrations_ok = migrations.as_ref().is_some_and(|m| m.applied >= expected);
    let st = |ok: bool| if ok { "ok" } else { "degraded" };
    let status = if !db_ok {
        "down"
    } else if cache_ok && outbox_ok && mail_ok && migrations_ok {
        "ok"
    } else {
        "degraded"
    };
    Ok(Json(json!({
        "status": status,
        "checked_at": now.format(&time::format_description::well_known::Rfc3339).map_err(internal)?,
        "version": env!("CARGO_PKG_VERSION"),
        "database": {"status": if db_ok { "ok" } else { "down" }, "latency_ms": latency_ms},
        "cache": {"status": st(cache_ok), "backend": s.cache.backend()},
        "outbox": {
            "status": st(outbox_ok), "pending": q.outbox_pending, "retrying": q.outbox_retrying,
            "oldest_at": q.outbox_oldest_at.map(|t| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()),
            "lag_seconds": lag, "dispatcher": if s.dispatcher.is_some() { "running" } else { "not_running" },
        },
        "mail": {
            "status": st(mail_ok), "queued": q.mail_queued, "failed": q.mail_failed,
            "oldest_age_seconds": age_seconds(now, q.mail_oldest_at),
        },
        "migrations": {
            "status": st(migrations_ok),
            "applied": migrations.as_ref().map(|m| m.applied), "latest": migrations.as_ref().and_then(|m| m.latest),
            "expected": expected, "expected_latest": expected_latest,
        },
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
    #[serde(default)]
    actor: Option<Uuid>,
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

/// The system chain through the shared explorer, newest first. System administrators' names are
/// resolved; every other actor keeps its pseudonymous subject reference.
async fn audit_events(
    State(s): State<AppState>,
    u: SystemUser,
    Query(q): Query<EventsQuery>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::SystemAuditRead)?;
    let f = Filter {
        from: q.from,
        to: q.to,
        actor: q.actor,
        action: q.action,
        category: q.category,
        object_type: q.object_type,
        object_id: q.object_id,
        result: q.result,
    };
    let cursor = explorer::parse_cursor(q.cursor.as_deref())?;
    let mut tx = db(&s)?.begin_system().await.map_err(internal)?;
    let page = explorer::query(&mut tx, &f, None, cursor, q.size.unwrap_or(50).clamp(1, MAX_PAGE)).await?;
    let subjects: Vec<Uuid> = page.items.iter().filter_map(|e| e.actor_subject_ref).collect();
    let names: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT u.subject_ref, u.display_name FROM users u WHERE u.subject_ref = ANY($1)
           AND EXISTS (SELECT 1 FROM system_role_grants g WHERE g.user_id = u.id)",
    )
    .bind(&subjects)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?
    .into_iter()
    .collect();
    tx.rollback().await.map_err(internal)?;
    let mut body = serde_json::to_value(&page).map_err(internal)?;
    if let Some(items) = body.get_mut("items").and_then(Value::as_array_mut) {
        for item in items {
            let name = item
                .get("actor_subject_ref")
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<Uuid>().ok())
                .and_then(|id| names.get(&id).cloned());
            if let Some(n) = name {
                item["actor_name"] = Value::String(n);
            }
        }
    }
    Ok(Json(body).into_response())
}

/// Recomputes the system chain and reports the first sequence that does not verify.
async fn audit_verify(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::SystemAuditRead)?;
    let ctx = procera_db::WsCtx { workspace_id: procera_db::SYSTEM_WORKSPACE };
    let r = procera_audit::verify(db(&s)?, ctx).await.map_err(internal)?;
    Ok(Json(json!({"ok": r.first_broken.is_none(), "checked": r.checked, "first_broken": r.first_broken}))
        .into_response())
}
