//! Files API (BPMS-060/062; FR-F01–F03, F05–F07): resumable upload, attachments to document drafts,
//! signed downloads re-authorized on every request, sandboxed inline previews of images and PDFs
//! (BPMS-061, FR-F04), and the workspace Files view (usage, orphans).

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_files::{Actor, FileError, Files, LinkPurpose, MAX_CHUNK, preview_kind};
use procera_library::{Library, Reader};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/files", get(list).post(create))
        .route("/files/usage", get(usage))
        .route("/files/orphans", get(orphans))
        .route("/files/orphans/purge", post(purge_orphans))
        .route("/files/retention", post(set_retention))
        .route("/files/{id}", get(get_file).delete(delete_file))
        .route("/files/{id}/content", put(append).layer(DefaultBodyLimit::max(MAX_CHUNK + 4096)))
        .route("/files/{id}/complete", post(complete))
        .route("/files/{id}/download-url", get(download_url))
        .route("/files/{id}/preview-url", get(preview_url))
        .route("/files/{id}/links", post(link))
        .route("/files/{id}/links/{object_type}/{object_id}", delete(unlink))
        .route("/versions/{id}/attachments", get(attachments))
        .route("/files/download/{token}", get(download))
        .route("/files/preview/{token}", get(preview))
}

impl From<FileError> for ApiError {
    fn from(e: FileError) -> Self {
        match e {
            FileError::NotFound => Self::not_found(),
            FileError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            FileError::Conflict(m) => {
                Self::new(409, "state_conflict", "Not allowed in the current state").detail(m)
            }
            FileError::TooLarge(m) => Self::new(413, "too_large", "Too large").detail(m),
            FileError::Internal(m) => {
                tracing::error!(error = %m, "files internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "files api internal error");
    ApiError::new(500, "internal", "Internal error")
}

pub(super) fn files(s: &AppState) -> Result<&Files, ApiError> {
    s.files.as_ref().ok_or_else(|| ApiError::unavailable("database"))
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

async fn allowed(
    s: &AppState,
    tx: &mut procera_db::WsTx<'_>,
    membership: Uuid,
    perm: &str,
) -> Result<bool, ApiError> {
    Ok(members(s)?.access().check(tx, membership, perm, &Resource::Workspace).await?.allowed)
}

/// Whether `membership` may see a file: its uploader, `file:view_all`, or a reader of a document
/// version it is attached to (A-14 visibility applies).
async fn can_view(
    s: &AppState,
    tx: &mut procera_db::WsTx<'_>,
    membership: Uuid,
    id: Uuid,
) -> Result<bool, ApiError> {
    let f = files(s)?.get(tx, id).await?;
    if f.uploaded_by == membership || allowed(s, tx, membership, "file:view_all").await? {
        return Ok(true);
    }
    let lib = s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    for (object_type, object_id) in files(s)?.links_of(tx, id).await? {
        if object_type != "artifact_version" {
            continue;
        }
        let Ok((v, a)) = lib.version(tx, object_id, None).await else { continue };
        if !allowed(s, tx, membership, procera_library::permission(&a.kind, "view")?).await? {
            continue;
        }
        let mut full = vec![];
        for action in ["create", "edit_draft", "review", "approve", "publish"] {
            if allowed(s, tx, membership, procera_library::permission(&a.kind, action)?).await? {
                full.push(a.kind.as_str());
                break;
            }
        }
        let reader = Reader { membership, full_kinds: &full };
        if lib.visible(tx, &a, &reader).await? && Library::version_visible(&a.kind, &v.lifecycle, &reader) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    name: String,
    size: i64,
    content_type: Option<String>,
}

async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:upload", Resource::Workspace).await?;
    let f =
        files(&s)?.create(&mut tx, &actor(&me, &headers), &r.name, r.size, r.content_type.as_deref()).await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(json!({ "file": f, "chunk_size": MAX_CHUNK }))).into_response())
}

#[derive(Debug, Deserialize)]
struct OffsetQuery {
    offset: i64,
}

/// One chunk at `offset` (raw bytes). Resume by reading `received` from `GET /files/{id}`.
async fn append(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<OffsetQuery>,
    body: Bytes,
) -> Result<Json<procera_files::File>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let f = files(&s)?.append(&mut tx, &actor(&me, &headers), id, q.offset, &body).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(f))
}

async fn complete(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<procera_files::File>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let f = files(&s)?.complete(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(f))
}

async fn get_file(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    if !can_view(&s, &mut tx, me.actor.membership, id).await? {
        return Err(ApiError::not_found());
    }
    let svc = files(&s)?;
    let f = svc.get(&mut tx, id).await?;
    let links: Vec<Value> = svc
        .links_of(&mut tx, id)
        .await?
        .into_iter()
        .map(|(t, o)| json!({"object_type": t, "object_id": o}))
        .collect();
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "file": f, "links": links })))
}

async fn delete_file(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let svc = files(&s)?;
    let f = svc.get(&mut tx, id).await?;
    // The uploader may delete their own unattached file; otherwise `file:delete`.
    let own_loose = f.uploaded_by == me.actor.membership && svc.links_of(&mut tx, id).await?.is_empty();
    if !own_loose && !allowed(&s, &mut tx, me.actor.membership, "file:delete").await? {
        return Err(if can_view(&s, &mut tx, me.actor.membership, id).await? {
            ApiError::new(403, "forbidden", "Forbidden")
        } else {
            ApiError::not_found()
        });
    }
    svc.delete(&mut tx, &actor(&me, &headers), id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    state: Option<String>,
}

/// The workspace Files view (`file:view_all`), or the caller's own uploads.
async fn list(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let all = allowed(&s, &mut tx, me.actor.membership, "file:view_all").await?;
    let svc = files(&s)?;
    let items = svc.list(&mut tx, q.state.as_deref(), (!all).then_some(me.actor.membership)).await?;
    let mut out = Vec::with_capacity(items.len());
    for f in items {
        let links: Vec<Value> = svc
            .links_of(&mut tx, f.id)
            .await?
            .into_iter()
            .map(|(t, o)| json!({"object_type": t, "object_id": o}))
            .collect();
        let class = procera_files::Files::retention_class(&mut tx, f.id).await?;
        let set = procera_files::Files::retention_overridden(&mut tx, f.id).await?;
        out.push(json!({ "file": f, "links": links, "retention_class": class, "retention_set": set }));
    }
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": out, "all": all })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionReq {
    file_ids: Vec<Uuid>,
    /// A record class, or null to follow what holds each file again.
    class: Option<String>,
    reason: String,
}

/// Bulk retention change (E2E-R1-12): sets or clears the class of the chosen files, with a reason.
async fn set_retention(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<RetentionReq>,
) -> Result<Json<Value>, ApiError> {
    if r.file_ids.is_empty() || r.file_ids.len() > 500 {
        return Err(
            ApiError::new(422, "validation_failed", "Validation failed").detail("choose 1 to 500 files")
        );
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:delete", Resource::Workspace).await?;
    let n = files(&s)?
        .set_retention(&mut tx, &actor(&me, &headers), &r.file_ids, r.class.as_deref(), &r.reason)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "updated": n })))
}

async fn usage(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:view_all", Resource::Workspace).await?;
    let u = files(&s)?.usage(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(u))
}

async fn orphans(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:view_all", Resource::Workspace).await?;
    let items = files(&s)?.orphans(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })))
}

async fn purge_orphans(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:delete", Resource::Workspace).await?;
    let n = files(&s)?.purge_orphans(&mut tx, &actor(&me, &headers)).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "purged": n })))
}

/// A signed, single-member download link valid for 5 minutes.
async fn download_url(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:download", Resource::Workspace).await?;
    if !can_view(&s, &mut tx, me.actor.membership, id).await? {
        return Err(ApiError::not_found());
    }
    let svc = files(&s)?;
    let f = svc.get(&mut tx, id).await?;
    if f.state != "clean" {
        return Err(ApiError::new(409, "state_conflict", "Not allowed in the current state")
            .detail(format!("the file is {}", f.state)));
    }
    let (token, exp) = svc.download_token(tx.ctx.workspace_id, id, me.actor.membership)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(
        json!({ "url": format!("/api/v1/files/download/{token}"), "expires_at": exp.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }),
    ))
}

/// Checks a signed link for `purpose` and re-authorizes its member (active, `file:download`, can see
/// the file). Returns the open transaction, the file id and the member as an audit actor.
async fn authorize_link<'a>(
    s: &'a AppState,
    headers: &HeaderMap,
    purpose: LinkPurpose,
    token: &str,
) -> Result<(procera_db::WsTx<'a>, Uuid, Actor), ApiError> {
    let invalid = || ApiError::new(403, "link_invalid", "This link is invalid or has expired");
    let (ws, id, membership) = files(s)?.verify_link(purpose, token).ok_or_else(invalid)?;
    let db = s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let mut tx = db.begin_ws(procera_db::WsCtx { workspace_id: ws }).await.map_err(internal)?;
    let active: Option<(Uuid,)> = sqlx::query_as("SELECT subject_ref FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1 AND m.status = 'active'")
        .bind(membership).fetch_optional(tx.conn()).await.map_err(internal)?;
    let Some((subject_ref,)) = active else { return Err(invalid()) };
    if !allowed(s, &mut tx, membership, "file:download").await?
        || !can_view(s, &mut tx, membership, id).await?
    {
        return Err(ApiError::not_found());
    }
    let c = super::auth::client_info(headers);
    Ok((tx, id, Actor { membership, subject_ref, ip: c.ip, user_agent: c.user_agent }))
}

/// A file name safe inside a quoted `Content-Disposition` parameter.
fn disposition(kind: &str, name: &str) -> Option<HeaderValue> {
    let safe: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_ ".contains(c) { c } else { '_' }).collect();
    HeaderValue::from_str(&format!("{kind}; filename=\"{safe}\"")).ok()
}

/// Serves a file for a valid token; re-checks membership, permission and file state every time.
async fn download(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let svc = files(&s)?;
    let (mut tx, id, who) = authorize_link(&s, &headers, LinkPurpose::Download, &token).await?;
    let (f, data) = svc.content(&mut tx, id).await?;
    svc.record_download(&mut tx, &who, id).await?;
    tx.commit().await.map_err(internal)?;
    let mut res = data.into_response();
    let h = res.headers_mut();
    let ct = f.sniffed_type.as_deref().unwrap_or("application/octet-stream");
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(ct).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(v) = disposition("attachment", &f.name) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}

/// The policy every preview is served under: a sandboxed, opaque-origin document that may load
/// nothing but itself (FR-F04 "no preview runs active content").
const PREVIEW_CSP: &str = "sandbox; default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'";

/// A signed preview link for a clean image or PDF (`kind: image|pdf`), or `kind: none` and no URL.
/// Same permission rules as `download-url`.
async fn preview_url(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "file:download", Resource::Workspace).await?;
    if !can_view(&s, &mut tx, me.actor.membership, id).await? {
        return Err(ApiError::not_found());
    }
    let svc = files(&s)?;
    let f = svc.get(&mut tx, id).await?;
    if f.state != "clean" {
        return Err(ApiError::new(409, "state_conflict", "Not allowed in the current state")
            .detail(format!("the file is {}", f.state)));
    }
    let Some((kind, _)) = preview_kind(f.sniffed_type.as_deref()) else {
        tx.rollback().await.map_err(internal)?;
        return Ok(Json(json!({ "url": null, "kind": "none", "expires_at": null })));
    };
    let (token, exp) = svc.link_token(LinkPurpose::Preview, tx.ctx.workspace_id, id, me.actor.membership)?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({
        "url": format!("/api/v1/files/preview/{token}"),
        "kind": kind,
        "expires_at": exp.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
    })))
}

/// Serves an image or PDF inline for a valid preview token, re-authorized on every request, under a
/// strict sandboxing CSP. Anything not on the preview allow-list is 415.
async fn preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let svc = files(&s)?;
    let (mut tx, id, who) = authorize_link(&s, &headers, LinkPurpose::Preview, &token).await?;
    let (f, ct, data) = match svc.preview(&mut tx, id).await {
        Err(FileError::Invalid(m)) => {
            return Err(ApiError::new(415, "unsupported_media_type", "This file has no preview").detail(m));
        }
        r => r?,
    };
    svc.record_preview(&mut tx, &who, id).await?;
    tx.commit().await.map_err(internal)?;
    let mut res = data.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    if let Some(v) = disposition("inline", &f.name) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(PREVIEW_CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert("cross-origin-resource-policy", HeaderValue::from_static("same-origin"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkReq {
    object_type: String,
    object_id: Uuid,
}

/// Attaching to a document version needs `edit_draft` on its kind, and the version must be a draft.
async fn authorize_draft(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    object_type: &str,
    object_id: Uuid,
) -> Result<(), ApiError> {
    if object_type != "artifact_version" {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("files attach to document versions (artifact_version)"));
    }
    let lib = s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let (v, a) = lib.version(tx, object_id, None).await?;
    me.require(members(s)?, tx, procera_library::permission(&a.kind, "edit_draft")?, Resource::Workspace)
        .await?;
    if v.lifecycle != "draft" {
        return Err(ApiError::new(409, "state_conflict", "Not allowed in the current state")
            .detail("attachments of a version past draft cannot change"));
    }
    Ok(())
}

async fn link(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<LinkReq>,
) -> Result<StatusCode, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_draft(&s, &me, &mut tx, &r.object_type, r.object_id).await?;
    if !can_view(&s, &mut tx, me.actor.membership, id).await? {
        return Err(ApiError::not_found());
    }
    files(&s)?.link(&mut tx, &actor(&me, &headers), id, &r.object_type, r.object_id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlink(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, object_type, object_id)): Path<(Uuid, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    authorize_draft(&s, &me, &mut tx, &object_type, object_id).await?;
    files(&s)?.unlink(&mut tx, &actor(&me, &headers), id, &object_type, object_id).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Files attached to a document version (readers of the version see them).
async fn attachments(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let lib = s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let (v, a) = lib.version(&mut tx, id, None).await?;
    me.require(members(&s)?, &mut tx, procera_library::permission(&a.kind, "view")?, Resource::Workspace)
        .await?;
    let full = super::library::full_kinds(&s, &me, &mut tx).await?;
    let reader = Reader { membership: me.actor.membership, full_kinds: &full };
    if !lib.visible(&mut tx, &a, &reader).await? || !Library::version_visible(&a.kind, &v.lifecycle, &reader)
    {
        return Err(ApiError::not_found());
    }
    let items = files(&s)?.linked(&mut tx, "artifact_version", id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items, "editable": v.lifecycle == "draft" })))
}
