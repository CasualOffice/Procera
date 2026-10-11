//! Campaigns API (BPMS-046, docs/08 "Campaigns"): acknowledgement campaigns over published SOP and
//! policy versions. Managing and tracking need `campaign:manage` / `campaign:view`. Members list and
//! complete their own acknowledgements with `sop:acknowledge` (SOPs) or `policy:view` (policies; the
//! catalogue has no separate policy acknowledge permission, docs/12 §5).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_campaigns::{Actor, CampaignError, Campaigns, Defaults, NewCampaign};
use procera_db::WsTx;
use procera_members::audience::Audience;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::auth::AuthUser;
use super::error::ApiError;
use super::settings::settings;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/campaigns", get(list).post(create))
        .route("/campaigns/{id}", get(get_campaign))
        .route("/campaigns/{id}/recipients", get(recipients))
        .route("/campaigns/{id}/evidence", get(evidence))
        .route("/campaigns/{id}/activate", post(activate))
        .route("/campaigns/{id}/close", post(close))
        .route("/campaigns/{id}/cancel", post(cancel))
        .route("/campaigns/{id}/remind", post(remind))
        .route("/me/acknowledgements", get(my_acknowledgements))
        .route("/acknowledgements/{id}/complete", post(complete))
}

fn campaigns(s: &AppState) -> Result<&Campaigns, ApiError> {
    s.campaigns.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<CampaignError> for ApiError {
    fn from(e: CampaignError) -> Self {
        match e {
            CampaignError::NotFound => Self::not_found(),
            CampaignError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            CampaignError::Conflict(m) => {
                Self::new(409, "state_conflict", "Not allowed in the current state").detail(m)
            }
            CampaignError::AlreadyAcknowledged => {
                Self::new(409, "already_acknowledged", "You already acknowledged this")
            }
            CampaignError::ContentChanged { current } => {
                Self::new(409, "content_changed", "The content changed; reload it before acknowledging")
                    .detail(format!("this campaign asks for content hash {current}"))
            }
            CampaignError::Internal(m) => {
                tracing::error!(error = %m, "campaigns internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "campaigns api internal error");
    ApiError::new(500, "internal", "Internal error")
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

/// The permission a member needs to acknowledge a version of `kind`.
fn ack_permission(kind: &str) -> &'static str {
    if kind == "sop" { "sop:acknowledge" } else { "policy:view" }
}

/// Workspace defaults (docs/20 §2.14), copied onto a new campaign.
async fn defaults(s: &AppState, tx: &mut WsTx<'_>) -> Result<Defaults, ApiError> {
    Ok(campaign_defaults(settings(s)?, tx).await?)
}

/// Workspace campaign defaults, also used for re-acknowledgement campaigns (BPMS-045).
pub(crate) async fn campaign_defaults(
    st: &procera_settings::Settings,
    tx: &mut WsTx<'_>,
) -> Result<Defaults, procera_settings::SettingsError> {
    let snapshot =
        st.effective(tx, "campaign.audience.snapshot").await?.value.as_str().unwrap_or("dynamic").to_owned();
    let cadence = st
        .effective(tx, "campaign.reminders.cadence")
        .await?
        .value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let escalation_after =
        st.effective(tx, "campaign.escalation.to_manager_after").await?.value.as_i64().filter(|s| *s > 0);
    let due_in = st.effective(tx, "campaign.default_due").await?.value.as_i64().unwrap_or(14 * 86_400);
    let ack_text = st.effective(tx, "campaign.ack_text").await?.value.as_str().unwrap_or_default().to_owned();
    Ok(Defaults { snapshot, cadence, escalation_after, due_in, ack_text })
}

#[derive(Debug, Deserialize)]
struct StateQuery {
    state: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<StateQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:view", Resource::Workspace).await?;
    let items = campaigns(&s)?.list(&mut tx, q.state.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewReq {
    version_id: Uuid,
    audience: Audience,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    due_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    audience_snapshot: Option<String>,
    #[serde(default)]
    ack_text: Option<String>,
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<NewReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:manage", Resource::Workspace).await?;
    let d = defaults(&s, &mut tx).await?;
    let new = NewCampaign {
        title: r.title,
        version_id: r.version_id,
        audience: r.audience,
        due_at: r.due_at,
        snapshot: r.audience_snapshot,
        ack_text: r.ack_text,
    };
    let c = campaigns(&s)?.create(&mut tx, &actor(&me, &headers), &new, &d).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(c)).into_response())
}

async fn get_campaign(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:view", Resource::Workspace).await?;
    let cmp = campaigns(&s)?;
    let c = cmp.get(&mut tx, id).await?;
    let mut body = serde_json::to_value(&c).map_err(internal)?;
    if c.state == "draft" {
        // Draft preview: how many active members the audience reaches right now.
        let aud: Audience = serde_json::from_value(c.audience.clone()).map_err(internal)?;
        body["audience_preview"] = json!(cmp.audience_size(&mut tx, &aud).await?);
    }
    tx.rollback().await.map_err(internal)?;
    let mut res = Json(body).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", c.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

async fn recipients(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<StateQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:view", Resource::Workspace).await?;
    if let Some(st) = q.state.as_deref()
        && !matches!(st, "pending" | "completed" | "overdue")
    {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("state must be pending, completed or overdue"));
    }
    let items = campaigns(&s)?.recipients(&mut tx, id, q.state.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// Evidence export as CSV (FR-AK04). The export itself is audited (`campaign.evidence_exported`).
async fn evidence(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:view", Resource::Workspace).await?;
    let csv = campaigns(&s)?.evidence_csv(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    let disposition = format!("attachment; filename=\"campaign-{id}-evidence.csv\"");
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        csv,
    )
        .into_response())
}

async fn activate(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:manage", Resource::Workspace).await?;
    let c = campaigns(&s)?.launch(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(c).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndReq {
    #[serde(default)]
    reason: Option<String>,
}

async fn end(
    s: &AppState,
    headers: &HeaderMap,
    me: &WsMember,
    id: Uuid,
    to: &str,
    reason: Option<&str>,
) -> Result<Response, ApiError> {
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:manage", Resource::Workspace).await?;
    let c = campaigns(s)?.end(&mut tx, &actor(me, headers), id, to, reason).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(c).into_response())
}

async fn close(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<EndReq>,
) -> Result<Response, ApiError> {
    end(&s, &headers, &me, id, "closed", r.reason.as_deref()).await
}

async fn cancel(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<EndReq>,
) -> Result<Response, ApiError> {
    end(&s, &headers, &me, id, "cancelled", r.reason.as_deref()).await
}

async fn remind(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "campaign:manage", Resource::Workspace).await?;
    let n = campaigns(&s)?.remind(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "reminded": n })).into_response())
}

/// The caller's own acknowledgements (`?state=pending|completed`; default: open and completed),
/// limited to the kinds they may acknowledge.
async fn my_acknowledgements(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<StateQuery>,
) -> Result<Response, ApiError> {
    if let Some(st) = q.state.as_deref()
        && !matches!(st, "pending" | "completed")
    {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("state must be pending or completed"));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let mut allowed = vec![];
    for kind in ["sop", "policy"] {
        if m.access()
            .check(&mut tx, me.actor.membership, ack_permission(kind), &Resource::Workspace)
            .await?
            .allowed
        {
            allowed.push(kind);
        }
    }
    let items: Vec<_> = campaigns(&s)?
        .my_assignments(&mut tx, me.actor.membership, q.state.as_deref())
        .await?
        .into_iter()
        .filter(|a| allowed.contains(&a.subject.kind.as_str()))
        .collect();
    tx.rollback().await.map_err(internal)?;
    let pending = items.iter().filter(|a| a.state == "pending").count();
    Ok(Json(json!({ "items": items, "pending_count": pending })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteReq {
    /// The content hash the member read; must match the campaign's version when given.
    #[serde(default)]
    content_hash: Option<String>,
}

async fn complete(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(claims): AuthUser,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CompleteReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let cmp = campaigns(&s)?;
    let kind = cmp.assignment_kind(&mut tx, id, me.actor.membership).await?;
    me.require(m, &mut tx, ack_permission(&kind), Resource::Workspace).await?;
    // How the member authenticated for this session (FR-AK04 "how authenticated").
    let evidence = json!({"amr": claims.amr, "auth_time": claims.auth_time});
    let a = cmp.acknowledge(&mut tx, &actor(&me, &headers), id, r.content_hash.as_deref(), evidence).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a).into_response())
}
