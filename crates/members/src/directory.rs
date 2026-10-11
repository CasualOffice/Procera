//! Directory (BPMS-023, FR-W02): department tree, static groups, positions, manager links, CSV
//! import and per-field source attribution.
//!
//! Manager resolution is deterministic: the explicit manager when active; otherwise the head of the
//! member's department, then of each ancestor department (nearest first), skipping the member
//! themselves and inactive heads; otherwise none. Manager and department cycles are rejected.

use std::collections::{BTreeMap, HashMap};

use procera_db::WsTx;
use procera_identity::ClientInfo;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{Actor, Members, MembersError};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Department {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    /// Ancestor ids, root first, ending with this department.
    pub path: Vec<Uuid>,
    pub head_membership_id: Option<Uuid>,
    pub member_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPatch {
    pub name: Option<String>,
    /// `Some(None)` moves the department to the root.
    #[serde(default, with = "double_option")]
    pub parent_id: Option<Option<Uuid>>,
    #[serde(default, with = "double_option")]
    pub head_membership_id: Option<Option<Uuid>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Position {
    pub id: Uuid,
    pub title: String,
    pub department_id: Option<Uuid>,
    pub holder_membership_id: Option<Uuid>,
}

/// Directory fields of a membership. `Some(None)` clears a field.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberPatch {
    #[serde(default, with = "double_option")]
    pub title: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub department_id: Option<Option<Uuid>>,
    #[serde(default, with = "double_option")]
    pub manager_membership_id: Option<Option<Uuid>>,
    #[serde(default, with = "double_option")]
    pub location: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedManager {
    pub membership_id: Option<Uuid>,
    /// `explicit`, `department_head` or `none`.
    pub via: &'static str,
    pub department_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportRow {
    pub line: usize,
    pub email: String,
    /// `update`, `unchanged` or `error`.
    pub outcome: &'static str,
    pub changes: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportReport {
    pub rows: Vec<ImportRow>,
    pub created_departments: Vec<String>,
    pub updated: usize,
    pub errors: usize,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

fn clean(name: &str, what: &str) -> Result<String, MembersError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 120 || n.contains('/') || n.chars().any(char::is_control) {
        return Err(MembersError::Invalid(format!("{what} must be 1-120 characters without '/'")));
    }
    Ok(n.to_owned())
}

fn conflict(e: sqlx::Error, msg: &str) -> MembersError {
    match e.as_database_error().and_then(|d| d.code()) {
        Some(c) if c == "23505" => MembersError::Conflict(msg.to_owned()),
        Some(c) if c == "23503" => {
            MembersError::Invalid("a referenced department, group or member does not exist".into())
        }
        _ => MembersError::Internal(e.to_string()),
    }
}

impl Members {
    /// Pre-checks a sibling-name clash (a failed INSERT would abort the caller's transaction).
    async fn ensure_sibling_free(
        tx: &mut WsTx<'_>,
        parent: Option<Uuid>,
        name: &str,
        except: Option<Uuid>,
    ) -> Result<(), MembersError> {
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM departments WHERE parent_id IS NOT DISTINCT FROM $1 AND lower(name) = lower($2) AND id IS DISTINCT FROM $3)")
            .bind(parent).bind(name).bind(except).fetch_one(tx.conn()).await?;
        if taken {
            Err(MembersError::Conflict("a sibling department already has this name".into()))
        } else {
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn dir_audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        action: &str,
        kind: &str,
        id: Uuid,
        diff: Value,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        self.audit_object(tx, Some(actor.subject_ref), action, kind, id, diff, client).await
    }

    async fn active_member(tx: &mut WsTx<'_>, id: Uuid) -> Result<(), MembersError> {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if ok { Ok(()) } else { Err(MembersError::Invalid(format!("{id} is not an active member"))) }
    }

    // ---------- departments ----------

    pub async fn list_departments(&self, tx: &mut WsTx<'_>) -> Result<Vec<Department>, MembersError> {
        let rows: Vec<(Uuid, Option<Uuid>, String, Vec<Uuid>, Option<Uuid>, i64)> = sqlx::query_as(
            "SELECT d.id, d.parent_id, d.name, d.path, d.head_membership_id,
                    (SELECT count(*) FROM memberships m WHERE m.department_id = d.id AND m.status = 'active')
             FROM departments d ORDER BY array_length(d.path, 1), lower(d.name)",
        )
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, parent_id, name, path, head_membership_id, member_count)| Department {
                id,
                parent_id,
                name,
                path,
                head_membership_id,
                member_count,
            })
            .collect())
    }

    async fn department_path(tx: &mut WsTx<'_>, id: Uuid) -> Result<Vec<Uuid>, MembersError> {
        sqlx::query_scalar("SELECT path FROM departments WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(MembersError::NotFound)
    }

    pub async fn create_department(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        name: &str,
        parent_id: Option<Uuid>,
        client: &ClientInfo,
    ) -> Result<Department, MembersError> {
        let d = self.insert_department(tx, name, parent_id).await?;
        self.dir_audit(
            tx,
            actor,
            "department.created",
            "department",
            d.id,
            json!({"name": d.name, "parent_id": parent_id}),
            client,
        )
        .await?;
        Ok(d)
    }

    /// Inserts without auditing (CSV import records created departments in its own event).
    async fn insert_department(
        &self,
        tx: &mut WsTx<'_>,
        name: &str,
        parent_id: Option<Uuid>,
    ) -> Result<Department, MembersError> {
        let name = clean(name, "department name")?;
        Self::ensure_sibling_free(tx, parent_id, &name, None).await?;
        let id = self.ids.new_id();
        let mut path = match parent_id {
            Some(p) => Self::department_path(tx, p)
                .await
                .map_err(|_| MembersError::Invalid("parent department does not exist".into()))?,
            None => vec![],
        };
        path.push(id);
        sqlx::query(
            "INSERT INTO departments (workspace_id, id, parent_id, name, path) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(parent_id)
        .bind(&name)
        .bind(&path)
        .execute(tx.conn())
        .await
        .map_err(|e| conflict(e, "a sibling department already has this name"))?;
        Ok(Department { id, parent_id, name, path, head_membership_id: None, member_count: 0 })
    }

    pub async fn update_department(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        id: Uuid,
        patch: &DepartmentPatch,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let old_path = Self::department_path(tx, id).await?;
        if let Some(name) = &patch.name {
            let name = clean(name, "department name")?;
            let parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM departments WHERE id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
            Self::ensure_sibling_free(tx, parent, &name, Some(id)).await?;
            sqlx::query("UPDATE departments SET name = $2 WHERE id = $1")
                .bind(id)
                .bind(&name)
                .execute(tx.conn())
                .await
                .map_err(|e| conflict(e, "a sibling department already has this name"))?;
        }
        if let Some(head) = patch.head_membership_id {
            if let Some(h) = head {
                Self::active_member(tx, h).await?;
            }
            sqlx::query("UPDATE departments SET head_membership_id = $2 WHERE id = $1")
                .bind(id)
                .bind(head)
                .execute(tx.conn())
                .await?;
        }
        if let Some(parent) = patch.parent_id {
            let current: String = sqlx::query_scalar("SELECT name FROM departments WHERE id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
            let mut new_path = match parent {
                Some(p) => {
                    let pp = Self::department_path(tx, p)
                        .await
                        .map_err(|_| MembersError::Invalid("parent department does not exist".into()))?;
                    if pp.contains(&id) {
                        return Err(MembersError::Cycle(
                            "a department cannot move under itself or its descendants".into(),
                        ));
                    }
                    pp
                }
                None => vec![],
            };
            Self::ensure_sibling_free(tx, parent, &current, Some(id)).await?;
            new_path.push(id);
            // Rewrite this department's and every descendant's path prefix.
            sqlx::query(
                "UPDATE departments SET path = $2::uuid[] || path[$3 + 1:] WHERE path @> ARRAY[$1]::uuid[]",
            )
            .bind(id)
            .bind(&new_path)
            .bind(i32::try_from(old_path.len()).unwrap_or(i32::MAX))
            .execute(tx.conn())
            .await?;
            sqlx::query("UPDATE departments SET parent_id = $2 WHERE id = $1")
                .bind(id)
                .bind(parent)
                .execute(tx.conn())
                .await
                .map_err(|e| conflict(e, "a sibling department already has this name"))?;
        }
        self.dir_audit(tx, actor, "department.updated", "department", id, json!({"name": patch.name, "parent_id": patch.parent_id, "head_membership_id": patch.head_membership_id}), client).await
    }

    pub async fn delete_department(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        Self::department_path(tx, id).await?;
        let (children, members, positions): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM departments WHERE parent_id = $1), (SELECT count(*) FROM memberships WHERE department_id = $1), (SELECT count(*) FROM positions WHERE department_id = $1)",
        ).bind(id).fetch_one(tx.conn()).await?;
        if children + members + positions > 0 {
            return Err(MembersError::Conflict(format!(
                "department is not empty ({children} sub-departments, {members} members, {positions} positions)"
            )));
        }
        sqlx::query("DELETE FROM departments WHERE id = $1").bind(id).execute(tx.conn()).await?;
        self.dir_audit(tx, actor, "department.deleted", "department", id, json!({}), client).await
    }

    // ---------- groups ----------

    pub async fn list_groups(&self, tx: &mut WsTx<'_>) -> Result<Vec<Group>, MembersError> {
        let rows: Vec<(Uuid, String, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT g.id, g.name, g.description, g.kind, (SELECT count(*) FROM group_members gm JOIN memberships m ON m.workspace_id = gm.workspace_id AND m.id = gm.membership_id AND m.status = 'active' WHERE gm.group_id = g.id)
             FROM groups g ORDER BY lower(g.name)",
        ).fetch_all(tx.conn()).await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, description, kind, member_count)| Group {
                id,
                name,
                description,
                kind,
                member_count,
            })
            .collect())
    }

    pub async fn create_group(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        name: &str,
        description: Option<&str>,
        client: &ClientInfo,
    ) -> Result<Group, MembersError> {
        let name = clean(name, "group name")?;
        let taken: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM groups WHERE lower(name) = lower($1))")
                .bind(&name)
                .fetch_one(tx.conn())
                .await?;
        if taken {
            return Err(MembersError::Conflict("a group with this name already exists".into()));
        }
        let description = description
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(|d| d.chars().take(500).collect::<String>());
        let id = self.ids.new_id();
        sqlx::query("INSERT INTO groups (workspace_id, id, name, description) VALUES ($1, $2, $3, $4)")
            .bind(tx.ctx.workspace_id)
            .bind(id)
            .bind(&name)
            .bind(&description)
            .execute(tx.conn())
            .await
            .map_err(|e| conflict(e, "a group with this name already exists"))?;
        self.dir_audit(tx, actor, "group.created", "group", id, json!({"name": name}), client).await?;
        Ok(Group { id, name, description, kind: "static".into(), member_count: 0 })
    }

    pub async fn delete_group(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let n = sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(id)
            .execute(tx.conn())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(MembersError::NotFound);
        }
        self.dir_audit(tx, actor, "group.deleted", "group", id, json!({}), client).await
    }

    pub async fn group_members(&self, tx: &mut WsTx<'_>, group: Uuid) -> Result<Vec<Uuid>, MembersError> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM groups WHERE id = $1)")
            .bind(group)
            .fetch_one(tx.conn())
            .await?;
        if !exists {
            return Err(MembersError::NotFound);
        }
        Ok(sqlx::query_scalar(
            "SELECT membership_id FROM group_members WHERE group_id = $1 ORDER BY added_at",
        )
        .bind(group)
        .fetch_all(tx.conn())
        .await?)
    }

    pub async fn add_group_member(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        group: Uuid,
        membership: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        self.group_members(tx, group).await?;
        Self::active_member(tx, membership).await?;
        let n = sqlx::query("INSERT INTO group_members (workspace_id, group_id, membership_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(group).bind(membership).execute(tx.conn()).await?.rows_affected();
        if n == 0 {
            return Err(MembersError::Conflict("already in the group".into()));
        }
        self.dir_audit(
            tx,
            actor,
            "group.member_added",
            "group",
            group,
            json!({"membership_id": membership}),
            client,
        )
        .await
    }

    pub async fn remove_group_member(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        group: Uuid,
        membership: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let n = sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND membership_id = $2")
            .bind(group)
            .bind(membership)
            .execute(tx.conn())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(MembersError::NotFound);
        }
        self.dir_audit(
            tx,
            actor,
            "group.member_removed",
            "group",
            group,
            json!({"membership_id": membership}),
            client,
        )
        .await
    }

    // ---------- positions ----------

    pub async fn list_positions(&self, tx: &mut WsTx<'_>) -> Result<Vec<Position>, MembersError> {
        let rows: Vec<(Uuid, String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
            "SELECT id, title, department_id, holder_membership_id FROM positions ORDER BY lower(title)",
        )
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, title, department_id, holder_membership_id)| Position {
                id,
                title,
                department_id,
                holder_membership_id,
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_position(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        title: &str,
        department_id: Option<Uuid>,
        holder: Option<Uuid>,
        client: &ClientInfo,
    ) -> Result<Position, MembersError> {
        let title = clean(title, "position title")?;
        if let Some(d) = department_id {
            Self::department_path(tx, d)
                .await
                .map_err(|_| MembersError::Invalid("department does not exist".into()))?;
        }
        if let Some(h) = holder {
            Self::active_member(tx, h).await?;
        }
        let id = self.ids.new_id();
        sqlx::query("INSERT INTO positions (workspace_id, id, title, department_id, holder_membership_id) VALUES ($1, $2, $3, $4, $5)")
            .bind(tx.ctx.workspace_id).bind(id).bind(&title).bind(department_id).bind(holder)
            .execute(tx.conn()).await.map_err(|e| conflict(e, "position conflict"))?;
        self.dir_audit(
            tx,
            actor,
            "position.created",
            "position",
            id,
            json!({"title": title, "department_id": department_id, "holder_membership_id": holder}),
            client,
        )
        .await?;
        Ok(Position { id, title, department_id, holder_membership_id: holder })
    }

    pub async fn delete_position(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(), MembersError> {
        let n = sqlx::query("DELETE FROM positions WHERE id = $1")
            .bind(id)
            .execute(tx.conn())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(MembersError::NotFound);
        }
        self.dir_audit(tx, actor, "position.deleted", "position", id, json!({}), client).await
    }

    // ---------- member directory fields ----------

    /// Rejects `manager` if following manager links from it reaches `membership`.
    async fn ensure_no_manager_cycle(
        tx: &mut WsTx<'_>,
        membership: Uuid,
        manager: Uuid,
    ) -> Result<(), MembersError> {
        if manager == membership {
            return Err(MembersError::Cycle("a member cannot manage themselves".into()));
        }
        let cycle: bool = sqlx::query_scalar(
            "WITH RECURSIVE chain(id, depth) AS (
                SELECT manager_membership_id, 1 FROM memberships WHERE id = $1
                UNION ALL
                SELECT m.manager_membership_id, c.depth + 1 FROM memberships m JOIN chain c ON m.id = c.id WHERE c.depth < 1000
             ) SELECT EXISTS (SELECT 1 FROM chain WHERE id = $2)",
        ).bind(manager).bind(membership).fetch_one(tx.conn()).await?;
        if cycle {
            Err(MembersError::Cycle("this manager link would create a reporting cycle".into()))
        } else {
            Ok(())
        }
    }

    /// Applies directory fields with source attribution; returns the names of the fields changed.
    async fn apply_member_patch(
        tx: &mut WsTx<'_>,
        membership: Uuid,
        patch: &MemberPatch,
        source: &str,
    ) -> Result<Vec<String>, MembersError> {
        let current: Option<(Option<String>, Option<Uuid>, Option<Uuid>, Option<String>, String)> =
            sqlx::query_as("SELECT title, department_id, manager_membership_id, location, status FROM memberships WHERE id = $1").bind(membership).fetch_optional(tx.conn()).await?;
        let (title, dept, manager, location, status) = current.ok_or(MembersError::NotFound)?;
        if status != "active" {
            return Err(MembersError::NotFound);
        }
        let norm = |v: &Option<String>| {
            v.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(120).collect::<String>())
        };
        let mut changed = vec![];
        let new_title = patch.title.as_ref().map(norm).unwrap_or(title.clone());
        if new_title != title {
            changed.push("title");
        }
        let new_location = patch.location.as_ref().map(norm).unwrap_or(location.clone());
        if new_location != location {
            changed.push("location");
        }
        let new_dept = patch.department_id.unwrap_or(dept);
        if new_dept != dept {
            if let Some(d) = new_dept {
                Self::department_path(tx, d)
                    .await
                    .map_err(|_| MembersError::Invalid("department does not exist".into()))?;
            }
            changed.push("department");
        }
        let new_manager = patch.manager_membership_id.unwrap_or(manager);
        if new_manager != manager {
            if let Some(m) = new_manager {
                Self::active_member(tx, m).await?;
                Self::ensure_no_manager_cycle(tx, membership, m).await?;
            }
            changed.push("manager");
        }
        if changed.is_empty() {
            return Ok(vec![]);
        }
        let sources: Value =
            json!(changed.iter().map(|f| (f.to_string(), json!(source))).collect::<BTreeMap<_, _>>());
        sqlx::query("UPDATE memberships SET title = $2, department_id = $3, manager_membership_id = $4, location = $5, field_sources = field_sources || $6 WHERE id = $1")
            .bind(membership).bind(&new_title).bind(new_dept).bind(new_manager).bind(&new_location).bind(sources)
            .execute(tx.conn()).await?;
        Ok(changed.into_iter().map(str::to_owned).collect())
    }

    pub async fn update_member_directory(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        membership: Uuid,
        patch: &MemberPatch,
        client: &ClientInfo,
    ) -> Result<Vec<String>, MembersError> {
        let changed = Self::apply_member_patch(tx, membership, patch, "manual").await?;
        if !changed.is_empty() {
            self.dir_audit(
                tx,
                actor,
                "member.directory_updated",
                "membership",
                membership,
                json!({"fields": changed, "source": "manual"}),
                client,
            )
            .await?;
        }
        Ok(changed)
    }

    /// Directory fields and their sources for one member.
    pub async fn member_directory(&self, tx: &mut WsTx<'_>, membership: Uuid) -> Result<Value, MembersError> {
        let row: Option<(Option<String>, Option<Uuid>, Option<Uuid>, Option<String>, Value)> =
            sqlx::query_as("SELECT title, department_id, manager_membership_id, location, field_sources FROM memberships WHERE id = $1").bind(membership).fetch_optional(tx.conn()).await?;
        let (title, department_id, manager_membership_id, location, sources) =
            row.ok_or(MembersError::NotFound)?;
        let manager = self.resolve_manager(tx, membership).await?;
        let group_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT gm.group_id FROM group_members gm JOIN groups g ON g.workspace_id = gm.workspace_id AND g.id = gm.group_id
             WHERE gm.membership_id = $1 ORDER BY lower(g.name)",
        )
        .bind(membership)
        .fetch_all(tx.conn())
        .await?;
        Ok(
            json!({"membership_id": membership, "title": title, "department_id": department_id, "manager_membership_id": manager_membership_id, "location": location, "field_sources": sources, "resolved_manager": manager, "group_ids": group_ids}),
        )
    }

    /// Deterministic manager resolution (see the module docs).
    pub async fn resolve_manager(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
    ) -> Result<ResolvedManager, MembersError> {
        let row: Option<(Option<Uuid>, Option<Uuid>)> =
            sqlx::query_as("SELECT manager_membership_id, department_id FROM memberships WHERE id = $1")
                .bind(membership)
                .fetch_optional(tx.conn())
                .await?;
        let (explicit, dept) = row.ok_or(MembersError::NotFound)?;
        if let Some(m) = explicit {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
            )
            .bind(m)
            .fetch_one(tx.conn())
            .await?;
            if active {
                return Ok(ResolvedManager { membership_id: Some(m), via: "explicit", department_id: None });
            }
        }
        if let Some(d) = dept {
            let path = Self::department_path(tx, d).await?;
            for dep in path.iter().rev() {
                let head: Option<Uuid> = sqlx::query_scalar(
                    "SELECT d.head_membership_id FROM departments d JOIN memberships m ON m.workspace_id = d.workspace_id AND m.id = d.head_membership_id AND m.status = 'active' WHERE d.id = $1",
                ).bind(dep).fetch_optional(tx.conn()).await?.flatten();
                if let Some(h) = head.filter(|h| *h != membership) {
                    return Ok(ResolvedManager {
                        membership_id: Some(h),
                        via: "department_head",
                        department_id: Some(*dep),
                    });
                }
            }
        }
        Ok(ResolvedManager { membership_id: None, via: "none", department_id: None })
    }

    // ---------- CSV import ----------

    /// Imports directory fields from CSV with columns `email` (required) and any of `department`
    /// (path like `Operations/Support`, created when missing), `title`, `manager_email`, `location`.
    /// Rows for unknown or inactive members are reported, not invited. Applies to `tx`; the caller
    /// commits (apply) or rolls back (plan). Empty cells leave a field unchanged.
    pub async fn import_csv(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        csv_text: &str,
        client: &ClientInfo,
    ) -> Result<ImportReport, MembersError> {
        let mut rdr =
            csv::ReaderBuilder::new().trim(csv::Trim::All).flexible(false).from_reader(csv_text.as_bytes());
        let headers: Vec<String> = rdr
            .headers()
            .map_err(|e| MembersError::Invalid(format!("CSV header: {e}")))?
            .iter()
            .map(|h| h.to_lowercase())
            .collect();
        let col = |name: &str| headers.iter().position(|h| h == name);
        let Some(email_col) = col("email") else {
            return Err(MembersError::Invalid("CSV needs an 'email' column".into()));
        };
        for h in &headers {
            if !["email", "department", "title", "manager_email", "location"].contains(&h.as_str()) {
                return Err(MembersError::Invalid(format!(
                    "unknown CSV column '{h}' (allowed: email, department, title, manager_email, location)"
                )));
            }
        }
        let (dept_col, title_col, mgr_col, loc_col) =
            (col("department"), col("title"), col("manager_email"), col("location"));
        let records: Vec<(usize, csv::StringRecord)> = rdr
            .records()
            .enumerate()
            .map(|(i, r)| {
                r.map(|r| (i + 2, r)).map_err(|e| MembersError::Invalid(format!("CSV line {}: {e}", i + 2)))
            })
            .collect::<Result<_, _>>()?;
        if records.len() > 10_000 {
            return Err(MembersError::Invalid("at most 10 000 rows per import".into()));
        }
        let members: HashMap<String, Uuid> = sqlx::query_as::<_, (String, Uuid)>("SELECT lower(u.email::text), m.id FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.status = 'active'")
            .fetch_all(tx.conn()).await?.into_iter().collect();
        let mut dept_cache: HashMap<String, Uuid> = HashMap::new();
        let mut created = vec![];
        let mut rows = vec![];
        // Pass 1: departments, titles, locations. Pass 2: managers (they may appear later in the file).
        let mut pending_managers = vec![];
        for (line, r) in &records {
            let get =
                |c: Option<usize>| c.and_then(|c| r.get(c)).map(str::to_owned).filter(|s| !s.is_empty());
            let email = r.get(email_col).unwrap_or_default().to_lowercase();
            let Some(&membership) = members.get(&email) else {
                rows.push(ImportRow {
                    line: *line,
                    email,
                    outcome: "error",
                    changes: vec![],
                    error: Some("not an active member; invite them first".into()),
                });
                continue;
            };
            let department_id = match get(dept_col) {
                Some(path) => {
                    match self.ensure_department_path(tx, &path, &mut dept_cache, &mut created).await {
                        Ok(id) => Some(Some(id)),
                        Err(e) => {
                            rows.push(ImportRow {
                                line: *line,
                                email,
                                outcome: "error",
                                changes: vec![],
                                error: Some(e.to_string()),
                            });
                            continue;
                        }
                    }
                }
                None => None,
            };
            let patch = MemberPatch {
                title: get(title_col).map(Some),
                department_id,
                manager_membership_id: None,
                location: get(loc_col).map(Some),
            };
            let changes = Self::apply_member_patch(tx, membership, &patch, "csv").await?;
            if let Some(m) = get(mgr_col) {
                pending_managers.push((rows.len(), membership, m.to_lowercase()));
            }
            rows.push(ImportRow {
                line: *line,
                email,
                outcome: if changes.is_empty() { "unchanged" } else { "update" },
                changes,
                error: None,
            });
        }
        for (idx, membership, manager_email) in pending_managers {
            let row = &mut rows[idx];
            let Some(&manager) = members.get(&manager_email) else {
                row.outcome = "error";
                row.error = Some(format!("manager {manager_email} is not an active member"));
                continue;
            };
            let patch = MemberPatch { manager_membership_id: Some(Some(manager)), ..MemberPatch::default() };
            match Self::apply_member_patch(tx, membership, &patch, "csv").await {
                Ok(c) => {
                    if !c.is_empty() {
                        row.outcome = "update";
                        row.changes.extend(c);
                    }
                }
                Err(e @ (MembersError::Cycle(_) | MembersError::Invalid(_))) => {
                    row.outcome = "error";
                    row.error = Some(e.to_string());
                }
                Err(e) => return Err(e),
            }
        }
        let updated = rows.iter().filter(|r| r.outcome == "update").count();
        let errors = rows.iter().filter(|r| r.outcome == "error").count();
        self.dir_audit(
            tx,
            actor,
            "member.directory_imported",
            "membership",
            actor.membership,
            json!({"rows": rows.len(), "updated": updated, "errors": errors, "created_departments": created}),
            client,
        )
        .await?;
        Ok(ImportReport { rows, created_departments: created, updated, errors })
    }

    async fn ensure_department_path(
        &self,
        tx: &mut WsTx<'_>,
        path: &str,
        cache: &mut HashMap<String, Uuid>,
        created: &mut Vec<String>,
    ) -> Result<Uuid, MembersError> {
        let mut parent: Option<Uuid> = None;
        let mut so_far = String::new();
        for part in path.split('/').map(str::trim) {
            let name = clean(part, "department name")?;
            so_far = if so_far.is_empty() { name.clone() } else { format!("{so_far}/{name}") };
            let key = so_far.to_lowercase();
            if let Some(id) = cache.get(&key) {
                parent = Some(*id);
                continue;
            }
            let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM departments WHERE parent_id IS NOT DISTINCT FROM $1 AND lower(name) = lower($2)")
                .bind(parent).bind(&name).fetch_optional(tx.conn()).await?;
            let id = match existing {
                Some(id) => id,
                None => {
                    created.push(so_far.clone());
                    self.insert_department(tx, &name, parent).await?.id
                }
            };
            cache.insert(key, id);
            parent = Some(id);
        }
        parent.ok_or_else(|| MembersError::Invalid("empty department path".into()))
    }
}
