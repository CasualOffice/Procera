//! Authentication API (ADR-0020): JWT access tokens in `Authorization: Bearer`, refresh token in an
//! HttpOnly, SameSite=Strict cookie that only `/api/v1/auth/refresh` and `/logout` read.

use std::time::Duration;

use axum::extract::{FromRequestParts, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_cache::CacheKey;
use procera_identity::jwt::Claims;
use procera_identity::{ClientInfo, Identity, LoginOutcome, Tokens};
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
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/me/sign-out-everywhere", post(sign_out_everywhere))
        .route("/me/mfa/totp", post(totp_begin))
        .route("/me/mfa/totp/confirm", post(totp_confirm))
}

fn identity(s: &AppState) -> Result<&Identity, ApiError> {
    s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

pub fn client_info(headers: &HeaderMap) -> ClientInfo {
    // Behind a trusted proxy the first X-Forwarded-For hop is the client (docs/06 §5).
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_owned())
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
    match identity(&s)?.login(&req.email, &req.password, &client).await? {
        LoginOutcome::Authenticated(t) => Ok(token_response(&s, *t)),
        LoginOutcome::MfaRequired { mfa_token } => Ok(Json(
            json!({"status": "mfa_required", "mfa_token": mfa_token, "methods": ["totp", "recovery_code"]}),
        )
        .into_response()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MfaReq {
    mfa_token: String,
    code: String,
}

async fn mfa(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<MfaReq>,
) -> Result<Response, ApiError> {
    let tokens = identity(&s)?.complete_mfa(&req.mfa_token, &req.code, &client_info(&headers)).await?;
    Ok(token_response(&s, tokens))
}

async fn refresh(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_csrf_header(&headers)?;
    let token = read_cookie(&s, &headers).ok_or_else(ApiError::unauthenticated)?;
    let tokens = identity(&s)?.refresh(&token, &client_info(&headers)).await?;
    Ok(token_response(&s, tokens))
}

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

async fn me(State(s): State<AppState>, AuthUser(c): AuthUser) -> Result<Response, ApiError> {
    let user = identity(&s)?.me(c.sub).await?.ok_or_else(ApiError::unauthenticated)?;
    Ok(Json(json!({"user": user, "token": {"amr": c.amr, "auth_time": c.auth_time, "plane": c.plane, "workspace": c.ws, "expires_at": c.exp}})).into_response())
}

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

async fn totp_confirm(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(req): Json<CodeReq>,
) -> Result<Response, ApiError> {
    let codes = identity(&s)?.totp_confirm(c.sub, &req.code, &client_info(&headers)).await?;
    let mut res = Json(json!({"recovery_codes": codes})).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}
