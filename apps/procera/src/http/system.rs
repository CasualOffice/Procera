//! System API (`/sysapi/v1`, docs/08 §5). Every route requires a system role and MFA (docs/06 §3.6),
//! except the one-time bootstrap.

use axum::extract::{FromRequestParts, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_platform::authz::{SystemAction, can};
use procera_platform::{NewWorkspace, Platform, PlatformError, Quotas, Workspace};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::auth::{AuthUser, client_info};
use super::error::ApiError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bootstrap", post(bootstrap))
        .route("/me", get(me))
        .route("/workspaces", get(list).post(create))
        .route("/workspaces/{id}", get(get_one).patch(rename))
        .route("/workspaces/{id}/suspend", post(suspend))
        .route("/workspaces/{id}/restore", post(restore))
        .route("/workspaces/{id}/schedule-deletion", post(schedule_deletion))
        .route("/workspaces/{id}/quotas", put(quotas))
}

fn platform(s: &AppState) -> Result<&Platform, ApiError> {
    s.platform.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<PlatformError> for ApiError {
    fn from(e: PlatformError) -> Self {
        match e {
            PlatformError::BootstrapInvalid => {
                Self::new(401, "bootstrap_invalid", "Bootstrap token invalid, expired or already used")
            }
            PlatformError::AlreadyBootstrapped => {
                Self::new(409, "already_bootstrapped", "The platform is already bootstrapped")
            }
            PlatformError::NotFound => Self::new(404, "not_found", "Not found"),
            PlatformError::SlugTaken => Self::new(409, "slug_taken", "Slug already in use"),
            PlatformError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            PlatformError::BadTransition(from) => Self::new(409, "state_conflict", "Transition not allowed")
                .detail(format!("current state: {from}")),
            PlatformError::StaleRevision { .. } => {
                Self::new(412, "precondition_failed", "The workspace changed; reload and retry")
            }
            PlatformError::Identity(i) => Self::from(i),
            PlatformError::Members(m) => Self::from(m),
            PlatformError::Internal(m) => {
                tracing::error!(error = %m, "platform internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

/// A system-plane caller: valid access token + at least one system role + MFA-authenticated.
#[derive(Debug, Clone)]
pub struct SystemUser {
    pub user_id: Uuid,
    pub roles: Vec<String>,
}

impl FromRequestParts<AppState> for SystemUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, s).await?;
        let roles = platform(s)?.system_roles(claims.sub).await?;
        if roles.is_empty() {
            return Err(ApiError::not_found()); // the system plane is invisible to non-system users
        }
        if !claims.amr.iter().any(|m| m == "otp" || m == "hwk") {
            return Err(ApiError::new(403, "mfa_required", "Multi-factor authentication required").detail(
                "System administration requires MFA. Enrol at POST /api/v1/me/mfa/totp and sign in again.",
            ));
        }
        Ok(Self { user_id: claims.sub, roles })
    }
}

impl SystemUser {
    fn require(&self, action: SystemAction) -> Result<(), ApiError> {
        if can(&self.roles, action) {
            Ok(())
        } else {
            Err(ApiError::new(403, "forbidden", "Your system role does not allow this action"))
        }
    }
}

fn if_match(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok())
}

fn ws_response(status: StatusCode, ws: &Workspace) -> Response {
    let mut res = (status, Json(ws)).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", ws.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapReq {
    token: String,
    email: String,
    display_name: String,
    password: String,
}

async fn bootstrap(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<BootstrapReq>,
) -> Result<Response, ApiError> {
    let user = platform(&s)?
        .bootstrap(&r.token, &r.email, &r.display_name, &r.password, &client_info(&headers))
        .await?;
    Ok((StatusCode::CREATED, Json(json!({"user": user, "roles": ["system.owner"], "next": "Sign in, then enrol MFA at POST /api/v1/me/mfa/totp before using the system API."}))).into_response())
}

async fn me(u: SystemUser) -> Json<serde_json::Value> {
    Json(json!({"user_id": u.user_id, "roles": u.roles}))
}

async fn list(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    Ok(Json(json!({"items": platform(&s)?.list_workspaces().await?})).into_response())
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Json(r): Json<NewWorkspace>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let ws = platform(&s)?.create_workspace(u.user_id, &r, &client_info(&headers)).await?;
    let mut res = ws_response(StatusCode::CREATED, &ws);
    if let Ok(v) = HeaderValue::from_str(&format!("/sysapi/v1/workspaces/{}", ws.id)) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    Ok(res)
}

async fn get_one(
    State(s): State<AppState>,
    u: SystemUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    Ok(ws_response(StatusCode::OK, &platform(&s)?.get_workspace(id).await?))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameReq {
    name: String,
}

async fn rename(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(r): Json<RenameReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let ws = platform(&s)?.rename(u.user_id, id, if_match(&headers), &r.name, &client_info(&headers)).await?;
    Ok(ws_response(StatusCode::OK, &ws))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuspendReq {
    mode: String,
    reason: String,
}

async fn suspend(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(r): Json<SuspendReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let locked = match r.mode.as_str() {
        "read_only" => false,
        "locked" => true,
        _ => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("mode must be read_only or locked"));
        }
    };
    let ws = platform(&s)?
        .suspend(u.user_id, id, if_match(&headers), locked, &r.reason, &client_info(&headers))
        .await?;
    Ok(ws_response(StatusCode::OK, &ws))
}

async fn restore(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let ws = platform(&s)?.restore(u.user_id, id, if_match(&headers), &client_info(&headers)).await?;
    Ok(ws_response(StatusCode::OK, &ws))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionReq {
    grace_days: i64,
}

async fn schedule_deletion(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(r): Json<DeletionReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let ws = platform(&s)?
        .schedule_deletion(u.user_id, id, if_match(&headers), r.grace_days, &client_info(&headers))
        .await?;
    Ok(ws_response(StatusCode::OK, &ws))
}

async fn quotas(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(q): Json<Quotas>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    let ws = platform(&s)?.set_quotas(u.user_id, id, &q, &client_info(&headers)).await?;
    Ok(ws_response(StatusCode::OK, &ws))
}
