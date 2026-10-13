//! Requests API (BPMS-103, docs/08 "Requests", §7.1): the built-in request types a member may
//! start, starting one (`Idempotency-Key` required), my and all requests, the detail with steps
//! and timeline, and cancelling. `request:start` lists types and starts; `request:view_own` lists
//! your own; `request:view_all` lists and opens every request; others open a request only when
//! involved (started it, are offered a step, or decided one). Cancelling needs to be the initiator
//! or hold `request:cancel`.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::idempotency::{self, Claim};
use procera_requests::{Actor, NewComment, RequestError, Requests, Scope};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/request-types", get(types))
        .route("/requests", get(list).post(start))
        .route("/requests/preview", post(preview))
        .route("/requests/{id}", get(detail))
        .route("/requests/{id}/cancel", post(cancel))
        .route("/requests/{id}/comments", get(conversation).post(comment))
        .route("/requests/{id}/watch", put(watch).delete(unwatch))
}

pub(crate) fn requests(s: &AppState) -> Result<&Requests, ApiError> {
    s.requests.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

pub(crate) fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "requests api internal error");
    ApiError::new(500, "internal", "Internal error")
}

impl From<RequestError> for ApiError {
    fn from(e: RequestError) -> Self {
        let field = |p: &str, code: &str, m: &str| procera_kernel::problem::FieldError {
            pointer: p.to_owned(),
            code: code.to_owned(),
            message: m.to_owned(),
        };
        match e {
            RequestError::NotFound => Self::not_found(),
            RequestError::UnknownType(t) => {
                let mut a = Self::new(422, "validation_failed", "Validation failed")
                    .detail(format!("unknown request type {t}"));
                a.problem.errors = vec![field("/type", "unknown_type", "Not a request type you can start")];
                a
            }
            RequestError::Invalid(errors) => {
                let mut a = Self::new(422, "validation_failed", "Validation failed")
                    .detail(format!("{} field(s) are invalid", errors.len()));
                a.problem.errors =
                    errors.into_iter().map(|e| field(&e.pointer, &e.code, &e.message)).collect();
                a
            }
            RequestError::Forbidden(m) => {
                Self::new(403, "forbidden", "You do not have permission for this action").detail(m)
            }
            RequestError::SoD(m) => {
                Self::new(403, "sod_violation", "Separation of duties does not allow this").detail(m)
            }
            RequestError::AlreadyDecided => {
                Self::new(409, "already_decided", "Someone already decided this step")
            }
            RequestError::Stale { current } => {
                Self::new(412, "precondition_failed", "This step changed; reload it")
                    .detail(format!("current revision is {current}"))
            }
            RequestError::Conflict(m) => {
                Self::new(409, "state_conflict", "Not allowed in the current state").detail(m)
            }
            RequestError::Internal(m) => internal(m),
        }
    }
}

pub(crate) fn actor(me: &WsMember, headers: &HeaderMap) -> Actor {
    let c = super::auth::client_info(headers);
    Actor {
        membership: me.actor.membership,
        subject_ref: me.actor.subject_ref,
        ip: c.ip,
        user_agent: c.user_agent,
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypesQuery {
    scope: Option<String>,
}

/// The request types the caller may start (enabled in this workspace, and the caller is in the
/// type's audience), with their start forms priced in the workspace currency. `?scope=all` lists
/// every built-in with `enabled` and `audience`, for Settings › Requests (`workspace:settings.view`).
async fn types(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<TypesQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let r = requests(&s)?;
    let (offer, listed, all) = match q.scope.as_deref() {
        None | Some("mine") => {
            me.require(m, &mut tx, "request:start", Resource::Workspace).await?;
            let (offer, offered) = r.offered(&mut tx, me.actor.membership).await?;
            (offer, offered, false)
        }
        Some("all") => {
            me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
            (r.offer(&mut tx).await?, r.catalogue().list().to_vec(), true)
        }
        Some(other) => {
            return Err(ApiError::new(400, "bad_request", "Unknown scope")
                .detail(format!("{other}: use mine or all")));
        }
    };
    tx.rollback().await.map_err(internal)?;
    let mut items = Vec::new();
    for t in &listed {
        let mut item = json!({
            "key": t.key, "name": t.name, "description": t.description(&offer.currency),
            "category": t.category, "icon": t.icon,
            "form": procera_requests::RequestType::priced(&t.start_form, &offer.currency),
        });
        if all {
            item["enabled"] = json!(offer.enabled(t.key));
            item["audience"] = serde_json::to_value(offer.audience(t.key)?).map_err(internal)?;
        }
        items.push(item);
    }
    Ok(Json(json!({ "items": items, "currency": offer.currency })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartReq {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    input: Map<String, Value>,
}

pub(crate) fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|k| (1..=200).contains(&k.len()))
        .map(str::to_owned)
}

pub(crate) fn bad_body(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(400, "bad_request", "The request body is not valid").detail(e.to_string())
}

async fn start(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    body: Bytes,
) -> Result<Response, ApiError> {
    let key = idempotency_key(&headers).ok_or_else(|| {
        ApiError::new(400, "idempotency_key_required", "Idempotency-Key header required")
            .detail("send a unique Idempotency-Key with every new request")
    })?;
    let r: StartReq = serde_json::from_slice(&body).map_err(bad_body)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "request:start", Resource::Workspace).await?;
    let hash = idempotency::request_hash("POST", "/api/v1/requests", &body);
    match idempotency::claim(&mut tx, me.actor.membership, "requests.start", &key, &hash)
        .await
        .map_err(internal)?
    {
        Claim::Fresh => {}
        Claim::Replay { status, response } => {
            tx.rollback().await.map_err(internal)?;
            let code = StatusCode::from_u16(u16::try_from(status).unwrap_or(200)).unwrap_or(StatusCode::OK);
            return Ok((code, Json(response)).into_response());
        }
        Claim::Conflict => {
            return Err(ApiError::new(
                409,
                "idempotency_key_reused",
                "This Idempotency-Key was used for another request",
            ));
        }
    }
    let started = requests(&s)?.start(&mut tx, &actor(&me, &headers), &r.ty, &r.input).await?;
    let body = serde_json::to_value(&started).map_err(internal)?;
    idempotency::complete(&mut tx, me.actor.membership, "requests.start", &key, 201, &body)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, [("location", format!("/api/v1/requests/{}", started.id))], Json(body))
        .into_response())
}

/// "Who approves this" before sending (`request:start`): a dry run of the routing for the answers
/// so far. A read: nothing is written, nothing is audited, the transaction rolls back.
async fn preview(State(s): State<AppState>, me: WsMember, body: Bytes) -> Result<Response, ApiError> {
    let r: StartReq = serde_json::from_slice(&body).map_err(bad_body)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "request:start", Resource::Workspace).await?;
    let p = requests(&s)?.preview(&mut tx, me.actor.membership, &r.ty, &r.input).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(p).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    scope: Option<String>,
    status: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let scope = match q.scope.as_deref().unwrap_or("mine") {
        "mine" => {
            me.require(m, &mut tx, "request:view_own", Resource::Workspace).await?;
            Scope::Mine(me.actor.membership)
        }
        "all" => {
            me.require(m, &mut tx, "request:view_all", Resource::Workspace).await?;
            Scope::All
        }
        other => {
            return Err(ApiError::new(400, "bad_request", "Unknown scope")
                .detail(format!("{other}: use mine or all")));
        }
    };
    if let Some(st) = q.status.as_deref()
        && !matches!(
            st,
            "in_progress" | "changes_requested" | "approved" | "rejected" | "cancelled" | "completed"
        )
    {
        return Err(ApiError::new(400, "bad_request", "Unknown status").detail(st.to_owned()));
    }
    let items = requests(&s)?.list(&mut tx, scope, q.status.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items, "next_cursor": null })).into_response())
}

/// Whether the caller may open a request: `request:view_all`, or involved in it. Hidden ≡ missing.
pub(crate) async fn can_view(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    id: Uuid,
) -> Result<(), ApiError> {
    if may_view(s, tx, me.actor.membership, id).await? { Ok(()) } else { Err(ApiError::not_found()) }
}

/// Whether member `who` may see request `id`: every request (`request:view_all` workspace-wide),
/// one they are involved in, or one started by someone in their team — a department they hold
/// `request:view_all` over (a Team Manager) or their reporting line (docs/12 §4).
pub(crate) async fn may_view(
    s: &AppState,
    tx: &mut procera_db::WsTx<'_>,
    who: Uuid,
    id: Uuid,
) -> Result<bool, ApiError> {
    let r = requests(s)?;
    let requester = r.initiator(tx, id).await?;
    let m = members(s)?;
    if m.access().check(tx, who, "request:view_all", &Resource::Workspace).await?.allowed
        || r.involved(tx, id, who).await?
        || m.access().check(tx, who, "request:view_all", &Resource::Member(requester)).await?.allowed
    {
        return Ok(true);
    }
    Ok(r.reports_to(tx, requester, who, &[]).await?)
}

async fn may_cancel(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    id: Uuid,
) -> Result<bool, ApiError> {
    if requests(s)?.initiator(tx, id).await? == me.actor.membership {
        return Ok(true);
    }
    Ok(members(s)?
        .access()
        .check(tx, me.actor.membership, "request:cancel", &Resource::Workspace)
        .await?
        .allowed)
}

async fn detail(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    can_view(&s, &me, &mut tx, id).await?;
    let d = requests(&s)?.detail(&mut tx, id).await?;
    let open = matches!(d.summary.status.as_str(), "in_progress" | "changes_requested");
    let can_cancel = open && may_cancel(&s, &me, &mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    let mut body = serde_json::to_value(&d).map_err(internal)?;
    body["can_cancel"] = json!(can_cancel);
    Ok(Json(body).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelReq {
    reason: String,
}

async fn cancel(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CancelReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    can_view(&s, &me, &mut tx, id).await?;
    if !may_cancel(&s, &me, &mut tx, id).await? {
        return Err(ApiError::new(
            403,
            "forbidden",
            "Only the requester or an administrator can cancel this request",
        )
        .detail("request:cancel"));
    }
    let out = requests(&s)?.cancel(&mut tx, &actor(&me, &headers), id, &r.reason).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(out).into_response())
}

/// The conversation on a request (internal notes hidden from the requester) and its watchers.
async fn conversation(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    can_view(&s, &me, &mut tx, id).await?;
    let c = requests(&s)?.conversation(&mut tx, id, me.actor.membership).await?;
    let can_comment = m
        .access()
        .check(&mut tx, me.actor.membership, "request:comment", &Resource::Workspace)
        .await?
        .allowed;
    let can_internal = can_comment && requests(&s)?.initiator(&mut tx, id).await? != me.actor.membership;
    tx.rollback().await.map_err(internal)?;
    let mut body = serde_json::to_value(&c).map_err(internal)?;
    body["can_comment"] = json!(can_comment);
    body["can_post_internal"] = json!(can_internal);
    Ok(Json(body).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentReq {
    body: String,
    #[serde(default)]
    internal: bool,
    #[serde(default)]
    mentions: Vec<Uuid>,
}

/// Posts a comment (`request:comment` and able to see the request). Each mentioned member must be
/// able to see the request too (422 on `/mentions` naming who can't). Audited `request.commented`.
async fn comment(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CommentReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    can_view(&s, &me, &mut tx, id).await?;
    me.require(m, &mut tx, "request:comment", Resource::Workspace).await?;
    let mut blind = Vec::new();
    for who in &r.mentions {
        let active: Option<String> = sqlx::query_scalar("SELECT status FROM memberships WHERE id = $1")
            .bind(who)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
        if active.as_deref() != Some("active") || !may_view(&s, &mut tx, *who, id).await? {
            let name: Option<String> = sqlx::query_scalar(
                "SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
            )
            .bind(who)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
            blind.push(name.unwrap_or_else(|| who.to_string()));
        }
    }
    if !blind.is_empty() {
        return Err(RequestError::Invalid(vec![procera_requests::FieldError::new(
            "mentions",
            "cannot_see",
            &format!("{} can’t see this request, so they can’t be mentioned.", blind.join(", ")),
        )])
        .into());
    }
    let c = NewComment { body: r.body, internal: r.internal, mentions: r.mentions };
    let out = requests(&s)?.comment(&mut tx, &actor(&me, &headers), id, &c).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(out)).into_response())
}

async fn set_watch(
    s: &AppState,
    headers: &HeaderMap,
    me: &WsMember,
    id: Uuid,
    on: bool,
) -> Result<Response, ApiError> {
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    can_view(s, me, &mut tx, id).await?;
    requests(s)?.watch(&mut tx, &actor(me, headers), id, on).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "watching": on })).into_response())
}

/// Follows a request: new comments and its outcome reach the caller. Audited `request.watched`.
async fn watch(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    set_watch(&s, &headers, &me, id, true).await
}

/// Stops following a request. Audited `request.unwatched`.
async fn unwatch(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    set_watch(&s, &headers, &me, id, false).await
}
