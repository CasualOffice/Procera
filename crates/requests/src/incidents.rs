//! Technical incidents (docs/05 §3.18 "Ops monitor", PR-09): work the runtime gave up on, kept apart
//! from business exceptions (unassigned or overdue steps, people away), which have their own views.
//! Two sources today: a job dead-lettered after its retries (an instance that could not advance),
//! and a reminder or escalation timer whose handler kept failing. An operator retries one (the job
//! is queued again, the timer rescheduled now, attempts reset) or dismisses it (the job is parked
//! as `failed`, the timer cancelled), always with a reason, audited as `incident.retried` or
//! `incident.dismissed`.

use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Actor, RequestError, Requests};

type R<T> = Result<T, RequestError>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IncidentRequest {
    pub id: Uuid,
    pub display_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Incident {
    /// `job` or `timer`.
    pub source: String,
    pub id: Uuid,
    /// What failed, in words.
    pub what: String,
    pub request: Option<IncidentRequest>,
    pub attempts: i32,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub failed_at: Option<OffsetDateTime>,
}

type Row = (
    String,
    Uuid,
    String,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    i32,
    Option<String>,
    Option<OffsetDateTime>,
);

impl Requests {
    /// Open incidents, newest first (at most 200).
    pub async fn incidents(&self, tx: &mut WsTx<'_>) -> R<Vec<Incident>> {
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT 'job', j.id, j.kind, NULL::text, j.instance_id, r.display_id, r.title, j.attempts,
                    coalesce(j.last_error->>'message', j.last_error #>> '{}'), j.finished_at
               FROM jobs j LEFT JOIN requests r ON r.workspace_id = j.workspace_id AND r.instance_id = j.instance_id
              WHERE j.state = 'dead'
             UNION ALL
             SELECT 'timer', t.id, t.purpose, w.name, t.instance_id, r.display_id, r.title, t.attempts, t.last_error, t.due_at
               FROM timers t LEFT JOIN requests r ON r.workspace_id = t.workspace_id AND r.instance_id = t.instance_id
               LEFT JOIN work_items w ON w.workspace_id = t.workspace_id AND t.timer_key LIKE w.work_key || '/esc/%'
              WHERE t.state = 'failed'
              ORDER BY 10 DESC NULLS LAST LIMIT 200",
        )
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let what = match (r.0.as_str(), r.2.as_str(), r.3.as_deref()) {
                    ("job", "advance", _) => "The request couldn’t move to its next step".to_owned(),
                    ("job", kind, _) => format!("Background job “{kind}” failed"),
                    (_, "reminder", Some(step)) => format!("Reminder for “{step}” couldn’t be sent"),
                    (_, "escalation", Some(step)) => format!("Escalation of “{step}” couldn’t run"),
                    (_, purpose, _) => format!("A {purpose} timer kept failing"),
                };
                Incident {
                    source: r.0,
                    id: r.1,
                    what,
                    request: r.4.zip(r.5).zip(r.6).map(|((id, display_id), title)| IncidentRequest {
                        id,
                        display_id,
                        title,
                    }),
                    attempts: r.7,
                    error: r.8,
                    failed_at: r.9,
                }
            })
            .collect())
    }

    /// Retries an open incident: the job runs again now, or the timer fires again now.
    pub async fn retry_incident(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        source: &str,
        id: Uuid,
        reason: &str,
    ) -> R<()> {
        let now = self.clock.now();
        let sql = match source {
            "job" => "UPDATE jobs SET state = 'queued', attempts = 0, run_at = $2, lease_owner = NULL, lease_until = NULL, finished_at = NULL
                       WHERE id = $1 AND state = 'dead'",
            "timer" => "UPDATE timers SET state = 'scheduled', attempts = 0, due_at = $2, fired_at = NULL WHERE id = $1 AND state = 'failed'",
            _ => return Err(RequestError::NotFound),
        };
        self.settle(tx, actor, source, id, reason, sql, now, "incident.retried").await
    }

    /// Dismisses an open incident: the job is parked as `failed`, the timer cancelled.
    pub async fn dismiss_incident(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        source: &str,
        id: Uuid,
        reason: &str,
    ) -> R<()> {
        let now = self.clock.now();
        let sql = match source {
            "job" => "UPDATE jobs SET state = 'failed', finished_at = $2 WHERE id = $1 AND state = 'dead'",
            "timer" => {
                "UPDATE timers SET state = 'cancelled', fired_at = coalesce(fired_at, $2) WHERE id = $1 AND state = 'failed'"
            }
            _ => return Err(RequestError::NotFound),
        };
        self.settle(tx, actor, source, id, reason, sql, now, "incident.dismissed").await
    }

    #[allow(clippy::too_many_arguments)]
    async fn settle(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        source: &str,
        id: Uuid,
        reason: &str,
        sql: &str,
        now: OffsetDateTime,
        action: &str,
    ) -> R<()> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(RequestError::Invalid(vec![crate::FieldError::new(
                "reason",
                "required",
                "Say why, in up to 500 characters.",
            )]));
        }
        let incident = self.incidents(tx).await?.into_iter().find(|i| i.source == source && i.id == id);
        let Some(incident) = incident else {
            // Gone or already settled: tell the two apart for the caller.
            let exists: Option<(Uuid,)> = sqlx::query_as(match source {
                "job" => "SELECT id FROM jobs WHERE id = $1",
                _ => "SELECT id FROM timers WHERE id = $1",
            })
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
            return Err(if exists.is_some() {
                RequestError::Conflict("This incident was already handled".into())
            } else {
                RequestError::NotFound
            });
        };
        sqlx::query(sql).bind(id).bind(now).execute(tx.conn()).await?;
        let diff: Value = json!({
            "source": source, "what": incident.what, "reason": reason, "attempts": incident.attempts,
            "error": incident.error, "request": incident.request.as_ref().map(|r| &r.display_id),
        });
        self.audit(tx, actor, action, if source == "job" { "job" } else { "timer" }, id, diff).await
    }
}
