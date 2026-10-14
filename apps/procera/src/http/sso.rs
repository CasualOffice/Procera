//! Single sign-on (BPMS-140, docs/06 §3.3): finding the workspaces whose provider vouches for an
//! email, starting a sign-in (redirect to the provider) and finishing it (the provider redirects
//! back; a session opens in that workspace and the browser returns to the app), and the workspace's
//! provider settings for admins (`workspace:settings.manage`, step-up to change).

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_sso::{ConfigInput, SsoError};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/discover", post(discover))
        .route("/auth/sso/start", get(start))
        .route("/auth/sso/callback", get(callback))
        .route("/sso", get(config).put(save).delete(remove))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "sso api internal error");
    ApiError::new(500, "internal", "Internal error")
}

impl From<SsoError> for ApiError {
    fn from(e: SsoError) -> Self {
        match e {
            SsoError::NotFound => Self::not_found(),
            SsoError::Invalid(m) if m == "stale" => {
                Self::new(412, "precondition_failed", "It changed since you loaded it")
                    .detail("Reload and try again.")
            }
            SsoError::Invalid(m) | SsoError::Refused(m) => {
                Self::new(422, "validation_failed", "Validation failed").detail(m)
            }
            SsoError::Internal(m) => internal(m),
        }
    }
}

fn sso(s: &AppState) -> Result<&procera_sso::Sso, ApiError> {
    s.sso.as_ref().ok_or_else(|| ApiError::unavailable("single sign-on"))
}

fn redirect_uri(s: &AppState) -> String {
    format!("{}/api/v1/auth/sso/callback", s.public_url)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoverReq {
    email: String,
}

/// `POST /auth/sso/discover`: the workspaces whose identity provider vouches for this email's
/// domain (`{items: [{id, slug, name}]}`); empty when none do. No session needed.
async fn discover(State(s): State<AppState>, Json(r): Json<DiscoverReq>) -> Result<Response, ApiError> {
    let items = sso(&s)?.workspaces_for(&r.email).await?;
    Ok(Json(json!({ "items": items.into_iter().map(|(id, slug, name)| json!({ "id": id, "slug": slug, "name": name })).collect::<Vec<_>>() })).into_response())
}

#[derive(Debug, Deserialize)]
struct StartQuery {
    workspace: Uuid,
    #[serde(default)]
    return_to: Option<String>,
}

/// `GET /auth/sso/start?workspace=&return_to=`: redirects (303) to the workspace's identity provider
/// with a one-time state, a nonce and a PKCE challenge. 404 when the workspace has no provider.
async fn start(State(s): State<AppState>, Query(q): Query<StartQuery>) -> Result<Response, ApiError> {
    let url = sso(&s)?.start(q.workspace, &redirect_uri(&s), q.return_to.as_deref()).await?;
    Ok(Redirect::to(&url).into_response())
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn back_to_sign_in(message: &str) -> Response {
    let q: String = message
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    Redirect::to(&format!("/sign-in?sso_error={q}")).into_response()
}

/// `GET /auth/sso/callback?code=&state=`: the provider's redirect. Verifies the sign-in, opens a
/// session in the workspace (refresh cookie) and redirects (303) into the app; any problem redirects
/// to the sign-in page with `sso_error` (refusals are logged). Audited `login.sso_succeeded`.
async fn callback(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    let client = super::auth::client_info(&headers);
    if let Some(e) = q.error {
        let why = q.error_description.unwrap_or(e);
        return Ok(back_to_sign_in(&format!("Your identity provider stopped the sign-in: {why}")));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return Ok(back_to_sign_in("The sign-in didn't come back complete. Start again."));
    };
    let verified = match sso(&s)?.callback(&state, &code, &redirect_uri(&s)).await {
        Ok(v) => v,
        Err(SsoError::Refused(m) | SsoError::Invalid(m)) => {
            tracing::info!(reason = %m, "sso sign-in refused");
            return Ok(back_to_sign_in(&m));
        }
        Err(e) => return Err(e.into()),
    };
    let mfa = verified
        .amr
        .iter()
        .any(|a| matches!(a.as_str(), "mfa" | "otp" | "hwk" | "swk" | "fpt" | "face" | "sms"));
    let tokens =
        super::auth::identity(&s)?.sso_login(verified.user_id, verified.workspace, mfa, &client).await?;
    let mut res = Redirect::to(verified.return_to.as_deref().unwrap_or("/")).into_response();
    let cookie =
        super::auth::refresh_cookie(&s, &tokens.refresh_token, std::time::Duration::from_secs(30 * 86400));
    res.headers_mut().insert(header::SET_COOKIE, cookie);
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// `GET /sso`: the workspace's provider settings (never the secret: `has_secret`), or `null`
/// (`workspace:settings.view`).
async fn config(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let c = sso(&s)?.config(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "config": c, "callback_url": redirect_uri(&s) })).into_response())
}

/// `PUT /sso`: sets the provider (`issuer`, `client_id`, `client_secret` (absent keeps it, empty
/// clears it), `domains`, `enabled`, `required`); the issuer's discovery document must answer first
/// (`workspace:settings.manage`, step-up; honours `If-Match`).
async fn save(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<ConfigInput>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.manage", Resource::Workspace).await?;
    me.require_step_up(&s, &mut tx).await?;
    let rev = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').parse().ok());
    let c = sso(&s)?.save(&mut tx, me.actor.membership, rev, &r).await?;
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "sso.configured",
        "sso_config",
        me.ctx.workspace_id,
        json!({ "issuer": c.issuer, "client_id": c.client_id, "domains": c.domains, "enabled": c.enabled, "required": c.required, "secret_changed": r.client_secret.is_some() }),
        &super::auth::client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    s.cache.delete(&procera_cache::CacheKey::system(&format!("sso-required:{}", me.ctx.workspace_id))).await;
    Ok(Json(json!({ "config": c, "callback_url": redirect_uri(&s) })).into_response())
}

/// `DELETE /sso`: removes the provider; everyone signs in with passwords again
/// (`workspace:settings.manage`, step-up). 204.
async fn remove(State(s): State<AppState>, headers: HeaderMap, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.manage", Resource::Workspace).await?;
    me.require_step_up(&s, &mut tx).await?;
    sso(&s)?.remove(&mut tx).await?;
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "sso.removed",
        "sso_config",
        me.ctx.workspace_id,
        json!({}),
        &super::auth::client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    s.cache.delete(&procera_cache::CacheKey::system(&format!("sso-required:{}", me.ctx.workspace_id))).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}
