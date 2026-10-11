//! Saved DataGrid views (BPMS-028; FR-V05, docs/05 §4, docs/08 "Views") and the audit record of
//! client-side exports of a grid. Views are personal unless shared (visible to the workspace); only
//! their owner changes them. `state` is opaque to the server (search, sort, filters, columns, density).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/views", get(list).post(create))
        .route("/views/{id}", patch(update).delete(remove))
        .route("/exports", post(record_export))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "views api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn invalid(m: &str) -> ApiError {
    ApiError::new(422, "validation_failed", "Validation failed").detail(m)
}

type Row = (Uuid, Uuid, String, String, Value, bool, bool, OffsetDateTime, i64);
const COLS: &str = "id, owner_membership_id, list, name, state, shared, is_default, updated_at, revision";

fn view_json(r: &Row, me: Uuid) -> Value {
    json!({
        "id": r.0, "list": r.2, "name": r.3, "state": r.4, "shared": r.5,
        "is_default": r.6 && r.1 == me, "mine": r.1 == me,
        "updated_at": r.7.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(), "revision": r.8,
    })
}

fn check_list(list: &str) -> Result<(), ApiError> {
    let ok = list.len() >= 2
        && list.len() <= 63
        && list.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && list.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c));
    if ok { Ok(()) } else { Err(invalid("list is a lowercase name like library.documents")) }
}

fn check_state(state: &Value) -> Result<(), ApiError> {
    if !state.is_object() || serde_json::to_vec(state).map(|v| v.len()).unwrap_or(usize::MAX) > 16 * 1024 {
        return Err(invalid("state must be an object under 16 KB"));
    }
    Ok(())
}

async fn audit(
    s: &AppState,
    tx: &mut procera_db::WsTx<'_>,
    me: &WsMember,
    headers: &HeaderMap,
    action: &str,
    id: Uuid,
    diff: Value,
) -> Result<(), ApiError> {
    members(s)?
        .audit_object(
            tx,
            Some(me.actor.subject_ref),
            action,
            "saved_view",
            id,
            diff,
            &super::auth::client_info(headers),
        )
        .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    list: String,
}

/// My views and shared views for a list (mine first, then by name).
async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    check_list(&q.list)?;
    let mut tx = me.tx(members(&s)?).await?;
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM saved_views WHERE list = $1 AND (owner_membership_id = $2 OR shared) ORDER BY owner_membership_id <> $2, lower(name), id"
    ))
    .bind(&q.list).bind(me.actor.membership).fetch_all(tx.conn()).await.map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": rows.iter().map(|r| view_json(r, me.actor.membership)).collect::<Vec<_>>() })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    list: String,
    name: String,
    state: Value,
    #[serde(default)]
    shared: bool,
    #[serde(default)]
    is_default: bool,
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    check_list(&r.list)?;
    check_state(&r.state)?;
    let name = r.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("a view name is 1–80 characters"));
    }
    let mut tx = me.tx(members(&s)?).await?;
    let id = Uuid::now_v7();
    let now = s.clock.now();
    if r.is_default {
        sqlx::query("UPDATE saved_views SET is_default = false WHERE owner_membership_id = $1 AND list = $2")
            .bind(me.actor.membership)
            .bind(&r.list)
            .execute(tx.conn())
            .await
            .map_err(internal)?;
    }
    sqlx::query("INSERT INTO saved_views (workspace_id, id, owner_membership_id, list, name, state, shared, is_default, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9)")
        .bind(tx.ctx.workspace_id).bind(id).bind(me.actor.membership).bind(&r.list).bind(name).bind(&r.state).bind(r.shared).bind(r.is_default).bind(now)
        .execute(tx.conn()).await.map_err(internal)?;
    audit(
        &s,
        &mut tx,
        &me,
        &headers,
        "view.created",
        id,
        json!({"list": r.list, "name": name, "shared": r.shared}),
    )
    .await?;
    let row: Row = sqlx::query_as(&format!("SELECT {COLS} FROM saved_views WHERE id = $1"))
        .bind(id)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(view_json(&row, me.actor.membership))).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    name: Option<String>,
    state: Option<Value>,
    shared: Option<bool>,
    is_default: Option<bool>,
}

/// Only the owner changes a view (shared views included).
async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<UpdateReq>,
) -> Result<Json<Value>, ApiError> {
    if let Some(st) = &r.state {
        check_state(st)?;
    }
    let mut tx = me.tx(members(&s)?).await?;
    let cur: Option<Row> =
        sqlx::query_as(&format!("SELECT {COLS} FROM saved_views WHERE id = $1 FOR UPDATE"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
    let Some(cur) = cur.filter(|c| c.1 == me.actor.membership || c.5) else {
        return Err(ApiError::not_found());
    };
    if cur.1 != me.actor.membership {
        return Err(ApiError::new(403, "forbidden", "Only the owner can change a view"));
    }
    let name = r.name.as_deref().map(str::trim).unwrap_or(&cur.3).to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("a view name is 1–80 characters"));
    }
    if r.is_default == Some(true) {
        sqlx::query("UPDATE saved_views SET is_default = false WHERE owner_membership_id = $1 AND list = $2")
            .bind(me.actor.membership)
            .bind(&cur.2)
            .execute(tx.conn())
            .await
            .map_err(internal)?;
    }
    sqlx::query("UPDATE saved_views SET name = $2, state = $3, shared = $4, is_default = $5, updated_at = $6, revision = revision + 1 WHERE id = $1")
        .bind(id).bind(&name).bind(r.state.as_ref().unwrap_or(&cur.4)).bind(r.shared.unwrap_or(cur.5)).bind(r.is_default.unwrap_or(cur.6)).bind(s.clock.now())
        .execute(tx.conn()).await.map_err(internal)?;
    audit(
        &s,
        &mut tx,
        &me,
        &headers,
        "view.updated",
        id,
        json!({"name": name, "shared": r.shared, "state_changed": r.state.is_some()}),
    )
    .await?;
    let row: Row = sqlx::query_as(&format!("SELECT {COLS} FROM saved_views WHERE id = $1"))
        .bind(id)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(view_json(&row, me.actor.membership)))
}

async fn remove(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let owner: Option<(Uuid, bool)> =
        sqlx::query_as("SELECT owner_membership_id, shared FROM saved_views WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
    match owner {
        Some((o, _)) if o == me.actor.membership => {}
        Some((_, true)) => return Err(ApiError::new(403, "forbidden", "Only the owner can delete a view")),
        _ => return Err(ApiError::not_found()),
    }
    sqlx::query("DELETE FROM saved_views WHERE id = $1")
        .bind(id)
        .execute(tx.conn())
        .await
        .map_err(internal)?;
    audit(&s, &mut tx, &me, &headers, "view.deleted", id, json!({})).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportReq {
    list: String,
    format: String,
    rows: u32,
    columns: Vec<String>,
    #[serde(default)]
    filters: Value,
}

/// Records that the member exported a grid view (the browser builds the file from rows it was
/// already allowed to see; FR-V06: "the download is audited").
async fn record_export(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<ExportReq>,
) -> Result<StatusCode, ApiError> {
    check_list(&r.list)?;
    if r.format != "csv" {
        return Err(invalid("format must be csv"));
    }
    if r.columns.len() > 100 {
        return Err(invalid("at most 100 columns"));
    }
    let mut tx = me.tx(members(&s)?).await?;
    members(&s)?.audit_object(&mut tx, Some(me.actor.subject_ref), "data.exported", "list", Uuid::nil(), json!({"list": r.list, "format": r.format, "rows": r.rows, "columns": r.columns, "filters": r.filters}), &super::auth::client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}
