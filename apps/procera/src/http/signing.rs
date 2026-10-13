//! Employee e-signing (BPMS-107, docs/08 "Signing"): envelopes from a PDF, recipients, fields,
//! send and void for senders (`document:send_for_signature`; everyone's with `document:view_all`,
//! voiding anyone's with `document:void`), and the signing ceremony for recipients: open it, sign
//! (consent, a typed or drawn signature, their fields) or decline. `strong` envelopes need a recent
//! sign-in to sign. API tokens never reach `/signing/*`. The document is served from here.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_signing::{Field, Listing, SignInput, SigningError};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/envelopes", get(list).post(create))
        .route("/envelopes/{id}", get(one))
        .route("/envelopes/{id}/recipients", put(recipients))
        .route("/envelopes/{id}/fields", put(fields))
        .route("/envelopes/{id}/send", post(send))
        .route("/envelopes/{id}/void", post(void))
        .route("/envelopes/{id}/send-bulk", post(send_bulk))
        .route("/envelopes/{id}/remind", post(remind))
        .route("/envelope-batches", get(batches))
        .route("/envelope-batches/{id}", get(batch))
        .route("/envelope-batches/{id}/remind", post(remind_batch))
        .route("/envelope-batches/{id}/void", post(void_batch))
        .route("/envelopes/{id}/document", get(document))
        .route("/me/signatures", get(inbox))
        .route("/signing/{recipient}", get(ceremony))
        .route("/signing/{recipient}/sign", post(sign))
        .route("/signing/{recipient}/decline", post(decline))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "signing api internal error");
    ApiError::new(500, "internal", "Internal error")
}

impl From<SigningError> for ApiError {
    fn from(e: SigningError) -> Self {
        match e {
            SigningError::NotFound => Self::not_found(),
            SigningError::Forbidden(m) => Self::new(403, "forbidden", "Not allowed").detail(m),
            SigningError::Conflict(m) if m == "stale" => {
                Self::new(412, "precondition_failed", "It changed since you loaded it")
                    .detail("Reload and try again.")
            }
            SigningError::Conflict(m) => Self::new(409, "conflict", "That can't be done now").detail(m),
            SigningError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            SigningError::Internal(m) => internal(m),
        }
    }
}

fn signing(s: &AppState) -> Result<&procera_signing::Signing, ApiError> {
    s.signing.as_ref().ok_or_else(|| ApiError::unavailable("signing"))
}

/// The caller as a signing actor, with how they signed in, in words.
fn actor(me: &WsMember, headers: &HeaderMap) -> procera_signing::Actor {
    let c = super::auth::client_info(headers);
    let method: Vec<&str> = me
        .amr
        .iter()
        .map(|a| match a.as_str() {
            "pwd" => "password",
            "otp" => "authenticator code",
            "hwk" | "swk" => "passkey",
            other => other,
        })
        .collect();
    procera_signing::Actor {
        membership: me.actor.membership,
        subject_ref: me.actor.subject_ref,
        ip: c.ip,
        user_agent: c.user_agent,
        auth_method: if method.is_empty() { "session".to_owned() } else { method.join(" + ") },
        auth_time: OffsetDateTime::from_unix_timestamp(me.auth_time).unwrap_or(OffsetDateTime::UNIX_EPOCH),
    }
}

/// An optional `If-Match: "<revision>"`.
fn if_match(headers: &HeaderMap) -> Option<i32> {
    headers.get(header::IF_MATCH)?.to_str().ok()?.trim().trim_matches('"').parse().ok()
}

async fn can(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    perm: &str,
) -> Result<bool, ApiError> {
    Ok(members(s)?.access().check(tx, me.actor.membership, perm, &Resource::Workspace).await?.allowed)
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    scope: Option<String>,
}

/// `GET /envelopes?scope=sent|all`: envelopes I sent (default), or every envelope with
/// `document:view_all`; newest first, at most 500, without fields.
async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let which = if q.scope.as_deref() == Some("all") {
        if !can(&s, &me, &mut tx, "document:view_all").await? {
            return Err(ApiError::new(403, "forbidden", "You do not have permission for this action")
                .detail("document:view_all"));
        }
        Listing::All
    } else {
        Listing::Sent(me.actor.membership)
    };
    let items = signing(&s)?.list(&mut tx, which).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    title: String,
    #[serde(default)]
    message: Option<String>,
    file_id: Uuid,
    #[serde(default = "sequential")]
    order: String,
    #[serde(default = "simple")]
    mode: String,
    #[serde(default = "thirty")]
    expires_in_days: i64,
    /// Sent to a group: each person signs their own copy (`send-bulk`).
    #[serde(default)]
    bulk: bool,
}

fn sequential() -> String {
    "sequential".into()
}
fn simple() -> String {
    "simple".into()
}
fn thirty() -> i64 {
    30
}

/// `POST /envelopes`: a draft envelope for an uploaded, clean PDF (`title`, optional `message`,
/// `file_id`, `order` sequential|parallel, `mode` simple|strong, `expires_in_days` 1–365, default 30, `bulk` to send
/// a copy to each person in a group;
/// `document:send_for_signature`). 201 with the envelope; 422 for a PDF that can't be signed.
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "document:send_for_signature", Resource::Workspace).await?;
    let e = signing(&s)?
        .create(
            &mut tx,
            &actor(&me, &headers),
            &r.title,
            r.message.as_deref(),
            r.file_id,
            &r.order,
            &r.mode,
            r.expires_in_days,
            r.bulk,
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(e)).into_response())
}

/// `GET /envelopes/{id}`: an envelope with its recipients and fields, for its sender, its recipients
/// (once sent) or `document:view_all`.
async fn one(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let e = signing(&s)?.get(&mut tx, id).await?;
    let on_it = e.created_by == me.actor.membership
        || (e.state != "draft" && e.recipients.iter().any(|r| r.membership_id == me.actor.membership));
    if !on_it && !can(&s, &me, &mut tx, "document:view_all").await? {
        return Err(ApiError::not_found());
    }
    tx.rollback().await.map_err(internal)?;
    let mut res = Json(e.clone()).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", e.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipientIn {
    membership_id: Uuid,
    #[serde(default = "signer")]
    role: String,
}
fn signer() -> String {
    "signer".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipientsReq {
    recipients: Vec<RecipientIn>,
}

/// `PUT /envelopes/{id}/recipients`: replaces a draft's recipients in signing order
/// (`[{membership_id, role: signer|cc}]`, at most 50, active members). Honours `If-Match`.
async fn recipients(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RecipientsReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "document:send_for_signature", Resource::Workspace).await?;
    let list: Vec<(Uuid, String)> = r.recipients.into_iter().map(|x| (x.membership_id, x.role)).collect();
    let e =
        signing(&s)?.set_recipients(&mut tx, &actor(&me, &headers), id, if_match(&headers), &list).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(e).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldsReq {
    fields: Vec<Field>,
}

/// `PUT /envelopes/{id}/fields`: replaces a draft's fields (`kind` signature|initials|date_signed|
/// name|text|checkbox, `recipient_id` of a signer, `page`, `x`/`y`/`w`/`h` as fractions of the page
/// from its top-left corner, `required`, `label`). Honours `If-Match`.
async fn fields(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<FieldsReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "document:send_for_signature", Resource::Workspace).await?;
    let e =
        signing(&s)?.set_fields(&mut tx, &actor(&me, &headers), id, if_match(&headers), &r.fields).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(e).into_response())
}

/// `POST /envelopes/{id}/send`: sends a draft (every signer needs a signature field); the document
/// and fields are frozen and the signers whose turn it is are notified. Honours `If-Match`.
async fn send(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "document:send_for_signature", Resource::Workspace).await?;
    let e = signing(&s)?.send(&mut tx, &actor(&me, &headers), id, if_match(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(e).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonReq {
    reason: String,
}

/// `POST /envelopes/{id}/void`: cancels a sent envelope with a `reason` (its sender, or anyone with
/// `document:void`); signers who were asked are told.
async fn void(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let any = can(&s, &me, &mut tx, "document:void").await?;
    let e = signing(&s)?.void(&mut tx, &actor(&me, &headers), id, &r.reason, any).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(e).into_response())
}

#[derive(Debug, Deserialize)]
struct DocQuery {
    #[serde(default)]
    signed: bool,
}

/// `GET /envelopes/{id}/document?signed=true|false`: the PDF (`application/pdf`): the original the
/// signers review, or the signed copy with its certificate once everyone has signed (409 before).
async fn document(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<DocQuery>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let all = can(&s, &me, &mut tx, "document:view_all").await?;
    let (name, bytes) = signing(&s)?.document(&mut tx, me.actor.membership, id, q.signed, all).await?;
    tx.commit().await.map_err(internal)?;
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " ._()-".contains(c) { c } else { '_' })
        .collect();
    let mut res = (StatusCode::OK, bytes).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{safe}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

/// `GET /me/signatures`: the envelopes I'm on: `to_sign` when it's my turn, then the rest (waiting,
/// signed, declined, completed copies).
async fn inbox(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let items = signing(&s)?.inbox(&mut tx, me.actor.membership).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `GET /signing/{recipient}`: the signing ceremony for my recipient row: the envelope, my fields,
/// whether it's my turn, everyone's progress, and the consent text. The first open is recorded.
async fn ceremony(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(recipient): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let v = signing(&s)?.ceremony(&mut tx, &actor(&me, &headers), recipient).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}

/// `POST /signing/{recipient}/sign`: signs on my turn with `document_hash` (the one shown),
/// `consent: true`, `adopted {kind: typed|drawn, text, image?}` and `values` for my text and checkbox
/// fields. `strong` envelopes need a recent sign-in (401 `step_up_required`). The last signature
/// completes the envelope.
async fn sign(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(recipient): Path<Uuid>,
    Json(r): Json<SignInput>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let strong: Option<String> = sqlx::query_scalar(
        "SELECT e.mode FROM envelope_recipients r JOIN envelopes e ON e.workspace_id = r.workspace_id AND e.id = r.envelope_id WHERE r.id = $1",
    )
    .bind(recipient)
    .fetch_optional(tx.conn())
    .await
    .map_err(internal)?;
    if strong.as_deref() == Some("strong") {
        me.require_step_up(&s, &mut tx).await?;
    }
    let e = signing(&s)?.sign(&mut tx, &actor(&me, &headers), recipient, &r).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "envelope_id": e.id, "state": e.state, "completed_file_id": e.completed_file_id }))
        .into_response())
}

/// `POST /signing/{recipient}/decline`: declines with a `reason`; the envelope ends for everyone and
/// the sender is told.
async fn decline(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(recipient): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let e = signing(&s)?.decline(&mut tx, &actor(&me, &headers), recipient, &r.reason).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "envelope_id": e.id, "state": e.state })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BulkReq {
    audience: procera_members::audience::Audience,
}

/// `POST /envelopes/{id}/send-bulk`: sends a bulk draft to everyone in `audience` (`all`,
/// `departments`, `groups`, `roles`, `locations`): one copy each, signed by that person, grouped in a
/// batch (at most 2,000 people). Honours `If-Match`. 201 with the batch.
async fn send_bulk(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<BulkReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "document:send_for_signature", Resource::Workspace).await?;
    let b =
        signing(&s)?.send_bulk(&mut tx, &actor(&me, &headers), id, if_match(&headers), &r.audience).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(b)).into_response())
}

/// `POST /envelopes/{id}/remind`: reminds the signer whose turn it is now (its sender; at most once
/// an hour per signer). Returns `{reminded}`.
async fn remind(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let n = signing(&s)?.remind(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "reminded": n })).into_response())
}

/// `GET /envelope-batches?scope=sent|all`: group sends I made (or everyone's with
/// `document:view_all`), newest first, with their counts.
async fn batches(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let sender = if q.scope.as_deref() == Some("all") {
        if !can(&s, &me, &mut tx, "document:view_all").await? {
            return Err(ApiError::new(403, "forbidden", "You do not have permission for this action")
                .detail("document:view_all"));
        }
        None
    } else {
        Some(me.actor.membership)
    };
    let items = signing(&s)?.batches(&mut tx, sender).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `GET /envelope-batches/{id}`: a group send with each person's progress (its sender or
/// `document:view_all`): `counts {total, signed, waiting, viewed, declined, cancelled, expired}`, `rows`.
async fn batch(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let b = signing(&s)?.batch(&mut tx, id).await?;
    if b.created_by != me.actor.membership && !can(&s, &me, &mut tx, "document:view_all").await? {
        return Err(ApiError::not_found());
    }
    tx.rollback().await.map_err(internal)?;
    Ok(Json(b).into_response())
}

/// `POST /envelope-batches/{id}/remind`: reminds everyone in a group send who hasn't signed yet
/// (its sender; at most once an hour each). Returns `{reminded}`.
async fn remind_batch(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let n = signing(&s)?.remind_batch(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "reminded": n })).into_response())
}

/// `POST /envelope-batches/{id}/void`: cancels every copy still waiting for a signature, with a
/// `reason` (its sender, or `document:void`). Returns `{cancelled}`.
async fn void_batch(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let any = can(&s, &me, &mut tx, "document:void").await?;
    let n = signing(&s)?.void_batch(&mut tx, &actor(&me, &headers), id, &r.reason, any).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "cancelled": n })).into_response())
}
