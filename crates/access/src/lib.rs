//! Workspace-plane authorization (BPMS-024, docs/12, ADR-0011).
//!
//! - Roles are bundles of catalogue permissions; built-ins are seeded per workspace, custom roles are
//!   created or cloned by admins and can never exceed the creator's own permissions (no escalation).
//! - Grants bind a role to a member within a scope (workspace / department / collection / process / case).
//! - Grants compile into Cedar `permit` policies, one per (role, scope), plus fixed `forbid` invariants
//!   (workspace isolation, inactive members). The compiled set is cached per workspace access revision.
//! - Each check builds only the entities it needs: the principal, its live grants, and the resource chain.

pub mod catalogue;

use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use cedar_policy::{
    Authorizer, Context, Decision, Entities, EntityUid, Policy, PolicyId, PolicySet, Request,
};
use procera_db::{DbError, WsTx};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use catalogue::{OWNER_ONLY, TEMPLATES, is_permission, template};

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("unknown permission {0}")]
    UnknownPermission(String),
    #[error("unknown role {0}")]
    UnknownRole(String),
    #[error("role key already exists")]
    RoleExists,
    #[error("built-in roles cannot be edited; clone it into a custom role")]
    BuiltinImmutable,
    #[error("you cannot grant or create permissions you do not hold: {0:?}")]
    Escalation(Vec<String>),
    #[error("not found")]
    NotFound,
    #[error("the last workspace owner cannot lose the owner role")]
    LastOwner,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for AccessError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for AccessError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

/// Where a grant applies (docs/12 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Scope {
    Workspace,
    Department(Uuid),
    Collection(Uuid),
    Process(Uuid),
    Case(Uuid),
}

impl Scope {
    fn kind(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Department(_) => "department",
            Self::Collection(_) => "collection",
            Self::Process(_) => "process",
            Self::Case(_) => "case",
        }
    }
    fn id(self) -> Option<Uuid> {
        match self {
            Self::Workspace => None,
            Self::Department(i) | Self::Collection(i) | Self::Process(i) | Self::Case(i) => Some(i),
        }
    }
    pub fn parse(kind: &str, id: Option<Uuid>) -> Result<Self, AccessError> {
        match (kind, id) {
            ("workspace", None) => Ok(Self::Workspace),
            ("department", Some(i)) => Ok(Self::Department(i)),
            ("collection", Some(i)) => Ok(Self::Collection(i)),
            ("process", Some(i)) => Ok(Self::Process(i)),
            ("case", Some(i)) => Ok(Self::Case(i)),
            _ => Err(AccessError::Invalid("scope_kind/scope_id combination is invalid".into())),
        }
    }
    fn entity_type(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Department(_) => "Department",
            Self::Collection(_) => "Collection",
            Self::Process(_) => "Process",
            Self::Case(_) => "Case",
        }
    }
}

/// The object of an authorization check. `ancestors` lists enclosing scopes (nearest first) for
/// scoped resources, e.g. a process inside a collection.
#[derive(Debug, Clone)]
pub enum Resource {
    Workspace,
    Member(Uuid),
    Role(Uuid),
    Scoped { scope: Scope, ancestors: Vec<Scope> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccessDecision {
    pub allowed: bool,
    /// Policy ids that determined the decision (explainable audit, docs/12 §1).
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Role {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub is_builtin: bool,
    pub builtin_template: Option<String>,
    pub permissions: Vec<String>,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Grant {
    pub id: Uuid,
    pub role_key: String,
    /// Exactly one of `membership_id` / `group_id` is set.
    pub membership_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub scope: Scope,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub reason: Option<String>,
}

/// Impact of a proposed custom-role change (docs/12 §5 item 5), shown before saving.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RolePreview {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Active members holding the role through a live grant (any scope).
    pub affected_members: Vec<Uuid>,
}

#[derive(Debug, Clone, Default)]
pub struct Access {
    cache: Arc<RwLock<HashMap<Uuid, (i64, Arc<PolicySet>)>>>,
}

/// Grants that apply to membership `$1`: direct, or through a static group it belongs to.
const APPLIES_TO_MEMBER: &str = "(g.membership_id = $1 OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = $1))";

fn uid(ty: &str, id: impl std::fmt::Display) -> Result<EntityUid, AccessError> {
    EntityUid::from_str(&format!("{ty}::\"{id}\"")).map_err(|e| AccessError::Internal(e.to_string()))
}

fn entity_ref(ty: &str, id: impl std::fmt::Display) -> Value {
    json!({"type": ty, "id": id.to_string()})
}

impl Access {
    pub fn new() -> Self {
        Self::default()
    }

    async fn bump(tx: &mut WsTx<'_>) -> Result<(), AccessError> {
        sqlx::query("INSERT INTO access_revisions (workspace_id, revision) VALUES ($1, 1) ON CONFLICT (workspace_id) DO UPDATE SET revision = access_revisions.revision + 1")
            .bind(tx.ctx.workspace_id).execute(tx.conn()).await?;
        Ok(())
    }

    /// Seeds the built-in roles into a new workspace (idempotent).
    pub async fn seed_workspace(&self, tx: &mut WsTx<'_>) -> Result<(), AccessError> {
        let ws = tx.ctx.workspace_id;
        for t in TEMPLATES {
            sqlx::query("INSERT INTO roles (workspace_id, id, key, name, description, builtin_template, is_builtin, permissions) VALUES ($1, $2, $3, $4, $5, $3, true, $6) ON CONFLICT (workspace_id, key) DO NOTHING")
                .bind(ws).bind(Uuid::now_v7()).bind(t.key).bind(t.name).bind(t.description).bind(t.permissions()).execute(tx.conn()).await?;
        }
        Self::bump(tx).await
    }

    pub async fn list_roles(&self, tx: &mut WsTx<'_>) -> Result<Vec<Role>, AccessError> {
        let rows: Vec<(Uuid, String, String, Option<String>, bool, Option<String>, Vec<String>, i64)> =
            sqlx::query_as("SELECT id, key, name, description, is_builtin, builtin_template, permissions, revision FROM roles ORDER BY is_builtin DESC, name").fetch_all(tx.conn()).await?;
        Ok(rows
            .into_iter()
            .map(|r| Role {
                id: r.0,
                key: r.1,
                name: r.2,
                description: r.3,
                is_builtin: r.4,
                builtin_template: r.5,
                permissions: r.6,
                revision: r.7,
            })
            .collect())
    }

    async fn role_by_key(tx: &mut WsTx<'_>, key: &str) -> Result<(Uuid, Vec<String>), AccessError> {
        sqlx::query_as("SELECT id, permissions FROM roles WHERE key = $1")
            .bind(key)
            .fetch_optional(tx.conn())
            .await?
            .ok_or_else(|| AccessError::UnknownRole(key.to_owned()))
    }

    /// Union of permissions from the member's live workspace-scope grants. Owners hold everything.
    pub async fn effective_workspace_permissions(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
    ) -> Result<BTreeSet<String>, AccessError> {
        let perms: Vec<Vec<String>> = sqlx::query_scalar(
            &format!("SELECT r.permissions FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
             WHERE {APPLIES_TO_MEMBER} AND g.scope_kind = 'workspace' AND (g.expires_at IS NULL OR g.expires_at > now())
               AND EXISTS (SELECT 1 FROM memberships m WHERE m.id = $1 AND m.status = 'active')"),
        ).bind(membership).fetch_all(tx.conn()).await?;
        Ok(perms.into_iter().flatten().collect())
    }

    /// Active members (membership, email) holding any of `perms` through a live grant, direct or via
    /// a group, at any scope: who to tell that work like a review is waiting (A-25). Authorization
    /// still happens when they act.
    pub async fn members_holding(
        &self,
        tx: &mut WsTx<'_>,
        perms: &[&str],
    ) -> Result<Vec<(Uuid, String)>, AccessError> {
        let perms: Vec<String> = perms.iter().map(|p| (*p).to_owned()).collect();
        Ok(sqlx::query_as(
            "SELECT DISTINCT m.id, u.email::text FROM memberships m JOIN users u ON u.id = m.user_id
             JOIN role_grants g ON g.workspace_id = m.workspace_id
              AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id))
             JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
             WHERE m.status = 'active' AND (g.expires_at IS NULL OR g.expires_at > now()) AND r.permissions && $1::text[]
             ORDER BY m.id",
        )
        .bind(&perms)
        .fetch_all(tx.conn())
        .await?)
    }

    async fn ensure_no_escalation(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        perms: &[String],
    ) -> Result<(), AccessError> {
        let Some(actor) = actor else { return Ok(()) }; // system plane (owner invite) is trusted
        let held = self.effective_workspace_permissions(tx, actor).await?;
        let missing: Vec<String> = perms.iter().filter(|p| !held.contains(*p)).cloned().collect();
        if missing.is_empty() { Ok(()) } else { Err(AccessError::Escalation(missing)) }
    }

    /// Creates a custom role (optionally cloned from an existing role's permissions).
    pub async fn create_role(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        key: &str,
        name: &str,
        permissions: &[String],
        cloned_from: Option<&str>,
    ) -> Result<Role, AccessError> {
        for p in permissions {
            if !is_permission(p) {
                return Err(AccessError::UnknownPermission(p.clone()));
            }
            if OWNER_ONLY.contains(&p.as_str()) {
                return Err(AccessError::Escalation(vec![p.clone()]));
            }
        }
        if !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
            || key.len() < 2
            || key.len() > 64
            || template(key).is_some()
        {
            return Err(AccessError::Invalid(
                "key must be 2-64 chars of a-z, 0-9, '_', '.', '-' and not a built-in key".into(),
            ));
        }
        self.ensure_no_escalation(tx, actor, permissions).await?;
        let lineage: Option<String> = match cloned_from {
            Some(k) => {
                sqlx::query_scalar("SELECT coalesce(builtin_template, key) FROM roles WHERE key = $1")
                    .bind(k)
                    .fetch_optional(tx.conn())
                    .await?
            }
            None => None,
        };
        let id = Uuid::now_v7();
        let mut perms = permissions.to_vec();
        perms.sort();
        perms.dedup();
        let n = sqlx::query("INSERT INTO roles (workspace_id, id, key, name, builtin_template, permissions) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (workspace_id, key) DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(id).bind(key).bind(name.trim()).bind(&lineage).bind(&perms).execute(tx.conn()).await?.rows_affected();
        if n == 0 {
            return Err(AccessError::RoleExists);
        }
        Self::bump(tx).await?;
        Ok(Role {
            id,
            key: key.into(),
            name: name.trim().into(),
            description: None,
            is_builtin: false,
            builtin_template: lineage,
            permissions: perms,
            revision: 1,
        })
    }

    /// Replaces a custom role's permissions (built-ins are immutable).
    pub async fn update_role(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        id: Uuid,
        permissions: &[String],
    ) -> Result<(), AccessError> {
        for p in permissions {
            if !is_permission(p) || OWNER_ONLY.contains(&p.as_str()) {
                return Err(AccessError::UnknownPermission(p.clone()));
            }
        }
        let builtin: bool = sqlx::query_scalar("SELECT is_builtin FROM roles WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(AccessError::NotFound)?;
        if builtin {
            return Err(AccessError::BuiltinImmutable);
        }
        // Symmetric: the editor must hold both the old and the new permission sets.
        let old: Vec<String> = sqlx::query_scalar("SELECT permissions FROM roles WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
        self.ensure_no_escalation(tx, actor, &old).await?;
        self.ensure_no_escalation(tx, actor, permissions).await?;
        let mut permissions = permissions.to_vec();
        permissions.sort();
        permissions.dedup();
        sqlx::query("UPDATE roles SET permissions = $2, revision = revision + 1 WHERE id = $1")
            .bind(id)
            .bind(&permissions)
            .execute(tx.conn())
            .await?;
        Self::bump(tx).await
    }

    /// Grants `role_key` to a member in `scope`. The granter must hold every permission in the role.
    #[allow(clippy::too_many_arguments)]
    pub async fn grant(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        membership: Uuid,
        role_key: &str,
        scope: Scope,
        expires_at: Option<OffsetDateTime>,
        reason: Option<&str>,
    ) -> Result<Uuid, AccessError> {
        let (role_id, perms) = Self::role_by_key(tx, role_key).await?;
        // Owners may appoint co-owners; admins cannot (they lack owner-only permissions).
        self.ensure_no_escalation(tx, actor, &perms).await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO role_grants (workspace_id, id, role_id, membership_id, scope_kind, scope_id, expires_at, granted_by, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tx.ctx.workspace_id).bind(id).bind(role_id).bind(membership).bind(scope.kind()).bind(scope.id()).bind(expires_at).bind(actor).bind(reason)
            .execute(tx.conn()).await?;
        Self::bump(tx).await?;
        Ok(id)
    }

    /// Revokes a grant. The revoker must hold every permission of the role (no stripping a role you
    /// could not grant), and the last active workspace owner keeps the owner role.
    /// Grants `role_key` to every member of a static group (docs/12 §5). Same no-escalation rule;
    /// the owner role is never granted to groups (owners are named people).
    #[allow(clippy::too_many_arguments)]
    pub async fn grant_to_group(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        group: Uuid,
        role_key: &str,
        scope: Scope,
        expires_at: Option<OffsetDateTime>,
        reason: Option<&str>,
    ) -> Result<Uuid, AccessError> {
        if role_key == "workspace.owner" {
            return Err(AccessError::Invalid("the owner role is granted to people, not groups".into()));
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM groups WHERE id = $1)")
            .bind(group)
            .fetch_one(tx.conn())
            .await?;
        if !exists {
            return Err(AccessError::NotFound);
        }
        let (role_id, perms) = Self::role_by_key(tx, role_key).await?;
        self.ensure_no_escalation(tx, actor, &perms).await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO role_grants (workspace_id, id, role_id, group_id, scope_kind, scope_id, expires_at, granted_by, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tx.ctx.workspace_id).bind(id).bind(role_id).bind(group).bind(scope.kind()).bind(scope.id()).bind(expires_at).bind(actor).bind(reason)
            .execute(tx.conn()).await?;
        Self::bump(tx).await?;
        Ok(id)
    }

    pub async fn revoke_grant(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        id: Uuid,
    ) -> Result<(), AccessError> {
        let row: Option<(String, Vec<String>, String)> = sqlx::query_as(
            "SELECT r.key, r.permissions, g.scope_kind FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id WHERE g.id = $1 FOR UPDATE OF g",
        ).bind(id).fetch_optional(tx.conn()).await?;
        let (key, perms, kind) = row.ok_or(AccessError::NotFound)?;
        self.ensure_no_escalation(tx, actor, &perms).await?;
        if key == "workspace.owner" && kind == "workspace" {
            let others: i64 = sqlx::query_scalar(
                "SELECT count(DISTINCT g.membership_id) FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
                 JOIN memberships m ON m.workspace_id = g.workspace_id AND m.id = g.membership_id AND m.status = 'active'
                 WHERE r.key = 'workspace.owner' AND g.scope_kind = 'workspace' AND g.id <> $1 AND (g.expires_at IS NULL OR g.expires_at > now())",
            ).bind(id).fetch_one(tx.conn()).await?;
            if others == 0 {
                return Err(AccessError::LastOwner);
            }
        }
        sqlx::query("DELETE FROM role_grants WHERE id = $1").bind(id).execute(tx.conn()).await?;
        Self::bump(tx).await
    }

    /// Previews replacing a custom role's permissions: what changes and who is affected.
    pub async fn preview_role_change(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        permissions: &[String],
    ) -> Result<RolePreview, AccessError> {
        for p in permissions {
            if !is_permission(p) {
                return Err(AccessError::UnknownPermission(p.clone()));
            }
        }
        let old: Vec<String> = sqlx::query_scalar("SELECT permissions FROM roles WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(AccessError::NotFound)?;
        let old: BTreeSet<String> = old.into_iter().collect();
        let new: BTreeSet<String> = permissions.iter().cloned().collect();
        let affected_members: Vec<Uuid> = sqlx::query_scalar(
            "SELECT m.id FROM memberships m WHERE m.status = 'active' AND EXISTS (
                SELECT 1 FROM role_grants g WHERE g.role_id = $1 AND (g.expires_at IS NULL OR g.expires_at > now())
                  AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id)))
             ORDER BY 1",
        ).bind(id).fetch_all(tx.conn()).await?;
        Ok(RolePreview {
            added: new.difference(&old).cloned().collect(),
            removed: old.difference(&new).cloned().collect(),
            affected_members,
        })
    }

    pub async fn list_grants(
        &self,
        tx: &mut WsTx<'_>,
        membership: Option<Uuid>,
    ) -> Result<Vec<Grant>, AccessError> {
        type Row = (
            Uuid,
            String,
            Option<Uuid>,
            Option<Uuid>,
            String,
            Option<Uuid>,
            Option<OffsetDateTime>,
            Option<String>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT g.id, r.key, g.membership_id, g.group_id, g.scope_kind, g.scope_id, g.expires_at, g.reason FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
             WHERE ($1::uuid IS NULL OR g.membership_id = $1) ORDER BY g.created_at",
        ).bind(membership).fetch_all(tx.conn()).await?;
        rows.into_iter()
            .map(|r| {
                Ok(Grant {
                    id: r.0,
                    role_key: r.1,
                    membership_id: r.2,
                    group_id: r.3,
                    scope: Scope::parse(&r.4, r.5)?,
                    expires_at: r.6,
                    reason: r.7,
                })
            })
            .collect()
    }

    /// Bumps the revision explicitly (e.g. membership deactivated).
    pub async fn invalidate(&self, tx: &mut WsTx<'_>) -> Result<(), AccessError> {
        Self::bump(tx).await
    }

    async fn policy_set(&self, tx: &mut WsTx<'_>) -> Result<Arc<PolicySet>, AccessError> {
        let ws = tx.ctx.workspace_id;
        let rev: i64 = sqlx::query_scalar(
            "SELECT coalesce((SELECT revision FROM access_revisions WHERE workspace_id = $1), 0)",
        )
        .bind(ws)
        .fetch_one(tx.conn())
        .await?;
        if let Ok(c) = self.cache.read()
            && let Some((r, ps)) = c.get(&ws)
            && *r == rev
        {
            return Ok(ps.clone());
        }
        let rows: Vec<(Uuid, String, Vec<String>, String, Option<Uuid>)> = sqlx::query_as(
            "SELECT DISTINCT r.id, r.key, r.permissions, g.scope_kind, g.scope_id FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id",
        ).fetch_all(tx.conn()).await?;
        let mut set = PolicySet::new();
        let add = |set: &mut PolicySet, id: &str, src: &str| -> Result<(), AccessError> {
            let p = Policy::parse(Some(PolicyId::new(id)), src)
                .map_err(|e| AccessError::Internal(format!("policy {id}: {e}")))?;
            set.add(p).map_err(|e| AccessError::Internal(e.to_string()))
        };
        add(
            &mut set,
            "invariant:workspace-isolation",
            "forbid(principal, action, resource) unless { resource.workspace == principal.workspace };",
        )?;
        add(
            &mut set,
            "invariant:active-members-only",
            "forbid(principal, action, resource) unless { principal.status == \"active\" };",
        )?;
        for (role_id, key, perms, kind, sid) in rows {
            if perms.is_empty() {
                continue;
            }
            let scope = Scope::parse(&kind, sid)?;
            let scope_entity = match scope {
                Scope::Workspace => format!("Workspace::\"{ws}\""),
                s => format!("{}::\"{}\"", s.entity_type(), s.id().unwrap_or_default()),
            };
            let actions = perms.iter().map(|p| format!("Action::\"{p}\"")).collect::<Vec<_>>().join(", ");
            let src = format!(
                "permit(principal in Grant::\"{role_id}|{kind}|{}\", action in [{actions}], resource in {scope_entity});",
                sid.map(|s| s.to_string()).unwrap_or_default()
            );
            add(
                &mut set,
                &format!("grant:{key}@{kind}{}", sid.map(|s| format!(":{s}")).unwrap_or_default()),
                &src,
            )?;
        }
        let ps = Arc::new(set);
        if let Ok(mut c) = self.cache.write() {
            c.insert(ws, (rev, ps.clone()));
        }
        Ok(ps)
    }

    /// Authorizes `membership` to perform `action` on `resource` in the current workspace.
    pub async fn check(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        action: &str,
        resource: &Resource,
    ) -> Result<AccessDecision, AccessError> {
        if !is_permission(action) {
            return Err(AccessError::UnknownPermission(action.to_owned()));
        }
        let ws = tx.ctx.workspace_id;
        let policies = self.policy_set(tx).await?;
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM memberships WHERE id = $1")
            .bind(membership)
            .fetch_optional(tx.conn())
            .await?;
        let Some(status) = status else { return Ok(AccessDecision { allowed: false, reasons: vec![] }) };
        let grants: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
            &format!("SELECT g.role_id, g.scope_kind, g.scope_id FROM role_grants g WHERE {APPLIES_TO_MEMBER} AND (g.expires_at IS NULL OR g.expires_at > now())"),
        ).bind(membership).fetch_all(tx.conn()).await?;
        let ws_ref = entity_ref("Workspace", ws);
        let mut entities =
            vec![json!({"uid": ws_ref, "attrs": {"workspace": {"__entity": ws_ref}}, "parents": []})];
        let mut parents = vec![];
        for (role, kind, sid) in &grants {
            let g = entity_ref(
                "Grant",
                format!("{role}|{kind}|{}", sid.map(|s| s.to_string()).unwrap_or_default()),
            );
            entities.push(json!({"uid": g, "attrs": {}, "parents": []}));
            parents.push(g);
        }
        entities.push(json!({"uid": entity_ref("Member", membership), "attrs": {"workspace": {"__entity": ws_ref}, "status": status}, "parents": parents}));
        let resource_uid = match resource {
            Resource::Workspace => uid("Workspace", ws)?,
            Resource::Member(id) => {
                // The member sits inside its department's subtree, so department-scoped grants apply.
                let path: Option<Vec<Uuid>> = sqlx::query_scalar(
                    "SELECT d.path FROM memberships m JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id WHERE m.id = $1",
                ).bind(id).fetch_optional(tx.conn()).await?;
                let mut parent = ws_ref.clone();
                for dep in path.unwrap_or_default() {
                    let d = entity_ref("Department", dep);
                    entities.push(
                        json!({"uid": d, "attrs": {"workspace": {"__entity": ws_ref}}, "parents": [parent]}),
                    );
                    parent = d;
                }
                entities.push(json!({"uid": entity_ref("Target", id), "attrs": {"workspace": {"__entity": ws_ref}}, "parents": [parent]}));
                uid("Target", id)?
            }
            Resource::Role(id) => {
                entities.push(json!({"uid": entity_ref("Role", id), "attrs": {"workspace": {"__entity": ws_ref}}, "parents": [ws_ref]}));
                uid("Role", id)?
            }
            Resource::Scoped { scope, ancestors } => {
                let chain: Vec<Scope> = std::iter::once(*scope).chain(ancestors.iter().copied()).collect();
                for (i, s) in chain.iter().enumerate() {
                    let parent = chain
                        .get(i + 1)
                        .map(|p| entity_ref(p.entity_type(), p.id().unwrap_or_default()))
                        .unwrap_or_else(|| ws_ref.clone());
                    entities.push(json!({"uid": entity_ref(s.entity_type(), s.id().unwrap_or_default()), "attrs": {"workspace": {"__entity": ws_ref}}, "parents": [parent]}));
                }
                uid(scope.entity_type(), scope.id().unwrap_or_default())?
            }
        };
        let entities = Entities::from_json_value(Value::Array(entities), None)
            .map_err(|e| AccessError::Internal(e.to_string()))?;
        let request = Request::new(
            uid("Member", membership)?,
            uid("Action", action)?,
            resource_uid,
            Context::empty(),
            None,
        )
        .map_err(|e| AccessError::Internal(e.to_string()))?;
        let resp = Authorizer::new().is_authorized(&request, &policies, &entities);
        Ok(AccessDecision {
            allowed: resp.decision() == Decision::Allow,
            reasons: resp.diagnostics().reason().map(|id| id.to_string()).collect(),
        })
    }
}
