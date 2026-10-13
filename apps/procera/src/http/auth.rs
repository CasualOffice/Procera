//! Authentication API (ADR-0020): JWT access tokens in `Authorization: Bearer`, refresh token in an
//! HttpOnly, SameSite=Strict cookie that only `/api/v1/auth/refresh` and `/logout` read.

use std::time::Duration;

use axum::extract::Path;
use axum::extract::{FromRequestParts, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use procera_cache::CacheKey;
use procera_identity::jwt::Claims;
use procera_identity::webauthn::CredentialJson;
use procera_identity::{ClientInfo, Factor, Identity, LoginOutcome, Tokens};
use serde::Deserialize;
use serde_json::json;

use super::AppState;
use super::error::ApiError;

/// Header required on cookie-authenticated endpoints. Cross-site forms cannot set custom headers,
/// so together with SameSite=Strict this blocks CSRF on refresh/logout without token plumbing.
pub const CSRF_HEADER: &str = "x-requested-with";
pub const CSRF_VALUE: &str = "procera";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/mfa", post(mfa))
        .route("/auth/mfa/webauthn/options", post(mfa_passkey_options))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/me/sign-out-everywhere", post(sign_out_everywhere))
        .route("/me/mfa/totp", post(totp_begin))
        .route("/me/mfa/totp/confirm", post(totp_confirm))
        .route("/me/mfa/webauthn", get(list_passkeys).post(register_passkey))
        .route("/me/mfa/webauthn/options", post(passkey_registration_options))
        .route("/me/mfa/webauthn/{id}", delete(remove_passkey))
}

fn identity(s: &AppState) -> Result<&Identity, ApiError> {
    s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

/// How many reverse proxies in front of the server append to `X-Forwarded-For`
/// (`PROCERA_TRUSTED_PROXY_HOPS`, default 1). 0 ignores the header.
pub static TRUSTED_PROXY_HOPS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

pub fn client_info(headers: &HeaderMap) -> ClientInfo {
    // Each trusted proxy appends the address it saw, so the client is the entry `hops` from the
    // end; anything before it may be forged by the client (docs/06 §5).
    let hops = *TRUSTED_PROXY_HOPS.get().unwrap_or(&1);
    let ip = (hops > 0)
        .then(|| headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()))
        .flatten()
        .and_then(|v| {
            let hops_seen: Vec<&str> = v.split(',').map(str::trim).collect();
            hops_seen.len().checked_sub(hops).map(|i| hops_seen[i].to_owned())
        })
        .filter(|s| s.parse::<std::net::IpAddr>().is_ok());
    let user_agent =
        headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(300).collect());
    ClientInfo { ip, user_agent }
}

fn cookie_name(s: &AppState) -> &'static str {
    if s.secure_cookies { "__Host-procera_refresh" } else { "procera_refresh" }
}

fn refresh_cookie(s: &AppState, value: &str, max_age: Duration) -> HeaderValue {
    let secure = if s.secure_cookies { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}",
        cookie_name(s),
        max_age.as_secs()
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn read_cookie(s: &AppState, headers: &HeaderMap) -> Option<String> {
    let name = cookie_name(s);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == name).then(|| v.to_owned())
        })
}

fn require_csrf_header(headers: &HeaderMap) -> Result<(), ApiError> {
    match headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok()) {
        Some(v) if v == CSRF_VALUE => Ok(()),
        _ => Err(ApiError::new(403, "csrf", "Missing anti-CSRF header")
            .detail("Send `X-Requested-With: procera` with cookie-authenticated requests")),
    }
}

fn token_response(s: &AppState, tokens: Tokens) -> Response {
    let cookie = refresh_cookie(s, &tokens.refresh_token, Duration::from_secs(30 * 86400));
    let mut res = (StatusCode::OK, Json(json!({"status": "authenticated", "access_token": tokens.access_token, "token_type": tokens.token_type, "expires_in": tokens.expires_in, "user": tokens.user}))).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie);
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginReq {
    email: String,
    password: String,
}

/// `POST /auth/login`: signs in with `email` and `password`. Returns an access token and sets the refresh
/// cookie, or answers `mfa_required` (with `mfa_token`) or `password_change_required`. 429 when rate-limited.
async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<Response, ApiError> {
    let client = client_info(&headers);
    // Coarse per-IP limit in front of the per-account backoff (docs/06 §3.6, ADR-0019).
    let key = CacheKey::system(&format!("rl:login:{}", client.ip.as_deref().unwrap_or("unknown")));
    let d = s.cache.rate_limit(&key, s.login_rate_per_min, Duration::from_secs(60)).await;
    if !d.allowed {
        let mut e = ApiError::new(429, "rate_limited", "Too many requests");
        e.retry_after = Some(i64::try_from(d.reset_after.as_secs()).unwrap_or(60).max(1));
        return Err(e);
    }
    Ok(login_response(&s, identity(&s)?.login(&req.email, &req.password, &client).await?))
}

pub(super) fn login_response(s: &AppState, outcome: LoginOutcome) -> Response {
    match outcome {
        LoginOutcome::Authenticated(t) => token_response(s, *t),
        LoginOutcome::MfaRequired { mfa_token, methods } => no_store(
            Json(json!({"status": "mfa_required", "mfa_token": mfa_token, "methods": methods}))
                .into_response(),
        ),
        LoginOutcome::PasswordChangeRequired { change_token } => no_store(
            Json(json!({"status": "password_change_required", "change_token": change_token})).into_response(),
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MfaReq {
    mfa_token: String,
    /// A TOTP or recovery code, or…
    #[serde(default)]
    code: Option<String>,
    /// …a passkey assertion for a challenge from `/auth/mfa/webauthn/options`.
    #[serde(default)]
    credential: Option<CredentialJson>,
}

/// `POST /auth/mfa`: completes sign-in with the `mfa_token` and exactly one of `code` (TOTP or recovery
/// code) or `credential` (passkey assertion); 422 otherwise. Returns an access token and the refresh cookie.
async fn mfa(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<MfaReq>,
) -> Result<Response, ApiError> {
    let factor = match (&req.code, &req.credential) {
        (Some(code), None) => Factor::Code(code),
        (None, Some(cred)) => Factor::Passkey(cred),
        _ => {
            return Err(ApiError::new(422, "validation_failed", "Validation failed")
                .detail("Send exactly one of `code` or `credential`"));
        }
    };
    let tokens = identity(&s)?.complete_mfa_with(&req.mfa_token, factor, &client_info(&headers)).await?;
    Ok(token_response(&s, tokens))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MfaTokenReq {
    mfa_token: String,
}

/// Passkey challenge for the second sign-in step (after the password).
async fn mfa_passkey_options(
    State(s): State<AppState>,
    Json(req): Json<MfaTokenReq>,
) -> Result<Response, ApiError> {
    Ok(no_store(Json(identity(&s)?.mfa_passkey_options(&req.mfa_token).await?).into_response()))
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// `POST /auth/refresh`: a new access token from the refresh cookie (rotating it). Requires the
/// `X-Requested-With: procera` header (403 without it); 401 without a valid cookie.
async fn refresh(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_csrf_header(&headers)?;
    let token = read_cookie(&s, &headers).ok_or_else(ApiError::unauthenticated)?;
    let tokens = identity(&s)?.refresh(&token, &client_info(&headers)).await?;
    Ok(token_response(&s, tokens))
}

/// `POST /auth/logout`: ends the session behind the refresh cookie and clears it; 204. Requires the
/// `X-Requested-With: procera` header.
async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_csrf_header(&headers)?;
    if let Some(token) = read_cookie(&s, &headers) {
        identity(&s)?.logout(&token, &client_info(&headers)).await?;
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, refresh_cookie(&s, "", Duration::ZERO));
    Ok(res)
}

/// An authenticated caller: a valid access JWT whose user is still active and whose token version
/// is current (checked against the database through a 30 s cache, so revocation bites quickly).
#[derive(Debug, Clone)]
pub struct AuthUser(pub Claims);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Self::Rejection> {
        // An API token, already checked by the token layer (BPMS-144): it acts as its member, whose
        // account must still be active. Step-up is never satisfied (`auth_time` 0).
        if let Some(t) = parts.extensions.get::<super::tokens::TokenAuth>().cloned() {
            let id = identity(s)?;
            let Some((status, tv)) = id.token_state(t.user_id).await? else {
                return Err(ApiError::new(401, "invalid_token", "Invalid, expired or revoked API token"));
            };
            if status != "active" {
                return Err(ApiError::new(401, "invalid_token", "Invalid, expired or revoked API token"));
            }
            return Ok(Self(Claims {
                iss: procera_identity::jwt::ISSUER.to_owned(),
                aud: procera_identity::jwt::AUDIENCE.to_owned(),
                sub: t.user_id,
                sid: t.token_id,
                plane: "workspace".to_owned(),
                ws: Some(t.workspace),
                amr: vec!["token".to_owned()],
                auth_time: 0,
                tv,
                typ: "access".to_owned(),
                iat: 0,
                exp: 0,
                jti: t.token_id,
            }));
        }
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthenticated)?;
        let id = identity(s)?;
        let claims = id
            .verify_access(token.trim())
            .ok_or_else(|| ApiError::new(401, "invalid_token", "Invalid or expired token"))?;
        let key = CacheKey::system(&format!("user-tv:{}", claims.sub));
        let state = match s.cache.get(&key).await.and_then(|b| String::from_utf8(b).ok()) {
            Some(v) => v,
            None => {
                let v = match id.token_state(claims.sub).await? {
                    Some((status, tv)) => format!("{status}:{tv}"),
                    None => "gone:0".to_owned(),
                };
                s.cache.set(&key, v.as_bytes(), Duration::from_secs(30)).await;
                v
            }
        };
        if state != format!("active:{}", claims.tv) {
            return Err(ApiError::new(401, "invalid_token", "Session revoked"));
        }
        Ok(Self(claims))
    }
}

/// `GET /me`: the signed-in user's account and the current token's facts (methods used, auth time, plane,
/// workspace, expiry).
async fn me(State(s): State<AppState>, AuthUser(c): AuthUser) -> Result<Response, ApiError> {
    let user = identity(&s)?.me(c.sub).await?.ok_or_else(ApiError::unauthenticated)?;
    Ok(Json(json!({"user": user, "token": {"amr": c.amr, "auth_time": c.auth_time, "plane": c.plane, "workspace": c.ws, "expires_at": c.exp}})).into_response())
}

/// `POST /me/sign-out-everywhere`: revokes every session of the caller's account and invalidates its
/// outstanding access tokens, then clears the refresh cookie; 204.
async fn sign_out_everywhere(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
) -> Result<Response, ApiError> {
    identity(&s)?.revoke_all(c.sub, "user signed out everywhere", &client_info(&headers)).await?;
    s.cache.delete(&CacheKey::system(&format!("user-tv:{}", c.sub))).await;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, refresh_cookie(&s, "", Duration::ZERO));
    Ok(res)
}

/// `POST /me/mfa/totp`: starts authenticator-app enrolment and returns the secret to scan; confirm it with
/// `POST /me/mfa/totp/confirm`.
async fn totp_begin(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
) -> Result<Response, ApiError> {
    let e = identity(&s)?.totp_begin(c.sub, &client_info(&headers)).await?;
    let mut res = Json(e).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeReq {
    code: String,
}

/// `POST /me/mfa/totp/confirm`: confirms TOTP enrolment with a `code`; returns the recovery codes (shown
/// once) and an upgraded access token for this session.
async fn totp_confirm(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(req): Json<CodeReq>,
) -> Result<Response, ApiError> {
    let id = identity(&s)?;
    let codes = id.totp_confirm(c.sub, &req.code, &client_info(&headers)).await?;
    // The code just proved the authenticator: this session now counts as two-step (no second sign-in).
    let (token, expires_in) = id.upgrade_session(&c, "otp").await?;
    let mut res = Json(json!({"recovery_codes": codes, "access_token": token, "token_type": "Bearer", "expires_in": expires_in})).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// Account-level actions (not tied to a workspace) need an authentication within this window,
/// the platform default of `auth.stepup.window` (docs/12 §11, docs/22 A-16).
pub const ACCOUNT_STEP_UP_WINDOW_SECS: i64 = 300;

pub fn require_recent_auth(s: &AppState, c: &Claims) -> Result<(), ApiError> {
    let age = s.clock.now().unix_timestamp() - c.auth_time;
    if (0..=ACCOUNT_STEP_UP_WINDOW_SECS).contains(&age) {
        Ok(())
    } else {
        Err(ApiError::new(401, "step_up_required", "Confirm it's you to continue").detail(format!(
            "POST /api/v1/session/step-up; this action needs authentication within the last {ACCOUNT_STEP_UP_WINDOW_SECS} seconds"
        )))
    }
}

/// `GET /me/mfa/webauthn`: the caller's registered passkeys.
async fn list_passkeys(State(s): State<AppState>, AuthUser(c): AuthUser) -> Result<Response, ApiError> {
    Ok(Json(json!({"items": identity(&s)?.passkeys(c.sub).await?})).into_response())
}

/// `POST /me/mfa/webauthn/options`: a WebAuthn registration challenge for adding a passkey.
async fn passkey_registration_options(
    State(s): State<AppState>,
    AuthUser(c): AuthUser,
) -> Result<Response, ApiError> {
    Ok(no_store(Json(identity(&s)?.passkey_registration_options(c.sub).await?).into_response()))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterPasskeyReq {
    name: String,
    credential: CredentialJson,
}

/// `POST /me/mfa/webauthn`: registers a passkey (`name`, `credential`) answering the challenge from
/// `/me/mfa/webauthn/options`; 201. Recovery codes are returned once when it is the first second factor.
async fn register_passkey(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(req): Json<RegisterPasskeyReq>,
) -> Result<Response, ApiError> {
    let (passkey, codes) =
        identity(&s)?.passkey_register(c.sub, &req.name, &req.credential, &client_info(&headers)).await?;
    let mut res =
        (StatusCode::CREATED, Json(json!({"passkey": passkey, "recovery_codes": codes}))).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// `DELETE /me/mfa/webauthn/{id}`: removes one of the caller's passkeys; 204. Needs a sign-in within the
/// last 5 minutes (401 `step_up_required`).
async fn remove_passkey(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, ApiError> {
    require_recent_auth(&s, &c)?;
    identity(&s)?.passkey_remove(c.sub, id, &client_info(&headers)).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod client_ip_tests {
    use super::*;

    fn ip(xff: &str) -> Option<String> {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_str(xff).unwrap());
        client_info(&h).ip
    }

    #[test]
    fn the_client_is_the_entry_the_trusted_proxy_appended() {
        // One proxy (the default): a forged first entry is ignored.
        assert_eq!(ip("6.6.6.6, 203.0.113.9").as_deref(), Some("203.0.113.9"));
        assert_eq!(ip("203.0.113.9").as_deref(), Some("203.0.113.9"));
        assert_eq!(ip("not-an-ip"), None);
    }
}
