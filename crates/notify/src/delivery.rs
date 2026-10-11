//! Delivery preferences: digests, quiet hours and bounce suppression (BPMS-048; FR-N04, docs/20 §2.9).
//!
//! - Each member's preferences fall back to the workspace settings `notify.digest.*` and
//!   `notify.quiet_hours.*`.
//! - A **normal** email to a member with a digest goes into their next digest instead of out now.
//!   The worker sends one digest email per member at their digest hour (member's time zone), daily
//!   or on Mondays for weekly.
//! - During **quiet hours** an email is held (`next_attempt_at` = the end of the window) unless it is
//!   **critical** and `notify.critical_override_quiet` is on.
//! - An address that bounced permanently is suppressed for every workspace; nothing is queued to it.
//!
//! In-app notifications are never held: they are silent by nature.

use procera_db::{WsCtx, WsTx};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{Notify, NotifyError, templates};

/// How urgent an email is. Only `Normal` goes into digests; only `Critical` may break quiet hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Normal,
    Urgent,
    Critical,
}

/// What happened to an email addressed to a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Queued,
    /// Held until the end of quiet hours.
    Held(OffsetDateTime),
    /// Added to the member's next digest.
    Digested,
    /// The address bounced permanently before.
    Suppressed,
    /// Already queued, held or digested (same dedupe key).
    Duplicate,
}

/// Effective preferences, with where each came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preferences {
    pub digest: String,
    pub digest_hour: i64,
    pub quiet_enabled: bool,
    pub quiet_start: i64,
    pub quiet_end: i64,
    pub critical_override_quiet: bool,
    /// The member's own overrides (`null` = workspace default).
    pub overrides: Overrides,
    /// The workspace defaults, for "Workspace default: …" hints.
    pub workspace: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub digest: Option<String>,
    pub digest_hour: Option<i32>,
    pub quiet_enabled: Option<bool>,
    pub quiet_start: Option<i32>,
    pub quiet_end: Option<i32>,
}

/// Whether `hour` falls in the quiet window [start, end) (wrapping past midnight when start > end).
pub fn in_quiet_window(hour: i64, start: i64, end: i64) -> bool {
    match start.cmp(&end) {
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Less => hour >= start && hour < end,
        std::cmp::Ordering::Greater => hour >= start || hour < end,
    }
}

impl Notify {
    async fn workspace_value(&self, tx: &mut WsTx<'_>, key: &str) -> Result<Value, NotifyError> {
        Ok(self.settings.effective(tx, key).await.map_err(|e| NotifyError::Internal(e.to_string()))?.value)
    }

    /// The member's effective delivery preferences.
    pub async fn preferences(&self, tx: &mut WsTx<'_>, membership: Uuid) -> Result<Preferences, NotifyError> {
        let o: Overrides = sqlx::query_as(
            "SELECT digest, digest_hour, quiet_enabled, quiet_start, quiet_end FROM notification_preferences WHERE membership_id = $1",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?
        .unwrap_or_default();
        let ws = json!({
            "digest": self.workspace_value(tx, "notify.digest.schedule").await?,
            "digest_hour": self.workspace_value(tx, "notify.digest.hour").await?,
            "quiet_enabled": self.workspace_value(tx, "notify.quiet_hours.enabled").await?,
            "quiet_start": self.workspace_value(tx, "notify.quiet_hours.start").await?,
            "quiet_end": self.workspace_value(tx, "notify.quiet_hours.end").await?,
        });
        let critical =
            self.workspace_value(tx, "notify.critical_override_quiet").await?.as_bool().unwrap_or(true);
        Ok(Preferences {
            digest: o.digest.clone().unwrap_or_else(|| ws["digest"].as_str().unwrap_or("daily").to_owned()),
            digest_hour: o.digest_hour.map_or_else(|| ws["digest_hour"].as_i64().unwrap_or(9), i64::from),
            quiet_enabled: o.quiet_enabled.unwrap_or_else(|| ws["quiet_enabled"].as_bool().unwrap_or(true)),
            quiet_start: o.quiet_start.map_or_else(|| ws["quiet_start"].as_i64().unwrap_or(20), i64::from),
            quiet_end: o.quiet_end.map_or_else(|| ws["quiet_end"].as_i64().unwrap_or(8), i64::from),
            critical_override_quiet: critical,
            overrides: o,
            workspace: ws,
        })
    }

    /// Replaces the member's overrides (`None` fields fall back to the workspace).
    pub async fn set_preferences(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        o: &Overrides,
    ) -> Result<Preferences, NotifyError> {
        if o.digest.as_deref().is_some_and(|d| !matches!(d, "off" | "daily" | "weekly")) {
            return Err(NotifyError::Invalid("digest must be off, daily or weekly".into()));
        }
        for h in [o.digest_hour, o.quiet_start, o.quiet_end].into_iter().flatten() {
            if !(0..=23).contains(&h) {
                return Err(NotifyError::Invalid("hours must be between 0 and 23".into()));
            }
        }
        sqlx::query(
            "INSERT INTO notification_preferences (workspace_id, membership_id, digest, digest_hour, quiet_enabled, quiet_start, quiet_end, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (workspace_id, membership_id) DO UPDATE SET digest = $3, digest_hour = $4, quiet_enabled = $5, quiet_start = $6, quiet_end = $7, updated_at = $8",
        )
        .bind(tx.ctx.workspace_id)
        .bind(membership)
        .bind(&o.digest)
        .bind(o.digest_hour)
        .bind(o.quiet_enabled)
        .bind(o.quiet_start)
        .bind(o.quiet_end)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        self.preferences(tx, membership).await
    }

    /// The member's local time now (their user time zone; UTC when unknown).
    async fn local_now(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
    ) -> Result<(i64, OffsetDateTime), NotifyError> {
        let now = self.clock.now();
        let hour: Option<f64> = sqlx::query_scalar(
            "SELECT extract(hour FROM $2::timestamptz AT TIME ZONE coalesce(nullif(u.timezone, ''), 'UTC'))::float8
             FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(membership)
        .bind(now)
        .fetch_optional(tx.conn())
        .await?;
        Ok((hour.map_or(now.hour().into(), |h| h as i64), now))
    }

    /// Emails a member, honouring their digest and quiet hours. Idempotent on `dedupe_key`.
    #[allow(clippy::too_many_arguments)]
    pub async fn email_member(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        kind: &str,
        to: &str,
        ctx: &Value,
        dedupe_key: &str,
        urgency: Urgency,
    ) -> Result<Delivery, NotifyError> {
        let suppressed: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM email_suppressions WHERE email = $1)")
                .bind(to.trim().to_lowercase())
                .fetch_one(tx.conn())
                .await?;
        if suppressed {
            tracing::info!(kind, "email not queued: address suppressed after a permanent bounce");
            return Ok(Delivery::Suppressed);
        }
        let p = self.preferences(tx, membership).await?;
        if urgency == Urgency::Normal && p.digest != "off" {
            let r = templates::email(kind, ctx)?;
            let link = ctx.get("link").and_then(Value::as_str);
            let n = sqlx::query("INSERT INTO digest_items (workspace_id, id, membership_id, kind, title, link, dedupe_key, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (workspace_id, dedupe_key) DO NOTHING")
                .bind(tx.ctx.workspace_id).bind(self.ids.new_id()).bind(membership).bind(kind).bind(&r.subject).bind(link).bind(dedupe_key).bind(self.clock.now())
                .execute(tx.conn()).await?.rows_affected();
            return Ok(if n == 0 { Delivery::Duplicate } else { Delivery::Digested });
        }
        let (hour, now) = self.local_now(tx, membership).await?;
        let quiet = p.quiet_enabled && in_quiet_window(hour, p.quiet_start, p.quiet_end);
        if quiet && !(urgency == Urgency::Critical && p.critical_override_quiet) {
            // Hours until the window ends, from the start of the current local hour.
            let wait = (p.quiet_end - hour).rem_euclid(24).max(1);
            let until = now
                .replace_minute(0)
                .unwrap_or(now)
                .replace_second(0)
                .unwrap_or(now)
                .replace_nanosecond(0)
                .unwrap_or(now)
                + Duration::hours(wait);
            let queued = self.queue_email(tx, kind, to, ctx, dedupe_key).await?;
            if !queued {
                return Ok(Delivery::Duplicate);
            }
            sqlx::query("UPDATE email_messages SET next_attempt_at = $2 WHERE dedupe_key = $1")
                .bind(dedupe_key)
                .bind(until)
                .execute(tx.conn())
                .await?;
            return Ok(Delivery::Held(until));
        }
        Ok(if self.queue_email(tx, kind, to, ctx, dedupe_key).await? {
            Delivery::Queued
        } else {
            Delivery::Duplicate
        })
    }

    /// Sends due digests in every workspace (worker). Returns how many digest emails were queued.
    pub async fn send_digests(&self) -> Result<usize, NotifyError> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.digest_pending_workspaces(1000)")
                .fetch_all(self.db.pool())
                .await?;
        let mut sent = 0;
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let members: Vec<(Uuid, String)> = sqlx::query_as(
                "SELECT DISTINCT d.membership_id, u.email::text FROM digest_items d
                 JOIN memberships m ON m.workspace_id = d.workspace_id AND m.id = d.membership_id AND m.status = 'active'
                 JOIN users u ON u.id = m.user_id WHERE d.sent_at IS NULL",
            )
            .fetch_all(tx.conn())
            .await?;
            for (membership, email) in members {
                if self.digest_due(&mut tx, membership).await? {
                    sent += usize::from(self.send_digest(&mut tx, membership, &email).await?);
                }
            }
            tx.commit().await?;
        }
        Ok(sent)
    }

    /// Due when it is the member's digest hour or later today (local), nothing went out today, and
    /// for weekly digests it is Monday. Members who turned digests off get their backlog now.
    async fn digest_due(&self, tx: &mut WsTx<'_>, membership: Uuid) -> Result<bool, NotifyError> {
        let p = self.preferences(tx, membership).await?;
        if p.digest == "off" {
            return Ok(true);
        }
        let now = self.clock.now();
        let (hour, dow, already): (f64, f64, bool) = sqlx::query_as(
            "SELECT extract(hour FROM l.t)::float8, extract(isodow FROM l.t)::float8,
                    EXISTS (SELECT 1 FROM digest_items d WHERE d.membership_id = $1 AND d.sent_at IS NOT NULL
                              AND (d.sent_at AT TIME ZONE l.tz)::date = l.t::date)
             FROM (SELECT $2::timestamptz AT TIME ZONE coalesce(nullif(u.timezone, ''), 'UTC') AS t, coalesce(nullif(u.timezone, ''), 'UTC') AS tz
                   FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1) l",
        )
        .bind(membership)
        .bind(now)
        .fetch_one(tx.conn())
        .await?;
        Ok(!already && hour as i64 >= p.digest_hour && (p.digest == "daily" || dow as i64 == 1))
    }

    async fn send_digest(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        email: &str,
    ) -> Result<bool, NotifyError> {
        let items: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
            "SELECT id, title, link FROM digest_items WHERE membership_id = $1 AND sent_at IS NULL ORDER BY created_at, id FOR UPDATE SKIP LOCKED",
        )
        .bind(membership)
        .fetch_all(tx.conn())
        .await?;
        if items.is_empty() {
            return Ok(false);
        }
        let workspace: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
            .bind(tx.ctx.workspace_id)
            .fetch_optional(tx.conn())
            .await?
            .unwrap_or_else(|| "your workspace".into());
        let now = self.clock.now();
        let ctx = json!({
            "workspace": workspace,
            "count": items.len(),
            "items": items.iter().map(|(_, t, l)| json!({"title": t, "link": l})).collect::<Vec<_>>(),
        });
        let first = items.first().map(|i| i.0).unwrap_or_default();
        self.queue_email(tx, "digest", email, &ctx, &format!("digest:{membership}:{first}")).await?;
        sqlx::query("UPDATE digest_items SET sent_at = $2 WHERE id = ANY($1)")
            .bind(items.iter().map(|i| i.0).collect::<Vec<_>>())
            .bind(now)
            .execute(tx.conn())
            .await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::in_quiet_window;

    #[test]
    fn quiet_window_wraps_past_midnight() {
        assert!(in_quiet_window(22, 20, 8) && in_quiet_window(3, 20, 8) && !in_quiet_window(8, 20, 8));
        assert!(!in_quiet_window(12, 20, 8) && in_quiet_window(12, 12, 13) && !in_quiet_window(13, 12, 13));
        assert!(!in_quiet_window(5, 6, 6)); // an empty window is never quiet
    }
}
