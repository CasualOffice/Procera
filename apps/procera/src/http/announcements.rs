//! Announcements API (BPMS-047, docs/08 "Announcements"): compose, approval with separation of
//! duties, scheduled publication, a member's feed with read and acknowledgement receipts, and
//! analytics. Permissions (docs/12 §5): `announcement:create` composes (own drafts),
//! `announcement:publish` approves, publishes and archives, `announcement:view` reads the feed, and
//! `announcement:manage` can do all of it, including analytics of anyone's announcement.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_announcements::{
    Actor, Announcement, AnnouncementError, Announcements, Compose, Defaults, ListScope,
};
use procera_db::WsTx;
use procera_members::Members;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::settings::settings;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/announcements", get(list).post(create))
        .route("/announcements/feed", get(feed))
        .route("/announcements/{id}", get(get_one).patch(edit))
        .route("/announcements/{id}/submit", post(submit))
        .route("/announcements/{id}/approve", post(approve))
        .route("/announcements/{id}/reject", post(reject))
        .route("/announcements/{id}/publish", post(publish))
        .route("/announcements/{id}/archive", post(archive))
        .route("/announcements/{id}/reads", post(read))
        .route("/announcements/{id}/acks", post(acknowledge))
        .route("/announcements/{id}/analytics", get(analytics))
}

fn service(s: &AppState) -> Result<&Announcements, ApiError> {
    s.announcements.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<AnnouncementError> for ApiError {
    fn from(e: AnnouncementError) -> Self {
        match e {
            AnnouncementError::NotFound => Self::not_found(),
            AnnouncementError::Invalid(m) => {
                Self::new(422, "validation_failed", "Validation failed").detail(m)
            }
            AnnouncementError::Conflict(m) => {
                Self::new(409, "state_conflict", "Not allowed in the current state").detail(m)
            }
            AnnouncementError::StaleRevision { current } => {
                Self::new(412, "precondition_failed", "The announcement changed; reload and retry")
                    .detail(format!("current revision {current}"))
            }
            AnnouncementError::SeparationOfDuties(m) => {
                Self::new(403, "sod_violation", "Separation of duties").detail(m)
            }
            AnnouncementError::Internal(m) => {
                tracing::error!(error = %m, "announcements internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "announcements api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn forbidden(perm: &str) -> ApiError {
    ApiError::new(403, "forbidden", "You do not have permission for this action").detail(perm.to_owned())
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

fn etag(mut res: Response, revision: i64) -> Response {
    if let Ok(v) = HeaderValue::from_str(&format!("\"{revision}\"")) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

fn if_match(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok())
}

/// The caller's announcement permissions, each evaluated by the policy engine.
#[derive(Debug, Clone, Copy)]
struct Perms {
    view: bool,
    create: bool,
    publish: bool,
    manage: bool,
}

async fn perms(m: &Members, tx: &mut WsTx<'_>, me: &WsMember) -> Result<Perms, ApiError> {
    let mut check = async |p: &str| -> Result<bool, ApiError> {
        Ok(m.access().check(tx, me.actor.membership, p, &Resource::Workspace).await?.allowed)
    };
    Ok(Perms {
        view: check("announcement:view").await?,
        create: check("announcement:create").await?,
        publish: check("announcement:publish").await?,
        manage: check("announcement:manage").await?,
    })
}

impl Perms {
    /// Author of `a` with `create`, or `manage`: may edit, submit and see analytics.
    fn owns(&self, me: &WsMember, a: &Announcement) -> bool {
        self.manage || (self.create && a.author_membership_id == me.actor.membership)
    }
    fn publisher(&self) -> bool {
        self.publish || self.manage
    }
}

async fn defaults(s: &AppState, tx: &mut WsTx<'_>) -> Result<Defaults, ApiError> {
    let st = settings(s)?;
    let require_ack =
        st.effective(tx, "announcements.require_ack.default").await?.value.as_bool().unwrap_or(false);
    let secs = st.effective(tx, "announcements.expiry.default").await?.value.as_i64().unwrap_or(30 * 86_400);
    Ok(Defaults { require_ack, expiry: time::Duration::seconds(secs) })
}

async fn approval_mode(s: &AppState, tx: &mut WsTx<'_>) -> Result<String, ApiError> {
    Ok(settings(s)?
        .effective(tx, "announcements.approval_required")
        .await?
        .value
        .as_str()
        .unwrap_or("for_workspace_wide")
        .to_owned())
}

async fn ensure_enabled(s: &AppState, tx: &mut WsTx<'_>) -> Result<(), ApiError> {
    if settings(s)?.effective(tx, "announcements.enabled").await?.value.as_bool().unwrap_or(true) {
        Ok(())
    } else {
        Err(ApiError::new(409, "state_conflict", "Not allowed in the current state")
            .detail("announcements are turned off in this workspace (announcements.enabled)"))
    }
}

/// Delivery channels: `announcements.channels`, plus email for critical announcements, limited to
/// the channels the workspace has enabled (`notify.channels.enabled`).
async fn channels(s: &AppState, tx: &mut WsTx<'_>, critical: bool) -> Result<Vec<String>, ApiError> {
    let st = settings(s)?;
    let list = |v: serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    let enabled = list(st.effective(tx, "notify.channels.enabled").await?.value);
    let mut wanted = list(st.effective(tx, "announcements.channels").await?.value);
    if critical {
        wanted.extend(["in_app".to_owned(), "email".to_owned()]);
    }
    let mut out: Vec<String> = wanted.into_iter().filter(|c| enabled.contains(c)).collect();
    out.sort();
    out.dedup();
    Ok(out)
}

#[derive(Debug, Deserialize)]
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
    let p = perms(m, &mut tx, &me).await?;
    if !(p.create || p.publisher()) {
        return Err(forbidden("announcement:create"));
    }
    let scope = ListScope { all: p.manage, others_submitted: p.publish, me: me.actor.membership };
    let items = service(&s)?.list(&mut tx, scope, q.state.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(c): Json<Compose>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let p = perms(m, &mut tx, &me).await?;
    if !(p.create || p.manage) {
        return Err(forbidden("announcement:create"));
    }
    ensure_enabled(&s, &mut tx).await?;
    let d = defaults(&s, &mut tx).await?;
    let a = service(&s)?.create(&mut tx, &actor(&me, &headers), &c, &d).await?;
    tx.commit().await.map_err(internal)?;
    let rev = a.revision;
    Ok(etag((StatusCode::CREATED, Json(a)).into_response(), rev))
}

#[derive(Debug, Deserialize)]
struct FeedQuery {
    #[serde(default)]
    unread: bool,
}

async fn feed(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<FeedQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "announcement:view", Resource::Workspace).await?;
    let items = service(&s)?.feed(&mut tx, me.actor.membership, q.unread).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// Loads an announcement the caller may see: its author (with `create`), managers, publishers once
/// it is submitted, and live recipients (with `view`). Anyone else gets 404 (hidden ≡ missing).
async fn visible(
    s: &AppState,
    m: &Members,
    tx: &mut WsTx<'_>,
    me: &WsMember,
    id: Uuid,
) -> Result<(Announcement, Perms), ApiError> {
    let ann = service(s)?;
    let a = ann.get(tx, id).await?;
    let p = perms(m, tx, me).await?;
    let ok = p.owns(me, &a)
        || (p.publish && a.state != "draft")
        || (p.view && ann.is_live_recipient(tx, id, me.actor.membership).await?);
    if ok { Ok((a, p)) } else { Err(ApiError::not_found()) }
}

async fn get_one(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, _) = visible(&s, m, &mut tx, &me, id).await?;
    tx.rollback().await.map_err(internal)?;
    let rev = a.revision;
    Ok(etag(Json(a).into_response(), rev))
}

async fn edit(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(c): Json<Compose>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !p.owns(&me, &a) {
        return Err(forbidden("announcement:create"));
    }
    let d = defaults(&s, &mut tx).await?;
    let a = service(&s)?.edit(&mut tx, &actor(&me, &headers), id, &c, &d, if_match(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    let rev = a.revision;
    Ok(etag(Json(a).into_response(), rev))
}

async fn submit(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !p.owns(&me, &a) {
        return Err(forbidden("announcement:create"));
    }
    ensure_enabled(&s, &mut tx).await?;
    let mode = approval_mode(&s, &mut tx).await?;
    let a = service(&s)?.submit(&mut tx, &actor(&me, &headers), id, &mode).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}

async fn approve(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !p.publisher() {
        return Err(forbidden("announcement:publish"));
    }
    ensure_enabled(&s, &mut tx).await?;
    let ch = channels(&s, &mut tx, a.priority == "critical").await?;
    let a = service(&s)?.approve(&mut tx, &actor(&me, &headers), id, &ch).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectReq {
    comment: String,
}

async fn reject(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RejectReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (_, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !p.publisher() {
        return Err(forbidden("announcement:publish"));
    }
    let a = service(&s)?.reject(&mut tx, &actor(&me, &headers), id, &r.comment).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}

async fn publish(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    // A draft is published by its author when they also hold `publish`, or by a manager.
    if !(p.manage || (p.publish && a.author_membership_id == me.actor.membership)) {
        return Err(forbidden("announcement:publish"));
    }
    ensure_enabled(&s, &mut tx).await?;
    let mode = approval_mode(&s, &mut tx).await?;
    let ch = channels(&s, &mut tx, a.priority == "critical").await?;
    let a = service(&s)?.publish(&mut tx, &actor(&me, &headers), id, &mode, &ch).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveReq {
    reason: String,
}

async fn archive(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ArchiveReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !(p.publisher() || p.owns(&me, &a)) {
        return Err(forbidden("announcement:publish"));
    }
    let a = service(&s)?.archive(&mut tx, &actor(&me, &headers), id, &r.reason).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}

async fn read(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "announcement:view", Resource::Workspace).await?;
    let r = service(&s)?.mark_read(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(r).into_response())
}

async fn acknowledge(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "announcement:view", Resource::Workspace).await?;
    let r = service(&s)?.acknowledge(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(r).into_response())
}

async fn analytics(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (a, p) = visible(&s, m, &mut tx, &me, id).await?;
    if !p.owns(&me, &a) {
        return Err(forbidden("announcement:manage"));
    }
    let stats = service(&s)?.analytics(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(stats).into_response())
}
