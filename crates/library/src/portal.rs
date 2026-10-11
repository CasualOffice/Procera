//! Visibility for readers, policy applicability and the employee policy portal (BPMS-045;
//! FR-PL01/PL02, A-14).
//!
//! - A reader holding an authoring permission on a kind sees all of it. Everyone else sees an
//!   artifact only once it is published, only its published history, and never a `restricted`
//!   artifact whose applicability excludes them.
//! - The portal lists the published policies that apply to the caller (no applicability = applies
//!   to everyone), with a "what changed" summary against the previous published version.

use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::diff::diff;
use crate::{Actor, Artifact, Library, LibraryError, PUBLISHED_HISTORY, Reader, WsTx};

/// "What changed" between the current published version and the one it superseded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WhatChanged {
    pub from_version_no: i32,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub change_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortalPolicy {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub category: Option<String>,
    pub version_id: Uuid,
    pub version_no: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    pub verified: bool,
    pub review_status: &'static str,
    /// `None` for a first version.
    pub what_changed: Option<WhatChanged>,
}

impl Library {
    /// Whether `reader` may see the artifact at all.
    pub async fn visible(
        &self,
        tx: &mut WsTx<'_>,
        a: &Artifact,
        reader: &Reader<'_>,
    ) -> Result<bool, LibraryError> {
        if reader.full(&a.kind) {
            return Ok(true);
        }
        if a.current_published_version_id.is_none() {
            return Ok(false);
        }
        match (&a.applicability, a.restricted) {
            (Some(aud), true) => Ok(sqlx::query_scalar("SELECT procera_audience_contains($1, $2)")
                .bind(aud)
                .bind(reader.membership)
                .fetch_one(tx.conn())
                .await?),
            _ => Ok(true),
        }
    }

    /// Whether `reader` may open a version in `lifecycle` of a visible artifact.
    pub fn version_visible(kind: &str, lifecycle: &str, reader: &Reader<'_>) -> bool {
        reader.full(kind) || PUBLISHED_HISTORY.contains(&lifecycle)
    }

    /// Of `ids`, the artifacts `reader` may see (search filtering).
    pub async fn visible_ids(
        &self,
        tx: &mut WsTx<'_>,
        ids: &[Uuid],
        reader: &Reader<'_>,
    ) -> Result<Vec<Uuid>, LibraryError> {
        let full: Vec<String> = reader.full_kinds.iter().map(|k| (*k).to_owned()).collect();
        Ok(sqlx::query_scalar(&format!(
            "SELECT a.id FROM artifacts a WHERE a.id = ANY($1) AND (a.kind = ANY($3) OR {}) AND $2::int IS NULL",
            crate::READER_VISIBLE
        ))
        .bind(ids)
        .bind(None::<i32>)
        .bind(&full)
        .bind(reader.membership)
        .fetch_all(tx.conn())
        .await?)
    }

    /// Sets category, applicability (an audience, validated by the caller; `None` = everyone) and
    /// the restricted flag.
    pub async fn set_applicability(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        category: Option<&str>,
        applicability: Option<&Value>,
        restricted: bool,
    ) -> Result<Artifact, LibraryError> {
        let category = category.map(str::trim).filter(|c| !c.is_empty());
        if category.is_some_and(|c| c.chars().count() > 60) {
            return Err(LibraryError::Invalid("category must be at most 60 characters".into()));
        }
        if restricted && applicability.is_none() {
            return Err(LibraryError::Invalid(
                "a restricted artifact needs an applicability audience".into(),
            ));
        }
        let before = self.artifact(tx, id).await?;
        sqlx::query("UPDATE artifacts SET category = $2, applicability = $3, restricted = $4, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(category).bind(applicability).bind(restricted).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.applicability_set",
            ("artifact", id),
            json!({
                "category": {"from": before.category, "to": category},
                "applicability": {"from": before.applicability, "to": applicability},
                "restricted": {"from": before.restricted, "to": restricted},
            }),
            None,
        )
        .await?;
        self.artifact(tx, id).await
    }

    /// Published policies that apply to `membership`, optionally filtered by text and category.
    pub async fn portal(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        q: Option<&str>,
        category: Option<&str>,
    ) -> Result<Vec<PortalPolicy>, LibraryError> {
        let q = q
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", q.replace(['%', '_', '\\'], "")));
        type Row = (Uuid, Uuid, i32, Option<OffsetDateTime>, Value);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT a.id, v.id, v.version_no, v.published_at, v.content
             FROM artifacts a JOIN artifact_versions v ON v.workspace_id = a.workspace_id AND v.id = a.current_published_version_id
             WHERE a.kind = 'policy' AND (a.applicability IS NULL OR procera_audience_contains(a.applicability, $1))
               AND ($2::text IS NULL OR a.title ILIKE $2 OR a.key ILIKE $2 OR a.category ILIKE $2)
               AND ($3::text IS NULL OR lower(a.category) = lower($3))
             ORDER BY lower(coalesce(a.category, '')), lower(a.title)",
        )
        .bind(membership)
        .bind(q)
        .bind(category)
        .fetch_all(tx.conn())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (artifact_id, version_id, version_no, published_at, content) in rows {
            let a = self.artifact(tx, artifact_id).await?;
            let prev: Option<(i32, Value)> = sqlx::query_as(
                "SELECT version_no, content FROM artifact_versions
                 WHERE artifact_id = $1 AND version_no < $2 AND published_at IS NOT NULL
                 ORDER BY version_no DESC LIMIT 1",
            )
            .bind(artifact_id)
            .bind(version_no)
            .fetch_optional(tx.conn())
            .await?;
            let reason: Option<String> =
                sqlx::query_scalar("SELECT change_reason FROM artifact_versions WHERE id = $1")
                    .bind(version_id)
                    .fetch_one(tx.conn())
                    .await?;
            let what_changed = prev.map(|(from, before)| {
                let d = diff(&before, &content);
                WhatChanged {
                    from_version_no: from,
                    added: d.added,
                    removed: d.removed,
                    changed: d.changed,
                    change_reason: reason,
                }
            });
            out.push(PortalPolicy {
                id: a.id,
                key: a.key,
                title: a.title,
                category: a.category,
                version_id,
                version_no,
                published_at,
                verified: a.governance.verified,
                review_status: a.governance.review_status,
                what_changed,
            });
        }
        Ok(out)
    }
}
