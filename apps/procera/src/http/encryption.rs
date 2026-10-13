//! Workspace encryption keys (BPMS-030; docs/04 §8, docs/06 §4): the KEK versions and how many values
//! each still wraps, and rotation. Rotation needs `workspace:settings.manage`, a reason and a recent
//! sign-in; the rewrap job moves existing values to the new version in the background.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/security/encryption", get(status))
        .route("/security/encryption/rotate", post(rotate))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "encryption api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn keys(s: &AppState) -> Result<&procera_keys::Keys, ApiError> {
    s.keys.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

/// `GET /security/encryption`: the active key version, every key version with how many values it
/// still wraps, and how many await rewrap. Needs `workspace:settings.view`.
async fn status(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let k = keys(&s)?;
    let (active, _) = k.active(&mut tx).await.map_err(internal)?;
    let versions = k.status(&mut tx).await.map_err(internal)?;
    // `active()` may have created v1 on first use: keep it.
    tx.commit().await.map_err(internal)?;
    let pending: i64 = versions.iter().filter(|v| v.version != active).map(|v| v.values).sum();
    Ok(Json(
        json!({ "algorithm": "AES-256-GCM", "active_version": active, "pending_rewrap": pending, "versions": versions }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RotateReq {
    reason: String,
}

/// `POST /security/encryption/rotate`: rotates to a new key version; existing values are rewrapped in
/// the background. Needs `workspace:settings.manage`, a recent sign-in and a `reason` (422 if empty).
async fn rotate(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<RotateReq>,
) -> Result<Json<Value>, ApiError> {
    let reason = r.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("a reason is required to rotate the key"));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.manage", Resource::Workspace).await?;
    me.require_step_up(&s, &mut tx).await?;
    let c = super::auth::client_info(&headers);
    let version = keys(&s)?
        .rotate(&mut tx, Some(me.actor.subject_ref), Some(reason), (c.ip, c.user_agent))
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "active_version": version })))
}
