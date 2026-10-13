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
        .route("/departments/{id}/usage", get(department_usage))
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/{id}", delete(delete_group))
        .route("/groups/{id}/members", get(group_members).post(add_group_member))
        .route("/groups/{id}/members/{membership_id}", delete(remove_group_member))
        .route("/positions", get(list_positions).post(create_position))
        .route("/locations", get(list_locations))
        .route("/positions/{id}", delete(delete_position))
        .route("/members/{id}", patch(update_member))
        .route("/members/{id}/directory", get(member_directory))
        .route("/members/{id}/holdings", get(member_holdings))
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

/// `GET /departments`: every department in the workspace (`directory:view`).
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

/// `POST /departments`: a new department (`name`, optional `parent_id`); `directory:manage`; 201.
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

/// `PATCH /departments/{id}`: renames a department, moves it (`parent_id`, `null` for the root) or
/// sets its head (`head_membership_id`); `directory:manage`; 204.
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

/// `DELETE /departments/{id}`: deletes an empty department (`directory:manage`); 204. A department that
/// still has sub-departments, members or positions is refused with 409.
async fn delete_department(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_department(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /groups`: every group in the workspace (`directory:view`).
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

/// `POST /groups`: a new group (`name`, optional `description`); `directory:manage`; 201.
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

/// `DELETE /groups/{id}`: deletes a group (`directory:manage`); 204.
async fn delete_group(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_group(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /groups/{id}/members`: the membership ids in a group (`directory:view`).
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

/// `POST /groups/{id}/members`: adds a member (`membership_id`) to a group (`directory:manage`); 204.
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

/// `DELETE /groups/{id}/members/{membership_id}`: removes a member from a group (`directory:manage`); 204.
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

/// `GET /positions`: every position in the workspace (`directory:view`).
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

/// `DELETE /positions/{id}`: deletes a position (`directory:manage`); 204.
async fn delete_position(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    write!(s, me, |m, tx| m.delete_position(&mut tx, me.actor, id, &client_info(&headers)).await?);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `PATCH /members/{id}`: sets a member's directory fields (`title`, `department_id`,
/// `manager_membership_id`, `location`; `null` clears one); `directory:manage`. Returns `{changed}`.
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

/// `GET /members/{id}/directory`: a member's directory fields, groups and resolved manager
/// (explicit, department head or none); `directory:view`.
async fn member_directory(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let d = read!(s, me, |m, tx| m.member_directory(&mut tx, id).await?);
    Ok(Json(d).into_response())
}

/// What refers to a department, shown before deleting it: role grants scoped to it, and active or
/// draft campaigns, announcements and policies whose audience names it. Titles only, and only for
/// people who manage the directory.
async fn department_usage(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:manage", Resource::Workspace).await?;
    let c = tx.conn();
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM departments WHERE id = $1)")
        .bind(id)
        .fetch_one(&mut *c)
        .await
        .map_err(internal)?;
    if !exists {
        return Err(ApiError::not_found());
    }
    let grants: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT g.id, r.name, coalesce(u.display_name, gr.name) FROM role_grants g
         JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
         LEFT JOIN memberships m ON m.workspace_id = g.workspace_id AND m.id = g.membership_id
         LEFT JOIN users u ON u.id = m.user_id
         LEFT JOIN groups gr ON gr.workspace_id = g.workspace_id AND gr.id = g.group_id
         WHERE g.scope_kind = 'department' AND g.scope_id = $1 AND (g.expires_at IS NULL OR g.expires_at > now())
         ORDER BY lower(r.name)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    // Audiences keep department ids as strings in `departments` (members::audience::Audience).
    let key = id.to_string();
    let campaigns: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, title, state FROM campaigns WHERE state IN ('draft', 'active') AND audience_rule->'departments' ? $1 ORDER BY lower(title)",
    )
    .bind(&key)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let announcements: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, title, state FROM announcements WHERE state <> 'archived' AND audience->'departments' ? $1 ORDER BY lower(title)",
    )
    .bind(&key)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let policies: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, title FROM artifacts WHERE state <> 'retired' AND applicability->'departments' ? $1 ORDER BY lower(title)",
    )
    .bind(&key)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let titled = |rows: Vec<(Uuid, String, String)>| {
        rows.into_iter()
            .map(|(id, title, state)| json!({"id": id, "title": title, "state": state}))
            .collect::<Vec<_>>()
    };
    Ok(Json(json!({
        "grants": grants.into_iter().map(|(id, role, who)| json!({"id": id, "role": role, "holder": who})).collect::<Vec<_>>(),
        "campaigns": titled(campaigns),
        "announcements": titled(announcements),
        "policies": policies.into_iter().map(|(id, title)| json!({"id": id, "title": title})).collect::<Vec<_>>(),
    }))
    .into_response())
}

/// What a member holds, for the leaving flow (docs/design/ia-competitive-analysis.md §8.3): documents
/// they own, drafts and reviews they started, campaigns they run, groups, direct reports, role grants
/// and open acknowledgements, plus the workspace's hand-over default. Titles and counts only.
async fn member_holdings(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:deactivate", Resource::Member(id)).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1)")
        .bind(id)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    if !exists {
        return Err(ApiError::not_found());
    }
    let strategy = settings(&s)?.effective(&mut tx, "directory.deactivation.reassign_strategy").await?.value;
    let c = tx.conn();
    let documents: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, title, kind FROM artifacts WHERE owner_membership_id = $1 AND state <> 'retired' ORDER BY lower(title)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let work: Vec<(Uuid, String, i32, String)> = sqlx::query_as(
        "SELECT a.id, a.title, v.version_no, v.lifecycle FROM artifact_versions v
         JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
         WHERE v.created_by = $1 AND v.lifecycle IN ('draft', 'in_review', 'approved') ORDER BY lower(a.title)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let campaigns: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, title, state FROM campaigns WHERE created_by = $1 AND state IN ('draft', 'active') ORDER BY lower(title)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let groups: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT g.id, g.name FROM group_members gm JOIN groups g ON g.workspace_id = gm.workspace_id AND g.id = gm.group_id
         WHERE gm.membership_id = $1 ORDER BY lower(g.name)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let reports: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT m.id, u.display_name FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE m.manager_membership_id = $1 AND m.status = 'active' ORDER BY lower(u.display_name)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let grants: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT g.id, r.name, g.scope_kind FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
         WHERE g.membership_id = $1 AND (g.expires_at IS NULL OR g.expires_at > now()) ORDER BY lower(r.name)",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM campaign_recipients r JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
         WHERE r.membership_id = $1 AND r.state = 'pending' AND c.state = 'active'",
    )
    .bind(id)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({
        "reassign_strategy": strategy,
        "documents": documents.into_iter().map(|(id, title, kind)| json!({"id": id, "title": title, "kind": kind})).collect::<Vec<_>>(),
        "work_in_progress": work.into_iter().map(|(id, title, v, l)| json!({"artifact_id": id, "title": title, "version_no": v, "lifecycle": l})).collect::<Vec<_>>(),
        "campaigns": campaigns.into_iter().map(|(id, title, state)| json!({"id": id, "title": title, "state": state})).collect::<Vec<_>>(),
        "groups": groups.into_iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "direct_reports": reports.into_iter().map(|(id, name)| json!({"id": id, "display_name": name})).collect::<Vec<_>>(),
        "grants": grants.into_iter().map(|(id, role, scope)| json!({"id": id, "role": role, "scope_kind": scope})).collect::<Vec<_>>(),
        "pending_acknowledgements": pending,
    }))
    .into_response())
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
