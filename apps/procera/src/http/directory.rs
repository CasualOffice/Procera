//! Directory API (BPMS-023, FR-W02, docs/08 "Directory"): departments, groups, positions, member
//! directory fields with manager resolution, and CSV import. Reads need `directory:view`, writes
//! `directory:manage`.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_members::directory::{DepartmentPatch, MemberPatch};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::settings::settings;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/departments", get(list_departments).post(create_department))
        .route("/departments/{id}", patch(update_department).delete(delete_department))
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/{id}", delete(delete_group))
        .route("/groups/{id}/members", get(group_members).post(add_group_member))
        .route("/groups/{id}/members/{membership_id}", delete(remove_group_member))
        .route("/positions", get(list_positions).post(create_position))
        .route("/locations", get(list_locations))
        .route("/positions/{id}", delete(delete_position))
        .route("/members/{id}", patch(update_member))
        .route("/members/{id}/directory", get(member_directory))
        .route("/members/import", post(import))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "directory api internal error");
    ApiError::new(500, "internal", "Internal error")
}

/// Runs a read under `directory:view`.
macro_rules! read {
    ($s:expr, $me:expr, |$m:ident, $tx:ident| $body:expr) => {{
        let $m = members(&$s)?;
        let mut $tx = $me.tx($m).await?;
        $me.require($m, &mut $tx, "directory:view", Resource::Workspace).await?;
        let out = $body;
        $tx.rollback().await.map_err(internal)?;
        out
    }};
}

/// Runs a write under `directory:manage` and commits.
macro_rules! write {
    ($s:expr, $me:expr, |$m:ident, $tx:ident| $body:expr) => {{
        let $m = members(&$s)?;
        let mut $tx = $me.tx($m).await?;
        $me.require($m, &mut $tx, "directory:manage", Resource::Workspace).await?;
        let out = $body;
        $tx.commit().await.map_err(internal)?;
        out
    }};
}

async fn list_departments(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let items = read!(s, me, |m, tx| m.list_departments(&mut tx).await?);
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDepartment {
    name: String,
    #[serde(default)]
    parent_id: Option<Uuid>,
}

async fn create_department(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<NewDepartment>,
) -> Result<Response, ApiError> {
    let d = write!(s, me, |m, tx| m
        .create_department(&mut tx, me.actor, &r.name, r.parent_id, &client_info(&headers))
        .await?);
    Ok((StatusCode::CREATED, Json(d)).into_response())
}

async fn update_department(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(p): Json<DepartmentPatch>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.update_department(&mut tx, me.actor, id, &p, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete_department(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_department(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn list_groups(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let items = read!(s, me, |m, tx| m.list_groups(&mut tx).await?);
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGroup {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

async fn create_group(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<NewGroup>,
) -> Result<Response, ApiError> {
    let g = write!(s, me, |m, tx| m
        .create_group(&mut tx, me.actor, &r.name, r.description.as_deref(), &client_info(&headers))
        .await?);
    Ok((StatusCode::CREATED, Json(g)).into_response())
}

async fn delete_group(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_group(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn group_members(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let items = read!(s, me, |m, tx| m.group_members(&mut tx, id).await?);
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupMember {
    membership_id: Uuid,
}

async fn add_group_member(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<GroupMember>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m
        .add_group_member(&mut tx, me.actor, id, r.membership_id, &client_info(&headers))
        .await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn remove_group_member(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, membership)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m
        .remove_group_member(&mut tx, me.actor, id, membership, &client_info(&headers))
        .await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The directory locations in use, with how many active members each has (audience pickers).
async fn list_locations(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let rows: Vec<(String, i64)> = read!(s, me, |m, tx| {
        sqlx::query_as(
        "SELECT min(trim(location)), count(*) FROM memberships WHERE status = 'active' AND location IS NOT NULL AND trim(location) <> ''
         GROUP BY lower(trim(location)) ORDER BY lower(trim(location))",
    )
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?
    });
    let items: Vec<serde_json::Value> =
        rows.into_iter().map(|(name, members)| json!({"name": name, "members": members})).collect();
    Ok(Json(json!({ "items": items })).into_response())
}

async fn list_positions(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let items = read!(s, me, |m, tx| m.list_positions(&mut tx).await?);
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPosition {
    title: String,
    #[serde(default)]
    department_id: Option<Uuid>,
    #[serde(default)]
    holder_membership_id: Option<Uuid>,
}

/// Positions are off unless `directory.positions.enabled` (docs/20 §2.5).
async fn create_position(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<NewPosition>,
) -> Result<Response, ApiError> {
    let p = write!(s, me, |m, tx| {
        let enabled = settings(&s)?.effective(&mut tx, "directory.positions.enabled").await?.value.as_bool()
            == Some(true);
        if !enabled {
            return Err(ApiError::new(
                409,
                "positions_disabled",
                "Positions are turned off for this workspace",
            )
            .detail("Enable the setting directory.positions.enabled"));
        }
        m.create_position(
            &mut tx,
            me.actor,
            &r.title,
            r.department_id,
            r.holder_membership_id,
            &client_info(&headers),
        )
        .await?
    });
    Ok((StatusCode::CREATED, Json(p)).into_response())
}

async fn delete_position(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_position(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn update_member(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(p): Json<MemberPatch>,
) -> Result<Response, ApiError> {
    let changed = write!(s, me, |m, tx| m
        .update_member_directory(&mut tx, me.actor, id, &p, &client_info(&headers))
        .await?);
    Ok(Json(json!({ "changed": changed })).into_response())
}

async fn member_directory(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let d = read!(s, me, |m, tx| m.member_directory(&mut tx, id).await?);
    Ok(Json(d).into_response())
}

#[derive(Debug, Deserialize)]
struct ImportQuery {
    #[serde(default = "plan")]
    mode: String,
}

fn plan() -> String {
    "plan".into()
}

/// CSV directory import. `mode=plan` (default) reports what would change and rolls back;
/// `mode=apply` commits.
async fn import(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Query(q): Query<ImportQuery>,
    body: String,
) -> Result<Response, ApiError> {
    let apply = match q.mode.as_str() {
        "plan" => false,
        "apply" => true,
        _ => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("mode must be plan or apply"));
        }
    };
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:manage", Resource::Workspace).await?;
    let report = m.import_csv(&mut tx, me.actor, &body, &client_info(&headers)).await?;
    if apply {
        tx.commit().await.map_err(internal)?;
    } else {
        tx.rollback().await.map_err(internal)?;
    }
    Ok(Json(json!({ "mode": q.mode, "report": report })).into_response())
}
