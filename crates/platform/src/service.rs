use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{hash_token, random_token};
use procera_db::{Db, DbError, WsTx};
use procera_identity::{ClientInfo, Identity, IdentityError, User};
use procera_kernel::{Clock, IdGen};
use procera_members::{Members, MembersError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("bootstrap token invalid, expired or already used")]
    BootstrapInvalid,
    #[error("the platform is already bootstrapped")]
    AlreadyBootstrapped,
    #[error("workspace not found")]
    NotFound,
    #[error("slug already in use")]
    SlugTaken,
    #[error("invalid workspace input: {0}")]
    Invalid(String),
    #[error("transition not allowed from state {0}")]
    BadTransition(String),
    #[error("revision mismatch (expected {expected}, current {current})")]
    StaleRevision { expected: i64, current: i64 },
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Members(#[from] MembersError),
    #[error(transparent)]
    Settings(#[from] procera_settings::SettingsError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for PlatformError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for PlatformError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for PlatformError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Provisioning,
    Active,
    SuspendedRo,
    SuspendedLocked,
    PendingDeletion,
    Deleted,
}

impl WorkspaceState {
    fn parse(s: &str) -> Self {
        serde_json::from_value(Value::String(s.to_owned())).unwrap_or(Self::Provisioning)
    }
    fn as_str(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Quotas {
    pub max_members: Option<i32>,
    pub max_active_instances: Option<i32>,
    pub max_storage_bytes: Option<i64>,
    pub api_rpm: Option<i32>,
    pub ai_tokens_month: Option<i64>,
}

impl Quotas {
    /// Defaults per configuration profile (docs/20 §0). `None` = unlimited.
    pub fn for_profile(profile: &str) -> Self {
        match profile {
            "starter" => Self {
                max_members: Some(100),
                max_active_instances: Some(5_000),
                max_storage_bytes: Some(10 << 30),
                api_rpm: Some(600),
                ai_tokens_month: Some(0),
            },
            "standard" => Self {
                max_members: Some(2_000),
                max_active_instances: Some(100_000),
                max_storage_bytes: Some(200 << 30),
                api_rpm: Some(3_000),
                ai_tokens_month: Some(0),
            },
            _ => Self {
                max_members: None,
                max_active_instances: None,
                max_storage_bytes: None,
                api_rpm: Some(10_000),
                ai_tokens_month: Some(0),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Workspace {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub profile: String,
    pub state: WorkspaceState,
    pub isolation_tier: String,
    pub region: String,
    pub owner_invite_email: Option<String>,
    pub suspended_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deletion_due_at: Option<OffsetDateTime>,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub quotas: Quotas,
}

/// Filters for the system console's workspace list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFilter {
    pub state: Option<String>,
    pub profile: Option<String>,
    pub q: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewWorkspace {
    pub name: String,
    pub slug: String,
    pub profile: String,
    pub owner_email: String,
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct WsRow {
    id: Uuid,
    slug: String,
    name: String,
    profile: String,
    state: String,
    isolation_tier: String,
    region: String,
    owner_invite_email: Option<String>,
    suspended_reason: Option<String>,
    deletion_due_at: Option<OffsetDateTime>,
    revision: i64,
    created_at: OffsetDateTime,
    max_members: Option<i32>,
    max_active_instances: Option<i32>,
    max_storage_bytes: Option<i64>,
    api_rpm: Option<i32>,
    ai_tokens_month: Option<i64>,
}

const SELECT_WS: &str = "SELECT w.id, w.slug::text AS slug, w.name, w.profile, w.state, w.isolation_tier, w.region, w.owner_invite_email::text AS owner_invite_email, w.suspended_reason,
    w.deletion_due_at, w.revision, w.created_at, q.max_members, q.max_active_instances, q.max_storage_bytes, q.api_rpm, q.ai_tokens_month
    FROM workspaces w LEFT JOIN workspace_quotas q ON q.target_workspace_id = w.id";

fn to_ws(r: WsRow) -> Workspace {
    Workspace {
        id: r.id,
        slug: r.slug,
        name: r.name,
        profile: r.profile,
        state: WorkspaceState::parse(&r.state),
        isolation_tier: r.isolation_tier,
        region: r.region,
        owner_invite_email: r.owner_invite_email,
        suspended_reason: r.suspended_reason,
        deletion_due_at: r.deletion_due_at,
        revision: r.revision,
        created_at: r.created_at,
        quotas: Quotas {
            max_members: r.max_members,
            max_active_instances: r.max_active_instances,
            max_storage_bytes: r.max_storage_bytes,
            api_rpm: r.api_rpm,
            ai_tokens_month: r.ai_tokens_month,
        },
    }
}

#[derive(Debug, Clone)]
pub struct Platform {
    pub(crate) db: Db,
    identity: Identity,
    members: Members,
    settings: procera_settings::Settings,
    pub(crate) clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl Platform {
    pub fn settings(&self) -> &procera_settings::Settings {
        &self.settings
    }

    /// The configuration profile recorded for the transaction's workspace (docs/20 §0).
    pub async fn workspace_profile(&self, tx: &mut WsTx<'_>) -> Result<String, PlatformError> {
        sqlx::query_scalar("SELECT profile FROM workspaces WHERE id = $1")
            .bind(tx.ctx.workspace_id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(PlatformError::NotFound)
    }

    /// Switches the transaction's workspace to `to` (CFG-017): applies the settings plan if it still
    /// matches `basis`, then records the profile on the workspace, in the caller's transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn switch_profile(
        &self,
        tx: &mut WsTx<'_>,
        actor: (Uuid, Uuid),
        to: &str,
        reason: &str,
        basis: Option<&str>,
        client: (Option<String>, Option<String>),
    ) -> Result<procera_settings::ProfilePlan, PlatformError> {
        let from = self.workspace_profile(tx).await?;
        let plan = self.settings.apply_profile_switch(tx, actor, &from, to, reason, basis, client).await?;
        sqlx::query("UPDATE workspaces SET profile = $2, revision = revision + 1 WHERE id = $1")
            .bind(tx.ctx.workspace_id)
            .bind(to)
            .execute(tx.conn())
            .await?;
        Ok(plan)
    }

    pub fn new(
        db: Db,
        identity: Identity,
        members: Members,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
    ) -> Self {
        let settings = procera_settings::Settings::new(clock.clone(), ids.clone());
        Self { db, identity, members, settings, clock, ids }
    }

    pub(crate) async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        action: &str,
        object: Option<Uuid>,
        diff: Option<Value>,
        client: &ClientInfo,
    ) -> Result<(), PlatformError> {
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Admin,
            action,
            Outcome::Allowed,
        );
        if let Some(id) = object {
            ev = ev.object("workspace", id);
        }
        ev.diff = diff;
        ev.ip = client.ip.clone();
        ev.user_agent = client.user_agent.clone();
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    pub(crate) async fn subject(&self, tx: &mut WsTx<'_>, user: Uuid) -> Result<Option<Uuid>, PlatformError> {
        Ok(sqlx::query_scalar("SELECT subject_ref FROM users WHERE id = $1")
            .bind(user)
            .fetch_optional(tx.conn())
            .await?)
    }

    /// FR-S01: if no system owner exists, issue a one-time bootstrap token (24 h). Returns `None` once bootstrapped.
    pub async fn issue_bootstrap_token(&self) -> Result<Option<String>, PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let has_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM system_role_grants WHERE role = 'system.owner')",
        )
        .fetch_one(tx.conn())
        .await?;
        if has_owner {
            return Ok(None);
        }
        let token = random_token();
        sqlx::query("INSERT INTO bootstrap_tokens (token_hash, expires_at) VALUES ($1, $2)")
            .bind(hash_token(&token))
            .bind(self.clock.now() + time::Duration::hours(24))
            .execute(tx.conn())
            .await?;
        self.audit(&mut tx, None, "platform.bootstrap_token_issued", None, None, &ClientInfo::default())
            .await?;
        tx.commit().await?;
        Ok(Some(token))
    }

    /// FR-S01: consumes a bootstrap token and creates the first `system.owner`. MFA enrolment is then
    /// mandatory before any system-plane action (enforced at the API).
    pub async fn bootstrap(
        &self,
        token: &str,
        email: &str,
        display_name: &str,
        password: &str,
        client: &ClientInfo,
    ) -> Result<User, PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let has_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM system_role_grants WHERE role = 'system.owner')",
        )
        .fetch_one(tx.conn())
        .await?;
        if has_owner {
            return Err(PlatformError::AlreadyBootstrapped);
        }
        // Refuse a weak password or a used email before the single-use token is spent.
        self.identity.check_new_account(email, password).await?;
        let n = sqlx::query("UPDATE bootstrap_tokens SET used_at = $2 WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2")
            .bind(hash_token(token)).bind(self.clock.now()).execute(tx.conn()).await?.rows_affected();
        if n != 1 {
            return Err(PlatformError::BootstrapInvalid);
        }
        tx.commit().await?; // token is spent even if user creation fails (single use)
        let user = self.identity.create_user(email, display_name, password, client).await?;
        let mut tx = self.db.begin_system().await?;
        sqlx::query("INSERT INTO system_role_grants (user_id, role) VALUES ($1, 'system.owner')")
            .bind(user.id)
            .execute(tx.conn())
            .await?;
        sqlx::query("UPDATE bootstrap_tokens SET used_by = $1 WHERE token_hash = $2")
            .bind(user.id)
            .bind(hash_token(token))
            .execute(tx.conn())
            .await?;
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "platform.bootstrapped",
            None,
            Some(json!({"role": "system.owner"})),
            client,
        )
        .await?;
        tx.commit().await?;
        Ok(user)
    }

    pub async fn system_roles(&self, user: Uuid) -> Result<Vec<String>, PlatformError> {
        Ok(sqlx::query_scalar("SELECT role FROM system_role_grants WHERE user_id = $1 ORDER BY role")
            .bind(user)
            .fetch_all(self.db.pool())
            .await?)
    }

    /// Grants a system role (used by tests and the future system-admin management UI).
    pub async fn grant_system_role(
        &self,
        granted_by: Option<Uuid>,
        user: Uuid,
        role: &str,
        client: &ClientInfo,
    ) -> Result<(), PlatformError> {
        let mut tx = self.db.begin_system().await?;
        sqlx::query("INSERT INTO system_role_grants (user_id, role, granted_by) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(user).bind(role).bind(granted_by).execute(tx.conn()).await?;
        let actor = match granted_by {
            Some(g) => self.subject(&mut tx, g).await?,
            None => None,
        };
        let target = self.subject(&mut tx, user).await?;
        self.audit(
            &mut tx,
            actor,
            "system.role_granted",
            None,
            Some(json!({"role": role, "user_subject": target})),
            client,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn create_workspace(
        &self,
        actor: Uuid,
        req: &NewWorkspace,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        if !matches!(req.profile.as_str(), "starter" | "standard" | "enterprise" | "regulated") {
            return Err(PlatformError::Invalid(
                "profile must be starter, standard, enterprise or regulated".into(),
            ));
        }
        let slug = req.slug.trim().to_lowercase();
        if !(3..=63).contains(&slug.len())
            || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || slug.starts_with('-')
            || slug.ends_with('-')
        {
            return Err(PlatformError::Invalid(
                "slug must be 3-63 chars of a-z, 0-9 and '-' (not at the ends)".into(),
            ));
        }
        if req.name.trim().is_empty() || req.name.chars().count() > 120 {
            return Err(PlatformError::Invalid("name must be 1-120 characters".into()));
        }
        if !req.owner_email.contains('@') {
            return Err(PlatformError::Invalid("owner_email must be an email address".into()));
        }
        let mut tx = self.db.begin_system().await?;
        let id = self.ids.new_id();
        let inserted = sqlx::query(
            "INSERT INTO workspaces (id, slug, name, profile, state, region, owner_invite_email, created_by) VALUES ($1, $2, $3, $4, 'provisioning', $5, $6, $7)
             ON CONFLICT (slug) DO NOTHING",
        )
        .bind(id).bind(&slug).bind(req.name.trim()).bind(&req.profile).bind(req.region.as_deref().unwrap_or("default"))
        .bind(req.owner_email.trim().to_lowercase()).bind(actor)
        .execute(tx.conn()).await?.rows_affected();
        if inserted == 0 {
            return Err(PlatformError::SlugTaken);
        }
        let q = crate::console::profile_defaults(&mut tx, &req.profile).await?;
        sqlx::query("INSERT INTO workspace_quotas (target_workspace_id, max_members, max_active_instances, max_storage_bytes, api_rpm, ai_tokens_month) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(id).bind(q.max_members).bind(q.max_active_instances).bind(q.max_storage_bytes).bind(q.api_rpm).bind(q.ai_tokens_month)
            .execute(tx.conn()).await?;
        let subject = self.subject(&mut tx, actor).await?;
        self.audit(
            &mut tx,
            subject,
            "workspace.created",
            Some(id),
            Some(json!({"slug": slug, "profile": req.profile})),
            client,
        )
        .await?;
        let ws = self.load(&mut tx, id).await?;
        // Same transaction: seed built-in roles and invite the first owner inside the new workspace.
        tx.rescope(procera_db::WsCtx { workspace_id: id }).await?;
        self.members.access().seed_workspace(&mut tx).await.map_err(MembersError::from)?;
        self.settings.apply_profile(&mut tx, &req.profile).await?;
        self.members.invite(&mut tx, None, &req.owner_email, &["workspace.owner".to_owned()], client).await?;
        tx.commit().await?;
        Ok(ws)
    }

    /// A fresh link to the owner's pending invitation, for the system admin to hand over when email
    /// isn't set up. Recorded on the system chain (who copied it) and the workspace chain (the
    /// invitation's token changed); the earlier link stops working. Returns (email, token, expires_at).
    pub async fn owner_invitation_link(
        &self,
        actor: Uuid,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(String, String, OffsetDateTime), PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let ws = self.load(&mut tx, id).await?;
        let subject = self.subject(&mut tx, actor).await?;
        self.audit(&mut tx, subject, "workspace.owner_invitation_link_copied", Some(ws.id), None, client)
            .await?;
        tx.rescope(procera_db::WsCtx { workspace_id: id }).await?;
        let invitation = self.members.pending_owner_invitation(&mut tx).await?.ok_or_else(|| {
            MembersError::Conflict("The owner has already joined, or their invitation was revoked.".into())
        })?;
        let (inv, token) = self.members.reissue_invitation_link(&mut tx, None, invitation, client).await?;
        tx.commit().await?;
        Ok((inv.email, token, inv.expires_at))
    }

    async fn load(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Workspace, PlatformError> {
        let row: Option<WsRow> = sqlx::query_as(&format!("{SELECT_WS} WHERE w.id = $1"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
        row.map(to_ws).ok_or(PlatformError::NotFound)
    }

    pub async fn get_workspace(&self, id: Uuid) -> Result<Workspace, PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let ws = self.load(&mut tx, id).await?;
        tx.rollback().await?;
        Ok(ws)
    }

    /// Workspaces that are not deleted, newest first, optionally filtered by state, profile and a
    /// name/slug substring (case-insensitive).
    pub async fn list_workspaces(&self, f: &WorkspaceFilter) -> Result<Vec<Workspace>, PlatformError> {
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(SELECT_WS);
        qb.push(" WHERE w.state <> 'deleted'");
        if let Some(st) = f.state.as_deref().filter(|v| !v.is_empty()) {
            qb.push(" AND w.state = ").push_bind(st.to_owned());
        }
        if let Some(pr) = f.profile.as_deref().filter(|v| !v.is_empty()) {
            qb.push(" AND w.profile = ").push_bind(pr.to_owned());
        }
        if let Some(q) = f.q.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            qb.push(" AND (w.name ILIKE ")
                .push_bind(like.clone())
                .push(" OR w.slug::text ILIKE ")
                .push_bind(like)
                .push(")");
        }
        qb.push(" ORDER BY w.created_at DESC LIMIT 500");
        let rows: Vec<WsRow> = qb.build_query_as().fetch_all(self.db.pool()).await?;
        Ok(rows.into_iter().map(to_ws).collect())
    }

    /// Applies a lifecycle transition with optimistic concurrency (`expected_revision` from If-Match).
    async fn transition(
        &self,
        actor: Uuid,
        id: Uuid,
        expected_revision: Option<i64>,
        action: &str,
        client: &ClientInfo,
        apply: impl FnOnce(
            WorkspaceState,
        )
            -> Result<(WorkspaceState, Option<String>, Option<OffsetDateTime>), PlatformError>,
    ) -> Result<Workspace, PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let (state, revision): (String, i64) =
            sqlx::query_as("SELECT state, revision FROM workspaces WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(PlatformError::NotFound)?;
        if let Some(exp) = expected_revision
            && exp != revision
        {
            return Err(PlatformError::StaleRevision { expected: exp, current: revision });
        }
        let from = WorkspaceState::parse(&state);
        let (to, reason, due) = apply(from)?;
        sqlx::query("UPDATE workspaces SET state = $2, suspended_reason = $3, deletion_due_at = $4, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(to.as_str()).bind(&reason).bind(due).execute(tx.conn()).await?;
        let subject = self.subject(&mut tx, actor).await?;
        self.audit(
            &mut tx,
            subject,
            action,
            Some(id),
            Some(json!({"state": {"before": from.as_str(), "after": to.as_str()}, "reason": reason})),
            client,
        )
        .await?;
        let ws = self.load(&mut tx, id).await?;
        tx.commit().await?;
        Ok(ws)
    }

    pub async fn suspend(
        &self,
        actor: Uuid,
        id: Uuid,
        rev: Option<i64>,
        locked: bool,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        let reason = reason.trim().to_owned();
        if reason.is_empty() {
            return Err(PlatformError::Invalid("a reason is required".into()));
        }
        self.transition(actor, id, rev, "workspace.suspended", client, |from| match from {
            WorkspaceState::Provisioning
            | WorkspaceState::Active
            | WorkspaceState::SuspendedRo
            | WorkspaceState::SuspendedLocked => Ok((
                if locked { WorkspaceState::SuspendedLocked } else { WorkspaceState::SuspendedRo },
                Some(reason),
                None,
            )),
            other => Err(PlatformError::BadTransition(other.as_str())),
        })
        .await
    }

    pub async fn restore(
        &self,
        actor: Uuid,
        id: Uuid,
        rev: Option<i64>,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        self.transition(actor, id, rev, "workspace.restored", client, |from| match from {
            WorkspaceState::SuspendedRo
            | WorkspaceState::SuspendedLocked
            | WorkspaceState::PendingDeletion => Ok((WorkspaceState::Active, None, None)),
            other => Err(PlatformError::BadTransition(other.as_str())),
        })
        .await
    }

    pub async fn schedule_deletion(
        &self,
        actor: Uuid,
        id: Uuid,
        rev: Option<i64>,
        grace_days: i64,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        if !(7..=90).contains(&grace_days) {
            return Err(PlatformError::Invalid("grace_days must be between 7 and 90".into()));
        }
        let reason = reason.trim().to_owned();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(PlatformError::Invalid("a reason (1-500 characters) is required".into()));
        }
        let due = self.clock.now() + time::Duration::days(grace_days);
        self.transition(actor, id, rev, "workspace.deletion_scheduled", client, move |from| match from {
            WorkspaceState::Provisioning
            | WorkspaceState::Active
            | WorkspaceState::SuspendedRo
            | WorkspaceState::SuspendedLocked => {
                Ok((WorkspaceState::PendingDeletion, Some(reason), Some(due)))
            }
            other => Err(PlatformError::BadTransition(other.as_str())),
        })
        .await
    }

    pub async fn rename(
        &self,
        actor: Uuid,
        id: Uuid,
        rev: Option<i64>,
        name: &str,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 120 {
            return Err(PlatformError::Invalid("name must be 1-120 characters".into()));
        }
        let mut tx = self.db.begin_system().await?;
        let (old, revision): (String, i64) =
            sqlx::query_as("SELECT name, revision FROM workspaces WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(PlatformError::NotFound)?;
        if let Some(exp) = rev
            && exp != revision
        {
            return Err(PlatformError::StaleRevision { expected: exp, current: revision });
        }
        sqlx::query("UPDATE workspaces SET name = $2, revision = revision + 1 WHERE id = $1")
            .bind(id)
            .bind(&name)
            .execute(tx.conn())
            .await?;
        let subject = self.subject(&mut tx, actor).await?;
        self.audit(
            &mut tx,
            subject,
            "workspace.renamed",
            Some(id),
            Some(json!({"name": {"before": old, "after": name}})),
            client,
        )
        .await?;
        let ws = self.load(&mut tx, id).await?;
        tx.commit().await?;
        Ok(ws)
    }

    pub async fn set_quotas(
        &self,
        actor: Uuid,
        id: Uuid,
        q: &Quotas,
        client: &ClientInfo,
    ) -> Result<Workspace, PlatformError> {
        let mut tx = self.db.begin_system().await?;
        let n = sqlx::query("UPDATE workspace_quotas SET max_members=$2, max_active_instances=$3, max_storage_bytes=$4, api_rpm=$5, ai_tokens_month=$6, updated_at=now() WHERE target_workspace_id=$1")
            .bind(id).bind(q.max_members).bind(q.max_active_instances).bind(q.max_storage_bytes).bind(q.api_rpm).bind(q.ai_tokens_month)
            .execute(tx.conn()).await.map_err(|e| PlatformError::Invalid(e.to_string()))?.rows_affected();
        if n == 0 {
            return Err(PlatformError::NotFound);
        }
        let subject = self.subject(&mut tx, actor).await?;
        self.audit(
            &mut tx,
            subject,
            "workspace.quotas_changed",
            Some(id),
            Some(serde_json::to_value(q).unwrap_or_default()),
            client,
        )
        .await?;
        let ws = self.load(&mut tx, id).await?;
        tx.commit().await?;
        Ok(ws)
    }
}
