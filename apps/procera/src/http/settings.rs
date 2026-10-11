//! Workspace settings API (BPMS-027, docs/20 §0-2): effective values with the explainer, history and
//! updates, department overrides, and profile switch plan/apply. Platform floors live on the system
//! plane (`/sysapi/v1/settings`). Reads go through the effective-settings cache when a shared cache
//! is configured (CFG-004); every write invalidates it after commit.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_access::{Resource, Scope};
use procera_settings::cache::{self, SettingsCache};
use procera_settings::{Settings, SettingsError};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/branding", get(branding))
        .route("/settings", get(list))
        .route("/settings/effective", get(effective))
        .route("/settings/{key}", get(effective_by_path).put(update))
        .route("/settings/{key}/history", get(history))
        .route("/settings/departments/{id}", get(department_view))
        .route("/settings/departments/{id}/{key}", put(set_override).delete(clear_override))
        .route("/settings/profile/plan", get(profile_plan))
        .route("/settings/profile/apply", post(profile_apply))
}

/// The effective-settings cache, when a shared (Valkey) cache is configured.
pub fn settings_cache(s: &AppState) -> Option<SettingsCache> {
    SettingsCache::shared(&s.cache)
}

/// After a settings change commits: drop cached effective values for the workspace.
pub async fn invalidate(s: &AppState, workspace: Uuid) {
    if let Some(c) = settings_cache(s) {
        c.invalidate_workspace(workspace).await;
    }
}

/// After a platform floor change commits: every workspace's cached values are stale.
pub async fn invalidate_floors(s: &AppState) {
    if let Some(c) = settings_cache(s) {
        c.invalidate_floors().await;
    }
}

fn with_cache_header(mut res: Response, outcome: cache::Outcome) -> Response {
    res.headers_mut().insert("x-settings-cache", HeaderValue::from_static(outcome.as_str()));
    res
}

fn with_etag(mut res: Response, tag: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(&format!("\"{tag}\"")) {
        res.headers_mut().insert(header::ETAG, v);
    }
    res
}

fn if_match(headers: &HeaderMap) -> Option<String> {
    headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()).map(|v| v.trim().trim_matches('"').to_owned())
}

/// The workspace look for every member (docs/05 §7, docs/20 §2.1): the accent with its contrast-safe
/// shades and the default theme. Readable without settings permissions: it is what the UI paints.
async fn branding(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let st = settings(&s)?;
    let accent = st.effective(&mut tx, "branding.accent").await?.value;
    let theme = st.effective(&mut tx, "branding.theme_default").await?.value;
    tx.rollback().await.map_err(internal)?;
    let derived = procera_settings::branding::derive(accent.as_str().unwrap_or("#EE4136"));
    Ok(Json(json!({ "accent": derived.accent, "light": derived.light, "dark": derived.dark, "adjusted": derived.adjusted, "theme_default": theme })).into_response())
}

pub fn settings(s: &AppState) -> Result<&Settings, ApiError> {
    s.platform.as_ref().map(|p| p.settings()).ok_or_else(|| ApiError::unavailable("database"))
}

impl From<SettingsError> for ApiError {
    fn from(e: SettingsError) -> Self {
        match e {
            SettingsError::UnknownKey(k) => Self::new(404, "unknown_setting", "Unknown setting").detail(k),
            SettingsError::Invalid { key, message } => {
                Self::new(422, "validation_failed", "Validation failed").detail(format!("{key}: {message}"))
            }
            SettingsError::BelowFloor { key, floor } => {
                Self::new(422, "setting_below_floor", "Looser than the platform floor")
                    .detail(format!("{key} must be at least as strict as the platform floor {floor}"))
            }
            SettingsError::Locked(k) => {
                Self::new(403, "setting_locked", "This setting is locked by the platform").detail(k)
            }
            SettingsError::ReasonRequired(k) => {
                Self::new(422, "reason_required", "A reason is required for this change").detail(k)
            }
            SettingsError::StaleRevision { current } => {
                Self::new(412, "precondition_failed", "The setting changed; reload and retry")
                    .detail(format!("current revision {current}"))
            }
            SettingsError::UnknownDepartment => Self::new(404, "not_found", "Department not found"),
            SettingsError::NotDepartmentScopable(k) => {
                Self::new(422, "not_department_scopable", "This setting cannot be overridden per department")
                    .detail(k)
            }
            SettingsError::NoOverride(k) => {
                Self::new(404, "no_override", "This department has no override for the setting").detail(k)
            }
            SettingsError::PlanChanged { current } => Self::new(
                412,
                "precondition_failed",
                "The settings changed since the plan was made; plan again",
            )
            .detail(format!("current basis {current}")),
            SettingsError::Internal(m) => {
                tracing::error!(error = %m, "settings internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "settings api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn client(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let c = client_info(headers);
    (c.ip, c.user_agent)
}

async fn list(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let (snap, outcome) = cache::snapshot(settings_cache(&s).as_ref(), &mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(with_cache_header(Json(json!({ "items": snap.effective_all(&[]) })).into_response(), outcome))
}

#[derive(Debug, Deserialize)]
struct KeyQuery {
    key: String,
    /// `department:<id>` resolves for that department (docs/20 §1); omitted = workspace level.
    #[serde(default)]
    scope: Option<String>,
}

/// The explainer: final value and every contributing layer (docs/20 §1).
async fn effective(
    s: State<AppState>,
    me: WsMember,
    Query(q): Query<KeyQuery>,
) -> Result<Response, ApiError> {
    let department = match q.scope.as_deref() {
        None | Some("workspace") => None,
        Some(sc) => {
            Some(sc.strip_prefix("department:").and_then(|id| id.parse::<Uuid>().ok()).ok_or_else(|| {
                ApiError::new(422, "validation_failed", "Validation failed")
                    .detail("scope must be workspace or department:<id>")
            })?)
        }
    };
    match department {
        None => explain(s, me, q.key).await,
        Some(dep) => explain_department(s, me, dep, q.key).await,
    }
}

/// The department-scope resource: the department under its ancestors (nearest first).
fn department_resource(path: &[Uuid]) -> Resource {
    let mut chain = path.iter().rev().map(|id| Scope::Department(*id));
    let scope = chain.next().unwrap_or(Scope::Workspace);
    Resource::Scoped { scope, ancestors: chain.collect() }
}

async fn explain_department(
    State(s): State<AppState>,
    me: WsMember,
    department: Uuid,
    key: String,
) -> Result<Response, ApiError> {
    let d = procera_settings::def(&key)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let path = settings(&s)?.department_path(&mut tx, department).await?;
    me.require(m, &mut tx, "workspace:settings.view", department_resource(&path)).await?;
    let (snap, outcome) = cache::snapshot(settings_cache(&s).as_ref(), &mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    let e = snap.department_entry(d, &path, department);
    let tag = e.override_revision.to_string();
    Ok(with_cache_header(with_etag(Json(&e).into_response(), &tag), outcome))
}

async fn effective_by_path(
    s: State<AppState>,
    me: WsMember,
    Path(key): Path<String>,
) -> Result<Response, ApiError> {
    explain(s, me, key).await
}

async fn explain(State(s): State<AppState>, me: WsMember, key: String) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let e = settings(&s)?.effective(&mut tx, &key).await?;
    tx.rollback().await.map_err(internal)?;
    let mut res = Json(&e).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", e.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

async fn history(
    State(s): State<AppState>,
    me: WsMember,
    Path(key): Path<String>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let items = settings(&s)?.history(&mut tx, &key).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateReq {
    value: Value,
    #[serde(default)]
    reason: Option<String>,
}

async fn update(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(key): Path<String>,
    Json(r): Json<UpdateReq>,
) -> Result<Response, ApiError> {
    let d = procera_settings::def(&key)?;
    let perm = d.perm.ok_or_else(|| SettingsError::Locked(key.clone()))?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, perm, Resource::Workspace).await?;
    if d.step_up {
        me.require_step_up(&s, &mut tx).await?;
    }
    let expected = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok());
    let e = settings(&s)?
        .set_workspace(
            &mut tx,
            (me.user_id, me.actor.subject_ref),
            &key,
            &r.value,
            r.reason.as_deref(),
            expected,
            client(&headers),
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    invalidate(&s, me.ctx.workspace_id).await;
    let mut res = (StatusCode::OK, Json(&e)).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", e.revision)) {
        res.headers_mut().insert(header::ETAG, v);
    }
    Ok(res)
}

/// Department-scopable keys resolved for one department, with its own overrides.
async fn department_view(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let path = settings(&s)?.department_path(&mut tx, id).await?;
    me.require(m, &mut tx, "workspace:settings.view", department_resource(&path)).await?;
    let (snap, outcome) = cache::snapshot(settings_cache(&s).as_ref(), &mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    let items: Vec<_> =
        snap.department_view(&path).into_iter().filter(|e| e.effective.department_scopable).collect();
    Ok(with_cache_header(Json(json!({ "department_id": id, "items": items })).into_response(), outcome))
}

/// Opens the transaction and authorizes the key's permission on the department scope.
async fn override_tx(
    s: &AppState,
    me: &WsMember,
    id: Uuid,
    key: &str,
) -> Result<procera_db::WsTx<'static>, ApiError> {
    let d = procera_settings::def(key)?;
    let perm = d.perm.ok_or_else(|| SettingsError::Locked(key.to_owned()))?;
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    let path = settings(s)?.department_path(&mut tx, id).await?;
    me.require(m, &mut tx, perm, department_resource(&path)).await?;
    Ok(tx)
}

async fn set_override(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, key)): Path<(Uuid, String)>,
    Json(r): Json<UpdateReq>,
) -> Result<Response, ApiError> {
    let mut tx = override_tx(&s, &me, id, &key).await?;
    let expected = if_match(&headers).and_then(|v| v.parse().ok());
    let e = settings(&s)?
        .set_department(
            &mut tx,
            (me.user_id, me.actor.subject_ref),
            id,
            &key,
            &r.value,
            r.reason.as_deref(),
            expected,
            client(&headers),
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    invalidate(&s, me.ctx.workspace_id).await;
    let tag = e.override_revision.to_string();
    Ok(with_etag(Json(&e).into_response(), &tag))
}

async fn clear_override(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, key)): Path<(Uuid, String)>,
) -> Result<Response, ApiError> {
    let mut tx = override_tx(&s, &me, id, &key).await?;
    let expected = if_match(&headers).and_then(|v| v.parse().ok());
    let e = settings(&s)?
        .clear_department(
            &mut tx,
            (me.user_id, me.actor.subject_ref),
            id,
            &key,
            None,
            expected,
            client(&headers),
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    invalidate(&s, me.ctx.workspace_id).await;
    Ok(Json(&e).into_response())
}

#[derive(Debug, Deserialize)]
struct PlanQuery {
    /// Target profile; omitted = the current one (shows what has drifted from it).
    #[serde(default)]
    profile: Option<String>,
}

fn platform(s: &AppState) -> Result<&procera_platform::Platform, ApiError> {
    s.platform.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

/// Preview of a profile switch (CFG-017): every key that would change. The ETag is the plan basis.
async fn profile_plan(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<PlanQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let from = platform(&s)?.workspace_profile(&mut tx).await?;
    let to = q.profile.unwrap_or_else(|| from.clone());
    let plan = settings(&s)?.plan_profile_switch(&mut tx, &from, &to).await?;
    tx.rollback().await.map_err(internal)?;
    let tag = plan.basis.clone();
    Ok(with_etag(Json(&plan).into_response(), &tag))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyReq {
    profile: String,
    reason: String,
}

/// Applies the plan whose basis is in `If-Match` (required): step-up, a reason, and the permission
/// of every key it changes. One audit event (`settings.profile_switched`).
async fn profile_apply(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<ApplyReq>,
) -> Result<Response, ApiError> {
    let basis = if_match(&headers).ok_or_else(|| {
        ApiError::new(428, "precondition_required", "If-Match is required").detail("send the plan's ETag")
    })?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.manage", Resource::Workspace).await?;
    let from = platform(&s)?.workspace_profile(&mut tx).await?;
    let preview = settings(&s)?.plan_profile_switch(&mut tx, &from, &r.profile).await?;
    let mut perms: Vec<&str> = preview.items.iter().filter_map(|i| i.editable_with).collect();
    perms.sort_unstable();
    perms.dedup();
    for p in perms {
        me.require(m, &mut tx, p, Resource::Workspace).await?;
    }
    me.require_step_up(&s, &mut tx).await?;
    let plan = platform(&s)?
        .switch_profile(
            &mut tx,
            (me.user_id, me.actor.subject_ref),
            &r.profile,
            &r.reason,
            Some(&basis),
            client(&headers),
        )
        .await?;
    tx.commit().await.map_err(internal)?;
    invalidate(&s, me.ctx.workspace_id).await;
    Ok(Json(&plan).into_response())
}
