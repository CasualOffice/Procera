//! Library API (BPMS-040, docs/08 "Library"): governed SOPs and policies with versions, reviews and
//! publication. Permissions are per kind (`sop:*`, `policy:*`); approving or publishing a
//! controlled artifact needs a recent authentication (docs/12 §11).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
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
        .route("/versions/{a}/diff/{b}", get(diff_versions))
        .route("/artifacts/{id}/review-cadence", put(set_cadence))
        .route("/artifacts/{id}/recertify", post(recertify))
        .route("/artifacts/{id}/verification", post(verify).delete(unverify))
        .route("/artifacts/{id}/change-requests", get(list_change_requests).post(open_change_request))
        .route("/change-requests/{id}", get(get_change_request))
        .route("/change-requests/{id}/resolve", post(resolve_change_request))
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

/// Semantic diff between two versions of the same artifact (BPMS-043, FR-G05): block-level
/// added/removed/changed with word-level changes inside changed blocks.
async fn diff_versions(
    State(s): State<AppState>,
    me: WsMember,
    Path((a, b)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, a, "view").await?;
    let lib = library(&s)?;
    let (va, _) = lib.version(&mut tx, a, None).await?;
    let (vb, _) = lib.version(&mut tx, b, None).await?;
    tx.rollback().await.map_err(internal)?;
    if va.artifact_id != vb.artifact_id {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("both versions must belong to the same artifact"));
    }
    let d = procera_library::diff::diff(&va.content, &vb.content);
    Ok(Json(json!({
        "artifact_id": va.artifact_id,
        "from": {"id": va.id, "version_no": va.version_no},
        "to": {"id": vb.id, "version_no": vb.version_no},
        "summary": {"added": d.added, "removed": d.removed, "changed": d.changed},
        "ops": d.ops,
    })))
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

// ---- BPMS-044: recertification, verification, change requests ----

/// Loads the artifact and checks `action` on its kind.
async fn authorize_artifact(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
    id: Uuid,
    action: &str,
) -> Result<procera_library::Artifact, ApiError> {
    let a = library(s)?.artifact(tx, id).await?;
    me.require(members(s)?, tx, permission(&a.kind, action)?, Resource::Workspace).await?;
    Ok(a)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CadenceReq {
    /// Days, or `null` to use the workspace default.
    days: Option<i32>,
}

async fn set_cadence(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CadenceReq>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "recertify").await?;
    let a = library(&s)?.set_cadence(&mut tx, &actor(&me, &headers), id, r.days).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecertifyReq {
    comment: Option<String>,
}

async fn recertify(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    body: Option<Json<RecertifyReq>>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let r = body.map(|Json(b)| b).unwrap_or_default();
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "recertify").await?;
    let a = library(&s)?.recertify(&mut tx, &actor(&me, &headers), id, r.comment.as_deref()).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

/// Verifying a controlled artifact needs a recent authentication, like approving it.
async fn verify(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let a = authorize_artifact(&s, &me, &mut tx, id, "verify").await?;
    if a.criticality == "controlled" {
        me.require_step_up(&s, &mut tx).await?;
    }
    let a = library(&s)?.verify(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

async fn unverify(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "verify").await?;
    let a = library(&s)?.unverify(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

#[derive(Debug, Deserialize)]
struct CrListQuery {
    state: Option<String>,
}

async fn list_change_requests(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<CrListQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "view").await?;
    let items = library(&s)?.change_requests(&mut tx, id, q.state.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenCrReq {
    title: String,
    #[serde(default)]
    body: String,
}

async fn open_change_request(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<OpenCrReq>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "propose_change").await?;
    let cr = library(&s)?.open_change_request(&mut tx, &actor(&me, &headers), id, &r.title, &r.body).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(cr)).into_response())
}

/// Kind check for a change request: through its artifact.
async fn change_request_artifact(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
    id: Uuid,
    action: &str,
) -> Result<procera_library::governance::ChangeRequest, ApiError> {
    let cr = library(s)?.change_request(tx, id).await?;
    authorize_artifact(s, me, tx, cr.artifact_id, action).await?;
    Ok(cr)
}

async fn get_change_request(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<procera_library::governance::ChangeRequest>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let cr = change_request_artifact(&s, &me, &mut tx, id, "view").await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(cr))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveCrReq {
    decision: String,
    resolution: Option<String>,
    version_id: Option<Uuid>,
}

async fn resolve_change_request(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ResolveCrReq>,
) -> Result<Json<procera_library::governance::ChangeRequest>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    change_request_artifact(&s, &me, &mut tx, id, "resolve_change").await?;
    let cr = library(&s)?
        .resolve_change_request(
            &mut tx,
            &actor(&me, &headers),
            id,
            &r.decision,
            r.resolution.as_deref(),
            r.version_id,
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(cr))
}
