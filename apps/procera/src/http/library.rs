//! Library API (BPMS-040, docs/08 "Library"): governed SOPs and policies with versions, reviews and
//! publication. Permissions are per kind (`sop:*`, `policy:*`); approving or publishing a
//! controlled artifact needs a recent authentication (docs/12 §11).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_db::WsTx;
use procera_library::{AUTHORABLE, Actor, Change, Library, LibraryError, Reader, ReviewPolicy, permission};
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
        .route("/artifacts/{id}/applicability", put(set_applicability))
        .route("/me/policies", get(portal))
        .route("/collections", get(list_collections).post(create_collection))
        .route("/collections/{id}", patch(update_collection).delete(delete_collection))
        .route("/collections/{id}/visibility", put(set_collection_visibility))
        .route("/artifacts/{id}/collection", put(set_artifact_collection))
        .route("/artifacts/{id}/owner", put(set_owner))
        .route("/artifacts/{id}/raci", get(get_raci).put(set_raci))
        .route("/raci/export", get(export_raci))
        .route("/versions/{id}/export", get(export_version))
        .route("/versions/{id}/submit", post(submit))
        .route("/versions/{id}/withdraw", post(withdraw))
        .route("/versions/{id}/reviews", post(review))
        .route("/versions/{id}/publish", post(publish))
        .route("/versions/{id}/publish-preview", get(publish_preview))
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

/// Artifact kinds on which the member holds an authoring permission: they see drafts and
/// restricted artifacts of those kinds (A-14).
pub(crate) async fn full_kinds(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
) -> Result<Vec<&'static str>, ApiError> {
    let m = members(s)?;
    let mut out = vec![];
    for kind in AUTHORABLE {
        for action in ["create", "edit_draft", "review", "approve", "publish"] {
            if m.access()
                .check(tx, me.actor.membership, permission(kind, action)?, &Resource::Workspace)
                .await?
                .allowed
            {
                out.push(*kind);
                break;
            }
        }
    }
    Ok(out)
}

/// Kind of a version's artifact, after checking `action` on it and that the member may see the
/// artifact and this version (hidden → 404).
async fn authorize_version(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
    id: Uuid,
    action: &str,
) -> Result<(String, String), ApiError> {
    let (artifact_id, lifecycle): (Uuid, String) =
        sqlx::query_as("SELECT artifact_id, lifecycle FROM artifact_versions WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?
            .ok_or_else(ApiError::not_found)?;
    let a = authorize_artifact(s, me, tx, artifact_id, action).await?;
    let full = full_kinds(s, me, tx).await?;
    if !Library::version_visible(
        &a.kind,
        &lifecycle,
        &Reader { membership: me.actor.membership, full_kinds: &full },
    ) {
        return Err(ApiError::not_found());
    }
    Ok((a.kind, a.criticality))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    kind: Option<String>,
    state: Option<String>,
    /// A collection; its sub-collections are included (BPMS-041).
    collection: Option<Uuid>,
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
    let full = full_kinds(&s, &me, &mut tx).await?;
    let reader = Reader { membership: me.actor.membership, full_kinds: &full };
    let items = library(&s)?.list(&mut tx, &kinds, q.state.as_deref(), q.collection, &reader).await?;
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
    let a = authorize_artifact(&s, &me, &mut tx, id, "view").await?;
    let full = full_kinds(&s, &me, &mut tx).await?;
    let reader = Reader { membership: me.actor.membership, full_kinds: &full };
    let versions: Vec<Value> = lib.versions(&mut tx, id).await?.into_iter()
        .filter(|v| Library::version_visible(&a.kind, &v.lifecycle, &reader))
        // Serialise through `Version` so timestamps are RFC 3339 (json! on OffsetDateTime is a tuple).
        .filter_map(|v| serde_json::to_value(&v).ok())
        .map(|v| json!({"id": v["id"], "version_no": v["version_no"], "lifecycle": v["lifecycle"], "content_hash": v["content_hash"], "effective_at": v["effective_at"], "published_at": v["published_at"], "change_reason": v["change_reason"]}))
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
    authorize_version(&s, &me, &mut tx, b, "view").await?;
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
    /// `major` (default) or `minor` (BPMS-045); ignored for version 1.
    #[serde(default)]
    change: Option<String>,
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
    let change = r.change.as_deref().map(Change::parse).transpose()?.unwrap_or_default();
    let lib = library(&s)?;
    let who = actor(&me, &headers);
    let v =
        lib.publish(&mut tx, &who, id, r.effective_at, r.change_reason.as_deref(), change, &policy).await?;
    // BPMS-045: a version that goes live now and requires it asks its audience to acknowledge again.
    let reack = if !v.reack_required {
        json!({"required": false, "due_at": null, "audience_size": null, "campaign_id": null})
    } else if v.lifecycle == "published" {
        let cmp = campaigns_service(&s)?;
        let by = procera_campaigns::Actor {
            membership: who.membership,
            subject_ref: who.subject_ref,
            ip: who.ip.clone(),
            user_agent: who.user_agent.clone(),
        };
        let c =
            crate::reack::start(lib, cmp, settings(&s)?, &mut tx, id, Some(&by)).await.map_err(internal)?;
        json!({"required": true, "due_at": c.as_ref().and_then(|c| c.due_at).map(rfc3339),
               "audience_size": c.as_ref().map_or(0, |c| c.progress.assigned), "campaign_id": c.map(|c| c.id)})
    } else {
        // Scheduled: the campaign starts when the version goes live.
        let (size, _) = reack_audience(&s, lib, &mut tx, id).await?;
        let window = lib.reack_window(&mut tx).await?;
        let due = v.effective_at.map(|t| t + window);
        json!({"required": true, "due_at": due.map(rfc3339), "audience_size": size, "campaign_id": null})
    };
    tx.commit().await.map_err(internal)?;
    let mut body = serde_json::to_value(&v).map_err(internal)?;
    body["reack"] = reack;
    Ok(Json(body).into_response())
}

fn campaigns_service(s: &AppState) -> Result<&procera_campaigns::Campaigns, ApiError> {
    s.campaigns.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// How many active members a re-acknowledgement of version `id` would reach now.
async fn reack_audience(
    s: &AppState,
    lib: &Library,
    tx: &mut WsTx<'_>,
    id: Uuid,
) -> Result<(usize, Value), ApiError> {
    let subject = lib.reack_subject(tx, id).await?.ok_or_else(ApiError::not_found)?;
    let aud = crate::reack::audience(&subject);
    let size = campaigns_service(s)?.audience_size(tx, &aud).await?;
    Ok((size, serde_json::to_value(&aud).map_err(internal)?))
}

/// What publishing a version would do (BPMS-045, docs/05 §3.22): its number as a major or minor
/// change, whether each asks for a re-acknowledgement, how many people that reaches and by when.
async fn publish_preview(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "publish").await?;
    let lib = library(&s)?;
    let plan = lib.publish_plan(&mut tx, id).await?;
    let (size, audience) = reack_audience(&s, lib, &mut tx, id).await?;
    let due = lib.reack_due(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    let mut body = serde_json::to_value(&plan).map_err(internal)?;
    body["audience"] = audience;
    body["audience_size"] = json!(size);
    body["due_at"] = json!(rfc3339(due));
    Ok(Json(body).into_response())
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

/// Loads the artifact, checks `action` on its kind and that the member may see it (hidden → 404).
async fn authorize_artifact(
    s: &AppState,
    me: &WsMember,
    tx: &mut WsTx<'_>,
    id: Uuid,
    action: &str,
) -> Result<procera_library::Artifact, ApiError> {
    let lib = library(s)?;
    let a = lib.artifact(tx, id).await?;
    me.require(members(s)?, tx, permission(&a.kind, action)?, Resource::Workspace).await?;
    let full = full_kinds(s, me, tx).await?;
    if !lib.visible(tx, &a, &Reader { membership: me.actor.membership, full_kinds: &full }).await? {
        return Err(ApiError::not_found());
    }
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

// ---- BPMS-045: applicability and the employee policy portal ----

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicabilityReq {
    category: Option<String>,
    /// An audience (`{all, departments, groups, roles}`); `null` = applies to everyone.
    applicability: Option<procera_members::audience::Audience>,
    #[serde(default)]
    restricted: bool,
}

async fn set_applicability(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ApplicabilityReq>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let aud = match r.applicability {
        Some(a) if a.is_empty() => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("applicability must name at least one audience, or be null for everyone"));
        }
        Some(a) => Some(serde_json::to_value(a).map_err(internal)?),
        None => None,
    };
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "publish").await?;
    let a = library(&s)?
        .set_applicability(
            &mut tx,
            &actor(&me, &headers),
            id,
            r.category.as_deref(),
            aud.as_ref(),
            r.restricted,
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

#[derive(Debug, Deserialize)]
struct PortalQuery {
    q: Option<String>,
    category: Option<String>,
}

/// The policies that apply to the caller (FR-PL02), with what changed since the previous version.
async fn portal(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<PortalQuery>,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "policy:view", Resource::Workspace).await?;
    let items =
        library(&s)?.portal(&mut tx, me.actor.membership, q.q.as_deref(), q.category.as_deref()).await?;
    tx.rollback().await.map_err(internal)?;
    let mut categories: Vec<&str> = items.iter().filter_map(|p| p.category.as_deref()).collect();
    categories.dedup();
    Ok(Json(json!({ "items": items, "categories": categories })))
}

// ---- BPMS-041: collections, owners, RACI ----

/// Whether the member manages collections (sees and edits all of them).
async fn manages_collections(s: &AppState, me: &WsMember, tx: &mut WsTx<'_>) -> Result<bool, ApiError> {
    Ok(members(s)?
        .access()
        .check(tx, me.actor.membership, "collection:manage", &Resource::Workspace)
        .await?
        .allowed)
}

async fn list_collections(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "collection:view", Resource::Workspace).await?;
    let all = manages_collections(&s, &me, &mut tx).await?;
    let full = full_kinds(&s, &me, &mut tx).await?;
    let reader = Reader { membership: me.actor.membership, full_kinds: &full };
    let items = library(&s)?.collections(&mut tx, &reader, all).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items, "can_manage": all })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionReq {
    name: String,
    parent_id: Option<Uuid>,
}

async fn create_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CollectionReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "collection:manage", Resource::Workspace).await?;
    let c = library(&s)?.create_collection(&mut tx, &actor(&me, &headers), &r.name, r.parent_id).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(c)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionPatch {
    name: Option<String>,
    /// Present (even `null` = top level) to move the collection.
    #[serde(default, deserialize_with = "present")]
    parent_id: Option<Option<Uuid>>,
}

/// Distinguishes an absent field (`None`) from an explicit `null` (`Some(None)`).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<Uuid>>, D::Error> {
    Option::<Uuid>::deserialize(d).map(Some)
}

async fn update_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CollectionPatch>,
) -> Result<Json<procera_library::collections::Collection>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "collection:manage", Resource::Workspace).await?;
    let c = library(&s)?
        .update_collection(&mut tx, &actor(&me, &headers), id, r.name.as_deref(), r.parent_id)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(c))
}

async fn delete_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "collection:manage", Resource::Workspace).await?;
    library(&s)?.delete_collection(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibilityReq {
    /// An audience; `null` = everyone in the workspace.
    visibility: Option<procera_members::audience::Audience>,
}

async fn set_collection_visibility(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<VisibilityReq>,
) -> Result<Json<procera_library::collections::Collection>, ApiError> {
    let v = match r.visibility {
        Some(a) if a.is_empty() => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("visibility must name at least one audience, or be null for everyone"));
        }
        Some(a) => Some(serde_json::to_value(a).map_err(internal)?),
        None => None,
    };
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "collection:manage", Resource::Workspace).await?;
    let c = library(&s)?.set_collection_visibility(&mut tx, &actor(&me, &headers), id, v.as_ref()).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(c))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReq {
    collection_id: Option<Uuid>,
}

/// Filing a document needs `publish` on its kind (the people who control where it lives).
async fn set_artifact_collection(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<FileReq>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "publish").await?;
    let a = library(&s)?.set_artifact_collection(&mut tx, &actor(&me, &headers), id, r.collection_id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerReq {
    membership_id: Uuid,
}

async fn set_owner(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<OwnerReq>,
) -> Result<Json<procera_library::Artifact>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "publish").await?;
    let a = library(&s)?.set_owner(&mut tx, &actor(&me, &headers), id, r.membership_id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(a))
}

async fn get_raci(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<procera_library::collections::Raci>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "view").await?;
    let r = library(&s)?.raci(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(r))
}

async fn set_raci(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<procera_library::collections::Raci>,
) -> Result<Json<procera_library::collections::Raci>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_artifact(&s, &me, &mut tx, id, "publish").await?;
    let out = library(&s)?.set_raci(&mut tx, &actor(&me, &headers), id, &r).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    collection: Option<Uuid>,
}

/// RACI of every document the caller may see, as CSV (FR-G01 "RACI exportable"). Audited.
async fn export_raci(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let mut kinds = vec![];
    for k in AUTHORABLE {
        if m.access()
            .check(&mut tx, me.actor.membership, permission(k, "view")?, &Resource::Workspace)
            .await?
            .allowed
        {
            kinds.push(*k);
        }
    }
    if kinds.is_empty() {
        return Err(ApiError::new(403, "forbidden", "Forbidden"));
    }
    let full = full_kinds(&s, &me, &mut tx).await?;
    let reader = Reader { membership: me.actor.membership, full_kinds: &full };
    let csv = library(&s)?.raci_csv(&mut tx, &reader, &kinds, q.collection).await?;
    let rows = csv.lines().count().saturating_sub(1);
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "raci.exported",
        "collection",
        q.collection.unwrap_or_default(),
        json!({"rows": rows, "collection_id": q.collection}),
        &super::auth::client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    let mut res = csv.into_response();
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    res.headers_mut()
        .insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"raci.csv\""));
    Ok(res)
}

// ---- BPMS-042: export a version as print-ready HTML or Markdown ----

#[derive(Debug, Deserialize)]
struct VersionExportQuery {
    #[serde(default)]
    format: Option<String>,
}

/// A self-contained, print-ready HTML document (browsers print it as a tagged PDF) or Markdown.
/// Superseded and retired versions carry a "not the current version" banner (FR-PL01). Audited.
async fn export_version(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<VersionExportQuery>,
) -> Result<Response, ApiError> {
    let format = q.format.as_deref().unwrap_or("html");
    if !matches!(format, "html" | "md") {
        return Err(
            ApiError::new(422, "validation_failed", "Validation failed").detail("format must be html or md")
        );
    }
    let mut tx = me.tx(members(&s)?).await?;
    authorize_version(&s, &me, &mut tx, id, "view").await?;
    let lib = library(&s)?;
    let (v, a) = lib.version(&mut tx, id, None).await?;
    lib.audit_export(&mut tx, &actor(&me, &headers), id, format).await?;
    tx.commit().await.map_err(internal)?;
    let superseded = matches!(v.lifecycle.as_str(), "deprecated" | "retired");
    let slug: String = a
        .key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '-' })
        .collect();
    if format == "md" {
        let mut md = format!("# {}\n\n", a.title);
        if superseded {
            md.push_str(&format!(
                "> **Superseded:** this is version {}, not the current version.\n\n",
                v.version_no
            ));
        }
        md.push_str(&procera_library::blocks::blocks_markdown(&v.content));
        let mut res = md.into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/markdown; charset=utf-8"));
        if let Ok(h) = HeaderValue::from_str(&format!("attachment; filename=\"{slug}-v{}.md\"", v.version_no))
        {
            res.headers_mut().insert(header::CONTENT_DISPOSITION, h);
        }
        return Ok(res);
    }
    let esc =
        |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let published = v
        .published_at
        .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok())
        .map(|t| t[..10].to_owned());
    let banner = if superseded {
        format!(
            "<p class=\"superseded\" role=\"note\"><strong>Superseded.</strong> This is version {}, not the current version.</p>",
            v.version_no
        )
    } else {
        String::new()
    };
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · v{no}</title>
<style>
  body {{ font: 11pt/1.55 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; color: #1c1c1a; max-width: 42rem; margin: 2.5rem auto; padding: 0 1.25rem; }}
  h1 {{ font-size: 1.75rem; line-height: 1.2; margin: 0 0 .25rem; }}
  .meta {{ color: #5f5f5a; font-size: .875rem; margin: 0 0 1.5rem; }}
  h2 {{ font-size: 1.25rem; margin: 1.75rem 0 .5rem; }} h3 {{ font-size: 1.0625rem; margin: 1.25rem 0 .5rem; }} h4 {{ font-size: 1rem; }}
  table {{ border-collapse: collapse; width: 100%; margin: 1rem 0; }} th, td {{ border: 1px solid #d6d6d0; padding: .375rem .5rem; text-align: left; vertical-align: top; }}
  th {{ background: #f1f1ef; }}
  .callout {{ border-left: 3px solid #5f5f5a; background: #f6f6f4; padding: .5rem .875rem; margin: 1rem 0; }} .callout.warning {{ border-color: #b45309; }}
  .superseded {{ border: 2px solid #b42318; color: #b42318; padding: .5rem .875rem; margin: 0 0 1.25rem; }}
  footer {{ margin-top: 2.5rem; padding-top: .75rem; border-top: 1px solid #e4e4e0; color: #5f5f5a; font-size: .75rem; }}
  @media print {{ body {{ margin: 0; max-width: none; }} a {{ color: inherit; }} }}
</style></head>
<body><main>
<h1>{title}</h1>
<p class="meta">{kind} · version {no}{semver}{published} · status {status}</p>
{banner}
{body}
</main>
<footer>Exported from Procera.{hash}</footer>
</body></html>"#,
        title = esc(&a.title),
        no = v.version_no,
        kind = if a.kind == "sop" { "SOP" } else { "Policy" },
        semver = v.semver.as_deref().map(|s| format!(" ({s})")).unwrap_or_default(),
        published = published.map(|p| format!(" · published {p}")).unwrap_or_default(),
        status = esc(&v.lifecycle),
        banner = banner,
        body = procera_library::blocks::blocks_html(&v.content),
        hash = v
            .content_hash
            .as_deref()
            .map(|h| format!(" Content fingerprint (SHA-256): {h}"))
            .unwrap_or_default(),
    );
    let mut res = Html(html).into_response();
    if let Ok(h) = HeaderValue::from_str(&format!("inline; filename=\"{slug}-v{}.html\"", v.version_no)) {
        res.headers_mut().insert(header::CONTENT_DISPOSITION, h);
    }
    Ok(res)
}
