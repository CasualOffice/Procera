//! Templates API (BPMS-106 P0 subset, docs/08 "Templates", A-38): the built-in template packs and
//! their install. Browsing needs `template:browse`; installing needs `template:install` plus
//! `<kind>:create` for every kind of document the install creates.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::WsTx;
use procera_library::templates::{self, Install, OnConflict, PACKS, Pack};
use procera_library::{Actor, permission};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test reads mutating routes from `.route(` lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/templates", get(list))
        .route("/templates/{key}/install", post(install))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "templates api internal error");
    ApiError::new(500, "internal", "Internal error")
}

async fn allowed(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>, action: &str) -> Result<bool, ApiError> {
    Ok(members(s)?.access().check(tx, me.actor.membership, action, &Resource::Workspace).await?.allowed)
}

/// Whether the member may install every document of `pack` (template:install + each kind's create).
async fn can_install(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>, pack: &Pack) -> Result<bool, ApiError> {
    if !allowed(s, me, tx, "template:install").await? {
        return Ok(false);
    }
    let docs: Vec<_> = pack.documents.iter().collect();
    for kind in Pack::kinds(&docs) {
        if !allowed(s, me, tx, permission(kind, "create")?).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The packs, what each creates and, for members who may install it, which of its documents
/// already exist in the workspace (by key).
async fn list(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "template:browse", Resource::Workspace).await?;
    let mut items = vec![];
    for p in PACKS {
        let installable = can_install(&s, &me, &mut tx, p).await?;
        let mut docs = vec![];
        for d in p.documents {
            // Only installers learn about existing drafts (readers never see other people's drafts, A-14).
            let existing =
                if installable { templates::existing(&mut tx, d.kind, d.key).await? } else { None };
            docs.push(json!({
                "key": d.key, "kind": d.kind, "title": d.title, "summary": d.summary,
                "existing": existing.map(|(id, state)| json!({"artifact_id": id, "state": state, "link": format!("/library/{id}")})),
            }));
        }
        items.push(json!({
            "key": p.key, "version": p.version, "name": p.name, "description": p.description,
            "documents": docs, "campaign": p.campaign, "can_install": installable,
        }));
    }
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallReq {
    /// Document keys to create; omitted = all.
    #[serde(default)]
    documents: Option<Vec<String>>,
    /// Return the pack's suggested acknowledgement campaign as a plan (default false).
    #[serde(default)]
    campaign: bool,
    /// `skip` (default) or `copy`.
    #[serde(default)]
    on_conflict: Option<String>,
}

/// `POST /templates/{key}/install`: creates a pack's documents as drafts (`documents` to pick some,
/// `on_conflict` `skip` or `copy`, `campaign` for a suggested plan). 201 when anything was created.
/// Needs `template:install` and `<kind>:create` for each kind installed.
async fn install(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(key): Path<String>,
    Json(r): Json<InstallReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let lib = s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let pack = templates::pack(&key).ok_or_else(ApiError::not_found)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "template:install", Resource::Workspace).await?;
    let install = Install {
        documents: r.documents,
        plan_campaign: r.campaign,
        on_conflict: r.on_conflict.as_deref().map(OnConflict::parse).transpose()?.unwrap_or_default(),
    };
    for kind in Pack::kinds(&pack.select(&install)?) {
        me.require(m, &mut tx, permission(kind, "create")?, Resource::Workspace).await?;
    }
    let c = super::auth::client_info(&headers);
    let actor = Actor {
        membership: me.actor.membership,
        subject_ref: me.actor.subject_ref,
        ip: c.ip,
        user_agent: c.user_agent,
    };
    let report = lib.install_pack(&mut tx, &actor, pack, &install).await?;
    tx.commit().await.map_err(internal)?;
    let created = report.documents.iter().any(|d| d.outcome != "skipped");
    let body: Value = serde_json::to_value(&report).map_err(internal)?;
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, Json(body)).into_response())
}
