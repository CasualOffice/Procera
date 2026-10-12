//! Out of office (BPMS-086, docs/12 §9, docs/08 "Work"): a member names a delegate for a date range
//! on Account › Profile; an admin or the member's manager can set it for them on the Person page.
//!
//! - `GET/PUT/DELETE /me/out-of-office`: your own. Changing it needs `work:delegate` and
//!   `access.delegation.self_service_ooo` (on by default); reading it needs only membership.
//! - `GET/PUT/DELETE /members/{id}/out-of-office`: someone else's. Needs `work:delegate` and either
//!   `work:reassign` (admins, process owners, team managers) or being the member's manager.
//!
//! Changes append `member.ooo_set` / `member.ooo_cleared` in the same transaction.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use procera_access::Resource;
use procera_requests::SetOutOfOffice;
use procera_requests::delegation::parse_date;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::requests::{actor, internal, requests};
use super::settings::settings;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/out-of-office", get(mine).put(set_mine).delete(clear_mine))
        .route("/members/{id}/out-of-office", get(theirs).put(set_theirs).delete(clear_theirs))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetReq {
    starts_on: String,
    ends_on: String,
    delegate: Uuid,
    #[serde(default)]
    note: Option<String>,
}

impl SetReq {
    fn parse(&self) -> Result<SetOutOfOffice, ApiError> {
        Ok(SetOutOfOffice {
            starts_on: parse_date("starts_on", &self.starts_on)?,
            ends_on: parse_date("ends_on", &self.ends_on)?,
            delegate: self.delegate,
            note: self.note.clone(),
        })
    }
}

async fn self_service(s: &AppState, tx: &mut procera_db::WsTx<'_>) -> Result<bool, ApiError> {
    Ok(settings(s)?
        .effective(tx, "access.delegation.self_service_ooo")
        .await?
        .value
        .as_bool()
        .unwrap_or(true))
}

async fn mine(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let ooo = requests(&s)?.out_of_office(&mut tx, me.actor.membership).await?;
    let can_set = self_service(&s, &mut tx).await?
        && m.access()
            .check(&mut tx, me.actor.membership, "work:delegate", &Resource::Workspace)
            .await?
            .allowed;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "out_of_office": ooo, "can_set": can_set })).into_response())
}

/// Opens the transaction and checks the caller may change their own out-of-office.
async fn own_tx(s: &AppState, me: &WsMember) -> Result<procera_db::WsTx<'static>, ApiError> {
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "work:delegate", Resource::Workspace).await?;
    if !self_service(s, &mut tx).await? {
        return Err(ApiError::new(403, "forbidden", "You do not have permission for this action")
            .detail("people can't set their own out-of-office in this workspace; ask an admin"));
    }
    Ok(tx)
}

async fn set_mine(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<SetReq>,
) -> Result<Response, ApiError> {
    let req = r.parse()?;
    let mut tx = own_tx(&s, &me).await?;
    let out =
        requests(&s)?.set_out_of_office(&mut tx, &actor(&me, &headers), me.actor.membership, &req).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(out).into_response())
}

async fn clear_mine(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
) -> Result<Response, ApiError> {
    let mut tx = own_tx(&s, &me).await?;
    requests(&s)?.clear_out_of_office(&mut tx, &actor(&me, &headers), me.actor.membership).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Opens the transaction and checks the caller may manage `id`'s out-of-office: `work:delegate`
/// and either `work:reassign` or being their manager. Someone not in the workspace is 404.
async fn others_tx(s: &AppState, me: &WsMember, id: Uuid) -> Result<procera_db::WsTx<'static>, ApiError> {
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "work:delegate", Resource::Workspace).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1)")
        .bind(id)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    if !exists {
        return Err(ApiError::not_found());
    }
    let admin =
        m.access().check(&mut tx, me.actor.membership, "work:reassign", &Resource::Workspace).await?.allowed;
    let manager =
        m.resolve_manager(&mut tx, id).await.map_err(internal)?.membership_id == Some(me.actor.membership);
    if !(admin || manager) {
        return Err(ApiError::new(403, "forbidden", "You do not have permission for this action")
            .detail("only an admin or the person's manager can set their out-of-office"));
    }
    Ok(tx)
}

async fn theirs(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let mut tx = others_tx(&s, &me, id).await?;
    let ooo = requests(&s)?.out_of_office(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "out_of_office": ooo, "can_set": true })).into_response())
}

async fn set_theirs(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<SetReq>,
) -> Result<Response, ApiError> {
    let req = r.parse()?;
    let mut tx = others_tx(&s, &me, id).await?;
    let out = requests(&s)?.set_out_of_office(&mut tx, &actor(&me, &headers), id, &req).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(out).into_response())
}

async fn clear_theirs(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = others_tx(&s, &me, id).await?;
    requests(&s)?.clear_out_of_office(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
