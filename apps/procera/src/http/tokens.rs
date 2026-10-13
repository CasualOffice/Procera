//! Personal API tokens (BPMS-144, docs/06 §3.5, docs/12 §4). A token acts as the member who made it,
//! narrowed to the permissions chosen for it ([`procera_access::token_scope`], applied inside every
//! permission check), in one workspace, until it expires or is revoked. It never decides,
//! acknowledges or signs, never touches the account or the session, and can't manage tokens; step-up
//! is never satisfied. Format `prc_live_<lookup>_<secret>`; only an HMAC of the secret under a
//! server-held key is stored (a root key rotation invalidates every token), plus its last 4
//! characters. Using a token updates its last-used time and address at most once a minute.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use procera_access::token_scope;
use serde::Deserialize;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

const PURPOSE: &str = "api-token";
const PREFIX: &str = "prc_live_";
const MAX_DAYS: i64 = 365;

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/api-tokens", get(mine).post(create))
        .route("/me/api-tokens/{id}", delete(revoke_mine))
        .route("/api-tokens", get(all))
        .route("/api-tokens/{id}", delete(revoke_any))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "api tokens internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn invalid(m: &str) -> ApiError {
    ApiError::new(422, "validation_failed", "Validation failed").detail(m)
}

fn bad_token() -> ApiError {
    ApiError::new(401, "invalid_token", "Invalid, expired or revoked API token")
}

/// Who an API token authenticates, for [`super::auth::AuthUser`] and [`WsMember`].
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub workspace: Uuid,
    pub user_id: Uuid,
    pub token_id: Uuid,
}

/// What a token may never reach, whatever its permissions (`path` is under `/api/v1`).
fn forbidden(method: &Method, path: &str) -> bool {
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    let write = method != Method::GET && method != Method::HEAD;
    match seg.as_slice() {
        ["auth", ..] | ["session", ..] | ["invitations", ..] | ["api-tokens", ..] => true,
        ["me", "api-tokens", ..] => true,
        ["me", _, ..] if write => true,
        ["members", _, "security", ..] => true,
        ["work", _, "decisions" | "delegate"] => true,
        ["announcements", _, "acks"] | ["acknowledgements", _, "complete"] => true,
        ["signing", ..] => true,
        _ => false,
    }
}

/// Splits `prc_live_<lookup>_<secret>`.
fn parse(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix(PREFIX)?;
    let (lookup, secret) = rest.split_once('_')?;
    (lookup.len() == 12 && secret.len() >= 32).then_some((lookup, secret))
}

/// Authenticates `Authorization: Bearer prc_…` requests and runs them under the token's scope.
/// Other requests pass through untouched.
pub async fn layer(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| t.starts_with("prc_"))
        .map(str::to_owned);
    let Some(token) = token else { return next.run(req).await };
    let path = req.uri().path().to_owned();
    let Some(api) = path.strip_prefix("/api/v1") else {
        return ApiError::new(403, "token_not_allowed", "API tokens can't be used here").into_response();
    };
    if forbidden(req.method(), api) {
        return ApiError::new(403, "token_not_allowed", "API tokens can't do this")
            .detail("Deciding, acknowledging, signing, account, session and token changes need a person signed in.")
            .into_response();
    }
    match resolve(&s, &token, req.headers()).await {
        Ok((auth, scope)) => {
            req.extensions_mut().insert(auth);
            token_scope::with(scope, next.run(req)).await
        }
        Err(e) => e.into_response(),
    }
}

async fn resolve(
    s: &AppState,
    token: &str,
    headers: &HeaderMap,
) -> Result<(TokenAuth, token_scope::Scope), ApiError> {
    let (lookup, secret) = parse(token).ok_or_else(bad_token)?;
    let m = members(s)?;
    let keys = s.keys.as_ref().ok_or_else(|| internal("keys are not configured"))?;
    let db = m.db();
    let ws: Option<Uuid> = sqlx::query_scalar("SELECT discovery.api_token_workspace($1)")
        .bind(lookup)
        .fetch_one(db.pool())
        .await
        .map_err(internal)?;
    let ws = ws.ok_or_else(bad_token)?;
    let mut tx = db.begin_ws(procera_db::WsCtx { workspace_id: ws }).await.map_err(internal)?;
    type Row = (
        Uuid,
        Uuid,
        Uuid,
        Vec<u8>,
        Vec<String>,
        OffsetDateTime,
        Option<OffsetDateTime>,
        Option<OffsetDateTime>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT t.id, t.membership_id, m.user_id, t.secret_mac, t.permissions, t.expires_at, t.revoked_at, t.last_used_at
           FROM api_tokens t JOIN memberships m ON m.workspace_id = t.workspace_id AND m.id = t.membership_id
          WHERE t.lookup = $1",
    )
    .bind(lookup)
    .fetch_optional(tx.conn())
    .await
    .map_err(internal)?;
    let (id, membership, user_id, mac, permissions, expires_at, revoked_at, last_used) =
        row.ok_or_else(bad_token)?;
    let expect = keys.mac(PURPOSE, secret.as_bytes()).map_err(internal)?;
    let now = s.clock.now();
    if !procera_crypto::mac_eq(&mac, &expect) || revoked_at.is_some() || expires_at <= now {
        return Err(bad_token());
    }
    if last_used.is_none_or(|t| now - t > Duration::minutes(1)) {
        sqlx::query("UPDATE api_tokens SET last_used_at = $2, last_used_ip = $3 WHERE id = $1")
            .bind(id)
            .bind(now)
            .bind(super::auth::client_info(headers).ip)
            .execute(tx.conn())
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    let scope = token_scope::Scope { membership, permissions: Arc::new(permissions.into_iter().collect()) };
    Ok((TokenAuth { workspace: ws, user_id, token_id: id }, scope))
}

type Listed = (
    Uuid,
    String,
    String,
    Vec<String>,
    OffsetDateTime,
    OffsetDateTime,
    Option<OffsetDateTime>,
    Option<String>,
    Option<OffsetDateTime>,
    Uuid,
    String,
);

const LIST: &str = "SELECT t.id, t.name, t.last4, t.permissions, t.created_at, t.expires_at, t.last_used_at, t.last_used_ip, t.revoked_at, t.membership_id, u.display_name
                      FROM api_tokens t JOIN memberships m ON m.workspace_id = t.workspace_id AND m.id = t.membership_id JOIN users u ON u.id = m.user_id";

fn listed(r: &Listed, now: OffsetDateTime) -> Value {
    let state = if r.8.is_some() {
        "revoked"
    } else if r.5 <= now {
        "expired"
    } else {
        "active"
    };
    let iso =
        |t: OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    json!({
        "id": r.0, "name": r.1, "last4": r.2, "permissions": r.3, "created_at": iso(r.4), "expires_at": iso(r.5),
        "last_used_at": r.6.map(iso), "last_used_ip": r.7, "revoked_at": r.8.map(iso), "state": state,
        "owner": { "id": r.9, "name": r.10 },
    })
}

/// `GET /me/api-tokens`: my tokens in this workspace, newest first.
async fn mine(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let mut tx = me.tx(members(&s)?).await?;
    let rows: Vec<Listed> =
        sqlx::query_as(&format!("{LIST} WHERE t.membership_id = $1 ORDER BY t.created_at DESC"))
            .bind(me.actor.membership)
            .fetch_all(tx.conn())
            .await
            .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let now = s.clock.now();
    Ok(Json(json!({ "items": rows.iter().map(|r| listed(r, now)).collect::<Vec<_>>() })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    name: String,
    permissions: Vec<String>,
    #[serde(default = "ninety")]
    expires_in_days: i64,
}

fn ninety() -> i64 {
    90
}

fn lookup_id() -> String {
    const ABC: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    procera_crypto::random_bytes::<12>().iter().map(|b| ABC[usize::from(*b) % ABC.len()] as char).collect()
}

/// `POST /me/api-tokens`: a new token with some of my own permissions; the secret is shown once.
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CreateReq>,
) -> Result<Response, ApiError> {
    let name = r.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(invalid("Give the token a name of up to 100 characters."));
    }
    if !(1..=MAX_DAYS).contains(&r.expires_in_days) {
        return Err(invalid("A token lasts from 1 to 365 days."));
    }
    let wanted: BTreeSet<String> = r.permissions.iter().map(|p| p.trim().to_owned()).collect();
    if wanted.is_empty() {
        return Err(invalid("Choose at least one permission."));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require_step_up(&s, &mut tx).await?;
    let held = m.access().effective_workspace_permissions(&mut tx, me.actor.membership).await?;
    if let Some(p) = wanted.iter().find(|p| !procera_access::catalogue::PERMISSIONS.contains(&p.as_str())) {
        return Err(invalid(&format!("“{p}” isn't a permission.")));
    }
    if let Some(p) = wanted.iter().find(|p| !held.contains(*p)) {
        return Err(
            ApiError::new(403, "forbidden", "A token can only have permissions you hold").detail(p.clone())
        );
    }
    let keys = s.keys.as_ref().ok_or_else(|| internal("keys are not configured"))?;
    let lookup = lookup_id();
    let secret = procera_crypto::random_token().replace(['-', '_'], "x");
    let mac = keys.mac(PURPOSE, secret.as_bytes()).map_err(internal)?;
    let id = Uuid::now_v7();
    let now = s.clock.now();
    let expires_at = now + Duration::days(r.expires_in_days);
    let last4: String = secret.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let permissions: Vec<String> = wanted.into_iter().collect();
    sqlx::query(
        "INSERT INTO api_tokens (workspace_id, id, lookup, membership_id, name, secret_mac, last4, permissions, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(me.ctx.workspace_id)
    .bind(id)
    .bind(&lookup)
    .bind(me.actor.membership)
    .bind(name)
    .bind(mac.as_slice())
    .bind(&last4)
    .bind(&permissions)
    .bind(now)
    .bind(expires_at)
    .execute(tx.conn())
    .await
    .map_err(internal)?;
    m.audit_object(
        &mut tx,
        Some(me.actor.subject_ref),
        "api_token.created",
        "api_token",
        id,
        json!({ "name": name, "permissions": permissions, "expires_at": expires_at, "last4": last4 }),
        &super::auth::client_info(&headers),
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    let token = format!("{PREFIX}{lookup}_{secret}");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "token": token, "name": name, "permissions": permissions, "expires_at": expires_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(), "last4": last4 })),
    )
        .into_response())
}

async fn revoke(
    s: &AppState,
    headers: &HeaderMap,
    me: &WsMember,
    id: Uuid,
    only_mine: bool,
) -> Result<Response, ApiError> {
    let m = members(s)?;
    let mut tx = me.tx(m).await?;
    if !only_mine {
        me.require(m, &mut tx, "api_token:manage", procera_access::Resource::Workspace).await?;
    }
    let owner: Option<(Uuid, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT membership_id, revoked_at FROM api_tokens WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
    let Some((owner, revoked)) = owner.filter(|(o, _)| !only_mine || *o == me.actor.membership) else {
        return Err(ApiError::not_found());
    };
    if revoked.is_none() {
        sqlx::query("UPDATE api_tokens SET revoked_at = $2, revoked_by = $3 WHERE id = $1")
            .bind(id)
            .bind(s.clock.now())
            .bind(me.actor.membership)
            .execute(tx.conn())
            .await
            .map_err(internal)?;
        m.audit_object(
            &mut tx,
            Some(me.actor.subject_ref),
            "api_token.revoked",
            "api_token",
            id,
            json!({ "owner": owner }),
            &super::auth::client_info(headers),
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /me/api-tokens/{id}`: revoke one of my tokens (repeating it is a no-op).
async fn revoke_mine(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    revoke(&s, &headers, &me, id, true).await
}

/// `GET /api-tokens`: every token in the workspace (`api_token:manage`).
async fn all(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "api_token:manage", procera_access::Resource::Workspace).await?;
    let rows: Vec<Listed> = sqlx::query_as(&format!("{LIST} ORDER BY t.created_at DESC LIMIT 500"))
        .fetch_all(tx.conn())
        .await
        .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let now = s.clock.now();
    Ok(Json(json!({ "items": rows.iter().map(|r| listed(r, now)).collect::<Vec<_>>() })).into_response())
}

/// `DELETE /api-tokens/{id}`: revoke anyone's token (`api_token:manage`).
async fn revoke_any(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    revoke(&s, &headers, &me, id, false).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_never_reach_ceremonies_or_the_account() {
        let post = Method::POST;
        let get = Method::GET;
        for p in [
            "/auth/login",
            "/session/step-up",
            "/me/password",
            "/me/api-tokens",
            "/api-tokens",
            "/work/abc/decisions",
            "/work/abc/delegate",
            "/announcements/abc/acks",
            "/acknowledgements/abc/complete",
            "/signing/abc/sign",
            "/members/abc/security/reset-mfa",
            "/invitations/accept",
        ] {
            assert!(forbidden(&post, p), "{p}");
        }
        assert!(forbidden(&get, "/me/api-tokens"));
        for (m, p) in [
            (&get, "/me"),
            (&get, "/me/permissions"),
            (&get, "/requests"),
            (&post, "/requests"),
            (&post, "/work/abc/reassign"),
        ] {
            assert!(!forbidden(m, p), "{p}");
        }
    }

    #[test]
    fn token_format() {
        let secret = "x".repeat(43);
        let token = format!("prc_live_abcdefghijkl_{secret}");
        assert_eq!(parse(&token), Some(("abcdefghijkl", secret.as_str())));
        assert!(parse("prc_live_short_secret").is_none());
        assert!(parse("eyJhbGciOi").is_none());
    }
}
