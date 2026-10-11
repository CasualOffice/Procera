//! Background audit exports (BPMS-064): queued by `POST /audit/exports {background: true}`, run by
//! the scheduler (and `/__test/audit/exports/run`). Each job pages through the requester's filter
//! (their own events only with `audit:view_own`), stores the result as a file of the requester
//! (encrypted at rest, signed-link download), appends `audit.exported`, and notifies them in-app.

use procera_audit::explorer::{ExportFormat, Filter, export_body};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::{Db, WsCtx};
use procera_files::Files;
use procera_kernel::{Clock, IdGen};
use procera_notify::Notify;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// Most rows one background export holds.
pub const ASYNC_ROW_CAP: usize = 1_000_000;

#[derive(Clone)]
pub struct AuditExports {
    pub db: Db,
    pub files: Files,
    pub notify: Notify,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for AuditExports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuditExports")
    }
}

type Job = (Uuid, Uuid, Uuid, bool, Value, String);

impl AuditExports {
    /// Runs every queued job. Returns how many finished (done or failed).
    pub async fn run_due(&self) -> Result<usize, String> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.audit_export_workspaces(100)")
                .fetch_all(self.db.pool())
                .await
                .map_err(|e| e.to_string())?;
        let mut n = 0;
        for ws in workspaces {
            loop {
                let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await.map_err(|e| e.to_string())?;
                let job: Option<Job> = sqlx::query_as(
                    "SELECT id, requested_by, subject_ref, own_only, filters, format FROM audit_export_jobs WHERE state = 'queued' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED",
                )
                .fetch_optional(tx.conn())
                .await
                .map_err(|e| e.to_string())?;
                let Some(job) = job else { break };
                let outcome = self.run_one(&mut tx, &job).await;
                if let Err(e) = &outcome {
                    tracing::error!(job = %job.0, error = %e, "audit export failed");
                    sqlx::query("UPDATE audit_export_jobs SET state = 'failed', error = left($2, 500), finished_at = $3 WHERE id = $1")
                        .bind(job.0).bind(e).bind(self.clock.now()).execute(tx.conn()).await.map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())?;
                n += 1;
            }
        }
        Ok(n)
    }

    async fn run_one(
        &self,
        tx: &mut procera_db::WsTx<'_>,
        (id, membership, subject, own_only, filters, format): &Job,
    ) -> Result<(), String> {
        let f: Filter = serde_json::from_value(filters.clone()).map_err(|e| e.to_string())?;
        let fmt = if format == "jsonl" { ExportFormat::Jsonl } else { ExportFormat::Csv };
        let (body, rows, truncated) = export_body(tx, &f, own_only.then_some(*subject), fmt, ASYNC_ROW_CAP)
            .await
            .map_err(|e| e.to_string())?;
        let actor = procera_files::Actor {
            membership: *membership,
            subject_ref: *subject,
            ip: None,
            user_agent: None,
        };
        let name = format!(
            "audit-export-{}.{}",
            self.clock.now().unix_timestamp(),
            if fmt == ExportFormat::Jsonl { "jsonl" } else { "csv" }
        );
        let bytes = body.into_bytes();
        let file = self
            .files
            .create(
                tx,
                &actor,
                &name,
                i64::try_from(bytes.len()).unwrap_or(i64::MAX).max(1),
                Some("text/csv"),
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut off = 0usize;
        for chunk in bytes.chunks(procera_files::MAX_CHUNK) {
            self.files
                .append(tx, &actor, file.id, i64::try_from(off).unwrap_or(0), chunk)
                .await
                .map_err(|e| e.to_string())?;
            off += chunk.len();
        }
        let done = self.files.complete(tx, &actor, file.id).await.map_err(|e| e.to_string())?;
        if done.state != "clean" {
            return Err(format!("the export file is {}", done.state));
        }
        // Held by its job, so the orphan purge (FR-F07) never takes a ready export away.
        self.files.link(tx, &actor, file.id, "audit_export", *id).await.map_err(|e| e.to_string())?;
        sqlx::query("UPDATE audit_export_jobs SET state = 'done', rows = $2, file_id = $3, finished_at = $4 WHERE id = $1")
            .bind(id).bind(i64::try_from(rows).unwrap_or(i64::MAX)).bind(file.id).bind(self.clock.now()).execute(tx.conn()).await.map_err(|e| e.to_string())?;
        let mut ev = AuditEvent::new(
            Some(*subject),
            ActorKind::User,
            Category::Export,
            "audit.exported",
            Outcome::Allowed,
        )
        .object("audit_export", *id);
        ev.diff = Some(
            json!({"format": format, "rows": rows, "truncated": truncated, "background": true, "filters": filters}),
        );
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await.map_err(|e| e.to_string())?;
        self.notify
            .notify_member(
                tx,
                *membership,
                "audit_export_ready",
                &json!({"rows": rows}),
                Some("/admin/audit?exports=1"),
                Some(("audit_export", *id)),
                &format!("audit_export:{id}"),
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
