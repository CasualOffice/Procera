//! Workspace settings API (BPMS-027, docs/20 §1-2): effective values with the explainer, history and
//! updates. Platform floors live on the system plane (`/sysapi/v1/settings`).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use procera_access::Resource;
use procera_settings::{Settings, SettingsError};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(list))
        .route("/settings/effective", get(effective))
        .route("/settings/{key}", get(effective_by_path).put(update))
        .route("/settings/{key}/history", get(history))
}

pub fn settings(s: &AppState) -> Result<&Settings, ApiError> {
    s.platform.as_ref().map(|p| p.settings()).ok_or_else(|| ApiError::unavailable("database"))
}

impl From<SettingsError> for ApiError {
    fn from(e: SettingsError) -> Self {
        match e {
            SettingsError::UnknownKey(k) => Self::new(404, "unknown_setting", "Unknown setting").detail(k),
            SettingsError::Invalid { key, message } => {
                Self::new(422, "validation_failed", "Validation failed").detail(format!("{key}: {message}"))
            }
            SettingsError::BelowFloor { key, floor } => {
                Self::new(422, "setting_below_floor", "Looser than the platform floor")
                    .detail(format!("{key} must be at least as strict as the platform floor {floor}"))
            }
            SettingsError::Locked(k) => {
                Self::new(403, "setting_locked", "This setting is locked by the platform").detail(k)
            }
            SettingsError::ReasonRequired(k) => {
                Self::new(422, "reason_required", "A reason is required for this change").detail(k)
            }
            SettingsError::StaleRevision { current } => {
                Self::new(412, "precondition_failed", "The setting changed; reload and retry")
                    .detail(format!("current revision {current}"))
            }
            SettingsError::Internal(m) => {
                tracing::error!(error = %m, "settings internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "settings api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn client(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let c = client_info(headers);
    (c.ip, c.user_agent)
}

async fn list(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let items = settings(&s)?.effective_all(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
struct KeyQuery {
    key: String,
}

/// The explainer: final value and every contributing layer (docs/20 §1).
async fn effective(
    s: State<AppState>,
    me: WsMember,
    Query(q): Query<KeyQuery>,
) -> Result<Response, ApiError> {
    explain(s, me, q.key).await
}

async fn effective_by_path(
    s: State<AppState>,
    me: WsMember,
    Path(key): Path<String>,
) -> Result<Response, ApiError> {
    explain(s, me, key).await
}

async fn explain(State(s): State<AppState>, me: WsMember, key: String) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let e = settings(&s)?.effective(&mut tx, &key).await?;
    tx.rollback().await.map_err(internal)?;
    let mut res = Json(&e).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", e.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

async fn history(
    State(s): State<AppState>,
    me: WsMember,
    Path(key): Path<String>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let items = settings(&s)?.history(&mut tx, &key).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    value: Value,
    #[serde(default)]
    reason: Option<String>,
}

async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(key): Path<String>,
    Json(r): Json<UpdateReq>,
) -> Result<Response, ApiError> {
    let d = procera_settings::def(&key)?;
    let perm = d.perm.ok_or_else(|| SettingsError::Locked(key.clone()))?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, perm, Resource::Workspace).await?;
    if d.step_up {
        me.require_step_up(&s, &mut tx).await?;
    }
    let expected = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok());
    let e = settings(&s)?
        .set_workspace(
            &mut tx,
            (me.user_id, me.actor.subject_ref),
            &key,
            &r.value,
            r.reason.as_deref(),
            expected,
            client(&headers),
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    let mut res = (StatusCode::OK, Json(&e)).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", e.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}
