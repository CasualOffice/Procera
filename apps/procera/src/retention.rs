//! Retention classes honoured by jobs (BPMS-066; PRV-04 policy part, docs/14 §4).
//!
//! | Class | Here | Job |
//! |---|---|---|
//! | `draft` | library drafts (unpublished versions) | deleted after `retention.draft.inactive_after` without activity; the author is told `retention.draft.notice_before` ahead |
//! | `operational` | delivery logs: sent/failed emails, read notifications, sent digest items | deleted `retention.operational.keep_for` after creation |
//! | `governance_record` | published, deprecated and retired versions, reviews, acknowledgements | never deleted by the app (disposition with certificates is P1) |
//! | `audit` | the audit chain | never deleted by the app role |
//!
//! Drafts referenced by a change request are kept. A never-published artifact goes with its only
//! draft unless it has change requests. Every run that removes something appends `retention.purged`
//! with counts (system actor) and records the run in `retention_runs`.

use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::{Db, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_notify::Notify;
use procera_settings::Settings;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const DAY: i64 = 86_400;

#[derive(Clone)]
pub struct Retention {
    db: Db,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    settings: Settings,
    notify: Notify,
}

impl std::fmt::Debug for Retention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Retention")
    }
}

/// The effective retention periods of a workspace.
#[derive(Debug, Clone, Copy)]
pub struct Periods {
    pub draft_inactive_after: Duration,
    pub draft_notice_before: Duration,
    pub operational_keep_for: Duration,
    pub governance_keep_for: Duration,
}

/// A draft that will be deleted unless someone works on it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DueDraft {
    pub artifact_id: Uuid,
    pub version_id: Uuid,
    pub title: String,
    pub version_no: i32,
    pub author: Option<String>,
    #[serde(skip)]
    pub author_membership: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub last_activity_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub deletes_on: OffsetDateTime,
}

type Err = String;

fn e(err: impl std::fmt::Display) -> Err {
    err.to_string()
}

impl Retention {
    pub fn new(db: Db, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>, notify: Notify) -> Self {
        let settings = Settings::new(clock.clone(), ids.clone());
        Self { db, clock, ids, settings, notify }
    }

    async fn secs(&self, tx: &mut WsTx<'_>, key: &str, default: i64) -> Result<Duration, Err> {
        let v = self.settings.effective(tx, key).await.map_err(e)?.value;
        Ok(Duration::seconds(v.as_i64().unwrap_or(default)))
    }

    pub async fn periods(&self, tx: &mut WsTx<'_>) -> Result<Periods, Err> {
        Ok(Periods {
            draft_inactive_after: self.secs(tx, "retention.draft.inactive_after", 180 * DAY).await?,
            draft_notice_before: self.secs(tx, "retention.draft.notice_before", 14 * DAY).await?,
            operational_keep_for: self.secs(tx, "retention.operational.keep_for", 400 * DAY).await?,
            governance_keep_for: self.secs(tx, "retention.governance_record.keep_for", 3650 * DAY).await?,
        })
    }

    /// Drafts that will be deleted within `within` from now (soonest first).
    pub async fn due_drafts(
        &self,
        tx: &mut WsTx<'_>,
        p: &Periods,
        within: Duration,
    ) -> Result<Vec<DueDraft>, Err> {
        let now = self.clock.now();
        type Row = (Uuid, Uuid, String, i32, Option<String>, Uuid, OffsetDateTime);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT a.id, v.id, a.title, v.version_no, u.display_name, v.created_by, v.last_activity_at
             FROM artifact_versions v JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
             LEFT JOIN memberships m ON m.workspace_id = v.workspace_id AND m.id = v.created_by
             LEFT JOIN users u ON u.id = m.user_id
             WHERE v.lifecycle = 'draft' AND v.last_activity_at + $1 <= $2
               AND NOT EXISTS (SELECT 1 FROM change_requests c WHERE c.version_id = v.id)
             ORDER BY v.last_activity_at, v.id",
        )
        .bind(p.draft_inactive_after)
        .bind(now + within)
        .fetch_all(tx.conn())
        .await
        .map_err(e)?;
        Ok(rows
            .into_iter()
            .map(|(artifact_id, version_id, title, version_no, author, by, last)| DueDraft {
                artifact_id,
                version_id,
                title,
                version_no,
                author,
                author_membership: Some(by),
                last_activity_at: last,
                deletes_on: last + p.draft_inactive_after,
            })
            .collect())
    }

    /// Runs every class job in every workspace (scheduler and `/__test/drain`). Returns rows removed.
    pub async fn run(&self) -> Result<usize, Err> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.retention_workspaces(1000)")
                .fetch_all(self.db.pool())
                .await
                .map_err(e)?;
        let mut total = 0;
        for ws in workspaces {
            total += self.run_workspace(ws).await?;
        }
        Ok(total)
    }

    async fn run_workspace(&self, ws: Uuid) -> Result<usize, Err> {
        let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await.map_err(e)?;
        let p = self.periods(&mut tx).await?;
        let now = self.clock.now();

        // Notices: drafts due within the notice window, once per draft and deletion date.
        let mut notices = 0;
        for d in self.due_drafts(&mut tx, &p, p.draft_notice_before).await? {
            let Some(author) = d.author_membership.filter(|_| d.deletes_on > now) else { continue };
            let date = d.deletes_on.date().to_string();
            let sent = self
                .notify
                .notify_member(
                    &mut tx,
                    author,
                    "draft_expiring",
                    &json!({"title": d.title, "version": d.version_no, "date": date}),
                    Some(&format!("/library/{}", d.artifact_id)),
                    Some(("artifact_version", d.version_id)),
                    &format!("draft_expiring:{}:{date}", d.version_id),
                )
                .await
                .map_err(e)?;
            notices += usize::from(sent);
        }

        // draft: delete expired drafts (and a never-published artifact with them).
        let mut deleted: Vec<Value> = vec![];
        for d in self.due_drafts(&mut tx, &p, Duration::ZERO).await? {
            sqlx::query("DELETE FROM reviews WHERE artifact_version_id = $1")
                .bind(d.version_id)
                .execute(tx.conn())
                .await
                .map_err(e)?;
            sqlx::query("DELETE FROM artifact_versions WHERE id = $1")
                .bind(d.version_id)
                .execute(tx.conn())
                .await
                .map_err(e)?;
            let (left, has_crs): (i64, bool) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM artifact_versions WHERE artifact_id = $1),
                        EXISTS (SELECT 1 FROM change_requests WHERE artifact_id = $1)",
            )
            .bind(d.artifact_id)
            .fetch_one(tx.conn())
            .await
            .map_err(e)?;
            let artifact_removed = left == 0 && !has_crs;
            if artifact_removed {
                sqlx::query("DELETE FROM artifacts WHERE id = $1")
                    .bind(d.artifact_id)
                    .execute(tx.conn())
                    .await
                    .map_err(e)?;
            } else {
                // The denormalised state follows the newest remaining version.
                sqlx::query(
                    "UPDATE artifacts a SET state = coalesce((SELECT lifecycle FROM artifact_versions v WHERE v.artifact_id = a.id ORDER BY version_no DESC LIMIT 1), a.state), revision = a.revision + 1 WHERE a.id = $1",
                )
                .bind(d.artifact_id)
                .execute(tx.conn())
                .await
                .map_err(e)?;
            }
            deleted.push(json!({"artifact_id": d.artifact_id, "version_id": d.version_id, "version_no": d.version_no, "artifact_removed": artifact_removed}));
        }

        // operational: delivery logs past their period.
        let before = now - p.operational_keep_for;
        let mut purge = async |sql: &str| -> Result<u64, Err> {
            Ok(sqlx::query(sql).bind(before).execute(tx.conn()).await.map_err(e)?.rows_affected())
        };
        let emails =
            purge("DELETE FROM email_messages WHERE state IN ('sent', 'failed') AND created_at < $1").await?;
        let notifications =
            purge("DELETE FROM notifications WHERE read_at IS NOT NULL AND created_at < $1").await?;
        let digests = purge("DELETE FROM digest_items WHERE sent_at IS NOT NULL AND created_at < $1").await?;

        let removed = deleted.len() + usize::try_from(emails + notifications + digests).unwrap_or(usize::MAX);
        if removed > 0 || notices > 0 {
            let purged = json!({
                "draft": deleted.len(),
                "operational": {"emails": emails, "notifications": notifications, "digest_items": digests},
                "notices": notices,
            });
            sqlx::query(
                "INSERT INTO retention_runs (workspace_id, id, ran_at, purged) VALUES ($1, $2, $3, $4)",
            )
            .bind(ws)
            .bind(self.ids.new_id())
            .bind(now)
            .bind(&purged)
            .execute(tx.conn())
            .await
            .map_err(e)?;
            if removed > 0 {
                let mut ev = AuditEvent::new(
                    None,
                    ActorKind::System,
                    Category::System,
                    "retention.purged",
                    Outcome::Allowed,
                );
                ev.diff = Some(json!({"purged": purged, "drafts": deleted}));
                append(&mut tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await.map_err(e)?;
            }
        }
        tx.commit().await.map_err(e)?;
        Ok(removed)
    }

    /// The latest run that removed or announced something.
    pub async fn last_run(&self, tx: &mut WsTx<'_>) -> Result<Option<(OffsetDateTime, Value)>, Err> {
        sqlx::query_as("SELECT ran_at, purged FROM retention_runs ORDER BY ran_at DESC, id DESC LIMIT 1")
            .fetch_optional(tx.conn())
            .await
            .map_err(e)
    }
}
