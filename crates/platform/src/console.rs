//! System console reads and platform quota defaults (BPMS-065, FR-S04, S08; docs/05 §3.20).
//!
//! Everything here is metadata: per-workspace counts and timestamps come from SECURITY DEFINER
//! discovery functions (migration 0022) that never return workspace rows, so a system admin sees
//! how big and how busy a workspace is without reading what is in it (docs/12 §1).

use std::collections::HashMap;

use procera_db::WsTx;
use procera_identity::ClientInfo;
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::service::{Platform, PlatformError, Quotas, Workspace, WorkspaceFilter};

pub const PROFILES: &[&str] = &["starter", "standard", "enterprise", "regulated"];

/// How much of a workspace is in use. `storage_bytes` stays `None` until files ship (BPMS-050).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Usage {
    pub members: i64,
    pub invited: i64,
    pub storage_bytes: Option<i64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_activity_at: Option<OffsetDateTime>,
}

/// A workspace as the console lists it: lifecycle metadata, quotas and usage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConsoleWorkspace {
    #[serde(flatten)]
    pub workspace: Workspace,
    pub usage: Usage,
}

/// The quota bundle a new workspace of `profile` starts with.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuotaDefaults {
    pub profile: String,
    #[serde(flatten)]
    pub quotas: Quotas,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Outbox and mail queue totals (FR-S08).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Queues {
    pub outbox_pending: i64,
    pub outbox_retrying: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub outbox_oldest_at: Option<OffsetDateTime>,
    pub mail_queued: i64,
    pub mail_failed: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub mail_oldest_at: Option<OffsetDateTime>,
}

/// Applied schema migrations, for the health page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Migrations {
    pub applied: i64,
    pub latest: Option<i64>,
}

type DefaultsRow =
    (String, Option<i32>, Option<i32>, Option<i64>, Option<i32>, Option<i64>, i64, OffsetDateTime);

const SELECT_DEFAULTS: &str = "SELECT profile, max_members, max_active_instances, max_storage_bytes, api_rpm, ai_tokens_month, revision, updated_at FROM platform_quota_defaults";

fn to_defaults(r: DefaultsRow) -> QuotaDefaults {
    QuotaDefaults {
        profile: r.0,
        quotas: Quotas {
            max_members: r.1,
            max_active_instances: r.2,
            max_storage_bytes: r.3,
            api_rpm: r.4,
            ai_tokens_month: r.5,
        },
        revision: r.6,
        updated_at: r.7,
    }
}

/// The platform default for `profile`; the built-in bundle if the row is missing.
pub(crate) async fn profile_defaults(tx: &mut WsTx<'_>, profile: &str) -> Result<Quotas, PlatformError> {
    let row: Option<DefaultsRow> = sqlx::query_as(&format!("{SELECT_DEFAULTS} WHERE profile = $1"))
        .bind(profile)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|r| to_defaults(r).quotas).unwrap_or_else(|| Quotas::for_profile(profile)))
}

fn validate(q: &Quotas) -> Result<(), PlatformError> {
    let positive = [
        q.max_members.map(i64::from),
        q.max_active_instances.map(i64::from),
        q.max_storage_bytes,
        q.api_rpm.map(i64::from),
    ];
    if positive.iter().flatten().any(|v| *v <= 0) || q.ai_tokens_month.is_some_and(|v| v < 0) {
        return Err(PlatformError::Invalid("limits must be positive, or null for unlimited".into()));
    }
    Ok(())
}

impl Platform {
    /// Usage of every listed workspace (or one), keyed by workspace id.
    pub async fn usage(&self, only: Option<Uuid>) -> Result<HashMap<Uuid, Usage>, PlatformError> {
        let rows: Vec<(Uuid, i64, i64, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT workspace_id, members, invited, last_activity_at FROM discovery.workspace_usage($1, $2)",
        )
        .bind(self.clock.now())
        .bind(only)
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, members, invited, last)| {
                (id, Usage { members, invited, storage_bytes: None, last_activity_at: last })
            })
            .collect())
    }

    /// The console's workspace grid: filtered workspaces with their usage.
    pub async fn console_workspaces(
        &self,
        f: &WorkspaceFilter,
    ) -> Result<Vec<ConsoleWorkspace>, PlatformError> {
        let list = self.list_workspaces(f).await?;
        let mut usage = self.usage(None).await?;
        Ok(list
            .into_iter()
            .map(|w| {
                let usage = usage.remove(&w.id).unwrap_or_default();
                ConsoleWorkspace { workspace: w, usage }
            })
            .collect())
    }

    /// One workspace with its usage.
    pub async fn console_workspace(&self, id: Uuid) -> Result<ConsoleWorkspace, PlatformError> {
        let workspace = self.get_workspace(id).await?;
        let usage = self.usage(Some(id)).await?.remove(&id).unwrap_or_default();
        Ok(ConsoleWorkspace { workspace, usage })
    }

    pub async fn quota_defaults(&self) -> Result<Vec<QuotaDefaults>, PlatformError> {
        let rows: Vec<DefaultsRow> = sqlx::query_as(&format!(
            "{SELECT_DEFAULTS} ORDER BY array_position(ARRAY['starter','standard','enterprise','regulated'], profile)"
        ))
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.into_iter().map(to_defaults).collect())
    }

    /// Changes a profile's default bundle. Existing workspaces keep their own quotas.
    pub async fn set_quota_defaults(
        &self,
        actor: Uuid,
        profile: &str,
        expected_revision: Option<i64>,
        q: &Quotas,
        client: &ClientInfo,
    ) -> Result<QuotaDefaults, PlatformError> {
        if !PROFILES.contains(&profile) {
            return Err(PlatformError::NotFound);
        }
        validate(q)?;
        let mut tx = self.db.begin_system().await?;
        let before: DefaultsRow = sqlx::query_as(&format!("{SELECT_DEFAULTS} WHERE profile = $1 FOR UPDATE"))
            .bind(profile)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(PlatformError::NotFound)?;
        if let Some(exp) = expected_revision
            && exp != before.6
        {
            return Err(PlatformError::StaleRevision { expected: exp, current: before.6 });
        }
        let row: DefaultsRow = sqlx::query_as(
            "UPDATE platform_quota_defaults SET max_members=$2, max_active_instances=$3, max_storage_bytes=$4, api_rpm=$5, ai_tokens_month=$6,
               revision = revision + 1, updated_by = $7, updated_at = $8 WHERE profile = $1
             RETURNING profile, max_members, max_active_instances, max_storage_bytes, api_rpm, ai_tokens_month, revision, updated_at",
        )
        .bind(profile).bind(q.max_members).bind(q.max_active_instances).bind(q.max_storage_bytes).bind(q.api_rpm).bind(q.ai_tokens_month)
        .bind(actor).bind(self.clock.now())
        .fetch_one(tx.conn())
        .await?;
        let subject = self.subject(&mut tx, actor).await?;
        let diff = json!({"profile": profile, "before": to_defaults(before).quotas, "after": q});
        self.audit(&mut tx, subject, "platform.quota_defaults_changed", None, Some(diff), client).await?;
        tx.commit().await?;
        Ok(to_defaults(row))
    }

    pub async fn queues(&self) -> Result<Queues, PlatformError> {
        type Row = (i64, i64, Option<OffsetDateTime>, i64, i64, Option<OffsetDateTime>);
        let r: Row =
            sqlx::query_as("SELECT * FROM discovery.platform_queues()").fetch_one(self.db.pool()).await?;
        Ok(Queues {
            outbox_pending: r.0,
            outbox_retrying: r.1,
            outbox_oldest_at: r.2,
            mail_queued: r.3,
            mail_failed: r.4,
            mail_oldest_at: r.5,
        })
    }

    pub async fn migrations(&self) -> Result<Migrations, PlatformError> {
        let (applied, latest): (i64, Option<i64>) =
            sqlx::query_as("SELECT count(*), max(version) FROM _sqlx_migrations WHERE success")
                .fetch_one(self.db.pool())
                .await?;
        Ok(Migrations { applied, latest })
    }
}
