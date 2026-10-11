//! Account recovery and admin account controls (OQ-15, A-39, docs/06 §3.6.1).
//!
//! - Forgot password: a reset link (32 random bytes, stored as SHA-256, single use, 30 minutes; a
//!   newer request voids older ones), rate-limited to 3 self-service requests per account per hour.
//!   The request always succeeds, so it never tells whether an account exists.
//! - Reset with the link, change while signed in, and a forced change at sign-in (an admin's
//!   "require a password change"): the password policy applies; the account's second factors stay.
//! - Admin controls: send a reset link, require a password change, sign out everywhere and reset
//!   two-step verification. The caller (workspace plane) checks the permission, step-up and the
//!   only-active-workspace rule; this module changes the account and audits it in the system chain.
//!
//! Emails are not sent here: the caller queues them in the system mail queue after the change
//! commits, from what these functions return.

use std::time::Duration;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{hash_token, random_token};
use procera_db::WsTx;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ClientInfo, Identity, IdentityError, LoginOutcome};
use crate::jwt::Claims;
use crate::password;

/// How long a reset link works.
pub const RESET_TTL: time::Duration = time::Duration::minutes(30);
/// Self-service reset requests per account per hour (more are accepted but send nothing).
pub const RESETS_PER_HOUR: i64 = 3;
/// How long the change token from a `password_change_required` sign-in works.
pub const CHANGE_TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

/// A reset link to email: the plaintext token exists only here and in the email.
#[derive(Debug, Clone)]
pub struct ResetLink {
    pub reset_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub token: String,
    pub expires_at: OffsetDateTime,
}

/// Who an account change notice goes to.
#[derive(Debug, Clone)]
pub struct AccountNotice {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
}

impl Identity {
    /// A system-chain event about account `target` (pseudonymous subject) by `actor`.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn audit_account(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        action: &str,
        result: Outcome,
        client: &ClientInfo,
        reason: Option<&str>,
        target: Uuid,
    ) -> Result<(), IdentityError> {
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Auth,
            action,
            result,
        )
        .object("user", target);
        ev.ip = client.ip.clone();
        ev.user_agent = client.user_agent.clone();
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// Creates a reset link for `user` and voids the older ones.
    pub(super) async fn new_reset(
        &self,
        tx: &mut WsTx<'_>,
        user: &super::User,
        requested_by: Option<Uuid>,
        client: &ClientInfo,
    ) -> Result<ResetLink, IdentityError> {
        let now = self.now();
        sqlx::query(
            "UPDATE password_resets SET used_at = $2, voided = true WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(user.id)
        .bind(now)
        .execute(tx.conn())
        .await?;
        let token = random_token();
        let id = self.ids.new_id();
        let expires_at = now + RESET_TTL;
        sqlx::query(
            "INSERT INTO password_resets (id, user_id, token_hash, created_at, expires_at, requested_by, requester_ip)
             VALUES ($1, $2, $3, $4, $5, $6, $7::inet)",
        )
        .bind(id)
        .bind(user.id)
        .bind(hash_token(&token))
        .bind(now)
        .bind(expires_at)
        .bind(requested_by)
        .bind(&client.ip)
        .execute(tx.conn())
        .await?;
        Ok(ResetLink {
            reset_id: id,
            user_id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            token,
            expires_at,
        })
    }

    /// "Forgot password?": always `Ok`. Returns the link to email for an active account within the
    /// rate limit, and `None` otherwise (unknown, inactive or rate-limited: the caller answers the
    /// same either way). Every request is audited `password.reset_requested`.
    pub async fn request_password_reset(
        &self,
        email: &str,
        client: &ClientInfo,
    ) -> Result<Option<ResetLink>, IdentityError> {
        let email = email.trim().to_lowercase();
        let mut tx = self.db.begin_system().await?;
        let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(tx.conn())
            .await?;
        let user = match id {
            Some(id) => self.load_user(&mut tx, id).await?,
            None => None,
        };
        let Some(user) = user.filter(|u| u.status == "active") else {
            let _ = hash_token(&random_token()); // keep the work similar to the found path
            self.audit(
                &mut tx,
                None,
                "password.reset_requested",
                Outcome::Denied,
                client,
                Some("unknown or inactive account"),
            )
            .await?;
            tx.commit().await?;
            return Ok(None);
        };
        let recent: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM password_resets WHERE user_id = $1 AND requested_by IS NULL AND created_at > $2",
        )
        .bind(user.id)
        .bind(self.now() - time::Duration::hours(1))
        .fetch_one(tx.conn())
        .await?;
        if recent >= RESETS_PER_HOUR {
            self.audit_account(
                &mut tx,
                None,
                "password.reset_requested",
                Outcome::Denied,
                client,
                Some("rate limited"),
                user.subject_ref,
            )
            .await?;
            tx.commit().await?;
            return Ok(None);
        }
        let link = self.new_reset(&mut tx, &user, None, client).await?;
        self.audit_account(
            &mut tx,
            None,
            "password.reset_requested",
            Outcome::Allowed,
            client,
            None,
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(link))
    }

    pub async fn check_reset_token(&self, token: &str) -> Result<bool, IdentityError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM password_resets r JOIN users u ON u.id = r.user_id
              WHERE r.token_hash = $1 AND r.used_at IS NULL AND r.expires_at > $2 AND u.status = 'active')",
        )
        .bind(hash_token(token.trim()))
        .bind(self.now())
        .fetch_one(self.db.pool())
        .await?)
    }

    /// Replaces the password credential, clears a required change, revokes every session and
    /// invalidates outstanding access tokens. Second factors are untouched.
    async fn replace_password(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
        new: &str,
        keep_family: Option<Uuid>,
    ) -> Result<(), IdentityError> {
        let phc = password::hash(new, self.cfg.cost).map_err(|e| IdentityError::Internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO user_credentials (id, user_id, kind, secret) VALUES ($1, $2, 'password', $3)
             ON CONFLICT (user_id) WHERE kind = 'password' DO UPDATE SET secret = EXCLUDED.secret, created_at = now()",
        )
        .bind(self.ids.new_id())
        .bind(user_id)
        .bind(phc.as_bytes())
        .execute(tx.conn())
        .await?;
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = coalesce(revoked_at, $2)
             WHERE user_id = $1 AND ($3::uuid IS NULL OR family_id <> $3)",
        )
        .bind(user_id)
        .bind(self.now())
        .bind(keep_family)
        .execute(tx.conn())
        .await?;
        sqlx::query(
            "UPDATE users SET token_version = token_version + 1, must_change_password = false WHERE id = $1",
        )
        .bind(user_id)
        .execute(tx.conn())
        .await?;
        Ok(())
    }

    /// Sets a new password with a reset link. Revokes every session; audited `password.reset`.
    pub async fn reset_password(
        &self,
        token: &str,
        new: &str,
        client: &ClientInfo,
    ) -> Result<AccountNotice, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT id, user_id FROM password_resets WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2 FOR UPDATE",
        )
        .bind(hash_token(token.trim()))
        .bind(self.now())
        .fetch_optional(tx.conn())
        .await?;
        let Some((reset, user_id)) = row else { return Err(IdentityError::InvalidToken) };
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" {
            return Err(IdentityError::InvalidToken);
        }
        // A rejected password leaves the link usable, so the person can try another.
        password::check_policy(new, &user.email, self.cfg.min_password_len)?;
        sqlx::query("UPDATE password_resets SET used_at = $2 WHERE id = $1")
            .bind(reset)
            .bind(self.now())
            .execute(tx.conn())
            .await?;
        self.replace_password(&mut tx, user.id, new, None).await?;
        self.clear_failures(&mut tx, &[format!("email:{}", user.email)]).await?;
        self.audit_account(
            &mut tx,
            Some(user.subject_ref),
            "password.reset",
            Outcome::Allowed,
            client,
            Some("reset link; all sessions revoked"),
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(AccountNotice { user_id: user.id, email: user.email, display_name: user.display_name })
    }

    /// Changes the password of the signed-in user. A wrong current password counts toward the
    /// sign-in lockout. Other sessions are revoked; the current one continues with the returned
    /// access token (the old one dies with the token-version bump). Audited `password.changed`.
    pub async fn change_password(
        &self,
        claims: &Claims,
        current: &str,
        new: &str,
        client: &ClientInfo,
    ) -> Result<(String, u64, AccountNotice), IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" || user.token_version != claims.tv {
            return Err(IdentityError::InvalidToken);
        }
        let keys = Self::throttle_keys(&user.email, client);
        if let Some(retry_after_secs) = self.throttled(&mut tx, &keys).await? {
            return Err(IdentityError::Throttled { retry_after_secs });
        }
        let phc: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT secret FROM user_credentials WHERE user_id = $1 AND kind = 'password'",
        )
        .bind(user.id)
        .fetch_optional(tx.conn())
        .await?;
        let ok = match phc {
            Some(phc) => {
                password::verify(current, &String::from_utf8(phc).unwrap_or_default(), self.cfg.cost).0
            }
            None => {
                password::dummy_verify(current, self.cfg.cost);
                false
            }
        };
        if !ok {
            self.audit_account(
                &mut tx,
                Some(user.subject_ref),
                "password.change_failed",
                Outcome::Denied,
                client,
                Some("wrong current password"),
                user.subject_ref,
            )
            .await?;
            tx.commit().await?;
            self.record_failure(&keys).await?;
            return Err(IdentityError::InvalidCredentials);
        }
        password::check_policy(new, &user.email, self.cfg.min_password_len)?;
        self.replace_password(&mut tx, user.id, new, Some(claims.sid)).await?;
        self.clear_failures(&mut tx, &keys).await?;
        self.audit_account(
            &mut tx,
            Some(user.subject_ref),
            "password.changed",
            Outcome::Allowed,
            client,
            Some("signed in; other sessions revoked"),
            user.subject_ref,
        )
        .await?;
        let user = self.load_user(&mut tx, user.id).await?.ok_or(IdentityError::InvalidToken)?;
        tx.commit().await?;
        let amr: Vec<&str> = claims.amr.iter().map(String::as_str).collect();
        let auth_time = OffsetDateTime::from_unix_timestamp(claims.auth_time).unwrap_or_else(|_| self.now());
        let token = self.sign(
            &user,
            claims.sid,
            &claims.plane,
            claims.ws,
            &amr,
            auth_time,
            "access",
            self.cfg.access_ttl,
        )?;
        Ok((
            token,
            self.cfg.access_ttl.as_secs(),
            AccountNotice { user_id: user.id, email: user.email, display_name: user.display_name },
        ))
    }

    /// The short-lived, single-use token a `password_change_required` sign-in returns. It is bound
    /// to the token version, which the change bumps, so it works once.
    pub(super) fn change_token(&self, user: &super::User) -> Result<String, IdentityError> {
        self.sign(user, Uuid::nil(), "workspace", None, &["pwd"], self.now(), "pwchange", CHANGE_TOKEN_TTL)
    }

    /// Whether the account must choose a new password before signing in.
    pub(super) async fn must_change_password(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
    ) -> Result<bool, IdentityError> {
        Ok(sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(tx.conn())
            .await?
            .unwrap_or(false))
    }

    /// Completes a sign-in that returned `password_change_required`: sets the new password
    /// (revoking every session) and continues as a password sign-in would, so two-step verification
    /// is asked for as usual. Audited `password.changed` plus the sign-in event.
    pub async fn complete_expired_change(
        &self,
        change_token: &str,
        new: &str,
        client: &ClientInfo,
    ) -> Result<(LoginOutcome, AccountNotice), IdentityError> {
        let claims = self.jwt.verify(change_token, "pwchange").ok_or(IdentityError::InvalidToken)?;
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active"
            || user.token_version != claims.tv
            || !self.must_change_password(&mut tx, user.id).await?
        {
            return Err(IdentityError::InvalidToken);
        }
        password::check_policy(new, &user.email, self.cfg.min_password_len)?;
        self.replace_password(&mut tx, user.id, new, None).await?;
        self.audit_account(
            &mut tx,
            Some(user.subject_ref),
            "password.changed",
            Outcome::Allowed,
            client,
            Some("required change at sign-in"),
            user.subject_ref,
        )
        .await?;
        let user = self.load_user(&mut tx, user.id).await?.ok_or(IdentityError::InvalidToken)?;
        let notice = AccountNotice {
            user_id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
        };
        let outcome = self.after_password(&mut tx, &user, client).await?;
        tx.commit().await?;
        Ok((outcome, notice))
    }

    /// Revokes every refresh family of `user_id` and invalidates outstanding access tokens. `by` is
    /// the acting admin (`None` = the person themselves). Audited `sessions.revoked_all`.
    pub async fn revoke_all_sessions(
        &self,
        user_id: Uuid,
        by: Option<Uuid>,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<AccountNotice, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::NotFound)?;
        sqlx::query("UPDATE refresh_tokens SET revoked_at = coalesce(revoked_at, $2) WHERE user_id = $1")
            .bind(user_id)
            .bind(self.now())
            .execute(tx.conn())
            .await?;
        sqlx::query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        let actor = by.unwrap_or(user.subject_ref);
        self.audit_account(
            &mut tx,
            Some(actor),
            "sessions.revoked_all",
            Outcome::Allowed,
            client,
            Some(reason),
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(AccountNotice { user_id: user.id, email: user.email, display_name: user.display_name })
    }
}
