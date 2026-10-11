//! Department overrides (docs/20 §1 layer 4, BPMS-027): a value per department for keys the
//! catalogue marks department-scopable. A department inherits its ancestors' overrides; the nearest
//! wins. The platform floor still clamps the result.

use procera_db::WsTx;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::catalogue::{CATALOGUE, Def};
use crate::{
    DeptOverride, Effective, Settings, SettingsError, at_least_as_strict, def, resolve_scoped, validate,
};

/// Every stored layer of one workspace: what the cache holds (CFG-004). Resolution from a snapshot
/// is pure, so cached and uncached reads give the same answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Platform floors: key → value.
    pub floors: Vec<(String, Value)>,
    /// Workspace values: key, value, set by an admin (`false` = from the profile), revision.
    pub workspace: Vec<(String, Value, bool, i64)>,
    /// Department overrides: department, key, value, revision.
    pub departments: Vec<(Uuid, String, Value, i64)>,
}

/// One key resolved for a department, with that department's own override.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DepartmentEffective {
    #[serde(flatten)]
    pub effective: Effective,
    pub department_id: Uuid,
    /// This department's own override (`None` when the value is inherited or not overridden).
    pub override_value: Option<Value>,
    /// Revision of this department's override; the If-Match for its PUT/DELETE (0 = none).
    pub override_revision: i64,
    /// The ancestor department whose override applies, when it is not this department's own.
    pub inherited_from: Option<Uuid>,
}

impl Snapshot {
    pub async fn load(tx: &mut WsTx<'_>) -> Result<Self, SettingsError> {
        let floors = Settings::floors(tx).await?;
        let workspace: Vec<(String, Value, Option<Uuid>, i64)> =
            sqlx::query_as("SELECT key, value, updated_by, revision FROM workspace_settings")
                .fetch_all(tx.conn())
                .await?;
        let departments =
            sqlx::query_as("SELECT department_id, key, value, revision FROM department_settings")
                .fetch_all(tx.conn())
                .await?;
        Ok(Self {
            floors,
            workspace: workspace.into_iter().map(|(k, v, by, rev)| (k, v, by.is_some(), rev)).collect(),
            departments,
        })
    }

    fn floor(&self, key: &str) -> Option<&Value> {
        self.floors.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The override stored for exactly this department.
    pub fn override_of(&self, department: Uuid, key: &str) -> Option<(&Value, i64)> {
        self.departments.iter().find(|(d, k, ..)| *d == department && k == key).map(|(_, _, v, r)| (v, *r))
    }

    /// Resolves `d` for a department `path` (root first, target last; empty = workspace level).
    pub fn effective(&self, d: &'static Def, path: &[Uuid]) -> Effective {
        let stored = self
            .workspace
            .iter()
            .find(|(k, ..)| k == d.key)
            .map(|(_, v, admin, rev)| (v, if *admin { "workspace" } else { "profile" }, *rev));
        let overrides: Vec<DeptOverride<'_>> = path
            .iter()
            .filter_map(|dep| {
                self.override_of(*dep, d.key).map(|(value, _)| DeptOverride { department_id: *dep, value })
            })
            .collect();
        resolve_scoped(d, self.floor(d.key), stored, &overrides)
    }

    pub fn effective_all(&self, path: &[Uuid]) -> Vec<Effective> {
        CATALOGUE.iter().map(|d| self.effective(d, path)).collect()
    }

    /// Every key resolved for the department at the end of `path`.
    pub fn department_view(&self, path: &[Uuid]) -> Vec<DepartmentEffective> {
        let Some(&dept) = path.last() else { return Vec::new() };
        CATALOGUE.iter().map(|d| self.department_entry(d, path, dept)).collect()
    }

    pub fn department_entry(&self, d: &'static Def, path: &[Uuid], dept: Uuid) -> DepartmentEffective {
        let effective = self.effective(d, path);
        let own = self.override_of(dept, d.key);
        let inherited_from = effective
            .layers
            .iter()
            .rev()
            .find(|l| l.layer == "department")
            .and_then(|l| l.scope_id)
            .filter(|id| *id != dept);
        DepartmentEffective {
            effective,
            department_id: dept,
            override_value: own.map(|(v, _)| v.clone()),
            override_revision: own.map_or(0, |(_, r)| r),
            inherited_from,
        }
    }
}

/// The department's ancestor path (root first, itself last), or `UnknownDepartment`.
pub(crate) async fn department_path(tx: &mut WsTx<'_>, id: Uuid) -> Result<Vec<Uuid>, SettingsError> {
    let path: Option<Vec<Uuid>> = sqlx::query_scalar("SELECT path FROM departments WHERE id = $1")
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
    path.ok_or(SettingsError::UnknownDepartment)
}

impl Settings {
    /// Every key resolved for one department, read from the database.
    pub async fn department_effective(
        &self,
        tx: &mut WsTx<'_>,
        department: Uuid,
    ) -> Result<Vec<DepartmentEffective>, SettingsError> {
        let path = department_path(tx, department).await?;
        Ok(Snapshot::load(tx).await?.department_view(&path))
    }

    /// The department's ancestor path (root first, itself last).
    pub async fn department_path(
        &self,
        tx: &mut WsTx<'_>,
        department: Uuid,
    ) -> Result<Vec<Uuid>, SettingsError> {
        department_path(tx, department).await
    }

    /// Locks and returns this department's override revision (0 = none) after the If-Match check.
    async fn lock_override(
        tx: &mut WsTx<'_>,
        department: Uuid,
        key: &str,
        expected_revision: Option<i64>,
    ) -> Result<Option<(Value, i64)>, SettingsError> {
        let row: Option<(Value, i64)> = sqlx::query_as(
            "SELECT value, revision FROM department_settings WHERE department_id = $1 AND key = $2 FOR UPDATE",
        )
        .bind(department)
        .bind(key)
        .fetch_optional(tx.conn())
        .await?;
        let current = row.as_ref().map_or(0, |(_, r)| *r);
        if let Some(exp) = expected_revision
            && exp != current
        {
            return Err(SettingsError::StaleRevision { current });
        }
        Ok(row)
    }

    /// Sets a department override. Permission is checked by the caller (on the department scope);
    /// this enforces the catalogue (department-scopable keys only), the floor and If-Match.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_department(
        &self,
        tx: &mut WsTx<'_>,
        actor: (Uuid, Uuid),
        department: Uuid,
        key: &str,
        value: &Value,
        reason: Option<&str>,
        expected_revision: Option<i64>,
        client: (Option<String>, Option<String>),
    ) -> Result<DepartmentEffective, SettingsError> {
        let (user_id, subject_ref) = actor;
        let d = def(key)?;
        if d.perm.is_none() {
            return Err(SettingsError::Locked(key.to_owned()));
        }
        if !d.department {
            return Err(SettingsError::NotDepartmentScopable(key.to_owned()));
        }
        let path = department_path(tx, department).await?;
        let v = validate(d, value)?;
        if let Some(cmp) = d.floor {
            let floors = Settings::floors(tx).await?;
            if let Some((_, f)) = floors.iter().find(|(k, _)| k == key)
                && !at_least_as_strict(cmp, &v, f)
            {
                return Err(SettingsError::BelowFloor { key: key.to_owned(), floor: f.clone() });
            }
        }
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        let before = Self::lock_override(tx, department, key, expected_revision).await?;
        // Revisions keep counting across clear and set, so an old If-Match never matches again.
        let next: i64 = sqlx::query_scalar(
            "SELECT count(*) + 1 FROM settings_revisions WHERE layer = 'department' AND scope_id = $1 AND key = $2",
        )
        .bind(department)
        .bind(key)
        .fetch_one(tx.conn())
        .await?;
        sqlx::query(
            "INSERT INTO department_settings (workspace_id, department_id, key, value, revision, updated_by, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (workspace_id, department_id, key) DO UPDATE SET value = EXCLUDED.value, revision = EXCLUDED.revision, updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at",
        )
        .bind(tx.ctx.workspace_id).bind(department).bind(key).bind(&v).bind(next).bind(user_id).bind(self.clock.now())
        .execute(tx.conn()).await?;
        let before_value = before.map(|(v, _)| v);
        let rev = self
            .record_scoped(
                tx,
                ("department", Some(department)),
                key,
                before_value.as_ref(),
                &v,
                reason,
                Some(user_id),
            )
            .await?;
        let diff = json!({"key": key, "department_id": department, "before": before_value, "after": v});
        self.audit(tx, Some(subject_ref), "setting.override_set", rev, diff, reason, client).await?;
        Self::changed(tx, &[d.key], Some(department), false).await?;
        Ok(Snapshot::load(tx).await?.department_entry(d, &path, department))
    }

    /// Clears a department override; the department falls back to its ancestors or the workspace.
    #[allow(clippy::too_many_arguments)]
    pub async fn clear_department(
        &self,
        tx: &mut WsTx<'_>,
        actor: (Uuid, Uuid),
        department: Uuid,
        key: &str,
        reason: Option<&str>,
        expected_revision: Option<i64>,
        client: (Option<String>, Option<String>),
    ) -> Result<DepartmentEffective, SettingsError> {
        let (user_id, subject_ref) = actor;
        let d = def(key)?;
        let path = department_path(tx, department).await?;
        let Some((before, _)) = Self::lock_override(tx, department, key, expected_revision).await? else {
            return Err(SettingsError::NoOverride(key.to_owned()));
        };
        sqlx::query("DELETE FROM department_settings WHERE department_id = $1 AND key = $2")
            .bind(department)
            .bind(key)
            .execute(tx.conn())
            .await?;
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        let rev = self
            .record_scoped(
                tx,
                ("department", Some(department)),
                key,
                Some(&before),
                &Value::Null,
                reason,
                Some(user_id),
            )
            .await?;
        let diff = json!({"key": key, "department_id": department, "before": before, "after": null});
        self.audit(tx, Some(subject_ref), "setting.override_cleared", rev, diff, reason, client).await?;
        Self::changed(tx, &[d.key], Some(department), false).await?;
        Ok(Snapshot::load(tx).await?.department_entry(d, &path, department))
    }
}
