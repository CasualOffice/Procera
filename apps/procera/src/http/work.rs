//! Work API (BPMS-085, docs/08 "Work", §7.2): my inbox (`work:view_assigned`), the steps I decided,
//! unassigned steps for admins (`work:reassign`), and decisions (`work:complete`, `If-Match`
//! required). A decision re-checks candidacy and separation of duties; exactly one decision wins
//! (409 `already_decided` for the others).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::idempotency::{self, Claim};
use procera_requests::WorkState;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::requests::{actor, bad_body, idempotency_key, internal, requests};
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/work", get(list))
        .route("/work/{id}/decisions", post(decide))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    state: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let state = match q.state.as_deref().unwrap_or("open") {
        "open" => WorkState::Open,
        "done" => WorkState::Done,
        "unassigned" => WorkState::Unassigned,
        other => {
            return Err(ApiError::new(400, "bad_request", "Unknown state")
                .detail(format!("{other}: use open, done or unassigned")));
        }
    };
    let perm = if state == WorkState::Unassigned { "work:reassign" } else { "work:view_assigned" };
    me.require(m, &mut tx, perm, Resource::Workspace).await?;
    let items = requests(&s)?.work(&mut tx, me.actor.membership, state).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionReq {
    outcome: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    fields: Map<String, Value>,
}

fn if_match(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok())
}

async fn decide(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let rev = if_match(&headers).ok_or_else(|| {
        ApiError::new(428, "precondition_required", "If-Match required")
            .detail("send the work item's revision from GET /work as If-Match")
    })?;
    let r: DecisionReq = serde_json::from_slice(&body).map_err(bad_body)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "work:complete", Resource::Workspace).await?;
    // Optional here: If-Match already makes a repeated decision fail rather than apply twice.
    let key = idempotency_key(&headers);
    if let Some(k) = &key {
        let hash = idempotency::request_hash("POST", &format!("/api/v1/work/{id}/decisions"), &body);
        match idempotency::claim(&mut tx, me.actor.membership, "work.decide", k, &hash)
            .await
            .map_err(internal)?
        {
            Claim::Fresh => {}
            Claim::Replay { status, response } => {
                tx.rollback().await.map_err(internal)?;
                let code =
                    StatusCode::from_u16(u16::try_from(status).unwrap_or(200)).unwrap_or(StatusCode::OK);
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
    }
    let d = requests(&s)?
        .decide(&mut tx, &actor(&me, &headers), id, rev, &r.outcome, r.comment.as_deref(), &r.fields)
        .await?;
    let out = serde_json::to_value(&d).map_err(internal)?;
    if let Some(k) = &key {
        idempotency::complete(&mut tx, me.actor.membership, "work.decide", k, 200, &out)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    let mut res = Json(out).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", d.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}
