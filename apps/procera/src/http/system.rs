//! System API (`/sysapi/v1`, docs/08 §5). Every route requires a system role and MFA (docs/06 §3.6),
//! except the one-time bootstrap. Workspace lifecycle and quota changes also need a recent
//! authentication (step-up, docs/12 §11). The console reads metadata, counts and health only; it
//! never reads workspace content (docs/12 §1). Console reads live in [`super::system_console`].

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_platform::authz::{SystemAction, can};
use procera_platform::{
    ConsoleWorkspace, NewWorkspace, Platform, PlatformError, Quotas, Workspace, WorkspaceFilter,
};
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
        .route("/settings", get(platform_settings))
        .route("/settings/{key}", put(set_floor))
        .merge(super::system_console::router())
}

pub(super) fn platform(s: &AppState) -> Result<&Platform, ApiError> {
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
            PlatformError::Settings(e) => Self::from(e),
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
    /// When the user last authenticated (sign-in or step-up), unix seconds.
    pub auth_time: i64,
}

/// The system plane's step-up window (docs/12 §11 default; the system plane has no workspace settings).
pub const SYSTEM_STEP_UP_SECONDS: i64 = 300;

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
        Ok(Self { user_id: claims.sub, roles, auth_time: claims.auth_time })
    }
}

impl SystemUser {
    /// Requires a recent authentication for lifecycle and quota changes (docs/12 §11).
    pub fn require_step_up(&self, s: &AppState) -> Result<(), ApiError> {
        let age = s.clock.now().unix_timestamp() - self.auth_time;
        if (0..=SYSTEM_STEP_UP_SECONDS).contains(&age) {
            Ok(())
        } else {
            Err(ApiError::new(401, "step_up_required", "Confirm it's you to continue").detail(format!(
                "POST /api/v1/session/step-up; this action needs authentication within the last {SYSTEM_STEP_UP_SECONDS} seconds"
            )))
        }
    }

    pub fn require(&self, action: SystemAction) -> Result<(), ApiError> {
        if can(&self.roles, action) {
            Ok(())
        } else {
            Err(ApiError::new(403, "forbidden", "Your system role does not allow this action"))
        }
    }
}

pub(super) fn if_match(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok())
}

fn ws_response(status: StatusCode, ws: &Workspace) -> Response {
    etag((status, Json(ws)).into_response(), ws.revision)
}

pub(super) fn etag(mut res: Response, revision: i64) -> Response {
    if let Ok(v) = HeaderValue::from_str(&format!("\"{revision}\"")) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

/// A lifecycle response: the workspace with its usage, as the console lists it.
async fn console_response(s: &AppState, ws: &Workspace) -> Result<Response, ApiError> {
    let row: ConsoleWorkspace = platform(s)?.console_workspace(ws.id).await?;
    Ok(etag((StatusCode::OK, Json(&row)).into_response(), row.workspace.revision))
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

/// The console's workspace grid (`?state=&profile=&q=`): metadata, quotas and usage counts.
async fn list(
    State(s): State<AppState>,
    u: SystemUser,
    Query(f): Query<WorkspaceFilter>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    Ok(Json(json!({"items": platform(&s)?.console_workspaces(&f).await?})).into_response())
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
    let row = platform(&s)?.console_workspace(id).await?;
    Ok(etag((StatusCode::OK, Json(&row)).into_response(), row.workspace.revision))
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
    u.require_step_up(&s)?;
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
    console_response(&s, &ws).await
}

async fn restore(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    u.require_step_up(&s)?;
    let ws = platform(&s)?.restore(u.user_id, id, if_match(&headers), &client_info(&headers)).await?;
    console_response(&s, &ws).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionReq {
    grace_days: i64,
    reason: String,
}

async fn schedule_deletion(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(r): Json<DeletionReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    u.require_step_up(&s)?;
    let ws = platform(&s)?
        .schedule_deletion(u.user_id, id, if_match(&headers), r.grace_days, &r.reason, &client_info(&headers))
        .await?;
    console_response(&s, &ws).await
}

async fn quotas(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(id): Path<Uuid>,
    Json(q): Json<Quotas>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceWrite)?;
    u.require_step_up(&s)?;
    let ws = platform(&s)?.set_quotas(u.user_id, id, &q, &client_info(&headers)).await?;
    console_response(&s, &ws).await
}

async fn platform_settings(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::WorkspaceRead)?;
    let db = s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let mut tx =
        db.begin_system().await.map_err(|e| ApiError::from(procera_settings::SettingsError::from(e)))?;
    let items = platform(&s)?.settings().platform_values(&mut tx).await?;
    tx.rollback().await.map_err(|e| ApiError::from(procera_settings::SettingsError::from(e)))?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FloorReq {
    value: serde_json::Value,
    reason: String,
}

/// Sets a platform floor. Raising it never rewrites workspace values; looser ones resolve as clamped.
async fn set_floor(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Path(key): Path<String>,
    Json(r): Json<FloorReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfig)?;
    let p = platform(&s)?;
    let subject = s
        .identity
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("database"))?
        .me(u.user_id)
        .await?
        .ok_or_else(ApiError::not_found)?
        .subject_ref;
    let db = s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let err = |e: procera_db::DbError| ApiError::from(procera_settings::SettingsError::from(e));
    let mut tx = db.begin_system().await.map_err(err)?;
    let c = client_info(&headers);
    let v = p
        .settings()
        .set_floor(&mut tx, subject, u.user_id, &key, &r.value, &r.reason, (c.ip, c.user_agent))
        .await?;
    tx.commit().await.map_err(err)?;
    super::settings::invalidate_floors(&s).await;
    Ok(Json(v).into_response())
}
