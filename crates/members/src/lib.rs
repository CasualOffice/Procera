//! Memberships and invitations (BPMS-023).
//!
//! - Invitations carry role keys; the inviter must hold every permission of those roles (no escalation).
//! - Tokens are 256-bit, stored hashed (lookup) and sealed (for the mailer, BPMS-048); 7-day expiry.
//! - Acceptance requires the invitation email to match the signed-in user (or registers a new user
//!   for that email). Accepting an owner invite activates a `provisioning` workspace.
//! - Every change is audited in the workspace chain and emitted to the outbox.

pub mod audience;
pub mod directory;

use std::sync::Arc;

use procera_access::{Access, AccessError, Scope};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{KeyProvider, hash_token, open, random_token, seal};
use procera_db::outbox::{self, OutboxEvent};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_identity::{ClientInfo, Identity, IdentityError, User};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

const INVITE_TTL_DAYS: i64 = 7;

/// The public face of a pending invitation, for the invitation page.
#[derive(Debug, Clone, Serialize)]
pub struct InvitationPreview {
    pub email: String,
    pub workspace: String,
    pub inviter: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub account_exists: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MembersError {
    #[error("invitation invalid, expired, revoked or already used")]
    InvitationInvalid,
    #[error("this invitation was sent to a different email address")]
    EmailMismatch,
    #[error("already a member of this workspace")]
    AlreadyMember,
    #[error("membership not found")]
    NotFound,
    #[error("the last workspace owner cannot be removed")]
    LastOwner,
    #[error("you cannot deactivate yourself")]
    SelfDeactivation,
    #[error(transparent)]
    Access(#[from] AccessError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Cycle(String),
    #[error("{0}")]
    Conflict(String),
    /// The workspace's member quota is reached (FR-S04 hard limit).
    #[error("member quota reached ({used} of {limit})")]
    QuotaExceeded { limit: i64, used: i64 },
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for MembersError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for MembersError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for MembersError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_crypto::CryptoError> for MembersError {
    fn from(e: procera_crypto::CryptoError) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Member {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub status: String,
    pub title: Option<String>,
    pub roles: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
    /// Their department (directory field), for lists that group people.
    pub department_id: Option<Uuid>,
    /// Out of office today (BPMS-086): last day away and who covers, so colleagues see it on people.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub away: Option<Away>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Away {
    /// Last day away (inclusive), `YYYY-MM-DD`.
    pub until: String,
    pub delegate: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Invitation {
    pub id: Uuid,
    pub email: String,
    pub role_keys: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserWorkspace {
    pub workspace_id: Uuid,
    pub slug: String,
    pub name: String,
    pub state: String,
}

/// Who performs a workspace action.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
}

#[derive(Debug, Clone)]
pub struct Members {
    db: Db,
    identity: Identity,
    access: Access,
    keys: Arc<dyn KeyProvider>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl Members {
    pub fn new(
        db: Db,
        identity: Identity,
        access: Access,
        keys: Arc<dyn KeyProvider>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
    ) -> Self {
        Self { db, identity, access, keys, clock, ids }
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        action: &str,
        object: Uuid,
        diff: serde_json::Value,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        self.audit_object(tx, actor, action, "membership", object, diff, client).await
    }

    /// Appends an access-category audit event in the caller's transaction. Used for role and grant
    /// changes made through [`Members::access`], so every access mutation is on the chain.
    #[allow(clippy::too_many_arguments)]
    pub async fn audit_object(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        action: &str,
        object_type: &str,
        object: Uuid,
        diff: serde_json::Value,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Access,
            action,
            Outcome::Allowed,
        )
        .object(object_type, object);
        ev.diff = Some(diff);
        ev.ip = client.ip.clone();
        ev.user_agent = client.user_agent.clone();
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// Creates an invitation and returns it with its one-time token (to be delivered by email).
    /// FR-S04 hard limit on members, set by the system admin (`workspace_quotas`, NULL = unlimited).
    /// Seats are active members plus open invitations, so accepting an invitation never overshoots;
    /// re-inviting the same address replaces its open invitation and needs no extra seat. The quota
    /// row is locked so concurrent invitations cannot both take the last seat.
    async fn check_member_quota(
        &self,
        tx: &mut WsTx<'_>,
        email: &str,
        now: time::OffsetDateTime,
    ) -> Result<(), MembersError> {
        let limit: Option<Option<i32>> = sqlx::query_scalar(
            "SELECT max_members FROM workspace_quotas WHERE target_workspace_id = $1 FOR UPDATE",
        )
        .bind(tx.ctx.workspace_id)
        .fetch_optional(tx.conn())
        .await?;
        let Some(Some(limit)) = limit else { return Ok(()) };
        let used: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM memberships WHERE status = 'active')
                  + (SELECT count(*) FROM invitations WHERE accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $1 AND email <> $2)",
        )
        .bind(now)
        .bind(email)
        .fetch_one(tx.conn())
        .await?;
        if used >= i64::from(limit) {
            return Err(MembersError::QuotaExceeded { limit: i64::from(limit), used });
        }
        Ok(())
    }

    pub async fn invite(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Actor>,
        email: &str,
        role_keys: &[String],
        client: &ClientInfo,
    ) -> Result<(Invitation, String), MembersError> {
        let email = email.trim().to_lowercase();
        if !email.contains('@') || email.len() > 254 {
            return Err(MembersError::Invalid("a valid email address is required".into()));
        }
        if role_keys.is_empty() {
            return Err(MembersError::Invalid("at least one role is required".into()));
        }
        // Validate roles exist and the inviter could grant them (no escalation).
        let held = match actor {
            Some(a) => Some(self.access.effective_workspace_permissions(tx, a.membership).await?),
            None => None,
        };
        for key in role_keys {
            let perms: Option<Vec<String>> =
                sqlx::query_scalar("SELECT permissions FROM roles WHERE key = $1")
                    .bind(key)
                    .fetch_optional(tx.conn())
                    .await?;
            let perms = perms.ok_or_else(|| AccessError::UnknownRole(key.clone()))?;
            if let Some(held) = &held {
                let missing: Vec<String> = perms.into_iter().filter(|p| !held.contains(p)).collect();
                if !missing.is_empty() {
                    return Err(AccessError::Escalation(missing).into());
                }
            }
        }
        let already: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships m JOIN users u ON u.id = m.user_id WHERE u.email = $1 AND m.status = 'active')")
            .bind(&email).fetch_one(tx.conn()).await?;
        if already {
            return Err(MembersError::AlreadyMember);
        }
        let now = self.clock.now();
        self.check_member_quota(tx, &email, now).await?;
        sqlx::query("UPDATE invitations SET revoked_at = $2 WHERE email = $1 AND accepted_at IS NULL AND revoked_at IS NULL").bind(&email).bind(now).execute(tx.conn()).await?;
        let id = self.ids.new_id();
        let token = random_token();
        let sealed = seal(self.keys.as_ref(), "secrets", &format!("invitations:{id}"), token.as_bytes())?;
        let expires_at = now + time::Duration::days(INVITE_TTL_DAYS);
        sqlx::query("INSERT INTO invitations (workspace_id, id, email, role_keys, token_hash, token_sealed, invited_by, expires_at) VALUES ($1,$2,$3,$4,$5,$6,(SELECT user_id FROM memberships WHERE id = $7),$8)")
            .bind(tx.ctx.workspace_id).bind(id).bind(&email).bind(role_keys).bind(hash_token(&token)).bind(sealed).bind(actor.map(|a| a.membership)).bind(expires_at)
            .execute(tx.conn()).await?;
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic: "invitation.created.v1",
                aggregate_type: "invitation",
                aggregate_id: id,
                aggregate_revision: 1,
                payload: json!({"invitation_id": id}),
                trace_id: None,
            },
        )
        .await?;
        self.audit(
            tx,
            actor.map(|a| a.subject_ref),
            "member.invited",
            id,
            json!({"roles": role_keys}),
            client,
        )
        .await?;
        Ok((Invitation { id, email, role_keys: role_keys.to_vec(), expires_at }, token))
    }

    /// Pending (not accepted, not revoked, not expired) invitations, newest first.
    pub async fn list_invitations(&self, tx: &mut WsTx<'_>) -> Result<Vec<Invitation>, MembersError> {
        let rows: Vec<(Uuid, String, Vec<String>, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, email::text, role_keys, expires_at FROM invitations WHERE accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $1 ORDER BY created_at DESC",
        ).bind(self.clock.now()).fetch_all(tx.conn()).await?;
        Ok(rows
            .into_iter()
            .map(|(id, email, role_keys, expires_at)| Invitation { id, email, role_keys, expires_at })
            .collect())
    }

    /// Revokes a pending invitation; its token stops working at once. Resending = inviting again,
    /// which revokes the previous token and issues a new one.
    pub async fn revoke_invitation(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let n = sqlx::query("UPDATE invitations SET revoked_at = $2 WHERE id = $1 AND accepted_at IS NULL AND revoked_at IS NULL")
            .bind(id).bind(self.clock.now()).execute(tx.conn()).await?.rows_affected();
        if n == 0 {
            return Err(MembersError::NotFound);
        }
        self.audit_object(
            tx,
            Some(actor.subject_ref),
            "member.invitation_revoked",
            "invitation",
            id,
            json!({}),
            client,
        )
        .await
    }

    /// Issues a fresh link for a pending invitation so the inviter can hand it over themselves (when
    /// email isn't set up). Like resending, the previous link stops working; the new one works once and
    /// expires in 7 days. Returns the invitation and its new token; the token is never listed.
    pub async fn reissue_invitation_link(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Actor>,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(Invitation, String), MembersError> {
        let row: Option<(String, Vec<String>)> = sqlx::query_as(
            "SELECT email::text, role_keys FROM invitations WHERE id = $1 AND accepted_at IS NULL AND revoked_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        let Some((email, role_keys)) = row else { return Err(MembersError::NotFound) };
        let now = self.clock.now();
        // An expired invitation comes back to life, so it needs its seat again.
        self.check_member_quota(tx, &email, now).await?;
        let token = random_token();
        let sealed = seal(self.keys.as_ref(), "secrets", &format!("invitations:{id}"), token.as_bytes())?;
        let expires_at = now + time::Duration::days(INVITE_TTL_DAYS);
        sqlx::query(
            "UPDATE invitations SET token_hash = $2, token_sealed = $3, expires_at = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(hash_token(&token))
        .bind(sealed)
        .bind(expires_at)
        .execute(tx.conn())
        .await?;
        self.audit_object(
            tx,
            actor.map(|a| a.subject_ref),
            "member.invitation_link_copied",
            "invitation",
            id,
            json!({"expires_at": expires_at.unix_timestamp()}),
            client,
        )
        .await?;
        Ok((Invitation { id, email, role_keys, expires_at }, token))
    }

    /// The owner invitation a system admin created with the workspace, while it is still pending.
    pub async fn pending_owner_invitation(&self, tx: &mut WsTx<'_>) -> Result<Option<Uuid>, MembersError> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM invitations WHERE invited_by IS NULL AND 'workspace.owner' = ANY(role_keys)
               AND accepted_at IS NULL AND revoked_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(tx.conn())
        .await?)
    }

    /// What the invitation email needs: (email, token, expires_at, inviter display name, workspace name).
    /// `None` once the invitation is accepted or revoked (nothing to send).
    pub async fn invitation_for_mail(
        &self,
        ctx: WsCtx,
        id: Uuid,
    ) -> Result<Option<(String, String, OffsetDateTime, Option<String>, String)>, MembersError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let row: Option<(String, Vec<u8>, OffsetDateTime, Option<String>, String)> = sqlx::query_as(
            "SELECT i.email::text, i.token_sealed, i.expires_at, u.display_name, w.name FROM invitations i
             JOIN workspaces w ON w.id = i.workspace_id LEFT JOIN users u ON u.id = i.invited_by
             WHERE i.id = $1 AND i.accepted_at IS NULL AND i.revoked_at IS NULL",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        tx.rollback().await?;
        let Some((email, sealed, expires, inviter, workspace)) = row else { return Ok(None) };
        let pt = open(self.keys.as_ref(), "secrets", &format!("invitations:{id}"), &sealed)?;
        let token = String::from_utf8(pt.to_vec()).map_err(|e| MembersError::Internal(e.to_string()))?;
        Ok(Some((email, token, expires, inviter, workspace)))
    }

    /// For a newly activated membership: (inviter's membership, new member's name, workspace name).
    pub async fn joined_context(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
    ) -> Result<Option<(Uuid, String, String)>, MembersError> {
        Ok(sqlx::query_as(
            "SELECT im.id, u.display_name, w.name FROM memberships m
             JOIN users u ON u.id = m.user_id
             JOIN workspaces w ON w.id = m.workspace_id
             JOIN LATERAL (SELECT invited_by FROM invitations i WHERE i.accepted_by = m.user_id ORDER BY i.accepted_at DESC LIMIT 1) inv ON true
             JOIN memberships im ON im.user_id = inv.invited_by AND im.status = 'active'
             WHERE m.id = $1",
        ).bind(membership).fetch_optional(tx.conn()).await?)
    }

    /// Decrypts the token of the latest pending invitation for `email` (mailer and E2E only).
    pub async fn pending_invitation_token(
        &self,
        ctx: WsCtx,
        email: &str,
    ) -> Result<Option<String>, MembersError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let row: Option<(Uuid, Vec<u8>)> = sqlx::query_as("SELECT id, token_sealed FROM invitations WHERE email = $1 AND accepted_at IS NULL AND revoked_at IS NULL ORDER BY created_at DESC LIMIT 1")
            .bind(email.trim().to_lowercase()).fetch_optional(tx.conn()).await?;
        tx.rollback().await?;
        let Some((id, sealed)) = row else { return Ok(None) };
        let pt = open(self.keys.as_ref(), "secrets", &format!("invitations:{id}"), &sealed)?;
        Ok(String::from_utf8(pt.to_vec()).ok())
    }

    async fn invitation_ctx(&self, token: &str) -> Result<WsCtx, MembersError> {
        let ws: Option<Uuid> = sqlx::query_scalar("SELECT discovery.invitation_workspace($1)")
            .bind(hash_token(token))
            .fetch_one(self.db.pool())
            .await?;
        ws.map(|workspace_id| WsCtx { workspace_id }).ok_or(MembersError::InvitationInvalid)
    }

    /// Email of a pending invitation (used by the registration flow).
    pub async fn invitation_email(&self, token: &str) -> Result<String, MembersError> {
        let ctx = self.invitation_ctx(token).await?;
        let mut tx = self.db.begin_ws(ctx).await?;
        let email: Option<String> = sqlx::query_scalar("SELECT email::text FROM invitations WHERE token_hash = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $2")
            .bind(hash_token(token)).bind(self.clock.now()).fetch_optional(tx.conn()).await?;
        tx.rollback().await?;
        email.ok_or(MembersError::InvitationInvalid)
    }

    /// What the invitation page shows to whoever holds the link (docs/22 A-24): the invited email,
    /// the workspace, who invited them, when the link expires and whether that email already has an
    /// account (sign in and accept) or not (create one). The token is the credential.
    pub async fn invitation_preview(&self, token: &str) -> Result<InvitationPreview, MembersError> {
        let ctx = self.invitation_ctx(token).await?;
        let mut tx = self.db.begin_ws(ctx).await?;
        let row: Option<(String, String, Option<String>, OffsetDateTime, bool)> = sqlx::query_as(
            "SELECT i.email::text, w.name, u.display_name, i.expires_at,
                    EXISTS (SELECT 1 FROM users x WHERE x.email = i.email AND x.status = 'active')
             FROM invitations i JOIN workspaces w ON w.id = i.workspace_id LEFT JOIN users u ON u.id = i.invited_by
             WHERE i.token_hash = $1 AND i.accepted_at IS NULL AND i.revoked_at IS NULL AND i.expires_at > $2",
        )
        .bind(hash_token(token))
        .bind(self.clock.now())
        .fetch_optional(tx.conn())
        .await?;
        tx.rollback().await?;
        let (email, workspace, inviter, expires_at, account_exists) =
            row.ok_or(MembersError::InvitationInvalid)?;
        Ok(InvitationPreview { email, workspace, inviter, expires_at, account_exists })
    }

    /// Accepts an invitation for an existing, signed-in user. Returns (workspace, membership).
    pub async fn accept(
        &self,
        token: &str,
        user_id: Uuid,
        client: &ClientInfo,
    ) -> Result<(Uuid, Uuid), MembersError> {
        let ctx = self.invitation_ctx(token).await?;
        let mut tx = self.db.begin_ws(ctx).await?;
        let inv: Option<(Uuid, String, Vec<String>)> = sqlx::query_as(
            "SELECT id, email::text, role_keys FROM invitations WHERE token_hash = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $2 FOR UPDATE",
        ).bind(hash_token(token)).bind(self.clock.now()).fetch_optional(tx.conn()).await?;
        let Some((inv_id, email, roles)) = inv else { return Err(MembersError::InvitationInvalid) };
        let user: (String, Uuid) =
            sqlx::query_as("SELECT email::text, subject_ref FROM users WHERE id = $1 AND status = 'active'")
                .bind(user_id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(MembersError::InvitationInvalid)?;
        if user.0 != email {
            return Err(MembersError::EmailMismatch);
        }
        let existing: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, status FROM memberships WHERE user_id = $1")
                .bind(user_id)
                .fetch_optional(tx.conn())
                .await?;
        let membership = match existing {
            Some((_, s)) if s == "active" => return Err(MembersError::AlreadyMember),
            Some((id, _)) => {
                sqlx::query("UPDATE memberships SET status = 'active', deactivated_at = NULL WHERE id = $1")
                    .bind(id)
                    .execute(tx.conn())
                    .await?;
                id
            }
            None => {
                let id = self.ids.new_id();
                sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status, joined_at) VALUES ($1, $2, $3, 'active', $4)")
                    .bind(ctx.workspace_id).bind(id).bind(user_id).bind(self.clock.now()).execute(tx.conn()).await?;
                id
            }
        };
        for key in &roles {
            // Authorized when the invitation was created; granted here by the system on the inviter's behalf.
            self.access
                .grant(&mut tx, None, membership, key, Scope::Workspace, None, Some("invitation"))
                .await?;
        }
        sqlx::query("UPDATE invitations SET accepted_at = $2, accepted_by = $3 WHERE id = $1")
            .bind(inv_id)
            .bind(self.clock.now())
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        if roles.iter().any(|r| r == "workspace.owner") {
            sqlx::query("UPDATE workspaces SET state = 'active', owner_invite_email = NULL, revision = revision + 1 WHERE id = $1 AND state = 'provisioning'")
                .bind(ctx.workspace_id).execute(tx.conn()).await?;
        }
        outbox::enqueue(
            &mut tx,
            OutboxEvent {
                topic: "membership.activated.v1",
                aggregate_type: "membership",
                aggregate_id: membership,
                aggregate_revision: 1,
                payload: json!({"membership_id": membership}),
                trace_id: None,
            },
        )
        .await?;
        self.audit(
            &mut tx,
            Some(user.1),
            "member.joined",
            membership,
            json!({"roles": roles, "invitation": inv_id}),
            client,
        )
        .await?;
        tx.commit().await?;
        Ok((ctx.workspace_id, membership))
    }

    /// Registers a new account for the invited email and accepts the invitation.
    pub async fn register(
        &self,
        token: &str,
        display_name: &str,
        password: &str,
        client: &ClientInfo,
    ) -> Result<(User, Uuid, Uuid), MembersError> {
        let email = self.invitation_email(token).await?;
        let user = self.identity.create_user(&email, display_name, password, client).await?;
        let (ws, m) = self.accept(token, user.id, client).await?;
        Ok((user, ws, m))
    }

    pub async fn list(&self, tx: &mut WsTx<'_>) -> Result<Vec<Member>, MembersError> {
        type Row = (
            Uuid,
            Uuid,
            String,
            String,
            String,
            Option<String>,
            OffsetDateTime,
            Vec<String>,
            Option<Uuid>,
            Option<String>,
            Option<String>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT m.id, m.user_id, u.email::text, u.display_name, m.status, m.title, m.joined_at,
                    coalesce(array_agg(DISTINCT r.key) FILTER (WHERE r.key IS NOT NULL), '{}'), m.department_id,
                    o.ends_on::text, du.display_name
             FROM memberships m JOIN users u ON u.id = m.user_id
             LEFT JOIN role_grants g ON g.workspace_id = m.workspace_id AND g.membership_id = m.id AND g.scope_kind = 'workspace'
             LEFT JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
             LEFT JOIN out_of_office o ON o.workspace_id = m.workspace_id AND o.membership_id = m.id
                    AND $1::date BETWEEN o.starts_on AND o.ends_on
             LEFT JOIN memberships dm ON dm.workspace_id = o.workspace_id AND dm.id = o.delegate_membership_id
             LEFT JOIN users du ON du.id = dm.user_id
             GROUP BY m.workspace_id, m.id, u.email, u.display_name, o.ends_on, du.display_name ORDER BY u.display_name",
        )
        .bind(self.clock.now().date())
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Member {
                id: r.0,
                user_id: r.1,
                email: r.2,
                display_name: r.3,
                status: r.4,
                title: r.5,
                joined_at: r.6,
                roles: r.7,
                department_id: r.8,
                away: r.9.zip(r.10).map(|(until, delegate)| Away { until, delegate }),
            })
            .collect())
    }

    /// Deactivates a membership (SEC-04 at workspace level): access is denied immediately by the
    /// active-members invariant; the user's other workspaces are unaffected.
    pub async fn deactivate(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        membership: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        if actor.membership == membership {
            return Err(MembersError::SelfDeactivation);
        }
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM memberships WHERE id = $1 FOR UPDATE")
                .bind(membership)
                .fetch_optional(tx.conn())
                .await?;
        if status.is_none() {
            return Err(MembersError::NotFound);
        }
        let is_owner: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id WHERE g.membership_id = $1 AND r.key = 'workspace.owner')")
            .bind(membership).fetch_one(tx.conn()).await?;
        if is_owner {
            let owners: i64 = sqlx::query_scalar("SELECT count(DISTINCT g.membership_id) FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id JOIN memberships m ON m.workspace_id = g.workspace_id AND m.id = g.membership_id WHERE r.key = 'workspace.owner' AND m.status = 'active'")
                .fetch_one(tx.conn()).await?;
            if owners <= 1 {
                return Err(MembersError::LastOwner);
            }
        }
        sqlx::query("UPDATE memberships SET status = 'deactivated', deactivated_at = $2 WHERE id = $1")
            .bind(membership)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        self.access.invalidate(tx).await?;
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic: "membership.deactivated.v1",
                aggregate_type: "membership",
                aggregate_id: membership,
                aggregate_revision: 2,
                payload: json!({"membership_id": membership}),
                trace_id: None,
            },
        )
        .await?;
        self.audit(
            tx,
            Some(actor.subject_ref),
            "member.deactivated",
            membership,
            json!({"status": {"before": "active", "after": "deactivated"}}),
            client,
        )
        .await?;
        Ok(())
    }

    /// Reactivates a deactivated membership (docs/22 A-41). Deactivation removed nothing, so the
    /// member comes back with the roles, groups and directory fields they had. Because that restores
    /// access, the actor must hold every permission of those roles (the invitation rule: no
    /// escalation), the account must not be disabled and the member quota must have a seat.
    /// Returns the restored role keys.
    pub async fn reactivate(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        membership: Uuid,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<Vec<String>, MembersError> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(MembersError::Invalid("a reason of 1-500 characters is required".into()));
        }
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT m.status, u.status, u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1 FOR UPDATE OF m",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?;
        let (status, account, email) = row.ok_or(MembersError::NotFound)?;
        if status != "deactivated" {
            return Err(MembersError::Conflict("this member is already active".into()));
        }
        if account == "disabled" {
            return Err(MembersError::Conflict(
                "this person's account is disabled; a system administrator must restore it first".into(),
            ));
        }
        let now = self.clock.now();
        // Every unexpired grant that will apply again, at any scope, direct or through a group.
        let grants: Vec<(String, Vec<String>)> = sqlx::query_as(
            "SELECT DISTINCT r.key, r.permissions FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
             WHERE (g.membership_id = $1 OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = $1))
               AND (g.expires_at IS NULL OR g.expires_at > $2)",
        )
        .bind(membership)
        .bind(now)
        .fetch_all(tx.conn())
        .await?;
        let held = self.access.effective_workspace_permissions(tx, actor.membership).await?;
        let mut missing: Vec<String> =
            grants.iter().flat_map(|(_, p)| p.iter()).filter(|p| !held.contains(*p)).cloned().collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            return Err(AccessError::Escalation(missing).into());
        }
        self.check_member_quota(tx, &email, now).await?;
        sqlx::query("UPDATE memberships SET status = 'active', deactivated_at = NULL WHERE id = $1")
            .bind(membership)
            .execute(tx.conn())
            .await?;
        self.access.invalidate(tx).await?;
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic: "membership.reactivated.v1",
                aggregate_type: "membership",
                aggregate_id: membership,
                aggregate_revision: 3,
                payload: json!({"membership_id": membership}),
                trace_id: None,
            },
        )
        .await?;
        let mut roles: Vec<String> = grants.into_iter().map(|(k, _)| k).collect();
        roles.sort();
        roles.dedup();
        self.audit(
            tx,
            Some(actor.subject_ref),
            "member.reactivated",
            membership,
            json!({"status": {"before": "deactivated", "after": "active"}, "roles": roles, "reason": reason}),
            client,
        )
        .await?;
        Ok(roles)
    }

    /// The caller's membership in a workspace: (membership id, status, subject_ref).
    pub async fn membership(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
    ) -> Result<Option<(Uuid, String, Uuid)>, MembersError> {
        Ok(sqlx::query_as("SELECT m.id, m.status, u.subject_ref FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.user_id = $1")
            .bind(user_id).fetch_optional(tx.conn()).await?)
    }

    pub async fn user_workspaces(&self, user_id: Uuid) -> Result<Vec<UserWorkspace>, MembersError> {
        let rows: Vec<(Uuid, String, String, String)> =
            sqlx::query_as("SELECT workspace_id, slug, name, state FROM discovery.user_workspaces($1)")
                .bind(user_id)
                .fetch_all(self.db.pool())
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| UserWorkspace { workspace_id: r.0, slug: r.1, name: r.2, state: r.3 })
            .collect())
    }

    /// The account behind a membership of the current workspace, for admin account controls
    /// (docs/06 §3.6.1): (user id, membership status, display name).
    pub async fn member_account(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
    ) -> Result<(Uuid, String, String), MembersError> {
        let row: Option<(Uuid, String, String)> = sqlx::query_as(
            "SELECT m.user_id, m.status, u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?;
        row.ok_or(MembersError::NotFound)
    }

    /// Whether an account is used outside workspace `ws`: an active membership of another workspace,
    /// or a system-plane role. Accounts are global, so a workspace admin may change their password
    /// state, sessions or second factors only when neither holds (A-39); otherwise a system
    /// administrator does it.
    pub async fn account_used_elsewhere(&self, user_id: Uuid, ws: Uuid) -> Result<bool, MembersError> {
        let others = self.user_workspaces(user_id).await?.iter().any(|w| w.workspace_id != ws);
        if others {
            return Ok(true);
        }
        Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM system_role_grants WHERE user_id = $1)")
            .bind(user_id)
            .fetch_one(self.db.pool())
            .await?)
    }

    pub fn db(&self) -> &Db {
        &self.db
    }
}
