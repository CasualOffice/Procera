//! Announcements (BPMS-047, FR-AN01–AN06, docs/04 §2.7, docs/20 §2.17).
//!
//! Lifecycle: `draft → (pending_approval →) (scheduled →) published → archived`.
//! - Drafts are composed and edited by their author. A database trigger freezes title, content,
//!   audience and the acknowledgement requirement once an announcement leaves draft.
//! - Approval follows `announcements.approval_required` (`off`, `for_workspace_wide`, `always`). When
//!   it applies, the announcement is submitted and an approver who is not the author approves it;
//!   a rejection (with a comment) returns it to draft. When it does not apply, a publisher publishes
//!   it directly (or approves it if it was submitted anyway).
//! - Publishing snapshots the audience (`announcement_recipients`), freezes `content_hash`, and emits
//!   `announcement.published.v1`; the notification consumer delivers it after commit. A future
//!   `publish_at` schedules it and the scheduler publishes it when due.
//! - Recipients see it in their feed until `expires_at`; reads and acknowledgements are receipts
//!   on the snapshot. Archiving withdraws it (reason required). Nothing is deleted.
//!
//! Permissions are checked by the caller (HTTP layer); this crate enforces the lifecycle and SoD.

use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::outbox::{self, OutboxEvent};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_members::audience::{self, Audience};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AnnouncementError {
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("the announcement changed (revision {current})")]
    StaleRevision { current: i64 },
    #[error("separation of duties: {0}")]
    SeparationOfDuties(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for AnnouncementError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for AnnouncementError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for AnnouncementError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_members::MembersError> for AnnouncementError {
    fn from(e: procera_members::MembersError) -> Self {
        Self::Internal(e.to_string())
    }
}

/// Who acts: a membership, its audit subject and the client it acts from.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Workspace defaults applied when composing (docs/20 §2.17), resolved by the caller from settings.
#[derive(Debug, Clone, Copy)]
pub struct Defaults {
    /// `announcements.require_ack.default`.
    pub require_ack: bool,
    /// `announcements.expiry.default`.
    pub expiry: time::Duration,
}

/// Whether approval applies under `announcements.approval_required` for this audience.
pub fn approval_applies(mode: &str, audience: &Audience) -> bool {
    match mode {
        "always" => true,
        "for_workspace_wide" => audience.all,
        _ => false,
    }
}

fn empty_doc() -> Value {
    json!({"blocks": []})
}

fn normal() -> String {
    "normal".into()
}

/// What an author composes (create and full-replace edit of a draft).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compose {
    pub title: String,
    /// Block document (`{"blocks": [...]}`), the same shape as library content.
    #[serde(default = "empty_doc")]
    pub content: Value,
    pub audience: Audience,
    /// `normal` or `critical`.
    #[serde(default = "normal")]
    pub priority: String,
    #[serde(default)]
    pub pinned: bool,
    /// Defaults to `announcements.require_ack.default`.
    #[serde(default)]
    pub require_ack: Option<bool>,
    /// Publish at this time instead of when approved or published.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub publish_at: Option<OffsetDateTime>,
    /// Defaults to the publication time plus `announcements.expiry.default`.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

fn hex_opt<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(b) => s.serialize_some(&b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct Announcement {
    pub id: Uuid,
    pub title: String,
    pub content: Value,
    /// Hex SHA-256 of the canonical content, set at publication.
    #[serde(serialize_with = "hex_opt")]
    pub content_hash: Option<Vec<u8>>,
    pub audience: Value,
    pub priority: String,
    pub pinned: bool,
    pub require_ack: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub publish_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub state: String,
    /// Published and past its expiry: hidden from feeds (still `published`; archived, not deleted).
    #[sqlx(skip)]
    pub expired: bool,
    pub approval_required: bool,
    pub channels: Vec<String>,
    pub author_membership_id: Uuid,
    #[serde(with = "time::serde::rfc3339::option")]
    pub submitted_at: Option<OffsetDateTime>,
    pub decided_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub decided_at: Option<OffsetDateTime>,
    pub decision_comment: Option<String>,
    pub published_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    pub recipient_count: Option<i32>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
    pub archive_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub revision: i64,
}

impl Announcement {
    pub fn audience(&self) -> Result<Audience, AnnouncementError> {
        serde_json::from_value(self.audience.clone()).map_err(|e| AnnouncementError::Internal(e.to_string()))
    }
}

const COLS: &str = "id, title, content, content_hash, audience, priority, pinned, require_ack, publish_at, expires_at, state, approval_required, channels, author_membership_id, submitted_at, decided_by, decided_at, decision_comment, published_by, published_at, recipient_count, archived_at, archive_reason, created_at, revision";

/// One item of a member's feed, with their receipt.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct FeedItem {
    pub id: Uuid,
    pub title: String,
    pub content: Value,
    pub priority: String,
    pub pinned: bool,
    pub require_ack: bool,
    pub author_membership_id: Uuid,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub acked_at: Option<OffsetDateTime>,
}

/// A recipient's receipt after reading or acknowledging.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Receipt {
    pub announcement_id: Uuid,
    #[serde(with = "time::serde::rfc3339::option")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub acked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct DepartmentStats {
    /// `None` for recipients without a department.
    pub department_id: Option<Uuid>,
    pub department: Option<String>,
    pub recipients: i64,
    pub read: i64,
    pub acknowledged: i64,
}

/// Reach and receipts (counts only; individual receipts stay private, docs/05 §3.23).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analytics {
    pub announcement_id: Uuid,
    pub state: String,
    pub require_ack: bool,
    pub recipients: i64,
    pub read: i64,
    pub acknowledged: i64,
    pub pending_ack: i64,
    pub by_department: Vec<DepartmentStats>,
}

/// What the notification consumer needs to deliver a published announcement.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub id: Uuid,
    pub title: String,
    pub critical: bool,
    pub require_ack: bool,
    pub channels: Vec<String>,
    pub author: String,
    pub workspace: String,
    /// (membership, email) of every recipient.
    pub recipients: Vec<(Uuid, String)>,
}

/// Which announcements a management list includes.
#[derive(Debug, Clone, Copy)]
pub struct ListScope {
    /// Everything (`announcement:manage`).
    pub all: bool,
    /// Everyone's submitted, scheduled, published and archived ones (`announcement:publish`).
    pub others_submitted: bool,
    /// The caller, whose own drafts are always included.
    pub me: Uuid,
}

/// Canonical hash of content. Same content, same hash, whatever the key order.
pub fn content_hash(content: &Value) -> Result<Vec<u8>, AnnouncementError> {
    let bytes = serde_jcs::to_vec(content).map_err(|e| AnnouncementError::Internal(e.to_string()))?;
    Ok(Sha256::digest(bytes).to_vec())
}

fn check_content(content: &Value) -> Result<(), AnnouncementError> {
    if !content.is_object() {
        return Err(AnnouncementError::Invalid("content must be a JSON object".into()));
    }
    if serde_json::to_vec(content).map(|v| v.len()).unwrap_or(usize::MAX) > 1_048_576 {
        return Err(AnnouncementError::Invalid("content is larger than 1 MB".into()));
    }
    Ok(())
}

#[derive(Clone)]
pub struct Announcements {
    db: Db,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Announcements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Announcements")
    }
}

impl Announcements {
    pub fn new(db: Db, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { db, clock, ids }
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        action: &str,
        id: Uuid,
        diff: Value,
        reason: Option<&str>,
    ) -> Result<(), AnnouncementError> {
        let kind = if actor.is_some() { ActorKind::User } else { ActorKind::System };
        let mut ev =
            AuditEvent::new(actor.map(|a| a.subject_ref), kind, Category::Content, action, Outcome::Allowed)
                .object("announcement", id);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    fn finish(&self, mut a: Announcement) -> Announcement {
        a.expired = a.state == "published" && a.expires_at <= self.clock.now();
        a
    }

    async fn load(&self, tx: &mut WsTx<'_>, id: Uuid, lock: bool) -> Result<Announcement, AnnouncementError> {
        let a: Option<Announcement> = sqlx::query_as(&format!(
            "SELECT {COLS} FROM announcements WHERE id = $1{}",
            if lock { " FOR UPDATE" } else { "" }
        ))
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        a.map(|a| self.finish(a)).ok_or(AnnouncementError::NotFound)
    }

    pub async fn get(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Announcement, AnnouncementError> {
        self.load(tx, id, false).await
    }

    /// Management list, newest first.
    pub async fn list(
        &self,
        tx: &mut WsTx<'_>,
        scope: ListScope,
        state: Option<&str>,
    ) -> Result<Vec<Announcement>, AnnouncementError> {
        let rows: Vec<Announcement> = sqlx::query_as(&format!(
            "SELECT {COLS} FROM announcements
             WHERE ($1 OR author_membership_id = $3 OR ($2 AND state <> 'draft')) AND ($4::text IS NULL OR state = $4)
             ORDER BY created_at DESC, id DESC LIMIT 200"
        ))
        .bind(scope.all)
        .bind(scope.others_submitted)
        .bind(scope.me)
        .bind(state)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|a| self.finish(a)).collect())
    }

    /// Validates a composition; returns (title, require_ack, expires_at).
    async fn validate(
        &self,
        tx: &mut WsTx<'_>,
        c: &Compose,
        d: &Defaults,
    ) -> Result<(String, bool, OffsetDateTime), AnnouncementError> {
        let title = c.title.trim().to_owned();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(AnnouncementError::Invalid("title must be 1-200 characters".into()));
        }
        check_content(&c.content)?;
        if !matches!(c.priority.as_str(), "normal" | "critical") {
            return Err(AnnouncementError::Invalid("priority must be normal or critical".into()));
        }
        if c.audience.is_empty() {
            return Err(AnnouncementError::Invalid(
                "the audience is empty: choose everyone, departments, groups or roles".into(),
            ));
        }
        let known_departments: i64 =
            sqlx::query_scalar("SELECT count(*) FROM departments WHERE id = ANY($1)")
                .bind(&c.audience.departments)
                .fetch_one(tx.conn())
                .await?;
        let known_groups: i64 = sqlx::query_scalar("SELECT count(*) FROM groups WHERE id = ANY($1)")
            .bind(&c.audience.groups)
            .fetch_one(tx.conn())
            .await?;
        let distinct = |v: &[Uuid]| v.iter().collect::<std::collections::BTreeSet<_>>().len() as i64;
        if known_departments != distinct(&c.audience.departments)
            || known_groups != distinct(&c.audience.groups)
        {
            return Err(AnnouncementError::Invalid(
                "the audience names an unknown department or group".into(),
            ));
        }
        let start = c.publish_at.map_or(self.clock.now(), |p| p.max(self.clock.now()));
        let expires_at = c.expires_at.unwrap_or(start + d.expiry);
        if expires_at <= start {
            return Err(AnnouncementError::Invalid("expires_at must be after the publication time".into()));
        }
        Ok((title, c.require_ack.unwrap_or(d.require_ack), expires_at))
    }

    /// Creates a draft.
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        c: &Compose,
        d: &Defaults,
    ) -> Result<Announcement, AnnouncementError> {
        let (title, require_ack, expires_at) = self.validate(tx, c, d).await?;
        let id = self.ids.new_id();
        sqlx::query("INSERT INTO announcements (workspace_id, id, title, content, audience, priority, pinned, require_ack, publish_at, expires_at, author_membership_id, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(tx.ctx.workspace_id).bind(id).bind(&title).bind(&c.content).bind(json!(c.audience)).bind(&c.priority)
            .bind(c.pinned).bind(require_ack).bind(c.publish_at).bind(expires_at).bind(actor.membership).bind(self.clock.now())
            .execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "announcement.created",
            id,
            json!({"priority": c.priority, "pinned": c.pinned, "require_ack": require_ack, "audience": c.audience,
                   "publish_at": c.publish_at.map(|t| t.unix_timestamp()), "expires_at": expires_at.unix_timestamp()}),
            None,
        )
        .await?;
        self.load(tx, id, false).await
    }

    /// Replaces a draft (optimistic concurrency on `revision`).
    pub async fn edit(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        c: &Compose,
        d: &Defaults,
        expected_revision: Option<i64>,
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if a.state != "draft" {
            return Err(AnnouncementError::Conflict(format!(
                "only drafts can be edited (this announcement is {})",
                a.state
            )));
        }
        if let Some(exp) = expected_revision
            && exp != a.revision
        {
            return Err(AnnouncementError::StaleRevision { current: a.revision });
        }
        let (title, require_ack, expires_at) = self.validate(tx, c, d).await?;
        sqlx::query("UPDATE announcements SET title = $2, content = $3, audience = $4, priority = $5, pinned = $6, require_ack = $7, publish_at = $8, expires_at = $9, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(&title).bind(&c.content).bind(json!(c.audience)).bind(&c.priority).bind(c.pinned)
            .bind(require_ack).bind(c.publish_at).bind(expires_at).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "announcement.edited",
            id,
            json!({"priority": c.priority, "pinned": c.pinned, "require_ack": require_ack, "audience": c.audience,
                   "publish_at": c.publish_at.map(|t| t.unix_timestamp()), "expires_at": expires_at.unix_timestamp()}),
            None,
        )
        .await?;
        self.load(tx, id, false).await
    }

    /// Submits a draft for approval. `mode` is `announcements.approval_required`.
    pub async fn submit(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        mode: &str,
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if a.state != "draft" {
            return Err(AnnouncementError::Conflict(format!(
                "only drafts can be submitted (this announcement is {})",
                a.state
            )));
        }
        let required = approval_applies(mode, &a.audience()?);
        sqlx::query("UPDATE announcements SET state = 'pending_approval', approval_required = $2, submitted_at = $3, decided_by = NULL, decided_at = NULL, decision_comment = NULL, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(required).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "announcement.submitted",
            id,
            json!({"approval_required": required}),
            None,
        )
        .await?;
        if required {
            // Approvers are told it is waiting (in-app and email, after commit).
            Self::decision_event(tx, "announcement.submitted.v1", &a, json!({})).await?;
        }
        self.load(tx, id, false).await
    }

    /// Approves a submitted announcement and publishes or schedules it. When approval applies, the
    /// approver must not be the author.
    pub async fn approve(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        channels: &[String],
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if a.state != "pending_approval" {
            return Err(AnnouncementError::Conflict(format!(
                "this announcement is {}, not pending approval",
                a.state
            )));
        }
        if a.approval_required && a.author_membership_id == actor.membership {
            return Err(AnnouncementError::SeparationOfDuties(
                "authors cannot approve their own announcement".into(),
            ));
        }
        sqlx::query("UPDATE announcements SET decided_by = $2, decided_at = $3, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(actor.membership).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "announcement.approved",
            id,
            json!({"approval_required": a.approval_required}),
            None,
        )
        .await?;
        if a.approval_required && a.author_membership_id != actor.membership {
            Self::decision_event(
                tx,
                "announcement.decided.v1",
                &a,
                json!({"outcome": "approved", "comment": null}),
            )
            .await?;
        }
        self.go_live(tx, actor, a, channels).await
    }

    /// Rejects a submitted announcement back to draft (a comment is required).
    pub async fn reject(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        comment: &str,
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if a.state != "pending_approval" {
            return Err(AnnouncementError::Conflict(format!(
                "this announcement is {}, not pending approval",
                a.state
            )));
        }
        let comment = comment.trim();
        if comment.is_empty() {
            return Err(AnnouncementError::Invalid("a comment is required to reject".into()));
        }
        sqlx::query("UPDATE announcements SET state = 'draft', decided_by = $2, decided_at = $3, decision_comment = $4, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(actor.membership).bind(self.clock.now()).bind(comment).execute(tx.conn()).await?;
        self.audit(tx, Some(actor), "announcement.rejected", id, json!({}), Some(comment)).await?;
        Self::decision_event(
            tx,
            "announcement.decided.v1",
            &a,
            json!({"outcome": "rejected", "comment": comment}),
        )
        .await?;
        self.load(tx, id, false).await
    }

    /// An approval-workflow event for the bus: who wrote it and what it is called.
    async fn decision_event(
        tx: &mut WsTx<'_>,
        topic: &'static str,
        a: &Announcement,
        extra: serde_json::Value,
    ) -> Result<(), AnnouncementError> {
        let mut payload = json!({"announcement_id": a.id, "title": a.title, "author_membership_id": a.author_membership_id});
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic,
                aggregate_type: "announcement",
                aggregate_id: a.id,
                aggregate_revision: a.revision + 1,
                payload,
                trace_id: None,
            },
        )
        .await?;
        Ok(())
    }

    /// Publishes (or schedules) a draft directly; refused when approval applies.
    pub async fn publish(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        mode: &str,
        channels: &[String],
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if a.state != "draft" {
            return Err(AnnouncementError::Conflict(format!(
                "only drafts can be published directly (this announcement is {})",
                a.state
            )));
        }
        if approval_applies(mode, &a.audience()?) {
            return Err(AnnouncementError::Conflict(
                "this announcement needs approval: submit it for approval".into(),
            ));
        }
        self.go_live(tx, actor, a, channels).await
    }

    async fn go_live(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        a: Announcement,
        channels: &[String],
    ) -> Result<Announcement, AnnouncementError> {
        let now = self.clock.now();
        if a.expires_at <= now {
            return Err(AnnouncementError::Invalid(
                "this announcement has already expired; edit its expiry first".into(),
            ));
        }
        sqlx::query("UPDATE announcements SET channels = $2 WHERE id = $1")
            .bind(a.id)
            .bind(channels)
            .execute(tx.conn())
            .await?;
        match a.publish_at.filter(|t| *t > now) {
            Some(at) => {
                sqlx::query(
                    "UPDATE announcements SET state = 'scheduled', revision = revision + 1 WHERE id = $1",
                )
                .bind(a.id)
                .execute(tx.conn())
                .await?;
                self.audit(
                    tx,
                    Some(actor),
                    "announcement.scheduled",
                    a.id,
                    json!({"publish_at": at.unix_timestamp(), "channels": channels}),
                    None,
                )
                .await?;
            }
            None => self.publish_now(tx, Some(actor), &a).await?,
        }
        self.load(tx, a.id, false).await
    }

    async fn publish_now(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        a: &Announcement,
    ) -> Result<(), AnnouncementError> {
        let now = self.clock.now();
        let recipients = audience::resolve(tx, &a.audience()?).await?;
        sqlx::query("INSERT INTO announcement_recipients (workspace_id, announcement_id, membership_id) SELECT $1, $2, unnest($3::uuid[])")
            .bind(tx.ctx.workspace_id).bind(a.id).bind(&recipients).execute(tx.conn()).await?;
        let hash = content_hash(&a.content)?;
        let count = i32::try_from(recipients.len()).unwrap_or(i32::MAX);
        let revision: i64 = sqlx::query_scalar("UPDATE announcements SET state = 'published', published_at = $2, published_by = $3, recipient_count = $4, content_hash = $5, revision = revision + 1 WHERE id = $1 RETURNING revision")
            .bind(a.id).bind(now).bind(actor.map(|x| x.membership)).bind(count).bind(&hash).fetch_one(tx.conn()).await?;
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic: "announcement.published.v1",
                aggregate_type: "announcement",
                aggregate_id: a.id,
                aggregate_revision: revision,
                payload: json!({"announcement_id": a.id, "recipients": count}),
                trace_id: None,
            },
        )
        .await?;
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        self.audit(
            tx,
            actor,
            "announcement.published",
            a.id,
            json!({"recipients": count, "content_hash": hex}),
            None,
        )
        .await?;
        Ok(())
    }

    /// Withdraws a scheduled or published announcement (reason required).
    pub async fn archive(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        reason: &str,
    ) -> Result<Announcement, AnnouncementError> {
        let a = self.load(tx, id, true).await?;
        if !matches!(a.state.as_str(), "scheduled" | "published") {
            return Err(AnnouncementError::Conflict(format!(
                "only scheduled or published announcements can be archived (this announcement is {})",
                a.state
            )));
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(AnnouncementError::Invalid("a reason is required to archive".into()));
        }
        sqlx::query("UPDATE announcements SET state = 'archived', archived_by = $2, archived_at = $3, archive_reason = $4, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(actor.membership).bind(self.clock.now()).bind(reason).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "announcement.archived",
            id,
            json!({"was": a.state, "expired": a.expired}),
            Some(reason),
        )
        .await?;
        self.load(tx, id, false).await
    }

    /// A member's feed: published, unexpired announcements they received; pinned first, newest first.
    pub async fn feed(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        unread_only: bool,
    ) -> Result<Vec<FeedItem>, AnnouncementError> {
        Ok(sqlx::query_as(
            "SELECT a.id, a.title, a.content, a.priority, a.pinned, a.require_ack, a.author_membership_id, a.published_at, a.expires_at, r.read_at, r.acked_at
             FROM announcement_recipients r JOIN announcements a ON a.workspace_id = r.workspace_id AND a.id = r.announcement_id
             WHERE r.membership_id = $1 AND a.state = 'published' AND a.expires_at > $2 AND ($3 = false OR r.read_at IS NULL)
             ORDER BY a.pinned DESC, a.published_at DESC, a.id DESC LIMIT 200",
        )
        .bind(membership)
        .bind(self.clock.now())
        .bind(unread_only)
        .fetch_all(tx.conn())
        .await?)
    }

    /// Whether `membership` received this announcement and it is live (published, unexpired).
    pub async fn is_live_recipient(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        membership: Uuid,
    ) -> Result<bool, AnnouncementError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM announcement_recipients r JOIN announcements a ON a.workspace_id = r.workspace_id AND a.id = r.announcement_id
             WHERE r.announcement_id = $1 AND r.membership_id = $2 AND a.state = 'published' AND a.expires_at > $3)",
        )
        .bind(id)
        .bind(membership)
        .bind(self.clock.now())
        .fetch_one(tx.conn())
        .await?)
    }

    /// The caller's receipt row, locked; `NotFound` unless they received it and it is live.
    async fn live_receipt(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        membership: Uuid,
    ) -> Result<(Option<OffsetDateTime>, Option<OffsetDateTime>, bool, Option<Vec<u8>>), AnnouncementError>
    {
        let r: Option<(Option<OffsetDateTime>, Option<OffsetDateTime>, bool, Option<Vec<u8>>)> = sqlx::query_as(
            "SELECT r.read_at, r.acked_at, a.require_ack, a.content_hash
             FROM announcement_recipients r JOIN announcements a ON a.workspace_id = r.workspace_id AND a.id = r.announcement_id
             WHERE r.announcement_id = $1 AND r.membership_id = $2 AND a.state = 'published' AND a.expires_at > $3
             FOR UPDATE OF r",
        )
        .bind(id)
        .bind(membership)
        .bind(self.clock.now())
        .fetch_optional(tx.conn())
        .await?;
        r.ok_or(AnnouncementError::NotFound)
    }

    /// Records that the caller read it (first read only; later calls change nothing).
    pub async fn mark_read(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<Receipt, AnnouncementError> {
        let (read_at, acked_at, _, _) = self.live_receipt(tx, id, actor.membership).await?;
        if let Some(read_at) = read_at {
            return Ok(Receipt { announcement_id: id, read_at: Some(read_at), acked_at });
        }
        let now = self.clock.now();
        sqlx::query("UPDATE announcement_recipients SET read_at = $3 WHERE announcement_id = $1 AND membership_id = $2")
            .bind(id).bind(actor.membership).bind(now).execute(tx.conn()).await?;
        self.audit(tx, Some(actor), "announcement.read", id, json!({}), None).await?;
        Ok(Receipt { announcement_id: id, read_at: Some(now), acked_at })
    }

    /// Acknowledges an announcement that requires it, recording time, client and content hash
    /// (FR-AN04). Acknowledging again changes nothing.
    pub async fn acknowledge(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<Receipt, AnnouncementError> {
        let (read_at, acked_at, require_ack, hash) = self.live_receipt(tx, id, actor.membership).await?;
        if !require_ack {
            return Err(AnnouncementError::Conflict(
                "this announcement does not ask for acknowledgement".into(),
            ));
        }
        if acked_at.is_some() {
            return Ok(Receipt { announcement_id: id, read_at, acked_at });
        }
        let now = self.clock.now();
        sqlx::query("UPDATE announcement_recipients SET acked_at = $3, read_at = coalesce(read_at, $3), ack_ip = $4, ack_user_agent = $5 WHERE announcement_id = $1 AND membership_id = $2")
            .bind(id).bind(actor.membership).bind(now).bind(&actor.ip).bind(&actor.user_agent).execute(tx.conn()).await?;
        let hex: Option<String> = hash.map(|h| h.iter().map(|b| format!("{b:02x}")).collect());
        self.audit(tx, Some(actor), "announcement.acknowledged", id, json!({"content_hash": hex}), None)
            .await?;
        Ok(Receipt { announcement_id: id, read_at: Some(read_at.unwrap_or(now)), acked_at: Some(now) })
    }

    /// Reach and receipts, overall and by department.
    pub async fn analytics(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Analytics, AnnouncementError> {
        let a = self.load(tx, id, false).await?;
        let (recipients, read, acknowledged): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), count(read_at), count(acked_at) FROM announcement_recipients WHERE announcement_id = $1",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        let by_department: Vec<DepartmentStats> = sqlx::query_as(
            "SELECT d.id AS department_id, d.name AS department, count(*) AS recipients, count(r.read_at) AS read, count(r.acked_at) AS acknowledged
             FROM announcement_recipients r
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
             LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
             WHERE r.announcement_id = $1 GROUP BY d.id, d.name ORDER BY d.name NULLS LAST",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        Ok(Analytics {
            announcement_id: id,
            state: a.state,
            require_ack: a.require_ack,
            recipients,
            read,
            acknowledged,
            pending_ack: if a.require_ack { recipients - acknowledged } else { 0 },
            by_department,
        })
    }

    /// Delivery details for the notification consumer; `None` unless it is published.
    pub async fn delivery(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Option<Delivery>, AnnouncementError> {
        let head: Option<(String, String, bool, Vec<String>, String, String)> = sqlx::query_as(
            "SELECT a.title, a.priority, a.require_ack, a.channels, u.display_name, w.name
             FROM announcements a
             JOIN memberships m ON m.workspace_id = a.workspace_id AND m.id = a.author_membership_id
             JOIN users u ON u.id = m.user_id
             JOIN workspaces w ON w.id = a.workspace_id
             WHERE a.id = $1 AND a.state = 'published'",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        let Some((title, priority, require_ack, channels, author, workspace)) = head else {
            return Ok(None);
        };
        let recipients: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT r.membership_id, u.email::text FROM announcement_recipients r
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
             JOIN users u ON u.id = m.user_id
             WHERE r.announcement_id = $1 AND m.status = 'active' ORDER BY r.membership_id",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        Ok(Some(Delivery {
            id,
            title,
            critical: priority == "critical",
            require_ack,
            channels,
            author,
            workspace,
            recipients,
        }))
    }

    /// Publishes scheduled announcements whose time has come (scheduler). Returns how many.
    pub async fn publish_due(&self) -> Result<usize, AnnouncementError> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.announcements_due_workspaces($1, 1000)")
                .bind(self.clock.now())
                .fetch_all(self.db.pool())
                .await?;
        let mut n = 0;
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let due: Vec<Announcement> = sqlx::query_as(&format!(
                "SELECT {COLS} FROM announcements WHERE state = 'scheduled' AND publish_at <= $1 ORDER BY publish_at FOR UPDATE SKIP LOCKED"
            ))
            .bind(self.clock.now())
            .fetch_all(tx.conn())
            .await?;
            for a in due {
                self.publish_now(&mut tx, None, &a).await?;
                n += 1;
            }
            tx.commit().await?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_modes() {
        let everyone = Audience { all: true, ..Audience::default() };
        let team = Audience { roles: vec!["member".into()], ..Audience::default() };
        assert!(!approval_applies("off", &everyone));
        assert!(approval_applies("for_workspace_wide", &everyone));
        assert!(!approval_applies("for_workspace_wide", &team));
        assert!(approval_applies("always", &team));
    }

    #[test]
    fn content_hash_ignores_key_order() -> Result<(), AnnouncementError> {
        let a = json!({"blocks": [{"type": "p", "text": "x"}], "v": 1});
        let b = json!({"v": 1, "blocks": [{"text": "x", "type": "p"}]});
        assert_eq!(content_hash(&a)?, content_hash(&b)?);
        assert_eq!(content_hash(&a)?.len(), 32);
        Ok(())
    }
}
