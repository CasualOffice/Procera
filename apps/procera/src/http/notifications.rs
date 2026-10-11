//! The signed-in member's in-app inbox (FR-N01). Self-service: only the caller's own notifications.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_notify::{Notify, NotifyError};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(inbox))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/{id}/read", post(read_one))
}

fn notify(s: &AppState) -> Result<&Notify, ApiError> {
    s.notify.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<NotifyError> for ApiError {
    fn from(e: NotifyError) -> Self {
        match e {
            NotifyError::NotFound => Self::not_found(),
            other => {
                tracing::error!(error = %other, "notify error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "notifications api internal error");
    ApiError::new(500, "internal", "Internal error")
}

#[derive(Debug, Deserialize)]
struct InboxQuery {
    #[serde(default)]
    unread: bool,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn inbox(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<InboxQuery>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let (items, unread) = notify(&s)?.inbox(&mut tx, me.actor.membership, q.unread, q.limit).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items, "unread_count": unread })).into_response())
}

async fn read_one(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    notify(&s)?.mark_read(&mut tx, me.actor.membership, Some(id)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn read_all(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let n = notify(&s)?.mark_read(&mut tx, me.actor.membership, None).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "marked": n })).into_response())
}
