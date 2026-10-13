//! Employee e-signing (BPMS-107, FR-DS01–07, docs/04 §2.7, docs/05 §3.22).
//!
//! - **Envelope**: one clean PDF (`files`) sent to members who sign it in order (`sequential`) or all
//!   at once (`parallel`); CC recipients only receive the completed copy. A draft gets its recipients
//!   and fields; sending freezes the document (SHA-256 and page sizes) and the fields (triggers in
//!   migration 0050) and notifies the signers whose turn it is.
//! - **Signing** (ceremony): the signer opens it (viewed), consents to signing electronically,
//!   adopts a typed or drawn signature and fills their fields; the server checks the document hash
//!   they were shown, the envelope state and their turn, records an immutable `signatures` row
//!   (ESIG-02) and the audit event, and tells the next signer. `strong` mode needs a recent sign-in
//!   (checked by the HTTP layer). Declining ends the envelope for everyone.
//! - **Completion**: when the last signer signs, in the same transaction, the values are stamped onto
//!   the PDF with a certificate of completion appended (`pdf`), stored as a new file linked to the
//!   envelope (so it can't be deleted), and its hash recorded. Everyone is notified.
//!
//! Authorization is the caller's: senders need `document:send_for_signature`, anyone else's envelopes
//! `document:view_all`, voiding someone else's `document:void`; signers act only on their own turn.

mod bulk;
pub mod pdf;

pub use bulk::Due;

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use procera_files::Files;
use procera_kernel::{Clock, IdGen};
use procera_notify::Notify;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// What a signature means, recorded with it and printed on the certificate (ESIG-01).
pub const MEANING: &str = "I have reviewed this document and I sign it";
const CONSENT: &str =
    "I agree to sign electronically. My electronic signature is as valid as my handwritten one.";
const MAX_DRAWN: usize = 300 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Internal(String),
}

type R<T> = Result<T, SigningError>;

fn internal(e: impl std::fmt::Display) -> SigningError {
    SigningError::Internal(e.to_string())
}

impl From<sqlx::Error> for SigningError {
    fn from(e: sqlx::Error) -> Self {
        internal(e)
    }
}

impl From<pdf::PdfError> for SigningError {
    fn from(e: pdf::PdfError) -> Self {
        match e {
            pdf::PdfError::Unsupported(m) => Self::Invalid(m),
            pdf::PdfError::Unreadable => Self::Invalid("The file couldn't be read as a PDF.".into()),
            pdf::PdfError::Write(m) => Self::Internal(m),
        }
    }
}

impl From<procera_files::FileError> for SigningError {
    fn from(e: procera_files::FileError) -> Self {
        match e {
            procera_files::FileError::NotFound => Self::NotFound,
            procera_files::FileError::Conflict(m) => Self::Conflict(m),
            procera_files::FileError::Invalid(m) | procera_files::FileError::TooLarge(m) => Self::Invalid(m),
            other => internal(other),
        }
    }
}

impl From<procera_notify::NotifyError> for SigningError {
    fn from(e: procera_notify::NotifyError) -> Self {
        internal(e)
    }
}

impl From<procera_audit::AuditError> for SigningError {
    fn from(e: procera_audit::AuditError) -> Self {
        internal(e)
    }
}

/// Who is acting, with how they signed in (for the signature record).
#[derive(Debug, Clone)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    /// `password`, `password+otp`, `passkey`…
    pub auth_method: String,
    pub auth_time: OffsetDateTime,
}

impl Actor {
    fn files(&self) -> procera_files::Actor {
        procera_files::Actor {
            membership: self.membership,
            subject_ref: self.subject_ref,
            ip: self.ip.clone(),
            user_agent: self.user_agent.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Recipient {
    pub id: Uuid,
    pub ord: i32,
    pub membership_id: Uuid,
    pub name: String,
    pub email: String,
    pub role: String,
    pub state: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub viewed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub signed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub declined_at: Option<OffsetDateTime>,
    pub decline_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Field {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub recipient_id: Uuid,
    pub kind: String,
    pub page: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default = "yes")]
    pub required: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, skip_deserializing)]
    pub value: Option<Value>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub id: Uuid,
    pub title: String,
    pub message: Option<String>,
    pub source_file_id: Uuid,
    pub document_hash: String,
    pub pages: Value,
    pub order_mode: String,
    pub mode: String,
    pub state: String,
    pub created_by: Uuid,
    pub sender_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub sent_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    pub completed_file_id: Option<Uuid>,
    pub completed_hash: Option<String>,
    pub ended_reason: Option<String>,
    pub revision: i32,
    /// A bulk draft: its single signer stands for each person it will be sent to.
    pub bulk: bool,
    /// The bulk send this envelope is one copy of.
    pub batch_id: Option<Uuid>,
    /// Days between reminders to the signer whose turn it is (null: no reminders).
    pub reminder_days: Option<i32>,
    pub recipients: Vec<Recipient>,
    pub fields: Vec<Field>,
}

type RecipientRow = (
    Uuid,
    i32,
    Uuid,
    String,
    String,
    String,
    String,
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<String>,
);
type FieldRow = (Uuid, Uuid, String, i32, f64, f64, f64, f64, bool, Option<String>, Option<Value>);
/// recipient, adopted kind, adopted text, drawn image, auth method, auth time, ip, signed at
type SigRow = (Uuid, String, String, Option<Vec<u8>>, String, OffsetDateTime, Option<String>, OffsetDateTime);

#[derive(sqlx::FromRow)]
struct EnvRow {
    id: Uuid,
    title: String,
    message: Option<String>,
    source_file_id: Uuid,
    document_hash: Vec<u8>,
    pages: Value,
    order_mode: String,
    mode: String,
    state: String,
    created_by: Uuid,
    sender_name: String,
    created_at: OffsetDateTime,
    sent_at: Option<OffsetDateTime>,
    expires_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    completed_file_id: Option<Uuid>,
    completed_hash: Option<Vec<u8>>,
    ended_reason: Option<String>,
    revision: i32,
    bulk: bool,
    batch_id: Option<Uuid>,
    reminder_days: Option<i32>,
}

/// Which envelopes to list.
#[derive(Debug, Clone, Copy)]
pub enum Listing {
    /// Ones I sent.
    Sent(Uuid),
    /// Every envelope (`document:view_all`).
    All,
}

/// A signer's adopted signature.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adopted {
    /// `typed` or `drawn`.
    pub kind: String,
    /// The typed signature (also the name printed under a drawn one).
    pub text: String,
    /// A drawn signature: a PNG, base64 (with or without a `data:image/png;base64,` prefix).
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignInput {
    /// The document hash the signer was shown (hex); must match the envelope's.
    pub document_hash: String,
    pub consent: bool,
    pub adopted: Adopted,
    /// Field id → value, for `text` (string) and `checkbox` (bool) fields; the others fill themselves.
    #[serde(default)]
    pub values: BTreeMap<Uuid, Value>,
}

#[derive(Clone)]
pub struct Signing {
    db: procera_db::Db,
    files: Files,
    notify: Notify,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    public_url: String,
}

impl std::fmt::Debug for Signing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Signing")
    }
}

const KINDS: &[&str] = &["signature", "initials", "date_signed", "name", "text", "checkbox"];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn day(t: OffsetDateTime) -> String {
    const M: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {} {}", t.day(), M[usize::from(u8::from(t.month())) - 1], t.year())
}

fn stamp_time(t: OffsetDateTime) -> String {
    format!("{} {:02}:{:02}:{:02} UTC", day(t), t.hour(), t.minute(), t.second())
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).take(3).collect()
}

/// RFC 3339, as every time in the API is written.
pub(crate) fn iso(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

impl Signing {
    pub fn new(
        db: procera_db::Db,
        files: Files,
        notify: Notify,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        public_url: &str,
    ) -> Self {
        Self { db, files, notify, clock, ids, public_url: public_url.trim_end_matches('/').to_owned() }
    }

    async fn audit(&self, tx: &mut WsTx<'_>, actor: &Actor, action: &str, id: Uuid, diff: Value) -> R<()> {
        let mut ev = AuditEvent::new(
            Some(actor.subject_ref),
            ActorKind::User,
            Category::Content,
            action,
            Outcome::Allowed,
        )
        .object("envelope", id);
        ev.ip = actor.ip.clone();
        ev.user_agent = actor.user_agent.clone();
        ev.diff = Some(diff);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// A domain event for the outbox (webhooks): ids and states only.
    async fn emit(&self, tx: &mut WsTx<'_>, topic: &str, id: Uuid, payload: Value) -> R<()> {
        procera_db::outbox::enqueue(
            tx,
            procera_db::outbox::OutboxEvent {
                topic,
                aggregate_type: "envelope",
                aggregate_id: id,
                aggregate_revision: 0,
                payload,
                trace_id: None,
            },
        )
        .await
        .map_err(internal)?;
        Ok(())
    }

    async fn audit_system(&self, tx: &mut WsTx<'_>, action: &str, id: Uuid, diff: Value) -> R<()> {
        let mut ev = AuditEvent::new(None, ActorKind::System, Category::Content, action, Outcome::Allowed)
            .object("envelope", id);
        ev.diff = Some(diff);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// A new draft envelope for a clean PDF the caller uploaded or may read. A `bulk` draft gets one
    /// placeholder signer (recorded as the sender) whose fields are copied for each person it is
    /// later sent to (`send_bulk`).
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        title: &str,
        message: Option<&str>,
        file_id: Uuid,
        order_mode: &str,
        mode: &str,
        expires_in_days: i64,
        bulk: bool,
    ) -> R<Envelope> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(SigningError::Invalid("Give the document a title of up to 200 characters.".into()));
        }
        let message = message.map(str::trim).filter(|m| !m.is_empty());
        if message.is_some_and(|m| m.chars().count() > 2000) {
            return Err(SigningError::Invalid("The message can be at most 2,000 characters.".into()));
        }
        if !["sequential", "parallel"].contains(&order_mode) {
            return Err(SigningError::Invalid("The order is sequential or parallel.".into()));
        }
        if !["simple", "strong"].contains(&mode) {
            return Err(SigningError::Invalid("The signing mode is simple or strong.".into()));
        }
        if !(1..=365).contains(&expires_in_days) {
            return Err(SigningError::Invalid("An envelope expires after 1 to 365 days.".into()));
        }
        let (file, bytes) = self.files.content(tx, file_id).await?;
        if file.sniffed_type.as_deref() != Some("application/pdf") {
            return Err(SigningError::Invalid(
                "Only PDF documents can be sent for signature for now.".into(),
            ));
        }
        let pages = pdf::inspect(&bytes)?;
        let hash = Sha256::digest(&bytes).to_vec();
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO envelopes (workspace_id, id, title, message, source_file_id, document_hash, pages, order_mode, mode, state, created_by, created_at, expires_at, bulk)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'draft', $10, $11, $12, $13)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(title)
        .bind(message)
        .bind(file_id)
        .bind(&hash)
        .bind(json!(pages))
        .bind(order_mode)
        .bind(mode)
        .bind(actor.membership)
        .bind(self.clock.now())
        // A placeholder until sending sets the real deadline: keeps the lifetime with the draft.
        .bind(self.clock.now() + Duration::days(expires_in_days))
        .bind(bulk)
        .execute(tx.conn())
        .await?;
        if bulk {
            sqlx::query("INSERT INTO envelope_recipients (workspace_id, envelope_id, id, ord, membership_id, role) VALUES ($1, $2, $3, 1, $4, 'signer')")
                .bind(tx.ctx.workspace_id).bind(id).bind(self.ids.new_id()).bind(actor.membership)
                .execute(tx.conn()).await?;
        }
        self.files.link(tx, &actor.files(), file_id, "envelope", id).await?;
        self.audit(
            tx,
            actor,
            "envelope.created",
            id,
            json!({ "title": title, "file_id": file_id, "pages": pages.len(), "document_hash": hex(&hash) }),
        )
        .await?;
        self.get(tx, id).await
    }

    pub async fn get(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Envelope> {
        let r: EnvRow = sqlx::query_as(
            "SELECT e.id, e.title, e.message, e.source_file_id, e.document_hash, e.pages, e.order_mode, e.mode, e.state, e.created_by,
                    u.display_name AS sender_name, e.created_at, e.sent_at, e.expires_at, e.completed_at, e.completed_file_id,
                    e.completed_hash, e.ended_reason, e.revision, e.bulk, e.batch_id, e.reminder_days
               FROM envelopes e JOIN memberships m ON m.workspace_id = e.workspace_id AND m.id = e.created_by JOIN users u ON u.id = m.user_id
              WHERE e.id = $1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?
        .ok_or(SigningError::NotFound)?;
        let recipients: Vec<RecipientRow> = sqlx::query_as(
            "SELECT r.id, r.ord, r.membership_id, u.display_name, u.email::text, r.role, r.state, r.viewed_at, r.signed_at, r.declined_at, r.decline_reason
               FROM envelope_recipients r JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id JOIN users u ON u.id = m.user_id
              WHERE r.envelope_id = $1 ORDER BY r.ord, u.display_name",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let fields: Vec<FieldRow> = sqlx::query_as(
            "SELECT id, recipient_id, kind, page, x, y, w, h, required, label, value FROM envelope_fields WHERE envelope_id = $1 ORDER BY page, y, x",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        Ok(Envelope {
            id: r.id,
            title: r.title,
            message: r.message,
            source_file_id: r.source_file_id,
            document_hash: hex(&r.document_hash),
            pages: r.pages,
            order_mode: r.order_mode,
            mode: r.mode,
            state: r.state,
            created_by: r.created_by,
            sender_name: r.sender_name,
            created_at: r.created_at,
            sent_at: r.sent_at,
            expires_at: r.expires_at,
            completed_at: r.completed_at,
            completed_file_id: r.completed_file_id,
            completed_hash: r.completed_hash.map(|h| hex(&h)),
            ended_reason: r.ended_reason,
            revision: r.revision,
            bulk: r.bulk,
            batch_id: r.batch_id,
            reminder_days: r.reminder_days,
            recipients: recipients
                .into_iter()
                .map(|x| Recipient {
                    id: x.0,
                    ord: x.1,
                    membership_id: x.2,
                    name: x.3,
                    email: x.4,
                    role: x.5,
                    state: x.6,
                    viewed_at: x.7,
                    signed_at: x.8,
                    declined_at: x.9,
                    decline_reason: x.10,
                })
                .collect(),
            fields: fields
                .into_iter()
                .map(|f| Field {
                    id: Some(f.0),
                    recipient_id: f.1,
                    kind: f.2,
                    page: f.3,
                    x: f.4,
                    y: f.5,
                    w: f.6,
                    h: f.7,
                    required: f.8,
                    label: f.9,
                    value: f.10,
                })
                .collect(),
        })
    }

    /// Envelopes, newest first (at most 500), without fields.
    pub async fn list(&self, tx: &mut WsTx<'_>, which: Listing) -> R<Vec<Envelope>> {
        let ids: Vec<Uuid> = match which {
            Listing::Sent(me) => {
                sqlx::query_scalar(
                    "SELECT id FROM envelopes WHERE created_by = $1 AND batch_id IS NULL AND NOT EXISTS (SELECT 1 FROM envelope_batches b WHERE b.template_id = envelopes.id) ORDER BY created_at DESC LIMIT 500",
                )
                .bind(me)
                .fetch_all(tx.conn())
                .await?
            }
            Listing::All => {
                sqlx::query_scalar("SELECT id FROM envelopes WHERE batch_id IS NULL AND NOT EXISTS (SELECT 1 FROM envelope_batches b WHERE b.template_id = envelopes.id) ORDER BY created_at DESC LIMIT 500")
                    .fetch_all(tx.conn())
                    .await?
            }
        };
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let mut e = self.get(tx, id).await?;
            e.fields.clear();
            out.push(e);
        }
        Ok(out)
    }

    async fn draft(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid, revision: Option<i32>) -> R<Envelope> {
        let state: Option<(String, Uuid, i32)> =
            sqlx::query_as("SELECT state, created_by, revision FROM envelopes WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?;
        let (state, by, rev) = state.ok_or(SigningError::NotFound)?;
        if by != actor.membership {
            return Err(SigningError::NotFound);
        }
        if state != "draft" {
            return Err(SigningError::Conflict(
                "It has been sent, so its recipients and fields can't change.".into(),
            ));
        }
        if revision.is_some_and(|r| r != rev) {
            return Err(SigningError::Conflict("stale".into()));
        }
        let sent_as_batch: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM envelope_batches WHERE template_id = $1)")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
        if sent_as_batch {
            return Err(SigningError::Conflict("It has been sent to a group, so it can't change.".into()));
        }
        self.get(tx, id).await
    }

    async fn bump(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<()> {
        sqlx::query("UPDATE envelopes SET revision = revision + 1 WHERE id = $1")
            .bind(id)
            .execute(tx.conn())
            .await?;
        Ok(())
    }

    /// Replaces the recipients, in order. Recipients who stay keep their fields; removed ones lose theirs.
    pub async fn set_recipients(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        revision: Option<i32>,
        list: &[(Uuid, String)],
    ) -> R<Envelope> {
        let e = self.draft(tx, actor, id, revision).await?;
        // A bulk draft's only signer is the placeholder for each person; it can add copies (CC).
        let bulk_list: Vec<(Uuid, String)>;
        let list = if e.bulk {
            if list.iter().any(|(m, role)| role != "cc" && *m != actor.membership) {
                return Err(SigningError::Invalid(
                    "This is sent to a group: each person signs their own copy. Add others only as copies (cc).".into(),
                ));
            }
            bulk_list = std::iter::once((actor.membership, "signer".to_owned()))
                .chain(list.iter().filter(|(m, _)| *m != actor.membership).cloned())
                .collect();
            &bulk_list[..]
        } else {
            list
        };
        if list.len() > 50 {
            return Err(SigningError::Invalid("An envelope can have at most 50 recipients.".into()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (m, role) in list {
            if !seen.insert(*m) {
                return Err(SigningError::Invalid("Each person can be a recipient once.".into()));
            }
            if !["signer", "cc"].contains(&role.as_str()) {
                return Err(SigningError::Invalid("A recipient signs or gets a copy (signer or cc).".into()));
            }
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
            )
            .bind(m)
            .fetch_one(tx.conn())
            .await?;
            if !active {
                return Err(SigningError::Invalid(
                    "Recipients must be active members of this workspace.".into(),
                ));
            }
        }
        let keep: Vec<Uuid> = list.iter().map(|(m, _)| *m).collect();
        for r in e.recipients.iter().filter(|r| !keep.contains(&r.membership_id)) {
            sqlx::query("DELETE FROM envelope_fields WHERE recipient_id = $1")
                .bind(r.id)
                .execute(tx.conn())
                .await?;
            sqlx::query("DELETE FROM envelope_recipients WHERE id = $1")
                .bind(r.id)
                .execute(tx.conn())
                .await?;
        }
        for (i, (m, role)) in list.iter().enumerate() {
            let ord = i32::try_from(i + 1).unwrap_or(i32::MAX);
            if let Some(r) = e.recipients.iter().find(|r| r.membership_id == *m) {
                sqlx::query("UPDATE envelope_recipients SET ord = $2, role = $3 WHERE id = $1")
                    .bind(r.id)
                    .bind(ord)
                    .bind(role)
                    .execute(tx.conn())
                    .await?;
                if role == "cc" {
                    sqlx::query("DELETE FROM envelope_fields WHERE recipient_id = $1")
                        .bind(r.id)
                        .execute(tx.conn())
                        .await?;
                }
            } else {
                sqlx::query("INSERT INTO envelope_recipients (workspace_id, envelope_id, id, ord, membership_id, role) VALUES ($1, $2, $3, $4, $5, $6)")
                    .bind(tx.ctx.workspace_id).bind(id).bind(self.ids.new_id()).bind(ord).bind(m).bind(role)
                    .execute(tx.conn()).await?;
            }
        }
        self.bump(tx, id).await?;
        self.audit(tx, actor, "envelope.recipients_set", id, json!({ "recipients": list.iter().map(|(m, role)| json!({"membership_id": m, "role": role})).collect::<Vec<_>>() })).await?;
        self.get(tx, id).await
    }

    /// Replaces the fields. Each sits on a page inside the page, and belongs to a signer.
    pub async fn set_fields(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        revision: Option<i32>,
        fields: &[Field],
    ) -> R<Envelope> {
        let e = self.draft(tx, actor, id, revision).await?;
        let pages = e.pages.as_array().map_or(0, Vec::len);
        if fields.len() > 500 {
            return Err(SigningError::Invalid("An envelope can have at most 500 fields.".into()));
        }
        for f in fields {
            if !KINDS.contains(&f.kind.as_str()) {
                return Err(SigningError::Invalid(format!("“{}” isn't a field type.", f.kind)));
            }
            if f.page < 1 || usize::try_from(f.page).unwrap_or(usize::MAX) > pages {
                return Err(SigningError::Invalid(format!("Page {} isn't in the document.", f.page)));
            }
            let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
            if !(inside(f.x)
                && inside(f.y)
                && f.w > 0.0
                && f.h > 0.0
                && f.x + f.w <= 1.000_000_1
                && f.y + f.h <= 1.000_000_1
                && f.x < 1.0
                && f.y < 1.0)
            {
                return Err(SigningError::Invalid("A field must sit inside its page.".into()));
            }
            if !e.recipients.iter().any(|r| r.id == f.recipient_id && r.role == "signer") {
                return Err(SigningError::Invalid("Every field belongs to one of the signers.".into()));
            }
            if f.label.as_deref().is_some_and(|l| l.chars().count() > 100) {
                return Err(SigningError::Invalid("A field label can be at most 100 characters.".into()));
            }
        }
        sqlx::query("DELETE FROM envelope_fields WHERE envelope_id = $1").bind(id).execute(tx.conn()).await?;
        for f in fields {
            sqlx::query("INSERT INTO envelope_fields (workspace_id, envelope_id, id, recipient_id, kind, page, x, y, w, h, required, label) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
                .bind(tx.ctx.workspace_id).bind(id).bind(self.ids.new_id()).bind(f.recipient_id).bind(&f.kind).bind(f.page)
                .bind(f.x).bind(f.y).bind(f.w).bind(f.h).bind(f.kind == "signature" || f.required).bind(f.label.as_deref().map(str::trim).filter(|l| !l.is_empty()))
                .execute(tx.conn()).await?;
        }
        self.bump(tx, id).await?;
        self.audit(tx, actor, "envelope.fields_set", id, json!({ "fields": fields.len() })).await?;
        self.get(tx, id).await
    }

    /// Sends a draft: it must have a signer, and every signer a signature field.
    pub async fn send(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        revision: Option<i32>,
    ) -> R<Envelope> {
        let e = self.draft(tx, actor, id, revision).await?;
        if e.bulk {
            return Err(SigningError::Invalid(
                "This is sent to a group: choose who receives it and send it to them.".into(),
            ));
        }
        let signers: Vec<&Recipient> = e.recipients.iter().filter(|r| r.role == "signer").collect();
        if signers.is_empty() {
            return Err(SigningError::Invalid("Add at least one person who signs.".into()));
        }
        if let Some(r) =
            signers.iter().find(|r| !e.fields.iter().any(|f| f.recipient_id == r.id && f.kind == "signature"))
        {
            return Err(SigningError::Invalid(format!("Place a signature field for {}.", r.name)));
        }
        let now = self.clock.now();
        let lifetime = e.expires_at.map_or(Duration::days(30), |x| x - e.created_at);
        let expires = now + lifetime;
        sqlx::query("UPDATE envelopes SET state = 'sent', sent_at = $2, expires_at = $3, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(now).bind(expires).execute(tx.conn()).await?;
        self.audit(tx, actor, "envelope.sent", id, json!({ "recipients": e.recipients.iter().map(|r| json!({"membership_id": r.membership_id, "role": r.role, "ord": r.ord})).collect::<Vec<_>>(), "order": e.order_mode, "expires_at": expires })).await?;
        self.emit(tx, "envelope.sent.v1", id, json!({ "envelope_id": id, "state": "sent" })).await?;
        let e = self.get(tx, id).await?;
        self.notify_turn(tx, &e).await?;
        Ok(e)
    }

    /// Signers whose turn it is and who haven't been told yet.
    async fn notify_turn(&self, tx: &mut WsTx<'_>, e: &Envelope) -> R<()> {
        let pending: Vec<&Recipient> =
            e.recipients.iter().filter(|r| r.role == "signer" && r.state == "pending").collect();
        let next: Vec<&Recipient> = if e.order_mode == "parallel" {
            pending
        } else {
            pending.into_iter().min_by_key(|r| r.ord).into_iter().collect()
        };
        for r in next {
            let told: Option<OffsetDateTime> =
                sqlx::query_scalar("SELECT notified_at FROM envelope_recipients WHERE id = $1")
                    .bind(r.id)
                    .fetch_one(tx.conn())
                    .await?;
            if told.is_some() {
                continue;
            }
            sqlx::query("UPDATE envelope_recipients SET notified_at = $2 WHERE id = $1")
                .bind(r.id)
                .bind(self.clock.now())
                .execute(tx.conn())
                .await?;
            let link = format!("/sign/{}", r.id);
            let vars = json!({ "title": e.title, "sender": e.sender_name, "message": e.message, "due": e.expires_at.map(day), "link": link });
            self.tell(
                tx,
                r.membership_id,
                Some(&r.email),
                "signature_requested",
                vars,
                &link,
                e.id,
                &format!("sign:{}:requested", r.id),
            )
            .await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn tell(
        &self,
        tx: &mut WsTx<'_>,
        to: Uuid,
        email: Option<&str>,
        kind: &str,
        mut vars: Value,
        link: &str,
        envelope: Uuid,
        dedupe: &str,
    ) -> R<()> {
        self.notify
            .notify_member(tx, to, kind, &vars, Some(link), Some(("envelope", envelope)), dedupe)
            .await?;
        if let Some(email) = email {
            vars["link"] = json!(format!("{}{link}", self.public_url));
            self.notify
                .email_member(tx, to, kind, email, &vars, dedupe, procera_notify::delivery::Urgency::Urgent)
                .await?;
        }
        Ok(())
    }

    /// The recipient row of `recipient` if it is the actor's, with its envelope.
    async fn mine(&self, tx: &mut WsTx<'_>, actor: &Actor, recipient: Uuid) -> R<(Envelope, Recipient)> {
        let env: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT envelope_id, membership_id FROM envelope_recipients WHERE id = $1")
                .bind(recipient)
                .fetch_optional(tx.conn())
                .await?;
        let Some((env, member)) = env.filter(|(_, m)| *m == actor.membership) else {
            return Err(SigningError::NotFound);
        };
        let _ = member;
        let e = self.get(tx, env).await?;
        if e.state == "draft" {
            return Err(SigningError::NotFound);
        }
        let r = e.recipients.iter().find(|r| r.id == recipient).cloned().ok_or(SigningError::NotFound)?;
        Ok((e, r))
    }

    /// Whether every signer before `r` has signed (always, in parallel mode).
    fn turn(e: &Envelope, r: &Recipient) -> bool {
        e.order_mode == "parallel"
            || e.recipients
                .iter()
                .filter(|x| x.role == "signer" && x.ord < r.ord)
                .all(|x| x.state == "signed")
    }

    /// What the signer sees: the envelope, their fields, whether it's their turn. Records the first view.
    pub async fn ceremony(&self, tx: &mut WsTx<'_>, actor: &Actor, recipient: Uuid) -> R<Value> {
        let (e, r) = self.mine(tx, actor, recipient).await?;
        let my_turn = r.role == "signer" && r.state == "pending" && e.state == "sent" && Self::turn(&e, &r);
        if r.viewed_at.is_none() && e.state == "sent" {
            sqlx::query("UPDATE envelope_recipients SET viewed_at = $2 WHERE id = $1 AND viewed_at IS NULL")
                .bind(r.id)
                .bind(self.clock.now())
                .execute(tx.conn())
                .await?;
            self.audit(tx, actor, "envelope.viewed", e.id, json!({ "recipient_id": r.id })).await?;
        }
        let expired = e.expires_at.is_some_and(|x| x <= self.clock.now()) && e.state == "sent";
        let mine: Vec<&Field> = e.fields.iter().filter(|f| f.recipient_id == r.id).collect();
        Ok(json!({
            "envelope": { "id": e.id, "title": e.title, "message": e.message, "sender_name": e.sender_name, "state": if expired { "expired" } else { e.state.as_str() },
                          "document_hash": e.document_hash, "pages": e.pages, "mode": e.mode, "order_mode": e.order_mode, "expires_at": e.expires_at.map(iso),
                          "completed_file_id": e.completed_file_id },
            "recipient": r,
            "my_turn": my_turn && !expired,
            "fields": mine,
            "progress": e.recipients.iter().filter(|x| x.role == "signer").map(|x| json!({ "name": x.name, "ord": x.ord, "state": x.state, "signed_at": x.signed_at.map(iso) })).collect::<Vec<_>>(),
            "consent_text": CONSENT,
            "meaning": MEANING,
        }))
    }

    /// Signs on the signer's turn; completes the envelope after the last signer.
    pub async fn sign(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        recipient: Uuid,
        input: &SignInput,
    ) -> R<Envelope> {
        sqlx::query("SELECT 1 FROM envelopes e JOIN envelope_recipients r ON r.workspace_id = e.workspace_id AND r.envelope_id = e.id WHERE r.id = $1 FOR UPDATE OF e")
            .bind(recipient).execute(tx.conn()).await?;
        let (e, r) = self.mine(tx, actor, recipient).await?;
        if r.role != "signer" {
            return Err(SigningError::Forbidden(
                "You receive a copy of this document; you don't sign it.".into(),
            ));
        }
        if r.state != "pending" {
            return Err(SigningError::Conflict(format!(
                "You have already {} this document.",
                if r.state == "signed" { "signed" } else { "declined" }
            )));
        }
        if e.state != "sent" {
            return Err(SigningError::Conflict(format!("This document is {} and can't be signed.", e.state)));
        }
        if e.expires_at.is_some_and(|x| x <= self.clock.now()) {
            return Err(SigningError::Conflict("This document expired before it was signed.".into()));
        }
        if !Self::turn(&e, &r) {
            return Err(SigningError::Conflict(
                "It isn't your turn yet: someone before you still has to sign.".into(),
            ));
        }
        if !input.document_hash.eq_ignore_ascii_case(&e.document_hash) {
            return Err(SigningError::Conflict(
                "The document you reviewed isn't the one sent for signature. Reload it and review it again."
                    .into(),
            ));
        }
        if !input.consent {
            return Err(SigningError::Invalid("Agree to sign electronically to continue.".into()));
        }
        let text = input.adopted.text.trim();
        if text.is_empty() || text.chars().count() > 100 {
            return Err(SigningError::Invalid(
                "Type your name to adopt a signature (up to 100 characters).".into(),
            ));
        }
        let image = match input.adopted.kind.as_str() {
            "typed" => None,
            "drawn" => {
                let raw = input.adopted.image.as_deref().unwrap_or_default();
                let b64 = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
                let bytes = base64_decode(b64)
                    .ok_or_else(|| SigningError::Invalid("The drawn signature couldn't be read.".into()))?;
                if bytes.is_empty() || bytes.len() > MAX_DRAWN {
                    return Err(SigningError::Invalid(
                        "Draw your signature again (it was empty or too large).".into(),
                    ));
                }
                pdf::check_png(&bytes)?; // a PNG we can place, before anything is recorded
                Some(bytes)
            }
            _ => return Err(SigningError::Invalid("A signature is typed or drawn.".into())),
        };
        let now = self.clock.now();
        let mine: Vec<&Field> = e.fields.iter().filter(|f| f.recipient_id == r.id).collect();
        for fid in input.values.keys() {
            if !mine.iter().any(|f| f.id == Some(*fid) && matches!(f.kind.as_str(), "text" | "checkbox")) {
                return Err(SigningError::Invalid(
                    "A value was sent for a field that isn't yours to fill.".into(),
                ));
            }
        }
        for f in &mine {
            let fid = f.id.unwrap_or_default();
            let value = match f.kind.as_str() {
                "signature" | "initials" => json!({ "adopted": input.adopted.kind }),
                "date_signed" => json!(day(now)),
                "name" => json!(r.name),
                "text" => match input.values.get(&fid) {
                    Some(Value::String(s)) if !s.trim().is_empty() => {
                        if s.chars().count() > 200 {
                            return Err(SigningError::Invalid(format!(
                                "“{}” can be at most 200 characters.",
                                f.label.as_deref().unwrap_or("Text")
                            )));
                        }
                        json!(s.trim())
                    }
                    _ if f.required => {
                        return Err(SigningError::Invalid(format!(
                            "Fill in “{}”.",
                            f.label.as_deref().unwrap_or("the text field")
                        )));
                    }
                    _ => continue,
                },
                "checkbox" => match input.values.get(&fid) {
                    Some(Value::Bool(b)) => {
                        if f.required && !b {
                            return Err(SigningError::Invalid(format!(
                                "Tick “{}”.",
                                f.label.as_deref().unwrap_or("the checkbox")
                            )));
                        }
                        json!(b)
                    }
                    _ if f.required => {
                        return Err(SigningError::Invalid(format!(
                            "Tick “{}”.",
                            f.label.as_deref().unwrap_or("the checkbox")
                        )));
                    }
                    _ => json!(false),
                },
                _ => continue,
            };
            sqlx::query("UPDATE envelope_fields SET value = $2 WHERE id = $1")
                .bind(fid)
                .bind(value)
                .execute(tx.conn())
                .await?;
        }
        let hash = hex_decode(&e.document_hash);
        sqlx::query(
            "INSERT INTO signatures (workspace_id, id, envelope_id, recipient_id, signer_subject_ref, record_hash, meaning, mode, auth_method, auth_time, adopted_kind, adopted_text, adopted_image, ip, user_agent, signed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(tx.ctx.workspace_id).bind(self.ids.new_id()).bind(e.id).bind(r.id).bind(actor.subject_ref).bind(&hash).bind(MEANING)
        .bind(&e.mode).bind(&actor.auth_method).bind(actor.auth_time).bind(&input.adopted.kind).bind(text).bind(image.as_deref())
        .bind(&actor.ip).bind(&actor.user_agent).bind(now)
        .execute(tx.conn()).await?;
        sqlx::query("UPDATE envelope_recipients SET state = 'signed', signed_at = $2, consented_at = $2, viewed_at = COALESCE(viewed_at, $2) WHERE id = $1")
            .bind(r.id).bind(now).execute(tx.conn()).await?;
        sqlx::query("UPDATE envelopes SET revision = revision + 1 WHERE id = $1")
            .bind(e.id)
            .execute(tx.conn())
            .await?;
        self.audit(tx, actor, "envelope.signed", e.id, json!({ "recipient_id": r.id, "document_hash": e.document_hash, "adopted": input.adopted.kind, "auth_method": actor.auth_method, "mode": e.mode })).await?;
        let e = self.get(tx, e.id).await?;
        if e.recipients.iter().filter(|x| x.role == "signer").all(|x| x.state == "signed") {
            self.complete(tx, actor, &e).await?;
        } else {
            self.notify_turn(tx, &e).await?;
        }
        self.get(tx, e.id).await
    }

    /// Stamps the PDF, appends the certificate, stores the signed copy and tells everyone.
    async fn complete(&self, tx: &mut WsTx<'_>, actor: &Actor, e: &Envelope) -> R<()> {
        let (_, src) = self.files.content(tx, e.source_file_id).await?;
        let sigs: Vec<SigRow> = sqlx::query_as(
            "SELECT recipient_id, adopted_kind, adopted_text, adopted_image, auth_method, auth_time, ip, signed_at FROM signatures WHERE envelope_id = $1",
        )
        .bind(e.id)
        .fetch_all(tx.conn())
        .await?;
        let sig_of = |rid: Uuid| sigs.iter().find(|s| s.0 == rid);
        let mut stamps = Vec::new();
        for f in &e.fields {
            let Some(v) = &f.value else { continue };
            let rect = pdf::Rect { x: f.x, y: f.y, w: f.w, h: f.h };
            let page = u32::try_from(f.page).unwrap_or(1);
            let mark = match (f.kind.as_str(), sig_of(f.recipient_id)) {
                ("signature", Some(s)) => match &s.3 {
                    Some(png) => pdf::Mark::Image(png.clone()),
                    None => pdf::Mark::Script(s.2.clone()),
                },
                ("initials", Some(s)) => match &s.3 {
                    Some(png) => pdf::Mark::Image(png.clone()),
                    None => pdf::Mark::Script(initials(&s.2)),
                },
                ("checkbox", _) if v == &json!(true) => pdf::Mark::Tick,
                ("checkbox", _) => continue,
                (_, _) => pdf::Mark::Text(v.as_str().unwrap_or_default().to_owned()),
            };
            stamps.push(pdf::Stamp { page, rect, mark });
        }
        let now = self.clock.now();
        let mut sections = vec![(
            "Document".to_owned(),
            vec![
                format!("Title: {}", e.title),
                format!("Envelope: {}", e.id),
                format!("Sent by {} on {}", e.sender_name, e.sent_at.map(stamp_time).unwrap_or_default()),
                format!("Original document SHA-256: {}", e.document_hash),
                format!(
                    "Pages: {} · Signing order: {} · Mode: {}",
                    e.pages.as_array().map_or(0, Vec::len),
                    e.order_mode,
                    e.mode
                ),
                format!("Completed on {}", stamp_time(now)),
            ],
        )];
        for r in e.recipients.iter().filter(|r| r.role == "signer") {
            let mut lines = vec![format!("{} <{}>", r.name, r.email)];
            if let Some(s) = sig_of(r.id) {
                lines.push(format!("Signed {} · {} signature · \"{}\"", stamp_time(s.7), s.1, s.2));
                lines.push(format!("Meaning: {MEANING}"));
                lines.push(format!(
                    "Authentication: {} at {} · IP {}",
                    s.4,
                    stamp_time(s.5),
                    s.6.as_deref().unwrap_or("not recorded")
                ));
            }
            if let Some(v) = r.viewed_at {
                lines.push(format!(
                    "Viewed {} · Consented to sign electronically {}",
                    stamp_time(v),
                    r.signed_at.map(stamp_time).unwrap_or_default()
                ));
            }
            sections.push((format!("Signer {}", r.ord), lines));
        }
        let ccs: Vec<String> = e
            .recipients
            .iter()
            .filter(|r| r.role == "cc")
            .map(|r| format!("{} <{}>", r.name, r.email))
            .collect();
        if !ccs.is_empty() {
            sections.push(("Copies to".into(), ccs));
        }
        sections.push((
            "About this certificate".into(),
            vec![
                "Generated by Procera when the last signer signed. The signatures are electronic signatures recorded with the".into(),
                "workspace's tamper-evident audit trail; any change to this file changes its SHA-256, which Procera keeps.".into(),
            ],
        ));
        let cert = pdf::Certificate { title: "Certificate of completion".into(), sections };
        let signed = pdf::stamp(&src, &stamps, &cert)?;
        let hash = Sha256::digest(&signed).to_vec();
        let name = format!("{} (signed).pdf", e.title.replace(['/', '\\'], "-"));
        let fa = actor.files();
        let file = self
            .files
            .create(tx, &fa, &name, i64::try_from(signed.len()).unwrap_or(i64::MAX), Some("application/pdf"))
            .await?;
        let mut offset = 0usize;
        for chunk in signed.chunks(procera_files::MAX_CHUNK) {
            self.files.append(tx, &fa, file.id, i64::try_from(offset).unwrap_or(0), chunk).await?;
            offset += chunk.len();
        }
        let file = self.files.complete(tx, &fa, file.id).await?;
        if file.state == "clean" {
            self.files.link(tx, &fa, file.id, "envelope", e.id).await?;
        }
        sqlx::query("UPDATE envelopes SET state = 'completed', completed_at = $2, completed_file_id = $3, completed_hash = $4, revision = revision + 1 WHERE id = $1")
            .bind(e.id).bind(now).bind(file.id).bind(&hash).execute(tx.conn()).await?;
        self.audit(tx, actor, "envelope.completed", e.id, json!({ "completed_file_id": file.id, "completed_hash": hex(&hash), "document_hash": e.document_hash })).await?;
        self.emit(
            tx,
            "envelope.completed.v1",
            e.id,
            json!({ "envelope_id": e.id, "state": "completed", "completed_file_id": file.id }),
        )
        .await?;
        let link = format!("/documents/{}", e.id);
        let mut told = std::collections::BTreeSet::new();
        let creator_email: Option<String> = sqlx::query_scalar(
            "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(e.created_by)
        .fetch_optional(tx.conn())
        .await?;
        for (to, email) in std::iter::once((e.created_by, creator_email))
            .chain(e.recipients.iter().map(|r| (r.membership_id, Some(r.email.clone()))))
        {
            if told.insert(to) {
                self.tell(
                    tx,
                    to,
                    email.as_deref(),
                    "signature_completed",
                    json!({ "title": e.title }),
                    &link,
                    e.id,
                    &format!("sign:{}:completed:{to}", e.id),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Declines: the envelope ends for everyone and the sender is told why.
    pub async fn decline(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        recipient: Uuid,
        reason: &str,
    ) -> R<Envelope> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 1000 {
            return Err(SigningError::Invalid("Say why you're declining (up to 1,000 characters).".into()));
        }
        sqlx::query("SELECT 1 FROM envelopes e JOIN envelope_recipients r ON r.workspace_id = e.workspace_id AND r.envelope_id = e.id WHERE r.id = $1 FOR UPDATE OF e")
            .bind(recipient).execute(tx.conn()).await?;
        let (e, r) = self.mine(tx, actor, recipient).await?;
        if r.role != "signer" || r.state != "pending" || e.state != "sent" {
            return Err(SigningError::Conflict("This document can't be declined any more.".into()));
        }
        let now = self.clock.now();
        sqlx::query("UPDATE envelope_recipients SET state = 'declined', declined_at = $2, decline_reason = $3 WHERE id = $1").bind(r.id).bind(now).bind(reason).execute(tx.conn()).await?;
        sqlx::query("UPDATE envelopes SET state = 'declined', ended_reason = $2, revision = revision + 1 WHERE id = $1").bind(e.id).bind(format!("{} declined: {reason}", r.name)).execute(tx.conn()).await?;
        self.audit(tx, actor, "envelope.declined", e.id, json!({ "recipient_id": r.id, "reason": reason }))
            .await?;
        self.emit(
            tx,
            "envelope.declined.v1",
            e.id,
            json!({ "envelope_id": e.id, "state": "declined", "recipient_id": r.id }),
        )
        .await?;
        let email: Option<String> = sqlx::query_scalar(
            "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(e.created_by)
        .fetch_optional(tx.conn())
        .await?;
        let link = format!("/documents/{}", e.id);
        self.tell(
            tx,
            e.created_by,
            email.as_deref(),
            "signature_declined",
            json!({ "title": e.title, "name": r.name, "reason": reason }),
            &link,
            e.id,
            &format!("sign:{}:declined", e.id),
        )
        .await?;
        self.get(tx, e.id).await
    }

    /// Cancels a sent envelope (`may_any`: the caller holds `document:void`; otherwise only the sender).
    pub async fn void(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        reason: &str,
        may_any: bool,
    ) -> R<Envelope> {
        self.void_inner(tx, actor, id, reason, may_any, true).await
    }

    /// [`Signing::void`]; a batch cancel audits once for all its copies (`audit` false here).
    async fn void_inner(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        reason: &str,
        may_any: bool,
        audit: bool,
    ) -> R<Envelope> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 1000 {
            return Err(SigningError::Invalid(
                "Say why you're cancelling it (up to 1,000 characters).".into(),
            ));
        }
        let row: Option<(String, Uuid)> =
            sqlx::query_as("SELECT state, created_by FROM envelopes WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?;
        let (state, by) = row.ok_or(SigningError::NotFound)?;
        if by != actor.membership && !may_any {
            return Err(SigningError::NotFound);
        }
        if state != "sent" {
            return Err(SigningError::Conflict(format!(
                "Only a sent document can be cancelled (this one is {state})."
            )));
        }
        sqlx::query(
            "UPDATE envelopes SET state = 'voided', ended_reason = $2, revision = revision + 1 WHERE id = $1",
        )
        .bind(id)
        .bind(reason)
        .execute(tx.conn())
        .await?;
        if audit {
            self.audit(tx, actor, "envelope.voided", id, json!({ "reason": reason })).await?;
        }
        self.emit(tx, "envelope.voided.v1", id, json!({ "envelope_id": id, "state": "voided" })).await?;
        let e = self.get(tx, id).await?;
        for r in e.recipients.iter().filter(|r| r.role == "signer" && r.state == "pending") {
            let told: Option<OffsetDateTime> =
                sqlx::query_scalar("SELECT notified_at FROM envelope_recipients WHERE id = $1")
                    .bind(r.id)
                    .fetch_one(tx.conn())
                    .await?;
            if told.is_some() {
                self.tell(
                    tx,
                    r.membership_id,
                    Some(&r.email),
                    "signature_voided",
                    json!({ "title": e.title, "sender": e.sender_name, "reason": reason }),
                    "/my-work?tab=signatures",
                    e.id,
                    &format!("sign:{}:voided", r.id),
                )
                .await?;
            }
        }
        Ok(e)
    }

    /// Envelopes waiting for the member's signature (their turn, not expired), then the rest they're on.
    pub async fn inbox(&self, tx: &mut WsTx<'_>, membership: Uuid) -> R<Vec<Value>> {
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT r.id, r.envelope_id FROM envelope_recipients r JOIN envelopes e ON e.workspace_id = r.workspace_id AND e.id = r.envelope_id
              WHERE r.membership_id = $1 AND e.state <> 'draft' ORDER BY e.sent_at DESC NULLS LAST LIMIT 500",
        )
        .bind(membership)
        .fetch_all(tx.conn())
        .await?;
        let now = self.clock.now();
        let mut out = Vec::new();
        for (rid, eid) in rows {
            let e = self.get(tx, eid).await?;
            let Some(r) = e.recipients.iter().find(|r| r.id == rid) else { continue };
            let expired = e.state == "sent" && e.expires_at.is_some_and(|x| x <= now);
            let to_sign = r.role == "signer"
                && r.state == "pending"
                && e.state == "sent"
                && !expired
                && Self::turn(&e, r);
            out.push(json!({
                "recipient_id": r.id, "envelope_id": e.id, "title": e.title, "sender_name": e.sender_name, "role": r.role,
                "state": r.state, "envelope_state": if expired { "expired" } else { e.state.as_str() }, "to_sign": to_sign,
                "sent_at": e.sent_at.map(iso), "expires_at": e.expires_at.map(iso), "signed_at": r.signed_at.map(iso), "completed_file_id": e.completed_file_id,
            }));
        }
        Ok(out)
    }

    /// The document bytes for someone who may see the envelope: the original or the signed copy.
    pub async fn document(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        id: Uuid,
        signed: bool,
        may_view_all: bool,
    ) -> R<(String, Vec<u8>)> {
        let e = self.get(tx, id).await?;
        let on_it = e.created_by == membership
            || e.recipients.iter().any(|r| r.membership_id == membership && (e.state != "draft"));
        if !on_it && !may_view_all {
            return Err(SigningError::NotFound);
        }
        let file = if signed {
            e.completed_file_id
                .ok_or_else(|| SigningError::Conflict("It isn't signed by everyone yet.".into()))?
        } else {
            e.source_file_id
        };
        let (_, bytes) = self.files.content(tx, file).await?;
        let name = if signed { format!("{} (signed).pdf", e.title) } else { format!("{}.pdf", e.title) };
        Ok((name, bytes))
    }
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_helpers() {
        assert_eq!(initials("ann marie asker"), "AMA");
        assert_eq!(hex_decode(&hex(&[0, 171, 255])), vec![0, 171, 255]);
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        let t = OffsetDateTime::from_unix_timestamp(1_791_964_800).unwrap();
        assert_eq!(day(t), "14 Oct 2026");
    }
}
