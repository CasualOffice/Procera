//! Processes API (0.3, BPMS-100 / BPMS-084, docs/08 "Processes"): the Guided Studio's backend.
//! A process is a library artifact of kind `process` whose versions hold `{ir, studio}`; this API
//! lists them, creates one from a template (blank or a built-in request type), saves the draft
//! (opening a new draft from the published version when there is none), validates a draft against
//! the workspace the way publishing will, and publishes through the library lifecycle (submit, then
//! publish when no review is needed). Publishing compiles the process and writes its release bundle
//! (the library's publish hook); from then on it is offered under New request.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_library::{Change, LibraryError, permission};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::library::{actor, library, review_policy};
use super::requests::{internal, requests};
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/processes", get(list).post(create))
        .route("/process-templates", get(templates))
        .route("/processes/validate", post(validate))
        .route("/processes/{id}", get(one))
        .route("/processes/{id}/draft", put(save_draft))
        .route("/processes/{id}/publish", post(publish))
}

async fn allowed(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    action: &str,
) -> Result<bool, ApiError> {
    let p = permission("process", action).map_err(ApiError::from)?;
    Ok(members(s)?.access().check(tx, me.actor.membership, p, &Resource::Workspace).await?.allowed)
}

async fn require(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    action: &str,
) -> Result<(), ApiError> {
    let m = members(s)?;
    me.require(m, tx, permission("process", action).map_err(ApiError::from)?, Resource::Workspace).await
}

type VersionRow = (Uuid, i32, String, i64, Value, Option<time::OffsetDateTime>);

/// The open draft (or review) and the published version of a process artifact.
async fn versions(
    tx: &mut procera_db::WsTx<'_>,
    artifact: Uuid,
) -> Result<(Option<VersionRow>, Option<VersionRow>), ApiError> {
    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT id, version_no, lifecycle, revision, content, published_at FROM artifact_versions
          WHERE artifact_id = $1 AND lifecycle IN ('draft', 'in_review', 'approved', 'scheduled', 'published') ORDER BY version_no DESC",
    )
    .bind(artifact)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?;
    let draft = rows.iter().find(|r| r.2 != "published").cloned();
    let published = rows.into_iter().find(|r| r.2 == "published");
    Ok((draft, published))
}

/// `GET /processes`: every process in the workspace, with its draft and published versions.
async fn list(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "view").await?;
    let rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, key, title, state FROM artifacts WHERE kind = 'process' ORDER BY lower(title)",
    )
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?;
    let mut items = Vec::new();
    for (id, key, title, state) in rows {
        let (draft, published) = versions(&mut tx, id).await?;
        items.push(json!({
            "id": id, "key": key, "title": title, "state": state,
            "draft": draft.map(|d| json!({ "version_id": d.0, "version_no": d.1, "lifecycle": d.2 })),
            "published": published.map(|p| json!({ "version_id": p.0, "version_no": p.1, "published_at": p.5 })),
        }));
    }
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `GET /process-templates`: a blank process and every built-in request type.
async fn templates(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "view").await?;
    tx.rollback().await.map_err(internal)?;
    let mut items = vec![
        json!({ "key": "blank", "name": "Blank", "description": "Start from nothing: add the form and the steps yourself." }),
    ];
    for t in requests(&s)?.catalogue().list() {
        items.push(
            json!({ "key": t.key, "name": t.name, "description": t.description("the workspace currency") }),
        );
    }
    Ok(Json(json!({ "items": items })).into_response())
}

/// The starting content for `template`, renamed to `key` and `title`.
fn template_content(s: &AppState, template: &str, key: &str, title: &str) -> Result<Value, ApiError> {
    let invalid = |m: &str| ApiError::from(LibraryError::Invalid(m.to_owned()));
    let mut studio = json!({ "request": { "name": title, "description": "", "category": "other", "prefix": prefix_for(title) }, "fields": {} });
    let mut ir = if template == "blank" {
        json!({
            "schemaVersion": 1,
            "process": { "key": key, "version": 1, "name": { "en": title } },
            "variables": [],
            "nodes": [
                { "id": "start", "kind": "start", "name": { "en": "Request" }, "trigger": "manual" },
                { "id": "end_approved", "kind": "end", "name": { "en": "Approved" }, "result": "completed" }
            ],
            "edges": [{ "id": "e1", "from": "start", "to": "end_approved" }],
            "refs": { "calendars": { "default": "calendar:default@1" } }
        })
    } else {
        let t = requests(s)?.catalogue().get(template).cloned().ok_or_else(|| invalid("unknown template"))?;
        let mut ir = serde_json::to_value(t.exe.program.ir()).map_err(internal)?;
        // A template can't bring the built-in's forms or SOP with it: drop those references (and the
        // steps' form names); the field labels move into `studio`.
        if let Some(refs) = ir["refs"].as_object_mut() {
            for k in ["forms", "sop", "policy", "documents", "rules"] {
                refs.remove(k);
            }
        }
        if let Some(nodes) = ir["nodes"].as_array_mut() {
            for n in nodes {
                if let Some(o) = n.as_object_mut() {
                    o.remove("inputForm");
                    o.remove("form");
                }
            }
        }
        if let Some(meta) = ir["meta"].as_object_mut() {
            meta.remove("templateRef");
        }
        studio["request"]["category"] = json!(t.category);
        studio["request"]["description"] = json!(t.description("the workspace currency"));
        for f in &t.start_form.fields {
            studio["fields"][&f.name] = json!({ "label": f.label });
        }
        ir["process"]["key"] = json!(key);
        ir["process"]["version"] = json!(1);
        ir["process"]["name"] = json!({ "en": title });
        ir
    };
    if ir["refs"].get("calendars").is_none() {
        ir["refs"]["calendars"] = json!({ "default": "calendar:default@1" });
    }
    Ok(json!({ "ir": ir, "studio": studio }))
}

/// Two to four capitals from the title ("Laptop request" → "LR").
fn prefix_for(title: &str) -> String {
    let p: String = title
        .split_whitespace()
        .filter_map(|w| w.chars().find(char::is_ascii_alphabetic))
        .take(3)
        .collect::<String>()
        .to_uppercase();
    if p.len() >= 2 { p } else { "RQ".into() }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    key: String,
    title: String,
    #[serde(default = "blank")]
    template: String,
}

fn blank() -> String {
    "blank".into()
}

/// `POST /processes`: a new process with its first draft from a template.
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "create").await?;
    let content = template_content(&s, &r.template, r.key.trim(), r.title.trim())?;
    let (a, _) = library(&s)?
        .create(&mut tx, &actor(&me, &headers), "process", r.key.trim(), r.title.trim(), "standard", &content)
        .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": a.id }))).into_response())
}

/// `GET /processes/{id}`.
async fn one(State(s): State<AppState>, me: WsMember, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "view").await?;
    let a = library(&s)?.artifact(&mut tx, id).await?;
    if a.kind != "process" {
        return Err(ApiError::not_found());
    }
    let (draft, published) = versions(&mut tx, id).await?;
    let can = json!({ "edit": allowed(&s, &me, &mut tx, "edit_draft").await?, "publish": allowed(&s, &me, &mut tx, "publish").await? });
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({
        "id": a.id, "key": a.key, "title": a.title, "state": a.state,
        "draft": draft.map(|d| json!({ "version_id": d.0, "version_no": d.1, "lifecycle": d.2, "revision": d.3, "content": d.4 })),
        "published": published.map(|p| json!({ "version_id": p.0, "version_no": p.1, "published_at": p.5, "content": p.4 })),
        "can": can,
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftReq {
    content: Value,
}

/// `PUT /processes/{id}/draft` with `If-Match: "<revision>"`: saves the open draft, or opens a new
/// draft from the published version first (then If-Match is not needed).
async fn save_draft(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<DraftReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "edit_draft").await?;
    let lib = library(&s)?;
    let a = lib.artifact(&mut tx, id).await?;
    if a.kind != "process" {
        return Err(ApiError::not_found());
    }
    let who = actor(&me, &headers);
    let (draft, _) = versions(&mut tx, id).await?;
    let (vid, expected) = match draft {
        Some(d) if d.2 == "draft" => {
            let rev = headers
                .get(header::IF_MATCH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().trim_matches('"').parse::<i64>().ok())
                .ok_or_else(|| {
                    ApiError::new(428, "precondition_required", "If-Match required")
                        .detail("send the draft's revision")
                })?;
            (d.0, Some(rev))
        }
        Some(d) => {
            return Err(LibraryError::Conflict(format!(
                "version {} is {}; it can't be edited",
                d.1,
                d.2.replace('_', " ")
            ))
            .into());
        }
        None => (lib.new_version(&mut tx, &who, id).await?.id, None),
    };
    // Keep the IR's own version number in step with the draft's.
    let no: i32 = sqlx::query_scalar("SELECT version_no FROM artifact_versions WHERE id = $1")
        .bind(vid)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    let mut content = r.content;
    if let Some(p) = content.pointer_mut("/ir/process").and_then(Value::as_object_mut) {
        p.insert("version".into(), json!(no));
        p.insert("key".into(), json!(a.key));
    }
    let v = lib.edit(&mut tx, &who, vid, &content, expected).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "version_id": v.id, "revision": v.revision })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateReq {
    content: Value,
}

/// `POST /processes/validate`: the problems publishing would report, in plain language.
async fn validate(
    State(s): State<AppState>,
    me: WsMember,
    Json(r): Json<ValidateReq>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "view").await?;
    let problems = procera_processes::validate(&mut tx, &r.content).await.map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let ok = !problems.iter().any(|p| p.severity == "error");
    Ok(Json(json!({ "ok": ok, "problems": problems })).into_response())
}

/// `POST /processes/{id}/publish`: submits the draft and, when no review is needed, publishes it.
/// A process that does not compile is refused with its problems (422 `{problems}`).
async fn publish(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    require(&s, &me, &mut tx, "publish").await?;
    let lib = library(&s)?;
    let a = lib.artifact(&mut tx, id).await?;
    if a.kind != "process" {
        return Err(ApiError::not_found());
    }
    let (draft, _) = versions(&mut tx, id).await?;
    let d =
        draft.ok_or_else(|| ApiError::from(LibraryError::Conflict("there is no draft to publish".into())))?;
    let problems = procera_processes::validate(&mut tx, &d.4).await.map_err(internal)?;
    if problems.iter().any(|p| p.severity == "error") {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "title": "This process can't be published yet", "problems": problems })),
        )
            .into_response());
    }
    let who = actor(&me, &headers);
    let policy = review_policy(&s, &mut tx).await?;
    let mut lifecycle = d.2.clone();
    if lifecycle == "draft" {
        lifecycle = lib.submit(&mut tx, &who, d.0, &policy).await?.lifecycle;
    }
    if lifecycle != "approved" {
        tx.commit().await.map_err(internal)?;
        return Ok(Json(json!({ "state": lifecycle, "version_no": d.1 })).into_response());
    }
    let reason = (d.1 > 1).then_some("Published from the designer");
    let change = if d.1 > 1 { Change::Minor } else { Change::Major };
    let v = lib.publish(&mut tx, &who, d.0, None, reason, change, &policy).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "state": v.lifecycle, "version_no": v.version_no })).into_response())
}
