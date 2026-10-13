//! Outgoing webhooks (BPMS-143, docs/08 "Webhooks"): endpoints (view `connector:view`, change
//! `connector:configure`, roll the secret `connector:manage_secrets`), their deliveries, replay and a
//! test event, and the event catalogue. API tokens can't manage webhooks (`/webhooks*` changes need
//! the configure permission, which a token rarely holds; secrets are only ever shown once).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_webhooks::{Actor, EVENTS, WebhookError};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/webhook-events", get(catalogue))
        .route("/webhooks", get(list).post(create))
        .route("/webhooks/{id}", get(one).patch(update).delete(remove))
        .route("/webhooks/{id}/rotate-secret", post(rotate))
        .route("/webhooks/{id}/test", post(test))
        .route("/webhooks/{id}/deliveries", get(deliveries))
        .route("/webhooks/deliveries/{id}/replay", post(replay))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "webhooks api internal error");
    ApiError::new(500, "internal", "Internal error")
}

impl From<WebhookError> for ApiError {
    fn from(e: WebhookError) -> Self {
        match e {
            WebhookError::NotFound => Self::not_found(),
            WebhookError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            WebhookError::Conflict(m) if m == "stale" => {
                Self::new(412, "precondition_failed", "It changed since you loaded it")
                    .detail("Reload and try again.")
            }
            WebhookError::Conflict(m) => Self::new(409, "conflict", "That can't be done now").detail(m),
            WebhookError::Internal(m) => internal(m),
        }
    }
}

fn hooks(s: &AppState) -> Result<&procera_webhooks::Webhooks, ApiError> {
    s.webhooks.as_ref().ok_or_else(|| ApiError::unavailable("webhooks"))
}

fn actor(me: &WsMember, headers: &HeaderMap) -> Actor {
    let c = super::auth::client_info(headers);
    Actor {
        membership: me.actor.membership,
        subject_ref: me.actor.subject_ref,
        ip: c.ip,
        user_agent: c.user_agent,
    }
}

fn if_match(headers: &HeaderMap) -> Option<i32> {
    headers.get(header::IF_MATCH)?.to_str().ok()?.trim().trim_matches('"').parse().ok()
}

/// `GET /webhook-events`: the event types a webhook can receive, with what each means.
async fn catalogue(_me: WsMember) -> Response {
    Json(json!({ "items": EVENTS.iter().map(|e| json!({ "type": e.1, "description": e.2 })).collect::<Vec<_>>() })).into_response()
}

/// `GET /webhooks`: the workspace's endpoints with their last delivery (`connector:view`).
async fn list(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:view", Resource::Workspace).await?;
    let items = hooks(&s)?.list(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    url: String,
    events: Vec<String>,
    #[serde(default)]
    description: Option<String>,
}

/// `POST /webhooks`: a new endpoint (`url` https, `events` types or `["*"]`, `description`;
/// `connector:configure`; at most 20). 201 with the endpoint and its `secret` (shown only here).
/// 422 for a URL inside a private network or a port other than 443/80/8443/8080.
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:configure", Resource::Workspace).await?;
    let (ep, secret) = hooks(&s)?
        .create(&mut tx, &actor(&me, &headers), &r.url, &r.events, r.description.as_deref())
        .await?;
    tx.commit().await.map_err(internal)?;
    let mut v = serde_json::to_value(ep).map_err(internal)?;
    v["secret"] = json!(secret);
    Ok((StatusCode::CREATED, Json(v)).into_response())
}

/// `GET /webhooks/{id}`: one endpoint (`connector:view`), with an `ETag` of its revision.
async fn one(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:view", Resource::Workspace).await?;
    let ep = hooks(&s)?.get(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    let rev = ep.revision;
    let mut res = Json(ep).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{rev}\"")) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

fn explicit<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    events: Option<Vec<String>>,
    #[serde(default, deserialize_with = "explicit")]
    description: Option<Option<String>>,
    #[serde(default)]
    enabled: Option<bool>,
}

/// `PATCH /webhooks/{id}`: changes the URL, events, description or on/off (`connector:configure`;
/// honours `If-Match`). Turning an endpoint back on clears why it was turned off.
async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<UpdateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:configure", Resource::Workspace).await?;
    let ep = hooks(&s)?
        .update(
            &mut tx,
            &actor(&me, &headers),
            id,
            if_match(&headers),
            r.url.as_deref(),
            r.events.as_deref(),
            r.description.as_ref().map(|d| d.as_deref()),
            r.enabled,
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(ep).into_response())
}

/// `DELETE /webhooks/{id}`: removes an endpoint and its delivery log (`connector:configure`). 204.
async fn remove(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:configure", Resource::Workspace).await?;
    hooks(&s)?.delete(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /webhooks/{id}/rotate-secret`: a new signing secret (`connector:manage_secrets`); the
/// previous one also signs for 24 hours. Returns `{secret, previous_valid_until}`.
async fn rotate(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:manage_secrets", Resource::Workspace).await?;
    me.require_step_up(&s, &mut tx).await?;
    let h = hooks(&s)?;
    let secret = h.rotate(&mut tx, &actor(&me, &headers), id).await?;
    let ep = h.get(&mut tx, id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "secret": secret, "previous_valid_until": ep.previous_secret_until })).into_response())
}

/// `POST /webhooks/{id}/test`: queues a `webhook.test` event for the endpoint
/// (`connector:configure`); the next delivery pass sends it. 202 with the delivery.
async fn test(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:configure", Resource::Workspace).await?;
    let d = hooks(&s)?.test(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::ACCEPTED, Json(d)).into_response())
}

/// `GET /webhooks/{id}/deliveries`: the endpoint's deliveries, newest first (at most 200), each with
/// its state, attempts, last status, error and duration, a response excerpt and the event body
/// (`connector:view`).
async fn deliveries(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:view", Resource::Workspace).await?;
    let items = hooks(&s)?.deliveries(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `POST /webhooks/deliveries/{id}/replay`: sends a delivery again from the start of its retry
/// schedule, with the same event id (`connector:configure`).
async fn replay(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "connector:configure", Resource::Workspace).await?;
    let d = hooks(&s)?.replay(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(d).into_response())
}
