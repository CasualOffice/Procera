//! Library API (BPMS-040, docs/08 "Library"): governed SOPs and policies with versions, reviews and
//! publication. Permissions are per kind (`sop:*`, `policy:*`); approving or publishing a
//! controlled artifact needs a recent authentication (docs/12 §11).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::WsTx;
use procera_library::{AUTHORABLE, Actor, Library, LibraryError, ReviewPolicy, permission};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::settings::settings;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/artifacts", get(list).post(create))
        .route("/artifacts/{id}", get(get_artifact))
        .route("/artifacts/{id}/versions", post(new_version))
        .route("/versions/{id}", get(get_version).patch(edit))
        .route("/versions/{id}/submit", post(submit))
        .route("/versions/{id}/withdraw", post(withdraw))
        .route("/versions/{id}/reviews", post(review))
        .route("/versions/{id}/publish", post(publish))
        .route("/versions/{id}/retire", post(retire))
}

fn library(s: &AppState) -> Result<&Library, ApiError> {
    s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

impl From<LibraryError> for ApiError {
    fn from(e: LibraryError) -> Self {
        match e {
            LibraryError::NotFound => Self::not_found(),
            LibraryError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            LibraryError::Conflict(m) => {
                Self::new(409, "state_conflict", "Not allowed in the current state").detail(m)
            }
            LibraryError::KeyTaken => Self::new(409, "key_taken", "An artifact with this key already exists"),
            LibraryError::StaleRevision { current } => {
                Self::new(412, "precondition_failed", "The version changed; reload and retry")
                    .detail(format!("current revision {current}"))
            }
            LibraryError::SeparationOfDuties(m) => {
                Self::new(403, "sod_violation", "Separation of duties").detail(m)
            }
            LibraryError::Internal(m) => {
                tracing::error!(error = %m, "library internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "library api internal error");
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

async fn review_policy(s: &AppState, tx: &mut WsTx<'_>) -> Result<ReviewPolicy, ApiError> {
    let st = settings(s)?;
    let required_for = st
        .effective(tx, "governance.review.required_for")
        .await?
        .value
        .as_str()
        .unwrap_or("controlled")
        .to_owned();
    let min_reviewers =
        st.effective(tx, "governance.review.min_reviewers").await?.value.as_i64().unwrap_or(1);
    let reason =
        st.effective(tx, "governance.reason_for_change.required").await?.value.as_bool().unwrap_or(true);
    Ok(ReviewPolicy { required_for, min_reviewers, reason_for_change_required: reason })
}

/// Kind of a version's artifact, after checking `action` on it.
async fn authorize_version(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
    id: Uuid,
    action: &str,
) -> Result<(String, String), ApiError> {
    let (kind, criticality): (String, String) = sqlx::query_as("SELECT a.kind, a.criticality FROM artifact_versions v JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id WHERE v.id = $1")
        .bind(id).fetch_optional(tx.conn()).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    me.require(members(s)?, tx, permission(&kind, action)?, Resource::Workspace).await?;
    Ok((kind, criticality))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    kind: Option<String>,
    state: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let wanted: Vec<&str> = match q.kind.as_deref() {
        Some(k) => vec![AUTHORABLE.iter().copied().find(|x| *x == k).ok_or_else(|| {
            ApiError::from(LibraryError::Invalid(format!("kind must be one of {}", AUTHORABLE.join(", "))))
        })?],
        None => AUTHORABLE.to_vec(),
    };
    // Only the kinds this member may view.
    let mut kinds = vec![];
    for k in wanted {
        if m.access()
            .check(&mut tx, me.actor.membership, permission(k, "view")?, &Resource::Workspace)
            .await?
            .allowed
        {
            kinds.push(k);
        }
    }
    let items = library(&s)?.list(&mut tx, &kinds, q.state.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewArtifact {
    kind: String,
    key: String,
    title: String,
    #[serde(default = "standard")]
    criticality: String,
    #[serde(default = "empty_doc")]
    content: Value,
}

fn standard() -> String {
    "standard".into()
}

fn empty_doc() -> Value {
    json!({"blocks": []})
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<NewArtifact>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, permission(&r.kind, "create")?, Resource::Workspace).await?;
    let (a, v) = library(&s)?
        .create(&mut tx, &actor(&me, &headers), &r.kind, &r.key, &r.title, &r.criticality, &r.content)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(json!({ "artifact": a, "version": v }))).into_response())
}

async fn get_artifact(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let lib = library(&s)?;
    let a = lib.artifact(&mut tx, id).await?;
    me.require(m, &mut tx, permission(&a.kind, "view")?, Resource::Workspace).await?;
    let versions: Vec<Value> = lib.versions(&mut tx, id).await?.into_iter()
        .map(|v| json!({"id": v.id, "version_no": v.version_no, "lifecycle": v.lifecycle, "content_hash": v.content_hash, "effective_at": v.effective_at, "published_at": v.published_at, "change_reason": v.change_reason}))
        .collect();
    tx.rollback().await.map_err(internal)?;
    Ok(etag(Json(json!({ "artifact": a, "versions": versions })).into_response(), a.revision))
}

async fn new_version(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let lib = library(&s)?;
    let a = lib.artifact(&mut tx, id).await?;
    me.require(m, &mut tx, permission(&a.kind, "edit_draft")?, Resource::Workspace).await?;
    let v = lib.new_version(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(v)).into_response())
}

async fn get_version(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "view").await?;
    let policy = review_policy(&s, &mut tx).await?;
    let (v, _) = library(&s)?.version(&mut tx, id, Some(&policy)).await?;
    tx.rollback().await.map_err(internal)?;
    let rev = v.revision;
    Ok(etag(Json(v).into_response(), rev))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditReq {
    content: Value,
}

async fn edit(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<EditReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "edit_draft").await?;
    let v = library(&s)?.edit(&mut tx, &actor(&me, &headers), id, &r.content, if_match(&headers)).await?;
    tx.commit().await.map_err(internal)?;
    let rev = v.revision;
    Ok(etag(Json(v).into_response(), rev))
}

async fn submit(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "submit").await?;
    let policy = review_policy(&s, &mut tx).await?;
    let v = library(&s)?.submit(&mut tx, &actor(&me, &headers), id, &policy).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}

async fn withdraw(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "edit_draft").await?;
    let v = library(&s)?.withdraw(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewReq {
    decision: String,
    #[serde(default)]
    comment: Option<String>,
}

async fn review(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReviewReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let action = if r.decision == "approve" { "approve" } else { "review" };
    let (_, criticality) = authorize_version(&s, &me, &mut tx, id, action).await?;
    if action == "approve" && criticality == "controlled" {
        me.require_step_up(&s, &mut tx).await?;
    }
    let policy = review_policy(&s, &mut tx).await?;
    let v = library(&s)?
        .review(&mut tx, &actor(&me, &headers), id, &r.decision, r.comment.as_deref(), &policy)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishReq {
    #[serde(default, with = "time::serde::rfc3339::option")]
    effective_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    change_reason: Option<String>,
}

async fn publish(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<PublishReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let (_, criticality) = authorize_version(&s, &me, &mut tx, id, "publish").await?;
    if criticality == "controlled" {
        me.require_step_up(&s, &mut tx).await?;
    }
    let policy = review_policy(&s, &mut tx).await?;
    let v = library(&s)?
        .publish(&mut tx, &actor(&me, &headers), id, r.effective_at, r.change_reason.as_deref(), &policy)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetireReq {
    reason: String,
}

async fn retire(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RetireReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "retire").await?;
    let v = library(&s)?.retire(&mut tx, &actor(&me, &headers), id, &r.reason).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(v).into_response())
}
