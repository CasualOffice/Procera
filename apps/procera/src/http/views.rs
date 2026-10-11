//! Saved DataGrid views (BPMS-028; FR-V04, docs/05 §4, docs/08 "Views") and grid exports. Views are
//! personal unless shared with the workspace (`shared`) or with the holders of one role
//! (`shared_role`, A-35); only their owner changes them. `state` is opaque to the server (search,
//! sort, filters and filter groups, columns, density). Client-side exports are recorded here.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/views", get(list).post(create))
        .route("/views/roles", get(share_roles))
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

/// id, owner, list, name, state, shared, is_default, updated_at, revision, shared_role, role name,
/// visible to the caller.
type Row = (
    Uuid,
    Uuid,
    String,
    String,
    Value,
    bool,
    bool,
    OffsetDateTime,
    i64,
    Option<String>,
    Option<String>,
    bool,
);

/// Whether member `$1` sees a view at time `$2`: theirs, shared with everyone, or shared with a role
/// they hold (an unexpired grant of it at any scope, directly or through a group).
const VISIBLE: &str = "(saved_views.owner_membership_id = $1 OR saved_views.shared OR (saved_views.shared_role IS NOT NULL AND EXISTS (\
    SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id \
    WHERE r.key = saved_views.shared_role AND (g.expires_at IS NULL OR g.expires_at > $2) \
    AND (g.membership_id = $1 OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = $1)))))";

/// The select list for [`Row`]; binds `$1` = member, `$2` = now.
fn cols() -> String {
    format!(
        "saved_views.id, saved_views.owner_membership_id, saved_views.list, saved_views.name, saved_views.state, \
         saved_views.shared, saved_views.is_default, saved_views.updated_at, saved_views.revision, saved_views.shared_role, \
         (SELECT r.name FROM roles r WHERE r.key = saved_views.shared_role), {VISIBLE}"
    )
}

async fn row_by_id(
    tx: &mut procera_db::WsTx<'_>,
    me: Uuid,
    now: OffsetDateTime,
    id: Uuid,
    lock: bool,
) -> Result<Option<Row>, ApiError> {
    let lock = if lock { " FOR UPDATE OF saved_views" } else { "" };
    sqlx::query_as(&format!("SELECT {} FROM saved_views WHERE saved_views.id = $3{lock}", cols()))
        .bind(me)
        .bind(now)
        .bind(id)
        .fetch_optional(tx.conn())
        .await
        .map_err(internal)
}

/// The views of a list the member sees: theirs first, then by name.
async fn visible_rows(
    tx: &mut procera_db::WsTx<'_>,
    me: Uuid,
    now: OffsetDateTime,
    list: &str,
) -> Result<Vec<Row>, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {} FROM saved_views WHERE saved_views.list = $3 AND {VISIBLE} \
         ORDER BY saved_views.owner_membership_id <> $1, lower(saved_views.name), saved_views.id",
        cols()
    ))
    .bind(me)
    .bind(now)
    .bind(list)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)
}

fn view_json(r: &Row, me: Uuid) -> Value {
    json!({
        "id": r.0, "list": r.2, "name": r.3, "state": r.4, "shared": r.5,
        "shared_role": r.9, "shared_role_name": r.10,
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

/// Who a view is shared with: everyone or one role, never both; the role must exist (422 otherwise).
async fn check_audience(
    tx: &mut procera_db::WsTx<'_>,
    shared: bool,
    role: Option<&str>,
) -> Result<(), ApiError> {
    let Some(role) = role else { return Ok(()) };
    if shared {
        return Err(invalid("a view is shared with everyone or with one role, not both"));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE key = $1)")
        .bind(role)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    if exists { Ok(()) } else { Err(invalid("shared_role is not a role of this workspace")) }
}

fn role_key(k: Option<String>) -> Option<String> {
    k.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty())
}

/// `Some(None)` for an explicit `null`, `None` when the field is absent (PATCH semantics).
fn explicit<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
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

/// My views and the views shared with me for a list (mine first, then by name).
async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    check_list(&q.list)?;
    let mut tx = me.tx(members(&s)?).await?;
    let rows = visible_rows(&mut tx, me.actor.membership, s.clock.now(), &q.list).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": rows.iter().map(|r| view_json(r, me.actor.membership)).collect::<Vec<_>>() })))
}

/// The roles a view can be shared with (key and name only, for every member; A-35).
async fn share_roles(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, name FROM roles ORDER BY lower(name), key")
        .fetch_all(tx.conn())
        .await
        .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": rows.iter().map(|(k, n)| json!({"key": k, "name": n})).collect::<Vec<_>>() })))
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
    shared_role: Option<String>,
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
    let role = role_key(r.shared_role.clone());
    let mut tx = me.tx(members(&s)?).await?;
    check_audience(&mut tx, r.shared, role.as_deref()).await?;
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
    sqlx::query("INSERT INTO saved_views (workspace_id, id, owner_membership_id, list, name, state, shared, shared_role, is_default, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10)")
        .bind(tx.ctx.workspace_id).bind(id).bind(me.actor.membership).bind(&r.list).bind(name).bind(&r.state).bind(r.shared).bind(&role).bind(r.is_default).bind(now)
        .execute(tx.conn()).await.map_err(internal)?;
    audit(
        &s,
        &mut tx,
        &me,
        &headers,
        "view.created",
        id,
        json!({"list": r.list, "name": name, "shared": r.shared, "shared_role": role}),
    )
    .await?;
    let row = row_by_id(&mut tx, me.actor.membership, now, id, false)
        .await?
        .ok_or_else(|| internal("view vanished"))?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(view_json(&row, me.actor.membership))).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    name: Option<String>,
    state: Option<Value>,
    shared: Option<bool>,
    /// A role key, or `null` to stop sharing with a role.
    #[serde(default, deserialize_with = "explicit")]
    shared_role: Option<Option<String>>,
    is_default: Option<bool>,
}

/// The audience after a PATCH: naming a role unshares from everyone, sharing with everyone drops the
/// role, and `shared: false` alone keeps the role (send `shared_role: null` to clear it).
fn next_audience(
    cur: (bool, Option<String>),
    shared: Option<bool>,
    role: Option<Option<String>>,
) -> (bool, Option<String>) {
    match (shared, role.map(role_key)) {
        (Some(s), Some(r)) => (s, r),
        (Some(true), None) => (true, None),
        (Some(false), None) => (false, cur.1),
        (None, Some(Some(k))) => (false, Some(k)),
        (None, Some(None)) => (cur.0, None),
        (None, None) => cur,
    }
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
    let now = s.clock.now();
    let Some(cur) = row_by_id(&mut tx, me.actor.membership, now, id, true).await?.filter(|c| c.11) else {
        return Err(ApiError::not_found());
    };
    if cur.1 != me.actor.membership {
        return Err(ApiError::new(403, "forbidden", "Only the owner can change a view"));
    }
    let name = r.name.as_deref().map(str::trim).unwrap_or(&cur.3).to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("a view name is 1–80 characters"));
    }
    let audience_changed = r.shared.is_some() || r.shared_role.is_some();
    let (shared, role) = next_audience((cur.5, cur.9.clone()), r.shared, r.shared_role);
    if audience_changed {
        check_audience(&mut tx, shared, role.as_deref()).await?;
    }
    if r.is_default == Some(true) {
        sqlx::query("UPDATE saved_views SET is_default = false WHERE owner_membership_id = $1 AND list = $2")
            .bind(me.actor.membership)
            .bind(&cur.2)
            .execute(tx.conn())
            .await
            .map_err(internal)?;
    }
    sqlx::query("UPDATE saved_views SET name = $2, state = $3, shared = $4, shared_role = $5, is_default = $6, updated_at = $7, revision = revision + 1 WHERE id = $1")
        .bind(id).bind(&name).bind(r.state.as_ref().unwrap_or(&cur.4)).bind(shared).bind(&role).bind(r.is_default.unwrap_or(cur.6)).bind(now)
        .execute(tx.conn()).await.map_err(internal)?;
    audit(
        &s,
        &mut tx,
        &me,
        &headers,
        "view.updated",
        id,
        json!({"name": name, "shared": shared, "shared_role": role, "state_changed": r.state.is_some()}),
    )
    .await?;
    let row = row_by_id(&mut tx, me.actor.membership, now, id, false)
        .await?
        .ok_or_else(|| internal("view vanished"))?;
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
    match row_by_id(&mut tx, me.actor.membership, s.clock.now(), id, true).await? {
        Some(r) if r.1 == me.actor.membership => {}
        Some(r) if r.11 => return Err(ApiError::new(403, "forbidden", "Only the owner can delete a view")),
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

#[cfg(test)]
mod tests;
