//! Notifications (BPMS-048, FR-N01/N02): an in-app inbox per member and an outbound email queue
//! delivered over SMTP by the worker. Email bodies are sealed at rest (they can carry invitation
//! links); delivery retries with backoff. Without SMTP configured, emails stay queued.

pub mod templates;

use std::sync::Arc;

use lettre::message::{Mailbox, MultiPart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use procera_crypto::{KeyProvider, open, seal};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("template error: {0}")]
    Template(String),
    #[error("not found")]
    NotFound,
    #[error("smtp: {0}")]
    Smtp(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for NotifyError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for NotifyError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_crypto::CryptoError> for NotifyError {
    fn from(e: procera_crypto::CryptoError) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notification {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub link: Option<String>,
    pub ref_type: Option<String>,
    pub ref_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A queued email with its decrypted body (worker and E2E controls only).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutboundEmail {
    pub id: Uuid,
    pub kind: String,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub state: String,
    pub attempts: i32,
}

/// SMTP delivery. `from` is the platform sender address (docs/20 §3 Email).
#[derive(Clone)]
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl std::fmt::Debug for SmtpMailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpMailer").field("from", &self.from.to_string()).finish_non_exhaustive()
    }
}

impl SmtpMailer {
    /// `url` like `smtp://user:pass@host:587?tls=required` (STARTTLS) or `smtps://host:465`.
    pub fn from_url(url: &str, from: &str) -> Result<Self, NotifyError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| NotifyError::Smtp(e.to_string()))?
            .build();
        let from = from
            .parse()
            .map_err(|e: lettre::address::AddressError| NotifyError::Smtp(format!("from address: {e}")))?;
        Ok(Self { transport, from })
    }

    async fn send(&self, e: &OutboundEmail, sender_name: Option<&str>) -> Result<(), NotifyError> {
        let from = match sender_name.filter(|n| !n.trim().is_empty()) {
            Some(n) => Mailbox::new(Some(n.trim().to_owned()), self.from.email.clone()),
            None => self.from.clone(),
        };
        let msg = Message::builder()
            .from(from)
            .to(e.to.parse().map_err(|err: lettre::address::AddressError| {
                NotifyError::Smtp(format!("to address: {err}"))
            })?)
            .subject(e.subject.clone())
            .message_id(Some(format!("<{}@procera>", e.id)))
            .multipart(MultiPart::alternative_plain_html(e.text.clone(), e.html.clone()))
            .map_err(|err| NotifyError::Smtp(err.to_string()))?;
        self.transport.send(msg).await.map(|_| ()).map_err(|err| NotifyError::Smtp(err.to_string()))
    }
}

#[derive(Clone)]
pub struct Notify {
    db: Db,
    keys: Arc<dyn KeyProvider>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Notify")
    }
}

const PURPOSE: &str = "secrets";

impl Notify {
    pub fn new(db: Db, keys: Arc<dyn KeyProvider>, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { db, keys, clock, ids }
    }

    /// Renders and queues an email in the caller's transaction. `dedupe_key` makes it idempotent
    /// (e.g. the outbox message id). Returns `false` when it was already queued.
    pub async fn queue_email(
        &self,
        tx: &mut WsTx<'_>,
        kind: &str,
        to: &str,
        ctx: &Value,
        dedupe_key: &str,
    ) -> Result<bool, NotifyError> {
        let r = templates::email(kind, ctx)?;
        let id = self.ids.new_id();
        let body = serde_json::to_vec(&json!({"text": r.text, "html": r.html}))
            .map_err(|e| NotifyError::Internal(e.to_string()))?;
        let sealed = seal(self.keys.as_ref(), PURPOSE, &format!("email_messages:{id}"), &body)?;
        let n = sqlx::query("INSERT INTO email_messages (workspace_id, id, kind, to_email, subject, body_sealed, dedupe_key, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (workspace_id, dedupe_key) DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(id).bind(kind).bind(to.trim().to_lowercase()).bind(&r.subject).bind(sealed).bind(dedupe_key).bind(self.clock.now())
            .execute(tx.conn()).await?.rows_affected();
        Ok(n == 1)
    }

    /// Adds an in-app notification for a member in the caller's transaction (idempotent on `dedupe_key`).
    #[allow(clippy::too_many_arguments)]
    pub async fn notify_member(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        kind: &str,
        ctx: &Value,
        link: Option<&str>,
        reference: Option<(&str, Uuid)>,
        dedupe_key: &str,
    ) -> Result<bool, NotifyError> {
        let r = templates::in_app(kind, ctx)?;
        let n = sqlx::query("INSERT INTO notifications (workspace_id, id, membership_id, kind, title, body, link, ref_type, ref_id, dedupe_key, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (workspace_id, dedupe_key) DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(self.ids.new_id()).bind(membership).bind(kind).bind(&r.title).bind(&r.body).bind(link)
            .bind(reference.map(|r| r.0)).bind(reference.map(|r| r.1)).bind(dedupe_key).bind(self.clock.now())
            .execute(tx.conn()).await?.rows_affected();
        Ok(n == 1)
    }

    /// A member's inbox, newest first, with the unread count.
    pub async fn inbox(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        unread_only: bool,
        limit: i64,
    ) -> Result<(Vec<Notification>, i64), NotifyError> {
        type Row = (
            Uuid,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<Uuid>,
            Option<OffsetDateTime>,
            OffsetDateTime,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, kind, title, body, link, ref_type, ref_id, read_at, created_at FROM notifications
             WHERE membership_id = $1 AND ($2 = false OR read_at IS NULL) ORDER BY created_at DESC, id DESC LIMIT $3",
        ).bind(membership).bind(unread_only).bind(limit.clamp(1, 200)).fetch_all(tx.conn()).await?;
        let unread: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM notifications WHERE membership_id = $1 AND read_at IS NULL",
        )
        .bind(membership)
        .fetch_one(tx.conn())
        .await?;
        Ok((
            rows.into_iter()
                .map(|(id, kind, title, body, link, ref_type, ref_id, read_at, created_at)| Notification {
                    id,
                    kind,
                    title,
                    body,
                    link,
                    ref_type,
                    ref_id,
                    read_at,
                    created_at,
                })
                .collect(),
            unread,
        ))
    }

    pub async fn mark_read(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        id: Option<Uuid>,
    ) -> Result<u64, NotifyError> {
        let n = sqlx::query("UPDATE notifications SET read_at = $3 WHERE membership_id = $1 AND ($2::uuid IS NULL OR id = $2) AND read_at IS NULL")
            .bind(membership).bind(id).bind(self.clock.now()).execute(tx.conn()).await?.rows_affected();
        if id.is_some() && n == 0 {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM notifications WHERE membership_id = $1 AND id = $2)",
            )
            .bind(membership)
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
            if !exists {
                return Err(NotifyError::NotFound);
            }
        }
        Ok(n)
    }

    fn decode(&self, id: Uuid, sealed: &[u8]) -> Result<(String, String), NotifyError> {
        let pt = open(self.keys.as_ref(), PURPOSE, &format!("email_messages:{id}"), sealed)?;
        let v: Value = serde_json::from_slice(&pt).map_err(|e| NotifyError::Internal(e.to_string()))?;
        Ok((
            v["text"].as_str().unwrap_or_default().to_owned(),
            v["html"].as_str().unwrap_or_default().to_owned(),
        ))
    }

    /// Queued and sent emails to `to` in a workspace, newest first (E2E controls and support).
    pub async fn emails_to(&self, ctx: WsCtx, to: &str) -> Result<Vec<OutboundEmail>, NotifyError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let rows: Vec<(Uuid, String, String, String, Vec<u8>, String, i32)> = sqlx::query_as(
            "SELECT id, kind, to_email::text, subject, body_sealed, state, attempts FROM email_messages WHERE to_email = $1 ORDER BY created_at DESC",
        ).bind(to.trim().to_lowercase()).fetch_all(tx.conn()).await?;
        tx.rollback().await?;
        rows.into_iter()
            .map(|(id, kind, to, subject, sealed, state, attempts)| {
                let (text, html) = self.decode(id, &sealed)?;
                Ok(OutboundEmail { id, kind, to, subject, text, html, state, attempts })
            })
            .collect()
    }

    /// Sends due emails in every workspace once. Returns (sent, failed).
    pub async fn send_due(&self, mailer: &SmtpMailer) -> Result<(usize, usize), NotifyError> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.email_pending_workspaces(1000)")
                .fetch_all(self.db.pool())
                .await?;
        let (mut sent, mut failed) = (0, 0);
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let sender: Option<String> = sqlx::query_scalar(
                "SELECT value #>> '{}' FROM workspace_settings WHERE key = 'branding.email_sender_name'",
            )
            .fetch_optional(tx.conn())
            .await?;
            let rows: Vec<(Uuid, String, String, String, Vec<u8>, i32)> = sqlx::query_as(
                "SELECT id, kind, to_email::text, subject, body_sealed, attempts FROM email_messages
                 WHERE state = 'queued' AND (next_attempt_at IS NULL OR next_attempt_at <= now()) ORDER BY created_at LIMIT 100 FOR UPDATE SKIP LOCKED",
            ).fetch_all(tx.conn()).await?;
            for (id, kind, to, subject, sealed, attempts) in rows {
                let (text, html) = self.decode(id, &sealed)?;
                let email =
                    OutboundEmail { id, kind, to, subject, text, html, state: "queued".into(), attempts };
                match mailer.send(&email, sender.as_deref()).await {
                    Ok(()) => {
                        sent += 1;
                        sqlx::query("UPDATE email_messages SET state = 'sent', sent_at = $2, attempts = attempts + 1, last_error = NULL WHERE id = $1").bind(id).bind(self.clock.now()).execute(tx.conn()).await?;
                    }
                    Err(e) => {
                        failed += 1;
                        // Retry with backoff for a day (2^n s, max 1 h), then give up and keep the error.
                        let give_up = attempts + 1 >= 12;
                        let backoff = 2_i64
                            .saturating_pow(u32::try_from(attempts.clamp(0, 12)).unwrap_or(12))
                            .min(3600);
                        tracing::warn!(email = %id, attempts = attempts + 1, error = %e, "email delivery failed");
                        sqlx::query("UPDATE email_messages SET attempts = attempts + 1, last_error = left($2, 500), state = CASE WHEN $3 THEN 'failed' ELSE 'queued' END, next_attempt_at = now() + make_interval(secs => $4) WHERE id = $1")
                            .bind(id).bind(e.to_string()).bind(give_up).bind(backoff as f64).execute(tx.conn()).await?;
                    }
                }
            }
            tx.commit().await?;
        }
        Ok((sent, failed))
    }
}
