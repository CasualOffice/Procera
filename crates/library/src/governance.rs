//! Recertification, verification and change requests (BPMS-044; FR-G05, FR-G07, FX-G01/G02).
//!
//! - **Review cadence.** Publishing sets `next_review_at = now + cadence`, where the cadence is the
//!   artifact's override or the workspace default for its criticality
//!   (`governance.review_cadence.default` / `.controlled`). An artifact is never published without
//!   a review date.
//! - **Overdue.** The worker flags artifacts whose review date has passed (`review_overdue_at`),
//!   once, and emits `artifact.review_overdue.v1` so the owner is notified. Recertifying (owner
//!   confirms the published content is still right) or publishing a new version clears the flag.
//! - **Verification.** A verification badge lasts `governance.verification.validity` and expires on
//!   its own; it requires a published version.
//! - **Change requests.** Readers propose a change; a publisher accepts, declines (with a reason) or
//!   marks it implemented by a version of the same artifact.

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsCtx;
use procera_db::outbox::{self, OutboxEvent};
use serde::Serialize;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{Actor, Artifact, Library, LibraryError, WsTx};

const DAY: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct ChangeRequest {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub title: String,
    pub body: String,
    pub state: String,
    pub requested_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub requested_at: OffsetDateTime,
    pub resolved_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    pub resolution: Option<String>,
    pub version_id: Option<Uuid>,
    pub revision: i64,
}

const CR_COLS: &str = "id, artifact_id, title, body, state, requested_by, requested_at, resolved_by, resolved_at, resolution, version_id, revision";

impl Library {
    pub(crate) async fn audit_artifact(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        action: &str,
        object: (&str, Uuid),
        diff: Value,
        reason: Option<&str>,
    ) -> Result<(), LibraryError> {
        let kind = if actor.is_some() { ActorKind::User } else { ActorKind::System };
        let mut ev =
            AuditEvent::new(actor.map(|a| a.subject_ref), kind, Category::Content, action, Outcome::Allowed)
                .object(object.0, object.1);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// The review cadence that applies to an artifact.
    pub async fn cadence(&self, tx: &mut WsTx<'_>, artifact_id: Uuid) -> Result<Duration, LibraryError> {
        let (days, criticality): (Option<i32>, String) =
            sqlx::query_as("SELECT review_cadence_days, criticality FROM artifacts WHERE id = $1")
                .bind(artifact_id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(LibraryError::NotFound)?;
        if let Some(d) = days {
            return Ok(Duration::days(i64::from(d)));
        }
        let key = if criticality == "controlled" {
            "governance.review_cadence.controlled"
        } else {
            "governance.review_cadence.default"
        };
        let secs = self.settings.effective(tx, key).await?.value.as_i64().unwrap_or(365 * DAY);
        Ok(Duration::seconds(secs))
    }

    /// Starts a new review period from `from`; clears any overdue flag. Returns the next review date.
    pub(crate) async fn schedule_review(
        &self,
        tx: &mut WsTx<'_>,
        artifact_id: Uuid,
        from: OffsetDateTime,
    ) -> Result<OffsetDateTime, LibraryError> {
        let next = from + self.cadence(tx, artifact_id).await?;
        sqlx::query("UPDATE artifacts SET next_review_at = $2, last_reviewed_at = $3, review_overdue_at = NULL, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id).bind(next).bind(from).execute(tx.conn()).await?;
        Ok(next)
    }

    async fn require_published(
        &self,
        tx: &mut WsTx<'_>,
        artifact_id: Uuid,
    ) -> Result<Artifact, LibraryError> {
        sqlx::query("SELECT 1 FROM artifacts WHERE id = $1 FOR UPDATE")
            .bind(artifact_id)
            .execute(tx.conn())
            .await?;
        let a = self.artifact(tx, artifact_id).await?;
        if a.current_published_version_id.is_none() {
            return Err(LibraryError::Conflict("the artifact has no published version".into()));
        }
        Ok(a)
    }

    /// Sets (or clears, with `None`) the artifact's own review cadence. A published artifact's next
    /// review date moves to `last_reviewed_at + new cadence`.
    pub async fn set_cadence(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        days: Option<i32>,
    ) -> Result<Artifact, LibraryError> {
        if days.is_some_and(|d| !(1..=3650).contains(&d)) {
            return Err(LibraryError::Invalid("review_cadence_days must be between 1 and 3650".into()));
        }
        let before = self.artifact(tx, artifact_id).await?;
        sqlx::query("UPDATE artifacts SET review_cadence_days = $2, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id)
            .bind(days)
            .execute(tx.conn())
            .await?;
        if let Some(last) = before.governance.last_reviewed_at {
            self.schedule_review(tx, artifact_id, last).await?;
        }
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.review_cadence_set",
            ("artifact", artifact_id),
            json!({"review_cadence_days": {"from": before.governance.review_cadence_days, "to": days}}),
            None,
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// The owner confirms the published content is still correct: a new review period starts now.
    pub async fn recertify(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        comment: Option<&str>,
    ) -> Result<Artifact, LibraryError> {
        let a = self.require_published(tx, artifact_id).await?;
        let next = self.schedule_review(tx, artifact_id, self.clock.now()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.recertified",
            ("artifact", artifact_id),
            json!({"was": a.governance.review_status, "next_review_at": next.unix_timestamp()}),
            comment.map(str::trim).filter(|c| !c.is_empty()),
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// Marks the artifact verified until `now + validity` (the workspace setting).
    pub async fn verify(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
    ) -> Result<Artifact, LibraryError> {
        self.require_published(tx, artifact_id).await?;
        let secs = self
            .settings
            .effective(tx, "governance.verification.validity")
            .await?
            .value
            .as_i64()
            .unwrap_or(90 * DAY);
        let now = self.clock.now();
        let expires = now + Duration::seconds(secs);
        sqlx::query("UPDATE artifacts SET verified_at = $2, verified_by = $3, verification_expires_at = $4, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id).bind(now).bind(actor.membership).bind(expires).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.verified",
            ("artifact", artifact_id),
            json!({"expires_at": expires.unix_timestamp()}),
            None,
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// Removes the verification badge.
    pub async fn unverify(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
    ) -> Result<Artifact, LibraryError> {
        let a = self.artifact(tx, artifact_id).await?;
        if a.governance.verified_at.is_none() {
            return Err(LibraryError::Conflict("the artifact is not verified".into()));
        }
        sqlx::query("UPDATE artifacts SET verified_at = NULL, verified_by = NULL, verification_expires_at = NULL, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.verification_removed",
            ("artifact", artifact_id),
            json!({"was_verified": a.governance.verified}),
            None,
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// Flags published artifacts whose review date has passed (worker). Returns how many.
    pub async fn flag_overdue(&self) -> Result<usize, LibraryError> {
        let now = self.clock.now();
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.library_review_due_workspaces($1, 1000)")
                .bind(now)
                .fetch_all(self.db.pool())
                .await?;
        let mut n = 0;
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let due: Vec<(Uuid, String, Option<Uuid>, OffsetDateTime, i64)> = sqlx::query_as(
                "SELECT id, title, owner_membership_id, next_review_at, revision FROM artifacts
                 WHERE review_overdue_at IS NULL AND next_review_at <= $1 AND current_published_version_id IS NOT NULL
                 ORDER BY next_review_at FOR UPDATE SKIP LOCKED",
            )
            .bind(now)
            .fetch_all(tx.conn())
            .await?;
            for (id, title, owner, due_at, revision) in due {
                sqlx::query(
                    "UPDATE artifacts SET review_overdue_at = $2, revision = revision + 1 WHERE id = $1",
                )
                .bind(id)
                .bind(now)
                .execute(tx.conn())
                .await?;
                outbox::enqueue(&mut tx, OutboxEvent { topic: "artifact.review_overdue.v1", aggregate_type: "artifact", aggregate_id: id, aggregate_revision: revision + 1, payload: json!({"artifact_id": id, "title": title, "owner_membership_id": owner, "next_review_at": due_at.unix_timestamp()}), trace_id: None }).await?;
                self.audit_artifact(
                    &mut tx,
                    None,
                    "artifact.review_overdue",
                    ("artifact", id),
                    json!({"next_review_at": due_at.unix_timestamp()}),
                    None,
                )
                .await?;
                n += 1;
            }
            tx.commit().await?;
        }
        Ok(n)
    }

    /// Proposes a change to an artifact.
    pub async fn open_change_request(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        title: &str,
        body: &str,
    ) -> Result<ChangeRequest, LibraryError> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(LibraryError::Invalid("title must be 1–200 characters".into()));
        }
        if body.chars().count() > 10_000 {
            return Err(LibraryError::Invalid("body must be at most 10 000 characters".into()));
        }
        self.artifact(tx, artifact_id).await?;
        let id = self.ids.new_id();
        sqlx::query("INSERT INTO change_requests (workspace_id, id, artifact_id, title, body, requested_by, requested_at) VALUES ($7, $1, $2, $3, $4, $5, $6)")
            .bind(id).bind(artifact_id).bind(title).bind(body).bind(actor.membership).bind(self.clock.now()).bind(tx.ctx.workspace_id).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "change_request.opened",
            ("change_request", id),
            json!({"artifact_id": artifact_id, "title": title}),
            None,
        )
        .await?;
        self.change_request(tx, id).await
    }

    pub async fn change_request(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<ChangeRequest, LibraryError> {
        sqlx::query_as(&format!("SELECT {CR_COLS} FROM change_requests WHERE id = $1"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(LibraryError::NotFound)
    }

    /// Change requests of an artifact, open first, then newest first.
    pub async fn change_requests(
        &self,
        tx: &mut WsTx<'_>,
        artifact_id: Uuid,
        state: Option<&str>,
    ) -> Result<Vec<ChangeRequest>, LibraryError> {
        Ok(sqlx::query_as(&format!("SELECT {CR_COLS} FROM change_requests WHERE artifact_id = $1 AND ($2::text IS NULL OR state = $2) ORDER BY state <> 'open', requested_at DESC, id"))
            .bind(artifact_id).bind(state).fetch_all(tx.conn()).await?)
    }

    /// Resolves a change request: `accepted` (open → accepted), `declined` (open → declined; needs
    /// a resolution), or `implemented` (open/accepted → implemented; needs a version of the same
    /// artifact).
    pub async fn resolve_change_request(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        decision: &str,
        resolution: Option<&str>,
        version_id: Option<Uuid>,
    ) -> Result<ChangeRequest, LibraryError> {
        let cr: ChangeRequest =
            sqlx::query_as(&format!("SELECT {CR_COLS} FROM change_requests WHERE id = $1 FOR UPDATE"))
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(LibraryError::NotFound)?;
        let resolution = resolution.map(str::trim).filter(|r| !r.is_empty());
        let allowed = match (cr.state.as_str(), decision) {
            ("open", "accepted" | "declined" | "implemented") | ("accepted", "implemented") => true,
            (_, "accepted" | "declined" | "implemented") => false,
            _ => {
                return Err(LibraryError::Invalid(
                    "decision must be accepted, declined or implemented".into(),
                ));
            }
        };
        if !allowed {
            return Err(LibraryError::Conflict(format!(
                "a change request that is {} cannot be {decision}",
                cr.state
            )));
        }
        if decision == "declined" && resolution.is_none() {
            return Err(LibraryError::Invalid("a reason is required to decline".into()));
        }
        if decision == "implemented" {
            let v = version_id.ok_or_else(|| LibraryError::Invalid("version_id is required".into()))?;
            let owner: Option<Uuid> =
                sqlx::query_scalar("SELECT artifact_id FROM artifact_versions WHERE id = $1")
                    .bind(v)
                    .fetch_optional(tx.conn())
                    .await?;
            if owner != Some(cr.artifact_id) {
                return Err(LibraryError::Invalid("the version must belong to the same artifact".into()));
            }
        }
        sqlx::query("UPDATE change_requests SET state = $2, resolution = COALESCE($3, resolution), version_id = COALESCE($4, version_id), resolved_by = $5, resolved_at = $6, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(decision).bind(resolution).bind(version_id.filter(|_| decision == "implemented")).bind(actor.membership).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "change_request.resolved",
            ("change_request", id),
            json!({"artifact_id": cr.artifact_id, "state": {"from": cr.state, "to": decision}, "version_id": version_id}),
            resolution,
        )
        .await?;
        self.change_request(tx, id).await
    }
}
