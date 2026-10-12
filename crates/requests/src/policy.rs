//! The policy or SOP that governs a request type (BPMS-103, slice 3): `requests.types.policy` maps a
//! type to a Library artifact (a policy or SOP). The request page and the decision panel show its
//! current published version ("Policy: Travel and expenses (v1.2)"), and a decision records the
//! exact published version the decider was shown, so the audit can say which version they saw.

use procera_db::WsTx;
use serde::Serialize;
use uuid::Uuid;

use crate::forms::FieldError;
use crate::{RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// A published policy or SOP version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyRef {
    pub artifact_id: Uuid,
    pub version_id: Uuid,
    /// `policy` or `sop`.
    pub kind: String,
    pub title: String,
    /// `major.minor` ("1.2").
    pub version: String,
}

type Row = (Uuid, Uuid, String, String, Option<String>, i32);

const SELECT: &str = "SELECT a.id, v.id, a.kind, a.title, v.semver, v.version_no
  FROM artifacts a JOIN artifact_versions v ON v.workspace_id = a.workspace_id AND v.artifact_id = a.id
 WHERE a.kind IN ('policy', 'sop')";

fn shaped(r: Row) -> PolicyRef {
    PolicyRef {
        artifact_id: r.0,
        version_id: r.1,
        kind: r.2,
        title: r.3,
        version: r.4.unwrap_or_else(|| format!("{}.0", r.5)),
    }
}

impl Requests {
    /// The artifact the workspace links to `key`, if any.
    async fn policy_artifact(&self, tx: &mut WsTx<'_>, key: &str) -> R<Option<Uuid>> {
        let map = procera_settings::Settings::new(self.clock.clone(), self.ids.clone())
            .effective(tx, "requests.types.policy")
            .await?
            .value;
        Ok(map.get(key).and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()))
    }

    /// The current published version of the policy linked to `key` (none when nothing is linked or
    /// the document has no published version).
    pub async fn policy_for(&self, tx: &mut WsTx<'_>, key: &str) -> R<Option<PolicyRef>> {
        let Some(artifact) = self.policy_artifact(tx, key).await? else { return Ok(None) };
        let row: Option<Row> =
            sqlx::query_as(&format!("{SELECT} AND a.id = $1 AND v.id = a.current_published_version_id"))
                .bind(artifact)
                .fetch_optional(tx.conn())
                .await?;
        Ok(row.map(shaped))
    }

    /// The policy version a decision records: `shown` when given (it must be a version of the
    /// linked document that was published), else the current published one.
    pub(crate) async fn shown_policy(
        &self,
        tx: &mut WsTx<'_>,
        key: &str,
        shown: Option<Uuid>,
    ) -> R<Option<PolicyRef>> {
        let Some(version) = shown else { return self.policy_for(tx, key).await };
        let bad = || {
            RequestError::Invalid(vec![FieldError::new(
                "policy_version",
                "unknown_policy_version",
                "This is not a published version of the policy linked to this request type. Reload the page.",
            )])
        };
        let artifact = self.policy_artifact(tx, key).await?.ok_or_else(bad)?;
        let row: Option<Row> =
            sqlx::query_as(&format!("{SELECT} AND a.id = $1 AND v.id = $2 AND v.published_at IS NOT NULL"))
                .bind(artifact)
                .bind(version)
                .fetch_optional(tx.conn())
                .await?;
        row.map(shaped).map(Some).ok_or_else(bad)
    }
}
