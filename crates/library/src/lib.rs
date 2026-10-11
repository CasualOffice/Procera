//! The governed library (BPMS-040, FR-G04/G05, docs/04 §2.4).
//!
//! Lifecycle: `draft → in_review → approved → (scheduled →) published → deprecated → retired`.
//! - One open version (draft or in review) per artifact; a new version starts from the latest content.
//! - Reviews: the number of approvals comes from the workspace review policy. Authors never review
//!   their own version; approvers are distinct people; a reject or change request returns it to draft.
//! - Approval freezes `content_hash = SHA-256(JCS(content))`; a database trigger forbids changing
//!   approved content and moving back to draft.
//! - Publishing supersedes the previous published version (→ deprecated) and emits
//!   `artifact.published.v1`. A future `effective_at` schedules it; the worker publishes when due.
//!
//! Permissions and step-up are checked by the caller (HTTP layer); this crate enforces the lifecycle.

pub mod blocks;
pub mod collections;
pub mod diff;
pub mod docx;
pub mod governance;
pub mod portal;
pub mod reack;

pub use reack::{Change, PublishPlan, ReackPolicy, ReackSubject};

use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::outbox::{self, OutboxEvent};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("an artifact with this key already exists")]
    KeyTaken,
    #[error("the version changed (revision {current})")]
    StaleRevision { current: i64 },
    #[error("separation of duties: {0}")]
    SeparationOfDuties(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for LibraryError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for LibraryError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_settings::SettingsError> for LibraryError {
    fn from(e: procera_settings::SettingsError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for LibraryError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}

/// Artifact kinds that can be authored today. Processes, forms and rules arrive with the designer.
pub const AUTHORABLE: &[&str] = &["sop", "policy"];

/// The permission for `action` on `kind` (docs/12 §5). Submitting uses `edit_draft` and retiring
/// uses `publish` for document kinds, which have no separate permissions; so do the BPMS-044
/// governance actions (see below).
pub fn permission(kind: &str, action: &str) -> Result<&'static str, LibraryError> {
    Ok(match (kind, action) {
        ("sop", "view") => "sop:view",
        ("sop", "create") => "sop:create",
        ("sop", "edit_draft" | "submit") => "sop:edit_draft",
        ("sop", "review") => "sop:review",
        ("sop", "approve") => "sop:approve",
        ("sop", "publish" | "retire") => "sop:publish",
        ("policy", "view") => "policy:view",
        ("policy", "create") => "policy:create",
        ("policy", "edit_draft" | "submit") => "policy:edit_draft",
        ("policy", "review") => "policy:review",
        ("policy", "approve") => "policy:approve",
        ("policy", "publish" | "retire") => "policy:publish",
        // BPMS-044: readers may propose changes; owners/publishers recertify and resolve them;
        // approvers verify.
        ("sop", "propose_change") => "sop:view",
        ("policy", "propose_change") => "policy:view",
        ("sop", "recertify" | "resolve_change") => "sop:publish",
        ("policy", "recertify" | "resolve_change") => "policy:publish",
        ("sop", "verify") => "sop:approve",
        ("policy", "verify") => "policy:approve",
        _ => return Err(LibraryError::Invalid(format!("kind must be one of {}", AUTHORABLE.join(", ")))),
    })
}

/// Who acts: a membership, its audit subject and the client it acts from.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// The workspace review policy (docs/20 §2.6), resolved by the caller from settings.
#[derive(Debug, Clone)]
pub struct ReviewPolicy {
    /// `none`, `controlled` or `all`.
    pub required_for: String,
    pub min_reviewers: i64,
    pub reason_for_change_required: bool,
}

impl ReviewPolicy {
    /// Approvals needed for an artifact of `criticality`. Controlled artifacts always need one.
    pub fn required(&self, criticality: &str) -> i64 {
        let controlled = criticality == "controlled";
        let needs = match self.required_for.as_str() {
            "all" => true,
            "controlled" => controlled,
            _ => false,
        };
        if controlled || needs { self.min_reviewers.max(1) } else { 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Artifact {
    pub id: Uuid,
    pub kind: String,
    pub key: String,
    pub title: String,
    pub owner_membership_id: Option<Uuid>,
    pub criticality: String,
    pub state: String,
    pub current_published_version_id: Option<Uuid>,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Review cadence, recertification and verification (BPMS-044).
    pub governance: Governance,
    /// Policy category (BPMS-045), free text.
    pub category: Option<String>,
    /// Who the artifact applies to (an audience; `None` = everyone).
    pub applicability: Option<Value>,
    /// Hidden from readers outside `applicability` (A-14).
    pub restricted: bool,
    /// The collection it is filed in (BPMS-041); `None` = top level.
    pub collection_id: Option<Uuid>,
}

/// Recertification and verification state of an artifact (BPMS-044, FR-G07, FX-G01/G02).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Governance {
    /// Per-artifact override in days; `None` uses the workspace default for its criticality.
    pub review_cadence_days: Option<i32>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_review_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_reviewed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub review_overdue_at: Option<OffsetDateTime>,
    /// `none` (never published), `current`, `due_soon` (within 30 days) or `overdue`.
    pub review_status: &'static str,
    /// True while a verification is in force.
    pub verified: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub verified_at: Option<OffsetDateTime>,
    pub verified_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub verification_expires_at: Option<OffsetDateTime>,
}

/// How close to its review date an artifact counts as "due soon".
pub const DUE_SOON: time::Duration = time::Duration::days(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewRecord {
    pub reviewer_membership_id: Uuid,
    pub decision: String,
    pub comment: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub decided_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Version {
    pub id: Uuid,
    pub artifact_id: Uuid,
    pub version_no: i32,
    pub lifecycle: String,
    pub content: Value,
    /// Hex SHA-256 of the canonical content, set at approval.
    pub content_hash: Option<String>,
    pub change_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub effective_at: Option<OffsetDateTime>,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    pub revision: i64,
    /// Reviews in the current review round (since the last submission).
    pub reviews: Vec<ReviewRecord>,
    pub approvals_required: Option<i64>,
    /// `major.minor`, set when the version is published or scheduled (BPMS-045, FR-PL03).
    pub semver: Option<String>,
    /// Whether going live asks the applicable audience to acknowledge again.
    pub reack_required: bool,
}

/// Canonical hash of content (hex). Same content, same hash, whatever the key order.
pub fn content_hash(content: &Value) -> Result<String, LibraryError> {
    let bytes = serde_jcs::to_vec(content).map_err(|e| LibraryError::Internal(e.to_string()))?;
    Ok(Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect())
}

fn hex_to_bytes(h: &str) -> Vec<u8> {
    (0..h.len()).step_by(2).filter_map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect()
}

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn check_content(content: &Value) -> Result<(), LibraryError> {
    if !content.is_object() {
        return Err(LibraryError::Invalid("content must be a JSON object".into()));
    }
    if serde_json::to_vec(content).map(|v| v.len()).unwrap_or(usize::MAX) > 1_048_576 {
        return Err(LibraryError::Invalid("content is larger than 1 MB".into()));
    }
    // The block model (ADR-0013): known block types and safe links only.
    blocks::validate(content)
}

type VersionRow = (
    Uuid,
    Uuid,
    i32,
    String,
    Value,
    Option<Vec<u8>>,
    Option<String>,
    Option<OffsetDateTime>,
    Uuid,
    OffsetDateTime,
    Option<OffsetDateTime>,
    i32,
    i64,
    Option<String>,
    bool,
);
const VERSION_COLS: &str = "id, artifact_id, version_no, lifecycle, content, content_hash, change_reason, effective_at, created_by, created_at, published_at, review_round, revision, semver, reack_required";

#[derive(sqlx::FromRow)]
struct ArtifactRow {
    id: Uuid,
    kind: String,
    key: String,
    title: String,
    owner_membership_id: Option<Uuid>,
    criticality: String,
    state: String,
    current_published_version_id: Option<Uuid>,
    revision: i64,
    created_at: OffsetDateTime,
    review_cadence_days: Option<i32>,
    next_review_at: Option<OffsetDateTime>,
    last_reviewed_at: Option<OffsetDateTime>,
    review_overdue_at: Option<OffsetDateTime>,
    verified_at: Option<OffsetDateTime>,
    verified_by: Option<Uuid>,
    verification_expires_at: Option<OffsetDateTime>,
    category: Option<String>,
    applicability: Option<Value>,
    restricted: bool,
    collection_id: Option<Uuid>,
}
const ARTIFACT_COLS: &str = "id, kind, key, title, owner_membership_id, criticality, state, current_published_version_id, revision, created_at, review_cadence_days, next_review_at, last_reviewed_at, review_overdue_at, verified_at, verified_by, verification_expires_at, category, applicability, restricted, collection_id";

/// Who is reading (A-14): their membership and the kinds they hold an authoring permission on.
#[derive(Debug, Clone, Copy)]
pub struct Reader<'a> {
    pub membership: Uuid,
    pub full_kinds: &'a [&'a str],
}

impl Reader<'_> {
    pub fn full(&self, kind: &str) -> bool {
        self.full_kinds.contains(&kind)
    }
}

/// Lifecycles a reader without authoring permissions may open.
pub const PUBLISHED_HISTORY: &[&str] = &["published", "deprecated", "retired"];

/// SQL: artifact `a` is visible to a view-only reader (membership is `$4`).
pub(crate) const READER_VISIBLE: &str = "(a.current_published_version_id IS NOT NULL AND (NOT a.restricted OR a.applicability IS NULL OR procera_audience_contains(a.applicability, $4)) AND (a.collection_id IS NULL OR procera_collection_visible(a.collection_id, $4)))";

/// Review status at `now`: overdue once the review date has passed (whether or not the worker has
/// flagged it yet), due soon within [`DUE_SOON`].
pub fn review_status(next_review_at: Option<OffsetDateTime>, now: OffsetDateTime) -> &'static str {
    match next_review_at {
        None => "none",
        Some(t) if t <= now => "overdue",
        Some(t) if t - now <= DUE_SOON => "due_soon",
        Some(_) => "current",
    }
}

fn artifact(r: ArtifactRow, now: OffsetDateTime) -> Artifact {
    Artifact {
        id: r.id,
        kind: r.kind,
        key: r.key,
        title: r.title,
        owner_membership_id: r.owner_membership_id,
        criticality: r.criticality,
        state: r.state,
        current_published_version_id: r.current_published_version_id,
        revision: r.revision,
        created_at: r.created_at,
        governance: Governance {
            review_cadence_days: r.review_cadence_days,
            review_status: review_status(r.next_review_at, now),
            next_review_at: r.next_review_at,
            last_reviewed_at: r.last_reviewed_at,
            review_overdue_at: r.review_overdue_at,
            verified: r.verification_expires_at.is_some_and(|t| t > now),
            verified_at: r.verified_at,
            verified_by: r.verified_by,
            verification_expires_at: r.verification_expires_at,
        },
        category: r.category,
        applicability: r.applicability,
        restricted: r.restricted,
        collection_id: r.collection_id,
    }
}

#[derive(Clone)]
pub struct Library {
    db: Db,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    settings: procera_settings::Settings,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Library")
    }
}

impl Library {
    pub fn new(db: Db, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        let settings = procera_settings::Settings::new(clock.clone(), ids.clone());
        Self { db, clock, ids, settings }
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        action: &str,
        version: Uuid,
        diff: Value,
        reason: Option<&str>,
    ) -> Result<(), LibraryError> {
        let kind = if actor.is_some() { ActorKind::User } else { ActorKind::System };
        let mut ev =
            AuditEvent::new(actor.map(|a| a.subject_ref), kind, Category::Content, action, Outcome::Allowed)
                .object("artifact_version", version);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    async fn refresh_state(tx: &mut WsTx<'_>, artifact_id: Uuid) -> Result<(), LibraryError> {
        sqlx::query(
            "UPDATE artifacts a SET state = v.lifecycle, revision = a.revision + 1,
                    current_published_version_id = (SELECT id FROM artifact_versions p WHERE p.artifact_id = a.id AND p.lifecycle = 'published')
             FROM (SELECT lifecycle FROM artifact_versions WHERE artifact_id = $1 ORDER BY version_no DESC LIMIT 1) v WHERE a.id = $1",
        ).bind(artifact_id).execute(tx.conn()).await?;
        Ok(())
    }

    pub async fn artifact(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Artifact, LibraryError> {
        let r: Option<ArtifactRow> =
            sqlx::query_as(&format!("SELECT {ARTIFACT_COLS} FROM artifacts WHERE id = $1"))
                .bind(id)
                .fetch_optional(tx.conn())
                .await?;
        let now = self.clock.now();
        r.map(|r| artifact(r, now)).ok_or(LibraryError::NotFound)
    }

    /// Artifacts of `kinds`, as `reader` may see them (A-14): kinds in `reader.full_kinds` show
    /// everything; other kinds show only published artifacts, minus restricted ones the reader is
    /// outside of.
    pub async fn list(
        &self,
        tx: &mut WsTx<'_>,
        kinds: &[&str],
        state: Option<&str>,
        collection: Option<Uuid>,
        reader: &Reader<'_>,
    ) -> Result<Vec<Artifact>, LibraryError> {
        let kinds: Vec<String> = kinds.iter().map(|k| (*k).to_owned()).collect();
        let full: Vec<String> = reader.full_kinds.iter().map(|k| (*k).to_owned()).collect();
        // `collection` includes its sub-collections.
        let rows: Vec<ArtifactRow> = sqlx::query_as(&format!("SELECT {ARTIFACT_COLS} FROM artifacts a WHERE kind = ANY($1) AND ($2::text IS NULL OR state = $2) AND (kind = ANY($3) OR {READER_VISIBLE}) AND ($5::uuid IS NULL OR a.collection_id IN (SELECT id FROM collections WHERE $5 = ANY(path))) ORDER BY lower(title)"))
            .bind(&kinds).bind(state).bind(&full).bind(reader.membership).bind(collection).fetch_all(tx.conn()).await?;
        let now = self.clock.now();
        Ok(rows.into_iter().map(|r| artifact(r, now)).collect())
    }

    /// Versions of an artifact, newest first (without reviews).
    pub async fn versions(&self, tx: &mut WsTx<'_>, artifact_id: Uuid) -> Result<Vec<Version>, LibraryError> {
        let rows: Vec<VersionRow> = sqlx::query_as(&format!(
            "SELECT {VERSION_COLS} FROM artifact_versions WHERE artifact_id = $1 ORDER BY version_no DESC"
        ))
        .bind(artifact_id)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|r| Self::to_version(r, vec![], None)).collect())
    }

    fn to_version(r: VersionRow, reviews: Vec<ReviewRecord>, approvals_required: Option<i64>) -> Version {
        Version {
            id: r.0,
            artifact_id: r.1,
            version_no: r.2,
            lifecycle: r.3,
            content: r.4,
            content_hash: r.5.as_deref().map(bytes_to_hex),
            change_reason: r.6,
            effective_at: r.7,
            created_by: r.8,
            created_at: r.9,
            published_at: r.10,
            revision: r.12,
            reviews,
            approvals_required,
            semver: r.13,
            reack_required: r.14,
        }
    }

    async fn load_version(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        lock: bool,
    ) -> Result<(VersionRow, Artifact), LibraryError> {
        let r: Option<VersionRow> = sqlx::query_as(&format!(
            "SELECT {VERSION_COLS} FROM artifact_versions WHERE id = $1{}",
            if lock { " FOR UPDATE" } else { "" }
        ))
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        let r = r.ok_or(LibraryError::NotFound)?;
        let a = self.artifact(tx, r.1).await?;
        Ok((r, a))
    }

    async fn round_reviews(
        tx: &mut WsTx<'_>,
        version: Uuid,
        round: i32,
    ) -> Result<Vec<ReviewRecord>, LibraryError> {
        let rows: Vec<(Uuid, String, Option<String>, OffsetDateTime)> = sqlx::query_as(
            "SELECT reviewer_membership_id, decision, comment, decided_at FROM reviews WHERE artifact_version_id = $1 AND review_round = $2 ORDER BY decided_at, id",
        ).bind(version).bind(round).fetch_all(tx.conn()).await?;
        Ok(rows
            .into_iter()
            .map(|(reviewer_membership_id, decision, comment, decided_at)| ReviewRecord {
                reviewer_membership_id,
                decision,
                comment,
                decided_at,
            })
            .collect())
    }

    /// One version with its current review round.
    pub async fn version(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        policy: Option<&ReviewPolicy>,
    ) -> Result<(Version, Artifact), LibraryError> {
        let (r, a) = self.load_version(tx, id, false).await?;
        let reviews = Self::round_reviews(tx, id, r.11).await?;
        let required = policy.map(|p| p.required(&a.criticality));
        Ok((Self::to_version(r, reviews, required), a))
    }

    /// Creates an artifact with its first draft (version 1).
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        kind: &str,
        key: &str,
        title: &str,
        criticality: &str,
        content: &Value,
    ) -> Result<(Artifact, Version), LibraryError> {
        permission(kind, "create")?;
        if !matches!(criticality, "standard" | "controlled") {
            return Err(LibraryError::Invalid("criticality must be standard or controlled".into()));
        }
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 200 {
            return Err(LibraryError::Invalid("title must be 1-200 characters".into()));
        }
        let key = key.trim().to_lowercase();
        if key.len() < 2
            || key.len() > 64
            || !key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
            || !key.starts_with(|c: char| c.is_ascii_alphanumeric())
        {
            return Err(LibraryError::Invalid(
                "key must be 2-64 characters of a-z, 0-9, '.', '_', '-'".into(),
            ));
        }
        check_content(content)?;
        let taken: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM artifacts WHERE kind = $1 AND key = $2)")
                .bind(kind)
                .bind(&key)
                .fetch_one(tx.conn())
                .await?;
        if taken {
            return Err(LibraryError::KeyTaken);
        }
        let (id, vid, now) = (self.ids.new_id(), self.ids.new_id(), self.clock.now());
        sqlx::query("INSERT INTO artifacts (workspace_id, id, kind, key, title, owner_membership_id, criticality, created_by, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$6,$8)")
            .bind(tx.ctx.workspace_id).bind(id).bind(kind).bind(&key).bind(title).bind(actor.membership).bind(criticality).bind(now).execute(tx.conn()).await?;
        sqlx::query("INSERT INTO artifact_versions (workspace_id, id, artifact_id, version_no, lifecycle, content, created_by, created_at) VALUES ($1,$2,$3,1,'draft',$4,$5,$6)")
            .bind(tx.ctx.workspace_id).bind(vid).bind(id).bind(content).bind(actor.membership).bind(now).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "artifact.created",
            vid,
            json!({"artifact_id": id, "kind": kind, "key": key, "criticality": criticality}),
            None,
        )
        .await?;
        let a = self.artifact(tx, id).await?;
        let (v, _) = self.version(tx, vid, None).await?;
        Ok((a, v))
    }

    /// Starts a new draft from the latest version's content.
    pub async fn new_version(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
    ) -> Result<Version, LibraryError> {
        let a = self.artifact(tx, artifact_id).await?;
        let open: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM artifact_versions WHERE artifact_id = $1 AND lifecycle IN ('draft', 'in_review'))").bind(a.id).fetch_one(tx.conn()).await?;
        if open {
            return Err(LibraryError::Conflict("this artifact already has an open draft or review".into()));
        }
        let (no, content): (i32, Value) = sqlx::query_as("SELECT version_no, content FROM artifact_versions WHERE artifact_id = $1 ORDER BY version_no DESC LIMIT 1").bind(a.id).fetch_one(tx.conn()).await?;
        let vid = self.ids.new_id();
        sqlx::query("INSERT INTO artifact_versions (workspace_id, id, artifact_id, version_no, lifecycle, content, created_by, created_at) VALUES ($1,$2,$3,$4,'draft',$5,$6,$7)")
            .bind(tx.ctx.workspace_id).bind(vid).bind(a.id).bind(no + 1).bind(&content).bind(actor.membership).bind(self.clock.now()).execute(tx.conn()).await?;
        Self::refresh_state(tx, a.id).await?;
        self.audit(
            tx,
            Some(actor),
            "version.created",
            vid,
            json!({"artifact_id": a.id, "version_no": no + 1}),
            None,
        )
        .await?;
        Ok(self.version(tx, vid, None).await?.0)
    }

    /// Replaces a draft's content (optimistic concurrency on `revision`).
    pub async fn edit(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        content: &Value,
        expected_revision: Option<i64>,
    ) -> Result<Version, LibraryError> {
        check_content(content)?;
        let (r, _) = self.load_version(tx, id, true).await?;
        if r.3 != "draft" {
            return Err(LibraryError::Conflict(format!(
                "only drafts can be edited (this version is {}); start a new version",
                r.3
            )));
        }
        if let Some(exp) = expected_revision
            && exp != r.12
        {
            return Err(LibraryError::StaleRevision { current: r.12 });
        }
        sqlx::query("UPDATE artifact_versions SET content = $2, last_activity_at = $3, revision = revision + 1 WHERE id = $1")
            .bind(id)
            .bind(content)
            .bind(self.clock.now()) // retention measures drafts from their last edit (BPMS-066)
            .execute(tx.conn())
            .await?;
        self.audit(
            tx,
            Some(actor),
            "version.edited",
            id,
            json!({"bytes": serde_json::to_vec(content).map(|v| v.len()).unwrap_or(0)}),
            None,
        )
        .await?;
        Ok(self.version(tx, id, None).await?.0)
    }

    async fn approve_now(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        content: &Value,
    ) -> Result<String, LibraryError> {
        let hash = content_hash(content)?;
        sqlx::query("UPDATE artifact_versions SET lifecycle = 'approved', content_hash = $2, approved_at = $3, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(hex_to_bytes(&hash)).bind(self.clock.now()).execute(tx.conn()).await?;
        Ok(hash)
    }

    /// Submits a draft for review; with no approvals required it is approved at once.
    pub async fn submit(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        policy: &ReviewPolicy,
    ) -> Result<Version, LibraryError> {
        let (r, a) = self.load_version(tx, id, true).await?;
        if r.3 != "draft" {
            return Err(LibraryError::Conflict(format!(
                "only drafts can be submitted (this version is {})",
                r.3
            )));
        }
        let content =
            r.4.as_object()
                .filter(|o| !o.is_empty())
                .ok_or_else(|| LibraryError::Invalid("an empty draft cannot be submitted".into()))?;
        let _ = content;
        sqlx::query("UPDATE artifact_versions SET lifecycle = 'in_review', submitted_at = $2, review_round = review_round + 1, revision = revision + 1 WHERE id = $1").bind(id).bind(self.clock.now()).execute(tx.conn()).await?;
        let required = policy.required(&a.criticality);
        self.audit(tx, Some(actor), "version.submitted", id, json!({"approvals_required": required}), None)
            .await?;
        if required > 0 {
            // Reviewers are told a draft is waiting (in-app and email, after commit).
            outbox::enqueue(tx, OutboxEvent { topic: "version.submitted.v1", aggregate_type: "artifact_version", aggregate_id: id, aggregate_revision: r.12 + 1, payload: json!({"artifact_id": a.id, "version_id": id, "version_no": r.2, "kind": a.kind, "title": a.title, "author_membership_id": r.8, "approvals_required": required}), trace_id: None }).await?;
        }
        if required == 0 {
            let hash = self.approve_now(tx, id, &r.4).await?;
            self.audit(
                tx,
                Some(actor),
                "version.approved",
                id,
                json!({"content_hash": hash, "approvals": 0}),
                Some("no review required by policy"),
            )
            .await?;
        }
        Self::refresh_state(tx, a.id).await?;
        Ok(self.version(tx, id, Some(policy)).await?.0)
    }

    /// Withdraws a version from review back to draft.
    pub async fn withdraw(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<Version, LibraryError> {
        let (r, a) = self.load_version(tx, id, true).await?;
        if r.3 != "in_review" {
            return Err(LibraryError::Conflict("only versions in review can be withdrawn".into()));
        }
        sqlx::query(
            "UPDATE artifact_versions SET lifecycle = 'draft', revision = revision + 1 WHERE id = $1",
        )
        .bind(id)
        .execute(tx.conn())
        .await?;
        Self::refresh_state(tx, a.id).await?;
        self.audit(tx, Some(actor), "version.withdrawn", id, json!({}), None).await?;
        Ok(self.version(tx, id, None).await?.0)
    }

    /// Records a review decision. Approval completes when enough distinct non-authors approve.
    pub async fn review(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        decision: &str,
        comment: Option<&str>,
        policy: &ReviewPolicy,
    ) -> Result<Version, LibraryError> {
        if !matches!(decision, "approve" | "reject" | "changes_requested") {
            return Err(LibraryError::Invalid(
                "decision must be approve, reject or changes_requested".into(),
            ));
        }
        let (r, a) = self.load_version(tx, id, true).await?;
        if r.3 != "in_review" {
            return Err(LibraryError::Conflict(format!("this version is {}, not in review", r.3)));
        }
        if r.8 == actor.membership {
            return Err(LibraryError::SeparationOfDuties("authors cannot review their own version".into()));
        }
        let round = Self::round_reviews(tx, id, r.11).await?;
        if round.iter().any(|x| x.reviewer_membership_id == actor.membership) {
            return Err(LibraryError::Conflict("you already reviewed this version in this round".into()));
        }
        let comment = comment.map(str::trim).filter(|c| !c.is_empty());
        if decision != "approve" && comment.is_none() {
            return Err(LibraryError::Invalid("a comment is required to reject or request changes".into()));
        }
        sqlx::query("INSERT INTO reviews (workspace_id, id, artifact_version_id, reviewer_membership_id, review_round, decision, comment, decided_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(tx.ctx.workspace_id).bind(self.ids.new_id()).bind(id).bind(actor.membership).bind(r.11).bind(decision).bind(comment).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit(tx, Some(actor), "version.reviewed", id, json!({"decision": decision}), comment).await?;
        let mut outcome = decision;
        if decision == "approve" {
            let approvals = round.iter().filter(|x| x.decision == "approve").count() as i64 + 1;
            let required = policy.required(&a.criticality).max(1);
            outcome = if approvals >= required { "approved" } else { "approval_recorded" };
            if approvals >= required {
                let hash = self.approve_now(tx, id, &r.4).await?;
                self.audit(
                    tx,
                    Some(actor),
                    "version.approved",
                    id,
                    json!({"content_hash": hash, "approvals": approvals}),
                    None,
                )
                .await?;
            }
        } else {
            sqlx::query(
                "UPDATE artifact_versions SET lifecycle = 'draft', revision = revision + 1 WHERE id = $1",
            )
            .bind(id)
            .execute(tx.conn())
            .await?;
        }
        // The author hears the outcome: approved (ready to publish), changes requested or rejected.
        if outcome != "approval_recorded" {
            outbox::enqueue(tx, OutboxEvent { topic: "version.reviewed.v1", aggregate_type: "artifact_version", aggregate_id: id, aggregate_revision: r.12 + 1, payload: json!({"artifact_id": a.id, "version_id": id, "version_no": r.2, "kind": a.kind, "title": a.title, "author_membership_id": r.8, "outcome": outcome, "comment": comment}), trace_id: None }).await?;
        }
        Self::refresh_state(tx, a.id).await?;
        Ok(self.version(tx, id, Some(policy)).await?.0)
    }

    async fn publish_now(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        id: Uuid,
        artifact_id: Uuid,
    ) -> Result<(), LibraryError> {
        let now = self.clock.now();
        let previous: Option<Uuid> = sqlx::query_scalar("UPDATE artifact_versions SET lifecycle = 'deprecated', revision = revision + 1 WHERE artifact_id = $1 AND lifecycle = 'published' RETURNING id")
            .bind(artifact_id).fetch_optional(tx.conn()).await?;
        sqlx::query("UPDATE artifact_versions SET lifecycle = 'published', published_at = $2, published_by = $3, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(now).bind(actor.map(|a| a.membership)).execute(tx.conn()).await?;
        Self::refresh_state(tx, artifact_id).await?;
        let next_review = self.schedule_review(tx, artifact_id, now).await?;
        let (no, hash, semver, reack): (i32, Option<Vec<u8>>, Option<String>, bool) = sqlx::query_as(
            "SELECT version_no, content_hash, semver, reack_required FROM artifact_versions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        // BPMS-045: consumers start the re-acknowledgement campaign when `reack_required`.
        outbox::enqueue(tx, OutboxEvent { topic: "artifact.published.v1", aggregate_type: "artifact", aggregate_id: artifact_id, aggregate_revision: i64::from(no), payload: json!({"artifact_id": artifact_id, "version_id": id, "version_no": no, "semver": semver, "reack_required": reack, "superseded_version_id": previous}), trace_id: None }).await?;
        self.audit(tx, actor, "version.published", id, json!({"version_no": no, "semver": semver, "reack_required": reack, "content_hash": hash.as_deref().map(bytes_to_hex), "superseded_version_id": previous, "next_review_at": next_review.unix_timestamp()}), None).await?;
        Ok(())
    }

    /// Publishes an approved version now, or schedules it for `effective_at`. Its number and whether
    /// it asks for a re-acknowledgement are fixed here from `change` (BPMS-045).
    #[allow(clippy::too_many_arguments)]
    pub async fn publish(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        effective_at: Option<OffsetDateTime>,
        change_reason: Option<&str>,
        change: Change,
        policy: &ReviewPolicy,
    ) -> Result<Version, LibraryError> {
        let (r, a) = self.load_version(tx, id, true).await?;
        if r.3 != "approved" {
            return Err(LibraryError::Conflict(format!(
                "only approved versions can be published (this version is {})",
                r.3
            )));
        }
        let reason = change_reason.map(str::trim).filter(|c| !c.is_empty());
        if r.2 > 1 && policy.reason_for_change_required && reason.is_none() {
            return Err(LibraryError::Invalid("a reason for change is required".into()));
        }
        let semver = Self::next_semver(tx, a.id, r.2, change).await?;
        let reack = self.reack_policy(tx).await?.requires(r.2, change);
        sqlx::query("UPDATE artifact_versions SET change_reason = $2, effective_at = $3, semver = $4, reack_required = $5 WHERE id = $1")
            .bind(id)
            .bind(reason)
            .bind(effective_at)
            .bind(&semver)
            .bind(reack)
            .execute(tx.conn())
            .await?;
        match effective_at.filter(|t| *t > self.clock.now()) {
            Some(at) => {
                sqlx::query("UPDATE artifact_versions SET lifecycle = 'scheduled', scheduled_by = $2, revision = revision + 1 WHERE id = $1").bind(id).bind(actor.membership).execute(tx.conn()).await?;
                Self::refresh_state(tx, a.id).await?;
                self.audit(
                    tx,
                    Some(actor),
                    "version.scheduled",
                    id,
                    json!({"effective_at": at.unix_timestamp(), "semver": semver, "reack_required": reack}),
                    reason,
                )
                .await?;
            }
            None => self.publish_now(tx, Some(actor), id, a.id).await?,
        }
        Ok(self.version(tx, id, None).await?.0)
    }

    /// Retires a published or deprecated version (read-only afterwards).
    pub async fn retire(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        reason: &str,
    ) -> Result<Version, LibraryError> {
        let (r, a) = self.load_version(tx, id, true).await?;
        if !matches!(r.3.as_str(), "published" | "deprecated") {
            return Err(LibraryError::Conflict(format!(
                "only published or deprecated versions can be retired (this version is {})",
                r.3
            )));
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(LibraryError::Invalid("a reason is required to retire".into()));
        }
        sqlx::query(
            "UPDATE artifact_versions SET lifecycle = 'retired', revision = revision + 1 WHERE id = $1",
        )
        .bind(id)
        .execute(tx.conn())
        .await?;
        Self::refresh_state(tx, a.id).await?;
        self.audit(tx, Some(actor), "version.retired", id, json!({"was": r.3}), Some(reason)).await?;
        Ok(self.version(tx, id, None).await?.0)
    }

    /// Publishes scheduled versions whose effective date has passed (worker). Returns how many.
    pub async fn publish_due(&self) -> Result<usize, LibraryError> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.library_due_workspaces($1, 1000)")
                .bind(self.clock.now())
                .fetch_all(self.db.pool())
                .await?;
        let mut n = 0;
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let due: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT id, artifact_id FROM artifact_versions WHERE lifecycle = 'scheduled' AND effective_at <= $1 ORDER BY effective_at FOR UPDATE SKIP LOCKED")
                .bind(self.clock.now()).fetch_all(tx.conn()).await?;
            for (id, artifact_id) in due {
                self.publish_now(&mut tx, None, id, artifact_id).await?;
                n += 1;
            }
            tx.commit().await?;
        }
        Ok(n)
    }
}
