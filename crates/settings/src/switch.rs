//! Profile switch as plan/apply (docs/20 §0, CFG-017): preview which keys another configuration
//! profile would change (current, proposed and floor-clamped values), then apply exactly that plan.
//! Content is never touched.

use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::catalogue::{self, CATALOGUE, Def};
use crate::{Settings, SettingsError, Snapshot, at_least_as_strict};

const PROFILES: [&str; 4] = ["starter", "standard", "enterprise", "regulated"];

/// Keys any profile sets, in catalogue order. A switch resets each of them to the target profile's
/// value, or to the code default when the target profile leaves it unset.
pub fn profile_managed_keys() -> Vec<&'static Def> {
    let sets: Vec<Vec<(&str, Value)>> = PROFILES.iter().filter_map(|p| catalogue::profile(p)).collect();
    CATALOGUE.iter().filter(|d| sets.iter().any(|s| s.iter().any(|(k, _)| *k == d.key))).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanItem {
    pub key: &'static str,
    pub group: &'static str,
    /// Effective value now.
    pub current: Value,
    pub current_source: &'static str,
    /// Value the target profile asks for (or the code default).
    pub proposed: Value,
    /// Value stored and in effect after the switch: `proposed`, raised to the floor if looser.
    pub after: Value,
    pub floor: Option<Value>,
    /// `proposed` is looser than the platform floor, so `after` is the floor.
    pub clamped: bool,
    /// The current value was set by an admin (not by a profile); the switch replaces it.
    pub customized: bool,
    pub editable_with: Option<&'static str>,
    pub step_up: bool,
    /// Value type, so clients can format the values.
    pub kind: catalogue::Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfilePlan {
    pub from: String,
    pub to: String,
    /// Only the keys whose effective value would change.
    pub items: Vec<PlanItem>,
    /// Concurrency token for apply (`If-Match`): changes when any input to the plan changes.
    pub basis: String,
}

/// FNV-1a: a stable, dependency-free fingerprint (not a security control).
fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Pure planning over a snapshot.
pub fn plan(snapshot: &Snapshot, from: &str, to: &str) -> Result<ProfilePlan, SettingsError> {
    let target = catalogue::profile(to).ok_or_else(|| SettingsError::Invalid {
        key: "general.profile".into(),
        message: format!("unknown profile {to}"),
    })?;
    let mut items = Vec::new();
    let mut inputs = Vec::new();
    for d in profile_managed_keys() {
        let now = snapshot.effective(d, &[]);
        let floor = snapshot.floors.iter().find(|(k, _)| k == d.key).map(|(_, v)| v.clone());
        let proposed =
            target.iter().find(|(k, _)| *k == d.key).map_or_else(|| (d.default)(), |(_, v)| v.clone());
        let proposed = crate::validate(d, &proposed)?;
        let (after, clamped) = match (d.floor, &floor) {
            (Some(cmp), Some(f)) if !at_least_as_strict(cmp, &proposed, f) => (f.clone(), true),
            _ => (proposed.clone(), false),
        };
        inputs.push(json!([d.key, now.value, now.source, now.revision, floor]));
        if after != now.value {
            items.push(PlanItem {
                key: d.key,
                group: d.group,
                current: now.value,
                current_source: now.source,
                proposed,
                after,
                floor,
                clamped,
                customized: now.source == "workspace",
                editable_with: d.perm,
                step_up: d.step_up,
                kind: d.kind,
            });
        }
    }
    let basis = fingerprint(json!([from, to, inputs]).to_string().as_bytes());
    Ok(ProfilePlan { from: from.to_owned(), to: to.to_owned(), items, basis })
}

impl Settings {
    /// Plans a switch from `from` (the workspace's current profile) to `to`.
    pub async fn plan_profile_switch(
        &self,
        tx: &mut WsTx<'_>,
        from: &str,
        to: &str,
    ) -> Result<ProfilePlan, SettingsError> {
        plan(&Snapshot::load(tx).await?, from, to)
    }

    /// Applies the plan for `from → to` if it still matches `expected_basis`. Stored values are
    /// marked as profile values; each key gets a revision and the switch one audit event. Permission,
    /// step-up and recording the new profile on the workspace are the caller's.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_profile_switch(
        &self,
        tx: &mut WsTx<'_>,
        actor: (Uuid, Uuid),
        from: &str,
        to: &str,
        reason: &str,
        expected_basis: Option<&str>,
        client: (Option<String>, Option<String>),
    ) -> Result<ProfilePlan, SettingsError> {
        let (user_id, subject_ref) = actor;
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(SettingsError::ReasonRequired("general.profile".into()));
        }
        // Serialize concurrent switches and edits of the managed keys for the plan's lifetime.
        let keys: Vec<&str> = profile_managed_keys().iter().map(|d| d.key).collect();
        sqlx::query("SELECT key FROM workspace_settings WHERE key = ANY($1) FOR UPDATE")
            .bind(&keys)
            .fetch_all(tx.conn())
            .await?;
        let p = plan(&Snapshot::load(tx).await?, from, to)?;
        if let Some(exp) = expected_basis
            && exp != p.basis
        {
            return Err(SettingsError::PlanChanged { current: p.basis });
        }
        let reason_text = format!("profile {from} → {to}: {reason}");
        let mut changes = Vec::new();
        for i in &p.items {
            sqlx::query(
                "INSERT INTO workspace_settings (workspace_id, key, value, updated_by, updated_at) VALUES ($1, $2, $3, NULL, $4)
                 ON CONFLICT (workspace_id, key) DO UPDATE SET value = EXCLUDED.value, updated_by = NULL, updated_at = EXCLUDED.updated_at, revision = workspace_settings.revision + 1",
            )
            .bind(tx.ctx.workspace_id).bind(i.key).bind(&i.after).bind(self.clock.now()).execute(tx.conn()).await?;
            self.record(tx, "profile", i.key, Some(&i.current), &i.after, Some(&reason_text), Some(user_id))
                .await?;
            changes.push(json!({"key": i.key, "before": i.current, "after": i.after, "clamped": i.clamped}));
        }
        let diff = json!({"from": from, "to": to, "changes": changes});
        // The audit object is the workspace (one event for the whole switch).
        let ws = tx.ctx.workspace_id;
        self.audit_object(
            tx,
            Some(subject_ref),
            "settings.profile_switched",
            ("workspace", ws),
            diff,
            Some(reason),
            client,
        )
        .await?;
        let changed: Vec<&str> = p.items.iter().map(|i| i.key).collect();
        Self::changed(tx, &changed, None, false).await?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_keys_cover_every_profile_key() {
        let managed: Vec<&str> = profile_managed_keys().iter().map(|d| d.key).collect();
        for p in PROFILES {
            for (k, _) in catalogue::profile(p).unwrap_or_default() {
                assert!(managed.contains(&k), "{p}: {k}");
            }
        }
    }

    #[test]
    fn plan_lists_changes_and_clamps_to_the_floor() -> Result<(), SettingsError> {
        let mut s = Snapshot::default();
        // A floor stricter than Starter's 8 h idle timeout.
        s.floors.push(("auth.session.idle_timeout".into(), json!(3600)));
        let p = plan(&s, "standard", "starter")?;
        let idle = p
            .items
            .iter()
            .find(|i| i.key == "auth.session.idle_timeout")
            .ok_or(SettingsError::Internal("missing".into()))?;
        assert_eq!(
            (idle.proposed.clone(), idle.after.clone(), idle.clamped),
            (json!(28800), json!(3600), true)
        );
        assert!(p.items.iter().all(|i| i.after != i.current), "only changes are listed");
        // Planning against the target itself changes nothing once applied.
        let applied = Snapshot {
            workspace: p.items.iter().map(|i| (i.key.to_owned(), i.after.clone(), false, 1)).collect(),
            ..s.clone()
        };
        assert!(plan(&applied, "starter", "starter")?.items.is_empty());
        assert_ne!(plan(&applied, "starter", "starter")?.basis, p.basis);
        assert!(plan(&s, "standard", "nope").is_err());
        Ok(())
    }
}
