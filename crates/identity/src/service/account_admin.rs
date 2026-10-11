//! Admin account controls (OQ-15, A-39, docs/06 §3.6.1): send a reset link, require a password
//! change at next sign-in, sign out everywhere and reset two-step verification. The workspace plane
//! checks the permission, step-up and the only-active-workspace rule before calling these; each is
//! audited in the system chain with the admin as the actor and the account as the object.

use procera_audit::Outcome;
use uuid::Uuid;

use super::recovery::{AccountNotice, ResetLink};
use super::{ClientInfo, Identity, IdentityError};

impl Identity {
    /// An admin sends a reset link to `user_id` (`by` = the admin's subject). Not rate-limited by the
    /// self-service budget; the admin plane audits and throttles it.
    pub async fn admin_reset_link(
        &self,
        user_id: Uuid,
        by: Uuid,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<ResetLink, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::NotFound)?;
        if user.status != "active" {
            return Err(IdentityError::Invalid("the account is not active".into()));
        }
        let link = self.new_reset(&mut tx, &user, Some(by), client).await?;
        self.audit_account(
            &mut tx,
            Some(by),
            "password.reset_requested",
            Outcome::Allowed,
            client,
            Some(reason),
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(link)
    }

    /// An admin requires `user_id` to choose a new password at next sign-in.
    pub async fn require_password_change(
        &self,
        user_id: Uuid,
        by: Uuid,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<AccountNotice, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::NotFound)?;
        sqlx::query("UPDATE users SET must_change_password = true WHERE id = $1")
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        self.audit_account(
            &mut tx,
            Some(by),
            "password.change_required",
            Outcome::Allowed,
            client,
            Some(reason),
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(AccountNotice { user_id: user.id, email: user.email, display_name: user.display_name })
    }

    /// An admin resets two-step verification (lost authenticator): removes TOTP, passkeys and
    /// recovery codes. Sessions and the password are untouched. Audited `mfa.reset`.
    pub async fn reset_mfa(
        &self,
        user_id: Uuid,
        by: Uuid,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<AccountNotice, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::NotFound)?;
        let removed = sqlx::query(
            "DELETE FROM user_credentials WHERE user_id = $1 AND kind IN ('totp', 'webauthn', 'recovery_code')",
        )
        .bind(user_id)
        .execute(tx.conn())
        .await?
        .rows_affected();
        let reason = format!("{reason} ({removed} credentials removed)");
        self.audit_account(
            &mut tx,
            Some(by),
            "mfa.reset",
            Outcome::Allowed,
            client,
            Some(&reason),
            user.subject_ref,
        )
        .await?;
        tx.commit().await?;
        Ok(AccountNotice { user_id: user.id, email: user.email, display_name: user.display_name })
    }
}
