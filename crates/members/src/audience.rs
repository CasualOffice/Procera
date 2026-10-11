//! Audience targeting shared by campaigns and announcements (docs/20 §2.14 `campaign.audience.*`).
//! An audience is a union: everyone, or members of any listed department (including its
//! sub-departments), static group, role (granted directly or through a group), or location (the
//! directory `location` field, compared case-insensitively). Only active members are ever included.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use procera_db::WsTx;

use crate::MembersError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audience {
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub departments: Vec<Uuid>,
    #[serde(default)]
    pub groups: Vec<Uuid>,
    /// Role keys (built-in or custom).
    #[serde(default)]
    pub roles: Vec<String>,
    /// Directory locations, e.g. "Bangalore" (E2E-R1-09).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<String>,
}

impl Audience {
    fn locations_lower(&self) -> Vec<String> {
        self.locations.iter().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect()
    }

    pub fn is_empty(&self) -> bool {
        !self.all
            && self.departments.is_empty()
            && self.groups.is_empty()
            && self.roles.is_empty()
            && self.locations.iter().all(|l| l.trim().is_empty())
    }
}

/// `$1 departments uuid[]`, `$2 groups uuid[]`, `$3 roles text[]`, `$4 all bool`, `$5 locations
/// text[]` (lower-cased); `m` is memberships.
const MATCHES: &str = "m.status = 'active' AND (
    $4
    OR EXISTS (SELECT 1 FROM departments d WHERE d.id = m.department_id AND d.path && $1::uuid[])
    OR EXISTS (SELECT 1 FROM group_members gm WHERE gm.membership_id = m.id AND gm.group_id = ANY($2::uuid[]))
    OR EXISTS (SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
               WHERE r.key = ANY($3::text[]) AND (g.expires_at IS NULL OR g.expires_at > now())
                 AND (g.membership_id = m.id OR g.group_id IN (SELECT gm2.group_id FROM group_members gm2 WHERE gm2.membership_id = m.id)))
    OR lower(trim(m.location)) = ANY($5::text[]))";

/// Active members in the audience, ordered by id.
pub async fn resolve(tx: &mut WsTx<'_>, a: &Audience) -> Result<Vec<Uuid>, MembersError> {
    if a.is_empty() {
        return Ok(vec![]);
    }
    Ok(sqlx::query_scalar(&format!("SELECT m.id FROM memberships m WHERE {MATCHES} ORDER BY m.id"))
        .bind(&a.departments)
        .bind(&a.groups)
        .bind(&a.roles)
        .bind(a.all)
        .bind(a.locations_lower())
        .fetch_all(tx.conn())
        .await?)
}

/// Whether one membership is in the audience (e.g. a new joiner for a dynamic campaign).
pub async fn contains(tx: &mut WsTx<'_>, a: &Audience, membership: Uuid) -> Result<bool, MembersError> {
    if a.is_empty() {
        return Ok(false);
    }
    Ok(sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM memberships m WHERE m.id = $6 AND {MATCHES})"
    ))
    .bind(&a.departments)
    .bind(&a.groups)
    .bind(&a.roles)
    .bind(a.all)
    .bind(a.locations_lower())
    .bind(membership)
    .fetch_one(tx.conn())
    .await?)
}
