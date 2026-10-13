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
        .route("/session/step-up", post(step_up))
        .route("/session/step-up/webauthn/options", post(step_up_passkey_options))
        .route("/invitations/preview", get(preview))
        .route("/invitations/accept", post(accept))
        .route("/invitations/register", post(register))
        .route("/permission-catalog", get(permission_catalog))
        .route("/me/permissions", get(my_permissions))
        .route("/members", get(list_members))
        .route("/members/invitations", get(list_invitations).post(invite))
        .route("/members/invitations/{id}", delete(revoke_invitation))
        .route("/members/invitations/{id}/link", post(copy_invitation_link))
        .route("/members/{id}/deactivate", post(deactivate))
        .route("/members/{id}/reactivate", post(reactivate))
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
            MembersError::Cycle(m) => Self::new(422, "cycle", "This change would create a cycle").detail(m),
            MembersError::Conflict(m) => Self::new(409, "conflict", "Conflict").detail(m),
            MembersError::QuotaExceeded { limit, used } => {
                Self::new(429, "quota_exceeded", "This workspace has reached its member limit").detail(format!(
                    "{used} of {limit} seats are taken (members and open invitations). Ask your system administrator to raise the limit, or revoke an invitation."
                ))
            }
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
    pub user_id: Uuid,
    /// When the user last authenticated (sign-in or step-up), unix seconds.
    pub auth_time: i64,
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
        let (Some(state), Some((membership, status, subject_ref))) = (state, membership) else {
            return Err(ApiError::not_found());
        };
        if status != "active" {
            return Err(ApiError::not_found());
        }
        // MFA required by role (docs/20 §2.3 auth.mfa.required_roles, ⚡: read from the database).
        // An API token counts: making one needed a recent sign-in that met this rule (step-up).
        let has_mfa = claims.amr.iter().any(|a| a == "otp" || a == "hwk" || a == "token");
        if !has_mfa {
            let required =
                super::settings::settings(s)?.effective(&mut tx, "auth.mfa.required_roles").await?.value;
            let required: Vec<&str> = required
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let roles: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT r.key FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
                 WHERE (g.membership_id = $1 OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = $1))
                   AND (g.expires_at IS NULL OR g.expires_at > now())",
            )
            .bind(membership)
            .fetch_all(tx.conn())
            .await
            .map_err(|e| MembersError::Internal(e.to_string()))?;
            if required.contains(&"*") || roles.iter().any(|r| required.contains(&r.as_str())) {
                return Err(ApiError::new(
                    403,
                    "mfa_required",
                    "This workspace requires multi-factor authentication for your role",
                )
                .detail("Enrol at POST /api/v1/me/mfa/totp, then sign in again."));
            }
        }
        tx.rollback().await.map_err(|e| MembersError::Internal(e.to_string()))?;
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
        if let Some(seen) = parts.extensions.get::<super::probe::Seen>() {
            seen.record(ctx, subject_ref); // a 404 on an object id becomes `access.denied` (probe.rs)
        }
        Ok(Self {
            ctx,
            actor: Actor { membership, subject_ref },
            user_id: claims.sub,
            auth_time: claims.auth_time,
        })
    }
}

impl WsMember {
    pub async fn tx(&self, m: &Members) -> Result<WsTx<'static>, ApiError> {
        m.db().begin_ws(self.ctx).await.map_err(|e| MembersError::Internal(e.to_string()).into())
    }

    /// Requires a recent authentication (docs/12 §11) within the workspace's `auth.stepup.window`.
    pub async fn require_step_up(&self, s: &AppState, tx: &mut WsTx<'_>) -> Result<(), ApiError> {
        let window = super::settings::settings(s)?
            .effective(tx, "auth.stepup.window")
            .await?
            .value
            .as_i64()
            .unwrap_or(300);
        let age = s.clock.now().unix_timestamp() - self.auth_time;
        if (0..=window).contains(&age) {
            Ok(())
        } else {
            Err(ApiError::new(401, "step_up_required", "Confirm it's you to continue")
                .detail(format!("POST /api/v1/session/step-up; this action needs authentication within the last {window} seconds")))
        }
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
struct StepUpReq {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    password: Option<String>,
    /// A passkey assertion for a challenge from `/session/step-up/webauthn/options`.
    #[serde(default)]
    credential: Option<procera_identity::webauthn::CredentialJson>,
}

/// Re-authenticates within the current sign-in and returns an access token with a fresh `auth_time`.
async fn step_up(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(r): Json<StepUpReq>,
) -> Result<Response, ApiError> {
    use procera_identity::Factor;
    let id = s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let factor = match (&r.credential, &r.code, &r.password) {
        (Some(cred), None, None) => Some(Factor::Passkey(cred)),
        (None, Some(code), _) => Some(Factor::Code(code)),
        (None, None, Some(pw)) => Some(Factor::Password(pw)),
        (None, None, None) => None,
        _ => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("Send a passkey `credential` on its own"));
        }
    };
    let (token, ttl) = id.step_up_with(&c, factor, &client_info(&headers)).await?;
    Ok(Json(json!({"access_token": token, "token_type": "Bearer", "expires_in": ttl})).into_response())
}

/// Passkey challenge for step-up.
async fn step_up_passkey_options(
    State(s): State<AppState>,
    AuthUser(c): AuthUser,
) -> Result<Response, ApiError> {
    let id = s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let mut res = Json(id.step_up_passkey_options(&c).await?).into_response();
    res.headers_mut()
        .insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenReq {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PreviewQuery {
    token: String,
}

/// Public: what the invitation page shows to the holder of the link (A-24).
async fn preview(State(s): State<AppState>, Query(q): Query<PreviewQuery>) -> Result<Response, ApiError> {
    Ok(Json(members(&s)?.invitation_preview(&q.token).await?).into_response())
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
    // A manager in the directory, or a Team Manager of a department, has a team: it opens Team.
    let (reports,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM memberships WHERE manager_membership_id = $1 AND status = 'active')",
    )
    .bind(me.actor.membership)
    .fetch_one(tx.conn())
    .await
    .map_err(internal)?;
    let manages = reports
        || !m.access().departments_with(&mut tx, me.actor.membership, "work:view_team").await?.is_empty();
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({"workspace_id": me.ctx.workspace_id, "membership_id": me.actor.membership, "permissions": perms, "manages": manages})).into_response())
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

/// A fresh one-time link for a pending invitation, for the inviter to hand over when email isn't set
/// up. The earlier link stops working. Only this response carries the token; lists never do.
async fn copy_invitation_link(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:invite", Resource::Workspace).await?;
    let (inv, token) = m.reissue_invitation_link(&mut tx, Some(me.actor), id, &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    let mut res = Json(json!({"invitation": inv, "link": invitation_link(&s, &token)})).into_response();
    res.headers_mut()
        .insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    Ok(res)
}

/// The invitation page link, as the invitation email carries it.
pub(super) fn invitation_link(s: &AppState, token: &str) -> String {
    format!("{}/invite?token={token}", s.public_url)
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReactivateReq {
    reason: String,
}

/// Brings a deactivated member back with the roles they had (docs/22 A-41). Restoring access is
/// treated like granting it: step-up, and no restoring roles the caller could not grant.
async fn reactivate(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReactivateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:deactivate", Resource::Member(id)).await?;
    me.require_step_up(&s, &mut tx).await?;
    let roles = m.reactivate(&mut tx, me.actor, id, &r.reason, &client_info(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({"roles": roles})).into_response())
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
    me.require_step_up(&s, &mut tx).await?;
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
    me.require_step_up(&s, &mut tx).await?;
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
    /// Exactly one of `membership_id` / `group_id`.
    #[serde(default)]
    membership_id: Option<Uuid>,
    #[serde(default)]
    group_id: Option<Uuid>,
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
    let actor = Some(me.actor.membership);
    let id = match (r.membership_id, r.group_id) {
        (Some(member), None) => {
            me.require(m, &mut tx, "member:manage_roles", Resource::Member(member)).await?;
            me.require_step_up(&s, &mut tx).await?;
            m.access()
                .grant(&mut tx, actor, member, &r.role_key, scope, r.expires_at, r.reason.as_deref())
                .await?
        }
        (None, Some(group)) => {
            me.require(m, &mut tx, "member:manage_roles", Resource::Workspace).await?;
            me.require_step_up(&s, &mut tx).await?;
            m.access()
                .grant_to_group(&mut tx, actor, group, &r.role_key, scope, r.expires_at, r.reason.as_deref())
                .await?
        }
        _ => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("give exactly one of membership_id or group_id"));
        }
    };
    m.audit_object(&mut tx, Some(me.actor.subject_ref), "role.granted", "role_grant", id, json!({"membership_id": r.membership_id, "group_id": r.group_id, "role": r.role_key, "scope": scope, "expires_at": r.expires_at.map(|t| t.unix_timestamp()), "reason": r.reason}), &client_info(&headers)).await?;
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
    me.require_step_up(&s, &mut tx).await?;
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
