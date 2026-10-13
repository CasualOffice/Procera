//! Work API (BPMS-085, docs/08 "Work", §7.2-§7.3): my inbox (`work:view_assigned`), the steps I
//! decided, unassigned and overdue steps for admins (`work:reassign`), decisions (`work:complete`,
//! `If-Match` required) and admin reassignment (`work:reassign`, a reason required). A decision
//! re-checks candidacy and separation of duties; exactly one decision wins (409 `already_decided`
//! for the others). A reassignment re-runs separation of duties (422 `sod_excluded` on
//! `/membership_id`).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::WsTx;
use procera_db::idempotency::{self, Claim};
use procera_requests::{Reach, WorkState};
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
        .route("/work/workload", get(workload))
        .route("/work/workload/{member}", get(held))
        .route("/operations/incidents", get(incidents))
        .route("/operations/incidents/{source}/{id}/retry", post(retry_incident))
        .route("/operations/incidents/{source}/{id}/dismiss", post(dismiss_incident))
        .route("/work/{id}/decisions", post(decide))
        .route("/work/{id}/reassign", post(reassign))
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
        "overdue" => WorkState::Overdue,
        other => {
            return Err(ApiError::new(400, "bad_request", "Unknown state")
                .detail(format!("{other}: use open, done, unassigned or overdue")));
        }
    };
    let admin = matches!(state, WorkState::Unassigned | WorkState::Overdue);
    let perm = if admin { "work:reassign" } else { "work:view_assigned" };
    me.require(m, &mut tx, perm, Resource::Workspace).await?;
    let items = requests(&s)?.work(&mut tx, me.actor.membership, state).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

async fn allowed(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>, action: &str) -> Result<bool, ApiError> {
    Ok(members(s)?.access().check(tx, me.actor.membership, action, &Resource::Workspace).await?.allowed)
}

/// The departments the caller manages by a department-scoped grant (a Team Manager, docs/12 §4).
async fn team_departments(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>) -> Result<Vec<Uuid>, ApiError> {
    Ok(members(s)?.access().departments_with(tx, me.actor.membership, "work:view_team").await?)
}

/// Sees every member's work: workspace oversight (`request:view_all` and `work:reassign`).
async fn oversees(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>) -> Result<bool, ApiError> {
    Ok(allowed(s, me, tx, "request:view_all").await? && allowed(s, me, tx, "work:reassign").await?)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkloadQuery {
    scope: Option<String>,
}

/// Workload per person (FR-A03): `scope=team` (default) is the caller's reporting line, for a
/// manager in the directory or `work:view_team`; `scope=all` is everyone and needs workspace oversight.
async fn workload(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<WorkloadQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let reach = match q.scope.as_deref().unwrap_or("team") {
        "team" => {
            // Managers in the directory have a team without a grant (docs/12 §4); `work:view_team` too.
            let svc = requests(&s)?;
            let depts = team_departments(&s, &me, &mut tx).await?;
            if !svc.manages(&mut tx, me.actor.membership, &depts).await? {
                me.require(m, &mut tx, "work:view_team", Resource::Workspace).await?;
            }
            Reach::Team(me.actor.membership, depts)
        }
        "all" => {
            if !oversees(&s, &me, &mut tx).await? {
                return Err(ApiError::new(403, "forbidden", "Forbidden"));
            }
            Reach::All
        }
        other => {
            return Err(ApiError::new(400, "bad_request", "Unknown scope")
                .detail(format!("{other}: use team or all")));
        }
    };
    let w = requests(&s)?.workload(&mut tx, reach).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(w).into_response())
}

/// The open steps one person holds: visible for someone in the caller's reporting line, or anyone
/// with workspace oversight. Others get 404.
async fn held(
    State(s): State<AppState>,
    me: WsMember,
    Path(member): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let svc = requests(&s)?;
    let depts = team_departments(&s, &me, &mut tx).await?;
    let ok = oversees(&s, &me, &mut tx).await?
        || svc.reports_to(&mut tx, member, me.actor.membership, &depts).await?;
    if !ok {
        return Err(ApiError::not_found());
    }
    let items = svc.work(&mut tx, me.actor.membership, WorkState::HeldBy(member)).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// Technical incidents (docs/05 §3.18 Ops monitor): `monitoring:view_dashboards` or `request:retry`.
async fn incidents(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    if !allowed(&s, &me, &mut tx, "request:retry").await? {
        me.require(m, &mut tx, "monitoring:view_dashboards", Resource::Workspace).await?;
    }
    let items = requests(&s)?.incidents(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleReq {
    reason: String,
}

async fn settle_incident(
    s: &AppState,
    headers: &HeaderMap,
    me: &WsMember,
    (source, id): (String, Uuid),
    body: &Bytes,
    retry: bool,
) -> Result<Response, ApiError> {
    let r: SettleReq = serde_json::from_slice(body).map_err(bad_body)?;
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "request:retry", Resource::Workspace).await?;
    let svc = requests(s)?;
    if retry {
        svc.retry_incident(&mut tx, &actor(me, headers), &source, id, &r.reason).await?;
    } else {
        svc.dismiss_incident(&mut tx, &actor(me, headers), &source, id, &r.reason).await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Runs an incident again (audited `incident.retried`, a reason required).
async fn retry_incident(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(key): Path<(String, Uuid)>,
    body: Bytes,
) -> Result<Response, ApiError> {
    settle_incident(&s, &headers, &me, key, &body, true).await
}

/// Gives up on an incident (audited `incident.dismissed`, a reason required).
async fn dismiss_incident(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(key): Path<(String, Uuid)>,
    body: Bytes,
) -> Result<Response, ApiError> {
    settle_incident(&s, &headers, &me, key, &body, false).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionReq {
    outcome: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    fields: Map<String, Value>,
    /// Files attached with the decision (uploaded by the decider, clean).
    #[serde(default)]
    files: Vec<Uuid>,
    /// The published policy version the decider was shown.
    #[serde(default)]
    policy_version: Option<Uuid>,
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
        .decide_with(
            &mut tx,
            &actor(&me, &headers),
            id,
            rev,
            &r.outcome,
            r.comment.as_deref(),
            &r.fields,
            &procera_requests::DecisionExtras { files: r.files, policy_version: r.policy_version },
        )
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReassignReq {
    membership_id: Uuid,
    reason: String,
}

/// Moves an open step to one member (admin oversight). `Idempotency-Key` optional: a replay of
/// the same key returns the first answer.
async fn reassign(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let r: ReassignReq = serde_json::from_slice(&body).map_err(bad_body)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    // A manager moves their own team's steps within the team without the grant (docs/12 §6).
    let depts = team_departments(&s, &me, &mut tx).await?;
    if !requests(&s)?
        .may_reassign_within_team(&mut tx, me.actor.membership, &depts, id, r.membership_id)
        .await?
    {
        me.require(m, &mut tx, "work:reassign", Resource::Workspace).await?;
    }
    let key = idempotency_key(&headers);
    if let Some(k) = &key {
        let hash = idempotency::request_hash("POST", &format!("/api/v1/work/{id}/reassign"), &body);
        match idempotency::claim(&mut tx, me.actor.membership, "work.reassign", k, &hash)
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
    let done = requests(&s)?.reassign(&mut tx, &actor(&me, &headers), id, r.membership_id, &r.reason).await?;
    let out = serde_json::to_value(&done).map_err(internal)?;
    if let Some(k) = &key {
        idempotency::complete(&mut tx, me.actor.membership, "work.reassign", k, 200, &out)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(out).into_response())
}
