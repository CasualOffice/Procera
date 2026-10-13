//! Account recovery and admin account controls (OQ-15, A-39, docs/06 §3.6.1, BPMS-021).
//!
//! Public (signed out): forgot password, check a reset link, reset with it, and set a new password
//! after a sign-in answered `password_change_required`. Signed in: change password. Workspace
//! plane: an admin with `member:security.manage` on the member, after step-up and with a reason,
//! sends a reset link, requires a password change, signs the member out everywhere or resets their
//! two-step verification; the last three only when this is the member's only active workspace.
//! Recovery emails go to the system mail queue after the change commits.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_access::Resource;
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_cache::CacheKey;
use procera_identity::{AccountNotice, ClientInfo, Identity, IdentityError, RESET_TTL, ResetLink};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::AppState;
use super::auth::{AuthUser, client_info};
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/password/forgot", post(forgot))
        .route("/auth/password/reset/check", get(check))
        .route("/auth/password/reset", post(reset))
        .route("/auth/password/expired", post(expired))
        .route("/me/password", post(change))
        .route("/members/{id}/security/reset-link", post(admin_reset_link))
        .route("/members/{id}/security/require-password-change", post(admin_require_change))
        .route("/members/{id}/security/sign-out-everywhere", post(admin_sign_out))
        .route("/members/{id}/security/reset-mfa", post(admin_reset_mfa))
}

fn identity(s: &AppState) -> Result<&Identity, ApiError> {
    s.identity.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// Per-IP budget for the signed-out recovery endpoints, shared with sign-in's (docs/06 §5 item 6).
async fn ip_limit(s: &AppState, client: &ClientInfo, what: &str) -> Result<(), ApiError> {
    let key = CacheKey::system(&format!("rl:{what}:{}", client.ip.as_deref().unwrap_or("unknown")));
    let d = s.cache.rate_limit(&key, s.login_rate_per_min, Duration::from_secs(60)).await;
    if d.allowed {
        Ok(())
    } else {
        let mut e = ApiError::new(429, "rate_limited", "Too many requests");
        e.retry_after = Some(i64::try_from(d.reset_after.as_secs()).unwrap_or(60).max(1));
        Err(e)
    }
}

/// Queues a system email; failures are logged, never shown (the change already happened).
async fn send(s: &AppState, kind: &str, to: &str, ctx: Value, dedupe: String) {
    let Some(n) = s.notify.as_ref() else { return };
    if let Err(e) = n.queue_system_email(kind, to, &ctx, &dedupe).await {
        tracing::error!(error = %e, kind, "system email not queued");
    }
}

async fn send_reset_link(s: &AppState, link: &ResetLink) {
    let url = format!("{}/reset-password?token={}", s.public_url, link.token);
    let ctx = json!({"name": link.display_name, "link": url, "minutes": RESET_TTL.whole_minutes()});
    send(s, "password_reset", &link.email, ctx, format!("password_reset:{}", link.reset_id)).await;
}

async fn send_changed(s: &AppState, n: &AccountNotice, how: &str) {
    let ctx = json!({"name": n.display_name, "how": how});
    send(s, "password_changed", &n.email, ctx, format!("password_changed:{}", Uuid::now_v7())).await;
}

/// Drops the cached token state so revocation applies to the next request, not within 30 s.
async fn forget_token_state(s: &AppState, user: Uuid) {
    s.cache.delete(&CacheKey::system(&format!("user-tv:{user}"))).await;
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForgotReq {
    email: String,
}

/// Always 202 with the same body, whether or not the account exists (no enumeration). The email is
/// queued in the background so the answer takes the same time either way.
async fn forgot(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ForgotReq>,
) -> Result<Response, ApiError> {
    let client = client_info(&headers);
    ip_limit(&s, &client, "password-forgot").await?;
    if r.email.trim().is_empty() || r.email.len() > 320 {
        return Err(
            ApiError::new(422, "validation_failed", "Validation failed").detail("Enter an email address")
        );
    }
    if let Some(link) = identity(&s)?.request_password_reset(&r.email, &client).await? {
        let s = s.clone();
        tokio::spawn(async move { send_reset_link(&s, &link).await });
    }
    Ok(no_store(
        (
            StatusCode::ACCEPTED,
            Json(json!({"status": "accepted", "detail": "If an account uses this address, we sent it a link to reset the password."})),
        )
            .into_response(),
    ))
}

#[derive(Debug, Deserialize)]
struct CheckQuery {
    token: String,
}

/// Whether a reset link still works, so the page can say so before the person types a password.
async fn check(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CheckQuery>,
) -> Result<Response, ApiError> {
    ip_limit(&s, &client_info(&headers), "password-reset").await?;
    let valid = identity(&s)?.check_reset_token(&q.token).await?;
    Ok(no_store(Json(json!({"valid": valid})).into_response()))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetReq {
    token: String,
    password: String,
}

/// `POST /auth/password/reset`: sets a new password from a reset link (`token`, `password`); 204, and the
/// account owner is emailed. An invalid, expired or used link is refused with 400. Rate-limited per IP.
async fn reset(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ResetReq>,
) -> Result<Response, ApiError> {
    let client = client_info(&headers);
    ip_limit(&s, &client, "password-reset").await?;
    let notice = identity(&s)?.reset_password(&r.token, &r.password, &client).await.map_err(|e| match e {
        IdentityError::InvalidToken => {
            ApiError::new(400, "reset_link_invalid", "This reset link is invalid, expired or already used")
                .detail("Ask for a new link from the sign-in page")
        }
        e => e.into(),
    })?;
    forget_token_state(&s, notice.user_id).await;
    send_changed(&s, &notice, "reset").await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpiredReq {
    change_token: String,
    password: String,
}

/// After `password_change_required`: sets the new password and continues the sign-in (two-step
/// verification next when the account has it).
async fn expired(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ExpiredReq>,
) -> Result<Response, ApiError> {
    let client = client_info(&headers);
    ip_limit(&s, &client, "password-expired").await?;
    let (outcome, notice) = identity(&s)?
        .complete_expired_change(&r.change_token, &r.password, &client)
        .await
        .map_err(|e| match e {
            IdentityError::InvalidToken => ApiError::new(
                401,
                "change_token_invalid",
                "This step expired. Sign in again to choose a new password",
            ),
            e => e.into(),
        })?;
    forget_token_state(&s, notice.user_id).await;
    send_changed(&s, &notice, "changed").await;
    Ok(super::auth::login_response(&s, outcome))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeReq {
    current_password: String,
    new_password: String,
}

/// Account › Security: change the password. Other sessions end; this one continues with the
/// returned access token.
async fn change(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(r): Json<ChangeReq>,
) -> Result<Response, ApiError> {
    let client = client_info(&headers);
    let (token, ttl, notice) = identity(&s)?
        .change_password(&c, &r.current_password, &r.new_password, &client)
        .await
        .map_err(|e| match e {
            IdentityError::InvalidCredentials => {
                ApiError::new(401, "wrong_password", "The current password is not correct")
            }
            e => e.into(),
        })?;
    forget_token_state(&s, notice.user_id).await;
    send_changed(&s, &notice, "changed").await;
    Ok(no_store(
        Json(json!({"access_token": token, "token_type": "Bearer", "expires_in": ttl})).into_response(),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonReq {
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdminAction {
    ResetLink,
    RequirePasswordChange,
    SignOutEverywhere,
    ResetMfa,
}

impl AdminAction {
    /// The workspace-chain action and the email notice's `action`.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::ResetLink => ("member.password_reset_link_sent", "reset_link"),
            Self::RequirePasswordChange => ("member.password_change_required", "require_password_change"),
            Self::SignOutEverywhere => ("member.signed_out_everywhere", "sign_out_everywhere"),
            Self::ResetMfa => ("member.mfa_reset", "reset_mfa"),
        }
    }
}

/// The shared admin flow: permission on the member, step-up, a reason, an active membership and
/// (except for a reset link) the only-active-workspace rule; then the account change (system
/// chain), the workspace-chain record and the email.
async fn admin(
    s: AppState,
    headers: HeaderMap,
    me: WsMember,
    id: Uuid,
    action: AdminAction,
    reason: String,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let client = client_info(&headers);
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "member:security.manage", Resource::Member(id)).await?;
    me.require_step_up(&s, &mut tx).await?;
    let reason = reason.trim().to_owned();
    if reason.is_empty() || reason.chars().count() > 500 {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("Give a reason of 1 to 500 characters"));
    }
    let (user_id, status, _name) = m.member_account(&mut tx, id).await?;
    if status != "active" {
        return Err(ApiError::new(409, "member_inactive", "This member is deactivated"));
    }
    if action != AdminAction::ResetLink && m.account_used_elsewhere(user_id, me.ctx.workspace_id).await? {
        return Err(ApiError::new(409, "other_workspaces", "A system administrator must do this").detail(
            "This account is also used outside this workspace (another workspace or the system console). Only a system administrator can change its password state, sessions or two-step verification.",
        ));
    }
    let workspace: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
        .bind(me.ctx.workspace_id)
        .fetch_one(tx.conn())
        .await
        .map_err(internal)?;
    let by = me.actor.subject_ref;
    let id_svc = identity(&s)?;
    let (ws_action, notice_action) = action.names();
    let mut link = None;
    let notice = match action {
        AdminAction::ResetLink => {
            let l = id_svc.admin_reset_link(user_id, by, &reason, &client).await?;
            let n = AccountNotice { user_id, email: l.email.clone(), display_name: l.display_name.clone() };
            link = Some(l);
            n
        }
        AdminAction::RequirePasswordChange => {
            id_svc.require_password_change(user_id, by, &reason, &client).await?
        }
        AdminAction::SignOutEverywhere => {
            id_svc.revoke_all_sessions(user_id, Some(by), &reason, &client).await?
        }
        AdminAction::ResetMfa => id_svc.reset_mfa(user_id, by, &reason, &client).await?,
    };
    let mut ev = AuditEvent::new(Some(by), ActorKind::User, Category::Access, ws_action, Outcome::Allowed)
        .object("membership", id);
    ev.reason = Some(reason.clone());
    ev.diff = Some(json!({"action": notice_action}));
    ev.ip = client.ip.clone();
    ev.user_agent = client.user_agent.clone();
    append(&mut tx, s.clock.as_ref(), &procera_kernel::UuidV7Gen, &ev).await.map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    forget_token_state(&s, user_id).await;
    match link {
        Some(l) => send_reset_link(&s, &l).await,
        None => {
            let ctx = json!({"name": notice.display_name, "workspace": workspace, "action": notice_action});
            send(
                &s,
                "account_security_changed",
                &notice.email,
                ctx,
                format!("account_security_changed:{}", Uuid::now_v7()),
            )
            .await;
        }
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "account security internal error");
    ApiError::new(500, "internal", "Internal error")
}

/// `POST /members/{id}/security/reset-link`: emails the member a password reset link. Needs
/// `member:security.manage` on the member, a recent step-up and a `reason`; 204, 409 if deactivated.
async fn admin_reset_link(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    admin(s, headers, me, id, AdminAction::ResetLink, r.reason).await
}

/// `POST /members/{id}/security/require-password-change`: makes the member choose a new password at next
/// sign-in (`member:security.manage`, step-up, `reason`); 204. 409 if deactivated or used in other workspaces.
async fn admin_require_change(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    admin(s, headers, me, id, AdminAction::RequirePasswordChange, r.reason).await
}

/// `POST /members/{id}/security/sign-out-everywhere`: ends all of the member's sessions
/// (`member:security.manage`, step-up, `reason`); 204. 409 if deactivated or used in other workspaces.
async fn admin_sign_out(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    admin(s, headers, me, id, AdminAction::SignOutEverywhere, r.reason).await
}

/// `POST /members/{id}/security/reset-mfa`: removes the member's two-step verification
/// (`member:security.manage`, step-up, `reason`); 204. 409 if deactivated or used in other workspaces.
async fn admin_reset_mfa(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> Result<Response, ApiError> {
    admin(s, headers, me, id, AdminAction::ResetMfa, r.reason).await
}
