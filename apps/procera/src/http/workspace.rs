//! Workspace-plane API (BPMS-023/024). Tenant = the `ws` claim of the access token (docs/12 §1);
//! every handler authorizes through Cedar via [`WsMember::require`].

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use procera_access::{AccessError, Resource, Scope, catalogue};
use procera_db::{WsCtx, WsTx};
use procera_members::{Actor, Members, MembersError};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::auth::{AuthUser, client_info};
use super::error::ApiError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/workspaces", get(my_workspaces))
        .route("/session/workspace", post(switch_workspace))
        .route("/invitations/accept", post(accept))
        .route("/invitations/register", post(register))
        .route("/permission-catalog", get(permission_catalog))
        .route("/me/permissions", get(my_permissions))
        .route("/members", get(list_members))
        .route("/members/invitations", get(list_invitations).post(invite))
        .route("/members/invitations/{id}", delete(revoke_invitation))
        .route("/members/{id}/deactivate", post(deactivate))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", patch(update_role))
        .route("/roles/{id}/preview", post(preview_role))
        .route("/role-grants", get(list_grants).post(create_grant))
        .route("/role-grants/{id}", delete(revoke_grant))
}

pub fn members(s: &AppState) -> Result<&Members, ApiError> {
    s.members.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<MembersError> for ApiError {
    fn from(e: MembersError) -> Self {
        match e {
            MembersError::InvitationInvalid => {
                Self::new(404, "invitation_invalid", "Invitation invalid, expired, revoked or already used")
            }
            MembersError::EmailMismatch => {
                Self::new(403, "email_mismatch", "This invitation was sent to a different email address")
            }
            MembersError::AlreadyMember => {
                Self::new(409, "already_member", "Already a member of this workspace")
            }
            MembersError::NotFound => Self::not_found(),
            MembersError::LastOwner => {
                Self::new(409, "last_owner", "The last workspace owner cannot be removed")
            }
            MembersError::SelfDeactivation => {
                Self::new(409, "self_deactivation", "You cannot deactivate yourself")
            }
            MembersError::Access(a) => Self::from(a),
            MembersError::Identity(i) => Self::from(i),
            MembersError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            MembersError::Internal(m) => {
                tracing::error!(error = %m, "members internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

impl From<AccessError> for ApiError {
    fn from(e: AccessError) -> Self {
        match e {
            AccessError::UnknownPermission(p) => {
                Self::new(422, "unknown_permission", "Unknown permission").detail(p)
            }
            AccessError::UnknownRole(r) => Self::new(422, "unknown_role", "Unknown role").detail(r),
            AccessError::RoleExists => Self::new(409, "role_exists", "A role with this key already exists"),
            AccessError::BuiltinImmutable => Self::new(
                409,
                "builtin_immutable",
                "Built-in roles cannot be edited; clone it into a custom role",
            ),
            AccessError::Escalation(missing) => {
                Self::new(403, "escalation", "You cannot grant permissions you do not hold")
                    .detail(missing.join(", "))
            }
            AccessError::NotFound => Self::not_found(),
            AccessError::LastOwner => {
                Self::new(409, "last_owner", "The last workspace owner cannot lose the owner role")
            }
            AccessError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            AccessError::Internal(m) => {
                tracing::error!(error = %m, "access internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

/// An active member of the token's workspace. Enforces the workspace state gate (FR-S02): read-only
/// suspension blocks writes, a locked workspace blocks everything (423).
#[derive(Debug, Clone)]
pub struct WsMember {
    pub ctx: WsCtx,
    pub actor: Actor,
}

impl FromRequestParts<AppState> for WsMember {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, s).await?;
        let ws = claims.ws.ok_or_else(|| {
            ApiError::new(400, "no_workspace", "Select a workspace first")
                .detail("POST /api/v1/session/workspace")
        })?;
        let m = members(s)?;
        let ctx = WsCtx { workspace_id: ws };
        let mut tx = m.db().begin_ws(ctx).await.map_err(|e| MembersError::Internal(e.to_string()))?;
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM workspaces WHERE id = $1")
            .bind(ws)
            .fetch_optional(tx.conn())
            .await
            .map_err(|e| MembersError::Internal(e.to_string()))?;
        let membership = m.membership(&mut tx, claims.sub).await?;
        tx.rollback().await.map_err(|e| MembersError::Internal(e.to_string()))?;
        let (Some(state), Some((membership, status, subject_ref))) = (state, membership) else {
            return Err(ApiError::not_found());
        };
        if status != "active" {
            return Err(ApiError::not_found());
        }
        let write = parts.method != Method::GET && parts.method != Method::HEAD;
        match state.as_str() {
            "suspended_locked" | "pending_deletion" | "deleted" => {
                return Err(ApiError::new(423, "workspace_suspended", "This workspace is unavailable")
                    .detail(format!("state: {state}")));
            }
            "suspended_ro" if write => {
                return Err(ApiError::new(423, "workspace_suspended", "This workspace is read-only")
                    .detail("A system administrator suspended changes"));
            }
            _ => {}
        }
        Ok(Self { ctx, actor: Actor { membership, subject_ref } })
    }
}

impl WsMember {
    pub async fn tx(&self, m: &Members) -> Result<WsTx<'static>, ApiError> {
        m.db().begin_ws(self.ctx).await.map_err(|e| MembersError::Internal(e.to_string()).into())
    }

    /// Authorizes through Cedar. Denials return 403 with the action (the member can see the workspace).
    pub async fn require(
        &self,
        m: &Members,
        tx: &mut WsTx<'_>,
        action: &str,
        resource: Resource,
    ) -> Result<(), ApiError> {
        let d = m.access().check(tx, self.actor.membership, action, &resource).await?;
        if d.allowed {
            Ok(())
        } else {
            Err(ApiError::new(403, "forbidden", "You do not have permission for this action")
                .detail(action.to_owned()))
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "workspace api internal error");
    ApiError::new(500, "internal", "Internal error")
}

async fn my_workspaces(State(s): State<AppState>, AuthUser(c): AuthUser) -> Result<Response, ApiError> {
    Ok(Json(json!({"items": members(&s)?.user_workspaces(c.sub).await?, "active": c.ws})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchReq {
    workspace_id: Option<Uuid>,
}

/// Issues an access token for the chosen workspace (or none, for the home view).
async fn switch_workspace(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(r): Json<SwitchReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    if let Some(ws) = r.workspace_id {
        let ok = m.user_workspaces(c.sub).await?.iter().any(|w| w.workspace_id == ws);
        if !ok {
            return Err(ApiError::not_found());
        }
    }
    let id = s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let (token, ttl) = id.issue_workspace_token(&c, r.workspace_id, &client_info(&headers)).await?;
    Ok(Json(json!({"access_token": token, "token_type": "Bearer", "expires_in": ttl, "workspace_id": r.workspace_id})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenReq {
    token: String,
}

async fn accept(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(r): Json<TokenReq>,
) -> Result<Response, ApiError> {
    let (ws, membership) = members(&s)?.accept(&r.token, c.sub, &client_info(&headers)).await?;
    Ok(Json(json!({"workspace_id": ws, "membership_id": membership})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterReq {
    token: String,
    display_name: String,
    password: String,
}

async fn register(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<RegisterReq>,
) -> Result<Response, ApiError> {
    let (user, ws, membership) =
        members(&s)?.register(&r.token, &r.display_name, &r.password, &client_info(&headers)).await?;
    Ok((StatusCode::CREATED, Json(json!({"user": user, "workspace_id": ws, "membership_id": membership, "next": "Sign in, then POST /api/v1/session/workspace"}))).into_response())
}

async fn permission_catalog(_m: WsMember) -> Json<serde_json::Value> {
    Json(
        json!({"permissions": catalogue::PERMISSIONS, "templates": catalogue::TEMPLATES.iter().map(|t| json!({"key": t.key, "name": t.name, "description": t.description, "permissions": t.permissions()})).collect::<Vec<_>>()}),
    )
}

async fn my_permissions(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let perms = m.access().effective_workspace_permissions(&mut tx, me.actor.membership).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"workspace_id": me.ctx.workspace_id, "membership_id": me.actor.membership, "permissions": perms})).into_response())
}

async fn list_members(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:view", Resource::Workspace).await?;
    let items = m.list(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"items": items})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteReq {
    email: String,
    roles: Vec<String>,
}

async fn invite(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<InviteReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:invite", Resource::Workspace).await?;
    let (inv, _token) = m.invite(&mut tx, Some(me.actor), &r.email, &r.roles, &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    // The token is delivered by email only (BPMS-048), never returned to the inviter.
    Ok((StatusCode::CREATED, Json(inv)).into_response())
}

async fn list_invitations(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:invite", Resource::Workspace).await?;
    let items = m.list_invitations(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"items": items})).into_response())
}

async fn revoke_invitation(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:invite", Resource::Workspace).await?;
    m.revoke_invitation(&mut tx, me.actor, id, &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn deactivate(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:deactivate", Resource::Member(id)).await?;
    m.deactivate(&mut tx, me.actor, id, &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn list_roles(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "role:view", Resource::Workspace).await?;
    let roles = m.access().list_roles(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"items": roles})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleReq {
    key: String,
    name: String,
    permissions: Vec<String>,
    #[serde(default)]
    cloned_from: Option<String>,
}

async fn create_role(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<RoleReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "role:manage", Resource::Workspace).await?;
    let role = m
        .access()
        .create_role(
            &mut tx,
            Some(me.actor.membership),
            &r.key,
            &r.name,
            &r.permissions,
            r.cloned_from.as_deref(),
        )
        .await?;
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "role.created",
        "role",
        role.id,
        json!({"key": role.key, "permissions": role.permissions, "cloned_from": role.builtin_template}),
        &client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(role)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RolePatch {
    permissions: Vec<String>,
}

async fn update_role(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RolePatch>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "role:manage", Resource::Role(id)).await?;
    let preview = m.access().preview_role_change(&mut tx, id, &r.permissions).await?;
    m.access().update_role(&mut tx, Some(me.actor.membership), id, &r.permissions).await?;
    m.audit_object(&mut tx, Some(me.actor.subject_ref), "role.updated", "role", id, json!({"added": preview.added, "removed": preview.removed, "affected_members": preview.affected_members.len()}), &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Shows what a role change would do before saving it (docs/12 §5 item 5).
async fn preview_role(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RolePatch>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "role:manage", Resource::Role(id)).await?;
    let preview = m.access().preview_role_change(&mut tx, id, &r.permissions).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(preview).into_response())
}

#[derive(Debug, Deserialize)]
struct GrantQuery {
    membership_id: Option<Uuid>,
}

async fn list_grants(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<GrantQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "role:view", Resource::Workspace).await?;
    let grants = m.access().list_grants(&mut tx, q.membership_id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"items": grants})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantReq {
    membership_id: Uuid,
    role_key: String,
    #[serde(default = "workspace_scope")]
    scope_kind: String,
    #[serde(default)]
    scope_id: Option<Uuid>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    expires_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    reason: Option<String>,
}

fn workspace_scope() -> String {
    "workspace".into()
}

async fn create_grant(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<GrantReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let scope = Scope::parse(&r.scope_kind, r.scope_id)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:manage_roles", Resource::Member(r.membership_id)).await?;
    let id = m
        .access()
        .grant(
            &mut tx,
            Some(me.actor.membership),
            r.membership_id,
            &r.role_key,
            scope,
            r.expires_at,
            r.reason.as_deref(),
        )
        .await?;
    m.audit_object(&mut tx, Some(me.actor.subject_ref), "role.granted", "role_grant", id, json!({"membership_id": r.membership_id, "role": r.role_key, "scope": scope, "expires_at": r.expires_at.map(|t| t.unix_timestamp()), "reason": r.reason}), &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(json!({"id": id}))).into_response())
}

async fn revoke_grant(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:manage_roles", Resource::Workspace).await?;
    m.access().revoke_grant(&mut tx, Some(me.actor.membership), id).await?;
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "role.revoked",
        "role_grant",
        id,
        json!({}),
        &client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
