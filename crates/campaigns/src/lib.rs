//! Acknowledgement campaigns (BPMS-046, FR-AK01–04, docs/04 §2.7, docs/20 §2.14).
//!
//! A campaign asks an audience to acknowledge one **published** SOP or policy version, or one
//! published announcement that requires acknowledgement (BPMS-047, FR-AN04: "one engine").
//! - `draft → active → closed | cancelled`. Launching snapshots the audience into one assignment per
//!   active member and notifies each (in-app + email). `dynamic` campaigns also enrol members who join
//!   later and match the audience (`membership.activated.v1`).
//! - The workspace defaults (`campaign.*`) are copied onto the campaign when it is created, so later
//!   settings changes never alter a running campaign.
//! - An acknowledgement records the exact version id and content hash, the statement text, the
//!   client IP and user agent, and how the member authenticated. It is frozen by a database trigger.
//! - Reminders follow the ladder (offsets from each member's assignment time); an open assignment is
//!   escalated once to the resolved manager after `escalation_after`. The scheduler runs them with the
//!   application clock; a missed ladder sends one reminder, not a burst.
//! - Announcement campaigns start when the announcement goes live: its recipients are the
//!   assignments (a fixed snapshot, never joiners), the announcement's own delivery is the
//!   assignment notice, and members acknowledge through the announcement, which completes the
//!   assignment (one record).
//!
//! Permissions are checked by the caller (HTTP layer); this crate enforces the lifecycle and the
//! "only your own assignment" rule.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::outbox::{self, OutboxEvent};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_members::Members;
use procera_members::audience::{self, Audience};
use procera_notify::Notify;
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum CampaignError {
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("already acknowledged")]
    AlreadyAcknowledged,
    #[error("the content changed: this campaign asks for version hash {current}")]
    ContentChanged { current: String },
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for CampaignError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for CampaignError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for CampaignError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_members::MembersError> for CampaignError {
    fn from(e: procera_members::MembersError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_notify::NotifyError> for CampaignError {
    fn from(e: procera_notify::NotifyError) -> Self {
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

/// Workspace defaults (docs/20 §2.14), resolved from settings by the caller.
#[derive(Debug, Clone)]
pub struct Defaults {
    /// `at_launch` or `dynamic`.
    pub snapshot: String,
    /// Reminder ladder, seconds after assignment.
    pub cadence: Vec<i64>,
    /// Seconds after assignment; `None` = off.
    pub escalation_after: Option<i64>,
    /// Seconds from launch (or enrolment) to the due date.
    pub due_in: i64,
    pub ack_text: String,
}

/// A new campaign. Absent fields take the workspace defaults.
#[derive(Debug, Clone)]
pub struct NewCampaign {
    pub title: Option<String>,
    pub version_id: Uuid,
    pub audience: Audience,
    pub due_at: Option<OffsetDateTime>,
    pub snapshot: Option<String>,
    pub ack_text: Option<String>,
}

/// What a campaign asks people to acknowledge. Library subjects carry the artifact and version;
/// announcement subjects carry `announcement_id` instead (absent fields are omitted).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Subject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<Uuid>,
    /// `sop`, `policy` or `announcement`.
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_no: Option<i32>,
    /// `major.minor` (BPMS-045).
    pub semver: Option<String>,
    /// Hex SHA-256 of the canonical content.
    pub content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announcement_id: Option<Uuid>,
    /// Where the subject opens in the web app.
    pub link: String,
}

impl Subject {
    fn new(
        artifact_id: Option<Uuid>,
        kind: String,
        title: String,
        version: (Option<Uuid>, Option<i32>, Option<String>),
        content_hash: String,
        announcement_id: Option<Uuid>,
    ) -> Self {
        let link = match (artifact_id, announcement_id) {
            (Some(a), _) => format!("/library/{a}"),
            (None, Some(a)) => announcement_path(a),
            (None, None) => String::new(),
        };
        let (version_id, version_no, semver) = version;
        Subject {
            artifact_id,
            kind,
            title,
            version_id,
            version_no,
            semver,
            content_hash,
            announcement_id,
            link,
        }
    }
}

/// The announcement an announcement campaign is started for (BPMS-047, FR-AN04).
#[derive(Debug, Clone)]
pub struct AnnouncementSubject {
    pub announcement_id: Uuid,
    pub title: String,
    /// The announcement's frozen content hash (what recipients acknowledge).
    pub content_hash: Vec<u8>,
    /// The announcement's audience rule, kept for display.
    pub audience: Value,
    /// The audience snapshot taken at publication: these become the assignments.
    pub recipients: Vec<Uuid>,
    pub due_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub assigned: i64,
    pub acknowledged: i64,
    pub pending: i64,
    pub overdue: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Campaign {
    pub id: Uuid,
    pub title: String,
    pub subject: Subject,
    /// `manual`, or `reack` when publishing a version started it (BPMS-045).
    pub origin: String,
    pub audience: Value,
    pub audience_snapshot: String,
    pub ack_text: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub due_at: Option<OffsetDateTime>,
    pub due_in_secs: i64,
    pub reminder_schedule: Vec<i64>,
    pub escalation_after_secs: Option<i64>,
    pub state: String,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub launched_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub end_reason: Option<String>,
    pub revision: i64,
    pub progress: Progress,
}

/// One member's assignment as the member sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assignment {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub campaign_title: String,
    pub campaign_state: String,
    pub subject: Subject,
    pub ack_text: String,
    pub state: String,
    #[serde(with = "time::serde::rfc3339")]
    pub assigned_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
    pub overdue: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub acknowledged_at: Option<OffsetDateTime>,
    pub acknowledged_version_id: Option<Uuid>,
    pub acknowledged_content_hash: Option<String>,
}

/// One assignment as campaign managers see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recipient {
    pub id: Uuid,
    pub membership_id: Uuid,
    pub name: String,
    pub email: String,
    pub department: Option<String>,
    pub source: String,
    pub state: String,
    #[serde(with = "time::serde::rfc3339")]
    pub assigned_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub due_at: OffsetDateTime,
    pub overdue: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub acknowledged_at: Option<OffsetDateTime>,
    pub reminders_sent: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub escalated_at: Option<OffsetDateTime>,
    pub escalated_to: Option<Uuid>,
}

/// What a scheduler pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DuePass {
    pub reminders: usize,
    pub escalations: usize,
}

#[derive(sqlx::FromRow)]
struct CampaignRow {
    id: Uuid,
    title: String,
    subject_version_id: Option<Uuid>,
    subject_hash: String,
    artifact_id: Option<Uuid>,
    kind: String,
    subject_title: String,
    version_no: Option<i32>,
    semver: Option<String>,
    subject_announcement_id: Option<Uuid>,
    origin: String,
    audience_rule: Value,
    audience_snapshot: String,
    ack_text: String,
    due_at: Option<OffsetDateTime>,
    due_in_secs: i64,
    reminder_schedule: Value,
    escalation_after_secs: Option<i64>,
    state: String,
    created_by: Uuid,
    created_at: OffsetDateTime,
    launched_at: Option<OffsetDateTime>,
    ended_at: Option<OffsetDateTime>,
    end_reason: Option<String>,
    revision: i64,
    assigned: i64,
    acknowledged: i64,
    overdue: i64,
}

/// `$1` = now (for overdue counts).
const CAMPAIGN_SELECT: &str = "SELECT c.id, c.title, c.subject_version_id, encode(c.subject_hash, 'hex') AS subject_hash,
       a.id AS artifact_id, coalesce(a.kind, 'announcement') AS kind, coalesce(a.title, an.title) AS subject_title,
       v.version_no, v.semver, c.subject_announcement_id, c.origin,
       c.audience_rule, c.audience_snapshot, c.ack_text, c.due_at, c.due_in_secs, c.reminder_schedule,
       c.escalation_after_secs, c.state, c.created_by, c.created_at, c.launched_at, c.ended_at, c.end_reason, c.revision,
       coalesce(p.assigned, 0) AS assigned, coalesce(p.acknowledged, 0) AS acknowledged, coalesce(p.overdue, 0) AS overdue
FROM campaigns c
LEFT JOIN artifact_versions v ON v.workspace_id = c.workspace_id AND v.id = c.subject_version_id
LEFT JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
LEFT JOIN announcements an ON an.workspace_id = c.workspace_id AND an.id = c.subject_announcement_id
LEFT JOIN LATERAL (
  SELECT count(*) AS assigned, count(*) FILTER (WHERE r.state = 'completed') AS acknowledged,
         count(*) FILTER (WHERE r.state = 'pending' AND r.due_at < $1) AS overdue
  FROM campaign_recipients r WHERE r.workspace_id = c.workspace_id AND r.campaign_id = c.id) p ON true";

impl From<CampaignRow> for Campaign {
    fn from(r: CampaignRow) -> Self {
        Campaign {
            id: r.id,
            title: r.title,
            subject: Subject::new(
                r.artifact_id,
                r.kind,
                r.subject_title,
                (r.subject_version_id, r.version_no, r.semver),
                r.subject_hash,
                r.subject_announcement_id,
            ),
            origin: r.origin,
            audience: r.audience_rule,
            audience_snapshot: r.audience_snapshot,
            ack_text: r.ack_text,
            due_at: r.due_at,
            due_in_secs: r.due_in_secs,
            reminder_schedule: serde_json::from_value(r.reminder_schedule).unwrap_or_default(),
            escalation_after_secs: r.escalation_after_secs,
            state: r.state,
            created_by: r.created_by,
            created_at: r.created_at,
            launched_at: r.launched_at,
            ended_at: r.ended_at,
            end_reason: r.end_reason,
            revision: r.revision,
            progress: Progress {
                assigned: r.assigned,
                acknowledged: r.acknowledged,
                pending: r.assigned - r.acknowledged,
                overdue: r.overdue,
            },
        }
    }
}

#[derive(sqlx::FromRow)]
struct AssignmentRow {
    id: Uuid,
    campaign_id: Uuid,
    campaign_title: String,
    campaign_state: String,
    ack_text: String,
    subject_version_id: Option<Uuid>,
    subject_hash: String,
    artifact_id: Option<Uuid>,
    kind: String,
    subject_title: String,
    version_no: Option<i32>,
    semver: Option<String>,
    subject_announcement_id: Option<Uuid>,
    state: String,
    assigned_at: OffsetDateTime,
    due_at: OffsetDateTime,
    completed_at: Option<OffsetDateTime>,
    ack_version_id: Option<Uuid>,
    ack_content_hash: Option<String>,
}

/// `$1` = now, `$2` = membership.
const ASSIGNMENT_SELECT: &str = "SELECT r.id, r.campaign_id, c.title AS campaign_title, c.state AS campaign_state, c.ack_text,
       c.subject_version_id, encode(c.subject_hash, 'hex') AS subject_hash, a.id AS artifact_id,
       coalesce(a.kind, 'announcement') AS kind, coalesce(a.title, an.title) AS subject_title, v.version_no, v.semver,
       c.subject_announcement_id, r.state, r.assigned_at, r.due_at, r.completed_at, r.ack_version_id,
       encode(r.ack_content_hash, 'hex') AS ack_content_hash
FROM campaign_recipients r
JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
LEFT JOIN artifact_versions v ON v.workspace_id = c.workspace_id AND v.id = c.subject_version_id
LEFT JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
LEFT JOIN announcements an ON an.workspace_id = c.workspace_id AND an.id = c.subject_announcement_id
WHERE r.membership_id = $2";

fn assignment(r: AssignmentRow, now: OffsetDateTime) -> Assignment {
    Assignment {
        id: r.id,
        campaign_id: r.campaign_id,
        campaign_title: r.campaign_title,
        campaign_state: r.campaign_state,
        subject: Subject::new(
            r.artifact_id,
            r.kind,
            r.subject_title,
            (r.subject_version_id, r.version_no, r.semver),
            r.subject_hash,
            r.subject_announcement_id,
        ),
        ack_text: r.ack_text,
        overdue: r.state == "pending" && r.due_at < now,
        state: r.state,
        assigned_at: r.assigned_at,
        due_at: r.due_at,
        acknowledged_at: r.completed_at,
        acknowledged_version_id: r.ack_version_id,
        acknowledged_content_hash: r.ack_content_hash,
    }
}

/// Values that start a formula in spreadsheet apps are prefixed so they stay text (CSV injection).
fn cell(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_owned() }
}

fn rfc3339(t: Option<OffsetDateTime>) -> String {
    t.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()).unwrap_or_default()
}

fn human_date(t: OffsetDateTime) -> String {
    t.format(time::macros::format_description!("[day padding:none] [month repr:short] [year]"))
        .unwrap_or_default()
}

/// Where a member's assignment opens in the web app.
fn assignment_path(id: Uuid) -> String {
    format!("/acknowledgements/{id}")
}

/// Where an announcement opens in the web app (it is acknowledged there).
fn announcement_path(id: Uuid) -> String {
    format!("/announcements/{id}")
}

type Locked = (String, Value, String, Option<OffsetDateTime>, i64, Option<Uuid>);

#[derive(Clone)]
pub struct Campaigns {
    db: Db,
    members: Members,
    notify: Notify,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    public_url: String,
}

impl std::fmt::Debug for Campaigns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Campaigns")
    }
}

/// Template variables shared by a campaign's notifications.
struct Mail {
    title: String,
    version_no: Option<i32>,
    /// Set for announcement campaigns: their notices use the `announcement_ack_*` templates and
    /// open the announcement.
    announcement: Option<Uuid>,
}

impl Mail {
    /// The notification kind for `base` (`reminder` or `escalated`).
    fn kind(&self, base: &str) -> String {
        match self.announcement {
            Some(_) => format!("announcement_ack_{base}"),
            None => format!("campaign_{base}"),
        }
    }

    /// Where a member's open assignment is acted on.
    fn link(&self, assignment: Uuid) -> String {
        self.announcement.map_or_else(|| assignment_path(assignment), announcement_path)
    }
}

impl Campaigns {
    pub fn new(
        db: Db,
        members: Members,
        notify: Notify,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        public_url: &str,
    ) -> Self {
        Self { db, members, notify, clock, ids, public_url: public_url.trim_end_matches('/').to_owned() }
    }

    #[allow(clippy::too_many_arguments)]
    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        category: Category,
        action: &str,
        object: (&str, Uuid),
        diff: Value,
        reason: Option<&str>,
    ) -> Result<(), CampaignError> {
        let kind = if actor.is_some() { ActorKind::User } else { ActorKind::System };
        let mut ev = AuditEvent::new(actor.map(|a| a.subject_ref), kind, category, action, Outcome::Allowed)
            .object(object.0, object.1);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    async fn event(
        tx: &mut WsTx<'_>,
        topic: &'static str,
        campaign: Uuid,
        revision: i64,
        payload: Value,
    ) -> Result<(), CampaignError> {
        outbox::enqueue(
            tx,
            OutboxEvent {
                topic,
                aggregate_type: "campaign",
                aggregate_id: campaign,
                aggregate_revision: revision,
                payload,
                trace_id: None,
            },
        )
        .await?;
        Ok(())
    }

    // ---------- reads ----------

    pub async fn get(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Campaign, CampaignError> {
        let r: Option<CampaignRow> = sqlx::query_as(&format!("{CAMPAIGN_SELECT} WHERE c.id = $2"))
            .bind(self.clock.now())
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
        r.map(Campaign::from).ok_or(CampaignError::NotFound)
    }

    /// Campaigns, newest first, optionally in one state.
    pub async fn list(&self, tx: &mut WsTx<'_>, state: Option<&str>) -> Result<Vec<Campaign>, CampaignError> {
        let rows: Vec<CampaignRow> = sqlx::query_as(&format!(
            "{CAMPAIGN_SELECT} WHERE ($2::text IS NULL OR c.state = $2) ORDER BY c.created_at DESC, c.id DESC LIMIT 500"
        ))
        .bind(self.clock.now())
        .bind(state)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(Campaign::from).collect())
    }

    /// How many active members the audience matches now (draft preview).
    pub async fn audience_size(&self, tx: &mut WsTx<'_>, a: &Audience) -> Result<usize, CampaignError> {
        Ok(audience::resolve(tx, a).await?.len())
    }

    /// Assignments of a campaign with member details, by name.
    pub async fn recipients(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        state: Option<&str>,
    ) -> Result<Vec<Recipient>, CampaignError> {
        self.get(tx, id).await?;
        type Row = (
            Uuid,
            Uuid,
            String,
            String,
            Option<String>,
            String,
            String,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
            i32,
            Option<OffsetDateTime>,
            Option<Uuid>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT r.id, r.membership_id, u.display_name, u.email::text, d.name, r.source, r.state, r.assigned_at, r.due_at,
                    r.completed_at, r.reminders_sent, r.escalated_at, r.escalated_to
             FROM campaign_recipients r
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
             JOIN users u ON u.id = m.user_id
             LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
             WHERE r.campaign_id = $1 AND ($2::text IS NULL OR r.state = $2 OR ($2 = 'overdue' AND r.state = 'pending' AND r.due_at < $3))
             ORDER BY lower(u.display_name), u.email",
        )
        .bind(id)
        .bind(state)
        .bind(self.clock.now())
        .fetch_all(tx.conn())
        .await?;
        let now = self.clock.now();
        Ok(rows
            .into_iter()
            .map(|r| Recipient {
                id: r.0,
                membership_id: r.1,
                name: r.2,
                email: r.3,
                department: r.4,
                source: r.5,
                overdue: r.6 == "pending" && r.8 < now,
                state: r.6,
                assigned_at: r.7,
                due_at: r.8,
                acknowledged_at: r.9,
                reminders_sent: r.10,
                escalated_at: r.11,
                escalated_to: r.12,
            })
            .collect())
    }

    /// The member's assignments: `pending` (open campaigns only), `completed`, or all.
    pub async fn my_assignments(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        state: Option<&str>,
    ) -> Result<Vec<Assignment>, CampaignError> {
        let now = self.clock.now();
        let rows: Vec<AssignmentRow> = sqlx::query_as(&format!(
            "{ASSIGNMENT_SELECT} AND (($3::text IS NULL AND (r.state = 'completed' OR c.state = 'active'))
               OR ($3 = 'pending' AND r.state = 'pending' AND c.state = 'active')
               OR ($3 = 'completed' AND r.state = 'completed'))
             ORDER BY r.state DESC, r.due_at, r.id"
        ))
        .bind(now)
        .bind(membership)
        .bind(state)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|r| assignment(r, now)).collect())
    }

    /// The subject kind (`sop`/`policy`/`announcement`) behind the member's own assignment, for the
    /// permission check. Someone else's assignment is reported as not found.
    pub async fn assignment_kind(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        membership: Uuid,
    ) -> Result<String, CampaignError> {
        let kind: Option<String> = sqlx::query_scalar(
            "SELECT coalesce(a.kind, 'announcement') FROM campaign_recipients r
             JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
             LEFT JOIN artifact_versions v ON v.workspace_id = c.workspace_id AND v.id = c.subject_version_id
             LEFT JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
             WHERE r.id = $1 AND r.membership_id = $2",
        )
        .bind(id)
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?;
        kind.ok_or(CampaignError::NotFound)
    }

    // ---------- lifecycle ----------

    /// Creates a draft campaign for a published SOP or policy version.
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        new: &NewCampaign,
        defaults: &Defaults,
    ) -> Result<Campaign, CampaignError> {
        self.create_as(tx, Some(actor), actor.membership, new, defaults, "manual").await
    }

    /// Starts the re-acknowledgement campaign of a just-published version (BPMS-045, FR-PL03): created
    /// and launched at once, at most one per version. `actor` is the publisher, or `None` when the
    /// scheduler published it (audited as the system, recorded as created by `by`). Returns `None`
    /// when the version already has one, is no longer the published version, or the audience matches
    /// no active member.
    pub async fn start_reack(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        by: Uuid,
        new: &NewCampaign,
        defaults: &Defaults,
    ) -> Result<Option<Campaign>, CampaignError> {
        let (exists, lifecycle): (bool, Option<String>) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM campaigns WHERE origin = 'reack' AND subject_version_id = $1),
                    (SELECT lifecycle FROM artifact_versions WHERE id = $1)",
        )
        .bind(new.version_id)
        .fetch_one(tx.conn())
        .await?;
        if exists
            || lifecycle.as_deref() != Some("published")
            || self.audience_size(tx, &new.audience).await? == 0
        {
            return Ok(None);
        }
        let c = self.create_as(tx, actor, by, new, defaults, "reack").await?;
        self.launch_as(tx, actor, by, c.id, None).await.map(Some)
    }

    /// Starts the acknowledgement campaign of a just-published announcement (BPMS-047, FR-AN04):
    /// created and launched at once, at most one per announcement, with the announcement's
    /// recipients as the assignments (`at_launch`: no joiners). Reminders follow
    /// `defaults.cadence`; the manager escalation, when on, never fires before the deadline
    /// (`max(escalation_after, window)`, A-32). The assignees are not notified here: the
    /// announcement's own delivery asks them. Returns `None` when the announcement already has a
    /// campaign or no recipients. `actor` is the publisher; `None` = the scheduler (audited as
    /// the system, recorded as created by `by`). A deadline already past also starts nothing.
    pub async fn start_announcement(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        by: Uuid,
        s: &AnnouncementSubject,
        defaults: &Defaults,
    ) -> Result<Option<Campaign>, CampaignError> {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM campaigns WHERE subject_announcement_id = $1)")
                .bind(s.announcement_id)
                .fetch_one(tx.conn())
                .await?;
        if exists || s.recipients.is_empty() {
            return Ok(None);
        }
        let now = self.clock.now();
        if s.due_at <= now {
            return Ok(None); // went live after its deadline (a scheduler far behind): nothing to ask
        }
        let ack_text = defaults.ack_text.trim();
        if ack_text.is_empty() || ack_text.chars().count() > 500 {
            return Err(CampaignError::Invalid("ack_text must be 1-500 characters".into()));
        }
        let title: String = s.title.trim().chars().take(200).collect();
        let window = (s.due_at - now).whole_seconds().max(1);
        let mut cadence = defaults.cadence.clone();
        cadence.sort_unstable();
        cadence.dedup();
        let escalation = defaults.escalation_after.filter(|e| *e > 0).map(|e| e.max(window));
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO campaigns (workspace_id, id, title, subject_kind, subject_announcement_id, subject_hash, audience_rule,
                                    audience_snapshot, ack_text, due_at, due_in_secs, reminder_schedule, escalation_after_secs,
                                    created_by, created_at, origin)
             VALUES ($1,$2,$3,'announcement',$4,$5,$6,'at_launch',$7,$8,$9,$10,$11,$12,$13,'announcement')",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(&title)
        .bind(s.announcement_id)
        .bind(&s.content_hash)
        .bind(&s.audience)
        .bind(ack_text)
        .bind(s.due_at)
        .bind(window)
        .bind(json!(cadence))
        .bind(escalation)
        .bind(by)
        .bind(now)
        .execute(tx.conn())
        .await?;
        self.audit(
            tx,
            actor,
            Category::Content,
            "campaign.created",
            ("campaign", id),
            json!({"announcement_id": s.announcement_id, "audience": s.audience, "audience_snapshot": "at_launch", "origin": "announcement"}),
            None,
        )
        .await?;
        self.launch_as(tx, actor, by, id, Some(&s.recipients)).await.map(Some)
    }

    /// The campaign of each announcement that has one: announcement → (campaign id, due date).
    pub async fn announcement_campaigns(
        &self,
        tx: &mut WsTx<'_>,
        announcements: &[Uuid],
    ) -> Result<HashMap<Uuid, (Uuid, Option<OffsetDateTime>)>, CampaignError> {
        let rows: Vec<(Uuid, Uuid, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT subject_announcement_id, id, due_at FROM campaigns WHERE subject_announcement_id = ANY($1)",
        )
        .bind(announcements)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|(a, c, d)| (a, (c, d))).collect())
    }

    /// Completes the member's open assignment in the active campaign of `announcement`, as part of
    /// acknowledging the announcement (FR-AN04). The evidence binds to the announcement and its
    /// content hash. It appends no audit event of its own: the caller's `announcement.acknowledged`
    /// is the one record. Returns (campaign, assignment), or `None` when there is nothing open.
    pub async fn complete_for_announcement(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        announcement: Uuid,
        evidence: Value,
    ) -> Result<Option<(Uuid, Uuid)>, CampaignError> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT c.id, r.id FROM campaigns c
             JOIN campaign_recipients r ON r.workspace_id = c.workspace_id AND r.campaign_id = c.id
             WHERE c.subject_announcement_id = $1 AND c.state = 'active' AND r.membership_id = $2 AND r.state = 'pending'
             FOR UPDATE OF r",
        )
        .bind(announcement)
        .bind(actor.membership)
        .fetch_optional(tx.conn())
        .await?;
        let Some((campaign, rid)) = row else { return Ok(None) };
        let now = self.clock.now();
        sqlx::query(
            "UPDATE campaign_recipients r SET state = 'completed', completed_at = $2, ack_announcement_id = $3,
                    ack_content_hash = c.subject_hash, ack_text = c.ack_text, ack_ip = $4::inet, ack_user_agent = $5, evidence = $6
             FROM campaigns c WHERE c.workspace_id = r.workspace_id AND c.id = r.campaign_id AND r.id = $1",
        )
        .bind(rid)
        .bind(now)
        .bind(announcement)
        .bind(&actor.ip)
        .bind(&actor.user_agent)
        .bind(&evidence)
        .execute(tx.conn())
        .await?;
        Self::event(
            tx,
            "campaign.recipient.completed.v1",
            campaign,
            now.unix_timestamp(),
            json!({"campaign_id": campaign, "recipient_id": rid, "membership_id": actor.membership, "announcement_id": announcement}),
        )
        .await?;
        Ok(Some((campaign, rid)))
    }

    /// Closes the active campaign of an archived announcement (open assignments stay open as the
    /// record of non-acknowledgement). Returns it, or `None` when there is none to close.
    pub async fn close_for_announcement(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        announcement: Uuid,
    ) -> Result<Option<Campaign>, CampaignError> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM campaigns WHERE subject_announcement_id = $1 AND state = 'active'",
        )
        .bind(announcement)
        .fetch_optional(tx.conn())
        .await?;
        match id {
            Some(id) => {
                self.end(tx, actor, id, "closed", Some("the announcement was archived")).await.map(Some)
            }
            None => Ok(None),
        }
    }

    async fn create_as(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        by: Uuid,
        new: &NewCampaign,
        defaults: &Defaults,
        origin: &str,
    ) -> Result<Campaign, CampaignError> {
        if new.audience.is_empty() {
            return Err(CampaignError::Invalid(
                "the audience is empty: choose everyone, departments, groups or roles".into(),
            ));
        }
        let snapshot = new.snapshot.as_deref().unwrap_or(&defaults.snapshot);
        if !matches!(snapshot, "at_launch" | "dynamic") {
            return Err(CampaignError::Invalid("audience_snapshot must be at_launch or dynamic".into()));
        }
        let ack_text = new.ack_text.as_deref().unwrap_or(&defaults.ack_text).trim().to_owned();
        if ack_text.is_empty() || ack_text.chars().count() > 500 {
            return Err(CampaignError::Invalid("ack_text must be 1-500 characters".into()));
        }
        if let Some(due) = new.due_at
            && due <= self.clock.now()
        {
            return Err(CampaignError::Invalid("due_at must be in the future".into()));
        }
        let subject: Option<(String, String, String, Option<Vec<u8>>)> = sqlx::query_as(
            "SELECT v.lifecycle, a.kind, a.title, v.content_hash FROM artifact_versions v
             JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id WHERE v.id = $1",
        )
        .bind(new.version_id)
        .fetch_optional(tx.conn())
        .await?;
        let (lifecycle, kind, artifact_title, hash) =
            subject.ok_or_else(|| CampaignError::Invalid("version_id does not exist".into()))?;
        if !matches!(kind.as_str(), "sop" | "policy") {
            return Err(CampaignError::Invalid("only SOP and policy versions can be acknowledged".into()));
        }
        let hash = match (lifecycle.as_str(), hash) {
            ("published", Some(h)) => h,
            _ => {
                return Err(CampaignError::Conflict(format!(
                    "only published versions can be acknowledged (this version is {lifecycle})"
                )));
            }
        };
        let title = new.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&artifact_title);
        if title.chars().count() > 200 {
            return Err(CampaignError::Invalid("title must be at most 200 characters".into()));
        }
        let mut cadence = defaults.cadence.clone();
        cadence.sort_unstable();
        cadence.dedup();
        let id = self.ids.new_id();
        let audience_rule =
            serde_json::to_value(&new.audience).map_err(|e| CampaignError::Internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO campaigns (workspace_id, id, title, subject_version_id, subject_hash, audience_rule, audience_snapshot,
                                    ack_text, due_at, due_in_secs, reminder_schedule, escalation_after_secs, created_by, created_at, origin)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(title)
        .bind(new.version_id)
        .bind(&hash)
        .bind(&audience_rule)
        .bind(snapshot)
        .bind(&ack_text)
        .bind(new.due_at)
        .bind(defaults.due_in)
        .bind(json!(cadence))
        .bind(defaults.escalation_after.filter(|s| *s > 0))
        .bind(by)
        .bind(self.clock.now())
        .bind(origin)
        .execute(tx.conn())
        .await?;
        self.audit(
            tx,
            actor,
            Category::Content,
            "campaign.created",
            ("campaign", id),
            json!({"version_id": new.version_id, "audience": audience_rule, "audience_snapshot": snapshot, "origin": origin}),
            None,
        )
        .await?;
        self.get(tx, id).await
    }

    /// Contact details for notifications: membership → (email, display name).
    async fn contacts(
        tx: &mut WsTx<'_>,
        members: &[Uuid],
    ) -> Result<HashMap<Uuid, (String, String)>, CampaignError> {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT m.id, u.email::text, u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = ANY($1)",
        )
        .bind(members)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|(id, email, name)| (id, (email, name))).collect())
    }

    async fn mail_vars(tx: &mut WsTx<'_>, campaign: Uuid) -> Result<Mail, CampaignError> {
        let (title, version_no, announcement): (String, Option<i32>, Option<Uuid>) = sqlx::query_as(
            "SELECT coalesce(a.title, an.title), v.version_no, c.subject_announcement_id FROM campaigns c
             LEFT JOIN artifact_versions v ON v.workspace_id = c.workspace_id AND v.id = c.subject_version_id
             LEFT JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
             LEFT JOIN announcements an ON an.workspace_id = c.workspace_id AND an.id = c.subject_announcement_id
             WHERE c.id = $1",
        )
        .bind(campaign)
        .fetch_one(tx.conn())
        .await?;
        Ok(Mail { title, version_no, announcement })
    }

    /// In-app notification and email to one member about their assignment (idempotent on `dedupe`).
    #[allow(clippy::too_many_arguments)]
    async fn tell(
        &self,
        tx: &mut WsTx<'_>,
        kind: &str,
        to: Uuid,
        email: Option<&str>,
        mail: &Mail,
        due: OffsetDateTime,
        link: &str,
        reference: (&str, Uuid),
        name: Option<&str>,
        dedupe: &str,
    ) -> Result<(), CampaignError> {
        let mut vars =
            json!({"title": mail.title, "version": mail.version_no, "due": human_date(due), "link": link});
        if let Some(n) = name {
            vars["name"] = json!(n);
        }
        self.notify.notify_member(tx, to, kind, &vars, Some(link), Some(reference), dedupe).await?;
        if let Some(email) = email {
            vars["link"] = json!(format!("{}{link}", self.public_url));
            // Honours the member's quiet hours (BPMS-048). Assignments, reminders and escalations ask
            // for action by a date, so they are urgent: never held for a digest.
            self.notify
                .email_member(tx, to, kind, email, &vars, dedupe, procera_notify::delivery::Urgency::Urgent)
                .await?;
        }
        Ok(())
    }

    /// Inserts assignments for `members` (skipping existing ones) and notifies each new assignee
    /// (except for announcement campaigns, whose announcement delivery is the notice).
    async fn assign(
        &self,
        tx: &mut WsTx<'_>,
        campaign: Uuid,
        members: &[Uuid],
        source: &str,
        due: OffsetDateTime,
    ) -> Result<Vec<Uuid>, CampaignError> {
        let ids: Vec<Uuid> = members.iter().map(|_| self.ids.new_id()).collect();
        let now = self.clock.now();
        let created: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "INSERT INTO campaign_recipients (workspace_id, id, campaign_id, membership_id, source, assigned_at, due_at)
             SELECT $1, t.id, $2, t.m, $3, $4, $5 FROM unnest($6::uuid[], $7::uuid[]) AS t(id, m)
             ON CONFLICT (workspace_id, campaign_id, membership_id) DO NOTHING RETURNING id, membership_id",
        )
        .bind(tx.ctx.workspace_id)
        .bind(campaign)
        .bind(source)
        .bind(now)
        .bind(due)
        .bind(&ids)
        .bind(members)
        .fetch_all(tx.conn())
        .await?;
        let who: Vec<Uuid> = created.iter().map(|c| c.1).collect();
        let mail = Self::mail_vars(tx, campaign).await?;
        if mail.announcement.is_some() {
            return Ok(who);
        }
        let contacts = Self::contacts(tx, &who).await?;
        for (rid, m) in &created {
            let email = contacts.get(m).map(|c| c.0.as_str());
            self.tell(
                tx,
                "campaign_assigned",
                *m,
                email,
                &mail,
                due,
                &assignment_path(*rid),
                ("campaign_recipient", *rid),
                None,
                &format!("campaign:{rid}:assigned"),
            )
            .await?;
        }
        Ok(who)
    }

    /// (state, audience rule, snapshot, due_at, due_in_secs, subject version), locked. The version
    /// is `None` for announcement campaigns.
    async fn lock(tx: &mut WsTx<'_>, id: Uuid) -> Result<Locked, CampaignError> {
        let r: Option<Locked> = sqlx::query_as(
            "SELECT state, audience_rule, audience_snapshot, due_at, due_in_secs, subject_version_id FROM campaigns WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        r.ok_or(CampaignError::NotFound)
    }

    /// Launches a draft: snapshots the audience into assignments and notifies every assignee.
    pub async fn launch(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<Campaign, CampaignError> {
        self.launch_as(tx, Some(actor), actor.membership, id, None).await
    }

    /// Launches a draft. `snapshot_of` gives the assignees of an announcement campaign (the
    /// announcement's recipients); library campaigns resolve their audience rule instead.
    async fn launch_as(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        by: Uuid,
        id: Uuid,
        snapshot_of: Option<&[Uuid]>,
    ) -> Result<Campaign, CampaignError> {
        let (state, rule, snapshot, due_at, due_in, version) = Self::lock(tx, id).await?;
        if state != "draft" {
            return Err(CampaignError::Conflict(format!(
                "only draft campaigns can be launched (this one is {state})"
            )));
        }
        let now = self.clock.now();
        let due = due_at.unwrap_or(now + time::Duration::seconds(due_in));
        if due <= now {
            return Err(CampaignError::Invalid("the due date has passed; create a new campaign".into()));
        }
        let members = match (version, snapshot_of) {
            (Some(version), _) => {
                let lifecycle: String =
                    sqlx::query_scalar("SELECT lifecycle FROM artifact_versions WHERE id = $1")
                        .bind(version)
                        .fetch_one(tx.conn())
                        .await?;
                if lifecycle != "published" {
                    return Err(CampaignError::Conflict(format!(
                        "the version is {lifecycle}; only a published version can be launched"
                    )));
                }
                let aud: Audience =
                    serde_json::from_value(rule).map_err(|e| CampaignError::Internal(e.to_string()))?;
                audience::resolve(tx, &aud).await?
            }
            (None, Some(members)) => members.to_vec(),
            (None, None) => {
                return Err(CampaignError::Conflict(
                    "an announcement campaign starts when its announcement is published".into(),
                ));
            }
        };
        if members.is_empty() && snapshot == "at_launch" {
            return Err(CampaignError::Invalid("the audience matches no active members".into()));
        }
        sqlx::query(
            "UPDATE campaigns SET state = 'active', launched_at = $2, launched_by = $3, due_at = $4, revision = revision + 1 WHERE id = $1",
        )
        .bind(id)
        .bind(now)
        .bind(by)
        .bind(due)
        .execute(tx.conn())
        .await?;
        let assigned = self.assign(tx, id, &members, "launch", due).await?;
        Self::event(
            tx,
            "campaign.activated.v1",
            id,
            2,
            json!({"campaign_id": id, "recipients": assigned.len()}),
        )
        .await?;
        self.audit(
            tx,
            actor,
            Category::Content,
            "campaign.activated",
            ("campaign", id),
            json!({"recipients": assigned.len(), "due_at": due.unix_timestamp(), "audience_snapshot": snapshot}),
            None,
        )
        .await?;
        self.get(tx, id).await
    }

    /// Ends a campaign: `closed` (active only; frozen as evidence) or `cancelled` (draft or active;
    /// a reason is required). Open assignments stay open as the record of who did not acknowledge.
    pub async fn end(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        to: &str,
        reason: Option<&str>,
    ) -> Result<Campaign, CampaignError> {
        let (state, ..) = Self::lock(tx, id).await?;
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        match (to, state.as_str()) {
            ("closed", "active") => {}
            ("cancelled", "draft" | "active") => {
                if reason.is_none() {
                    return Err(CampaignError::Invalid("a reason is required to cancel a campaign".into()));
                }
            }
            ("closed" | "cancelled", s) => {
                return Err(CampaignError::Conflict(format!("this campaign is {s}; it cannot be {to}")));
            }
            _ => return Err(CampaignError::Internal(format!("unknown end state {to}"))),
        }
        let rev: i64 = sqlx::query_scalar(
            "UPDATE campaigns SET state = $2, ended_at = $3, ended_by = $4, end_reason = $5, revision = revision + 1 WHERE id = $1 RETURNING revision",
        )
        .bind(id)
        .bind(to)
        .bind(self.clock.now())
        .bind(actor.membership)
        .bind(reason)
        .fetch_one(tx.conn())
        .await?;
        let c = self.get(tx, id).await?;
        let topic = if to == "closed" { "campaign.closed.v1" } else { "campaign.cancelled.v1" };
        Self::event(tx, topic, id, rev, json!({"campaign_id": id, "was": state})).await?;
        self.audit(
            tx,
            Some(actor),
            Category::Content,
            if to == "closed" { "campaign.closed" } else { "campaign.cancelled" },
            ("campaign", id),
            json!({"was": state, "acknowledged": c.progress.acknowledged, "pending": c.progress.pending}),
            reason,
        )
        .await?;
        Ok(c)
    }

    /// Sends a reminder now to every open assignment of active members (manual "Remind").
    pub async fn remind(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> Result<usize, CampaignError> {
        let (state, ..) = Self::lock(tx, id).await?;
        if state != "active" {
            return Err(CampaignError::Conflict(format!(
                "only active campaigns send reminders (this one is {state})"
            )));
        }
        let now = self.clock.now();
        let open: Vec<(Uuid, Uuid, OffsetDateTime)> = sqlx::query_as(
            "SELECT r.id, r.membership_id, r.due_at FROM campaign_recipients r
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id AND m.status = 'active'
             WHERE r.campaign_id = $1 AND r.state = 'pending' ORDER BY r.id FOR UPDATE OF r",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let who: Vec<Uuid> = open.iter().map(|o| o.1).collect();
        let contacts = Self::contacts(tx, &who).await?;
        let mail = Self::mail_vars(tx, id).await?;
        let kind = mail.kind("reminder");
        for (rid, m, due) in &open {
            self.tell(
                tx,
                &kind,
                *m,
                contacts.get(m).map(|c| c.0.as_str()),
                &mail,
                *due,
                &mail.link(*rid),
                ("campaign_recipient", *rid),
                None,
                &format!("campaign:{rid}:remind:{}", now.unix_timestamp()),
            )
            .await?;
        }
        let rids: Vec<Uuid> = open.iter().map(|o| o.0).collect();
        sqlx::query("UPDATE campaign_recipients SET last_reminded_at = $2 WHERE id = ANY($1)")
            .bind(&rids)
            .bind(now)
            .execute(tx.conn())
            .await?;
        self.audit(
            tx,
            Some(actor),
            Category::Content,
            "campaign.reminded",
            ("campaign", id),
            json!({"recipients": open.len()}),
            None,
        )
        .await?;
        Ok(open.len())
    }

    /// Records the member's acknowledgement of their own assignment. `expected_hash`, when given,
    /// must match the campaign's version (the content the member read).
    pub async fn acknowledge(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        expected_hash: Option<&str>,
        evidence: Value,
    ) -> Result<Assignment, CampaignError> {
        let row: Option<(Uuid, String, String, Option<Uuid>, String, String)> = sqlx::query_as(
            "SELECT r.campaign_id, r.state, c.state, c.subject_version_id, encode(c.subject_hash, 'hex'), c.ack_text
             FROM campaign_recipients r JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
             WHERE r.id = $1 AND r.membership_id = $2 FOR UPDATE OF r",
        )
        .bind(id)
        .bind(actor.membership)
        .fetch_optional(tx.conn())
        .await?;
        let (campaign, state, campaign_state, version, hash, ack_text) =
            row.ok_or(CampaignError::NotFound)?;
        if state == "completed" {
            return Err(CampaignError::AlreadyAcknowledged);
        }
        // One record per acknowledgement: an announcement is acknowledged on the announcement,
        // which also records the receipt there.
        let Some(version) = version else {
            return Err(CampaignError::Conflict(
                "acknowledge this announcement from the announcement itself".into(),
            ));
        };
        if campaign_state != "active" {
            return Err(CampaignError::Conflict(format!("this campaign is {campaign_state}")));
        }
        if let Some(h) = expected_hash
            && !h.trim().eq_ignore_ascii_case(&hash)
        {
            return Err(CampaignError::ContentChanged { current: hash });
        }
        let now = self.clock.now();
        sqlx::query(
            "UPDATE campaign_recipients SET state = 'completed', completed_at = $2, ack_version_id = $3,
                    ack_content_hash = decode($4, 'hex'), ack_text = $5, ack_ip = $6::inet, ack_user_agent = $7, evidence = $8
             WHERE id = $1",
        )
        .bind(id)
        .bind(now)
        .bind(version)
        .bind(&hash)
        .bind(&ack_text)
        .bind(&actor.ip)
        .bind(&actor.user_agent)
        .bind(&evidence)
        .execute(tx.conn())
        .await?;
        Self::event(
            tx,
            "campaign.recipient.completed.v1",
            campaign,
            now.unix_timestamp(),
            json!({"campaign_id": campaign, "recipient_id": id, "membership_id": actor.membership, "version_id": version}),
        )
        .await?;
        self.audit(
            tx,
            Some(actor),
            Category::Content,
            "acknowledgement.completed",
            ("campaign_recipient", id),
            json!({"campaign_id": campaign, "version_id": version, "content_hash": hash}),
            None,
        )
        .await?;
        let r: AssignmentRow = sqlx::query_as(&format!("{ASSIGNMENT_SELECT} AND r.id = $3"))
            .bind(now)
            .bind(actor.membership)
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
        Ok(assignment(r, now))
    }

    /// Evidence export (FR-AK04): one CSV row per assignment. Audited as an export.
    pub async fn evidence_csv(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<String, CampaignError> {
        self.get(tx, id).await?;
        type Row = (
            String,
            String,
            Option<String>,
            String,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
            Option<i32>,
            Option<Uuid>,
            Option<String>,
            Option<Value>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT u.display_name, u.email::text, d.name, r.state, r.assigned_at, r.due_at, r.completed_at, v.version_no,
                    r.ack_version_id, encode(r.ack_content_hash, 'hex'), r.evidence
             FROM campaign_recipients r
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
             JOIN users u ON u.id = m.user_id
             LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
             LEFT JOIN artifact_versions v ON v.workspace_id = r.workspace_id AND v.id = r.ack_version_id
             WHERE r.campaign_id = $1 ORDER BY lower(u.display_name), u.email",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let mut w = csv::Writer::from_writer(vec![]);
        let header = [
            "member",
            "email",
            "department",
            "state",
            "assigned_at",
            "due_at",
            "acknowledged_at",
            "version",
            "version_id",
            "content_hash",
            "auth_method",
        ];
        w.write_record(header).map_err(|e| CampaignError::Internal(e.to_string()))?;
        for r in &rows {
            let amr =
                r.10.as_ref()
                    .and_then(|e| e["amr"].as_array())
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("+"))
                    .unwrap_or_default();
            w.write_record([
                cell(&r.0),
                cell(&r.1),
                cell(r.2.as_deref().unwrap_or("")),
                r.3.clone(),
                rfc3339(Some(r.4)),
                rfc3339(Some(r.5)),
                rfc3339(r.6),
                r.7.map(|n| n.to_string()).unwrap_or_default(),
                r.8.map(|v| v.to_string()).unwrap_or_default(),
                r.9.clone().unwrap_or_default(),
                cell(&amr),
            ])
            .map_err(|e| CampaignError::Internal(e.to_string()))?;
        }
        let bytes = w.into_inner().map_err(|e| CampaignError::Internal(e.to_string()))?;
        self.audit(
            tx,
            Some(actor),
            Category::Export,
            "campaign.evidence_exported",
            ("campaign", id),
            json!({"rows": rows.len(), "format": "csv"}),
            None,
        )
        .await?;
        String::from_utf8(bytes).map_err(|e| CampaignError::Internal(e.to_string()))
    }

    // ---------- reactions and scheduled work ----------

    /// Enrols a newly active member in every active `dynamic` campaign whose audience matches
    /// (FR-AK03). Their due date keeps the campaign's original window. Returns how many.
    pub async fn enroll_joiner(&self, tx: &mut WsTx<'_>, membership: Uuid) -> Result<usize, CampaignError> {
        let open: Vec<(Uuid, Value, OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
            // Announcement campaigns are always `at_launch` (their recipients are a snapshot).
            "SELECT id, audience_rule, launched_at, due_at FROM campaigns
             WHERE state = 'active' AND audience_snapshot = 'dynamic' AND subject_kind = 'artifact_version' ORDER BY id",
        )
        .fetch_all(tx.conn())
        .await?;
        let now = self.clock.now();
        let mut n = 0;
        for (id, rule, launched_at, due_at) in open {
            let aud: Audience =
                serde_json::from_value(rule).map_err(|e| CampaignError::Internal(e.to_string()))?;
            if !audience::contains(tx, &aud, membership).await? {
                continue;
            }
            let due = now + (due_at - launched_at);
            if self.assign(tx, id, &[membership], "joiner", due).await?.is_empty() {
                continue;
            }
            n += 1;
            self.audit(
                tx,
                None,
                Category::Content,
                "campaign.recipient_enrolled",
                ("campaign", id),
                json!({"membership_id": membership, "due_at": due.unix_timestamp()}),
                Some("new member matches a dynamic audience"),
            )
            .await?;
        }
        Ok(n)
    }

    /// Sends due reminder steps and manager escalations in every workspace once (scheduler).
    pub async fn run_due(&self) -> Result<DuePass, CampaignError> {
        let now = self.clock.now();
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.campaign_due_workspaces($1, 1000)")
                .bind(now)
                .fetch_all(self.db.pool())
                .await?;
        let mut pass = DuePass::default();
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let p = self.run_due_in(&mut tx, now).await?;
            tx.commit().await?;
            pass.reminders += p.reminders;
            pass.escalations += p.escalations;
        }
        Ok(pass)
    }

    async fn run_due_in(&self, tx: &mut WsTx<'_>, now: OffsetDateTime) -> Result<DuePass, CampaignError> {
        type Row = (Uuid, Uuid, Uuid, i32, i32, bool, OffsetDateTime, bool);
        let due: Vec<Row> = sqlx::query_as(
            "SELECT r.id, r.campaign_id, r.membership_id, r.reminders_sent, s.steps_due::int,
                    (c.escalation_after_secs IS NOT NULL AND r.escalated_at IS NULL
                     AND r.assigned_at + make_interval(secs => c.escalation_after_secs::double precision) <= $1) AS escalate,
                    r.due_at, coalesce(an.expires_at <= $1, false) AS lapsed
             FROM campaign_recipients r
             JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
             JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id AND m.status = 'active'
             LEFT JOIN announcements an ON an.workspace_id = c.workspace_id AND an.id = c.subject_announcement_id
             CROSS JOIN LATERAL (SELECT count(*) AS steps_due FROM jsonb_array_elements_text(c.reminder_schedule) x
                                 WHERE r.assigned_at + make_interval(secs => x::double precision) <= $1) s
             WHERE c.state = 'active' AND r.state = 'pending'
               AND (s.steps_due > r.reminders_sent
                    OR (c.escalation_after_secs IS NOT NULL AND r.escalated_at IS NULL
                        AND r.assigned_at + make_interval(secs => c.escalation_after_secs::double precision) <= $1))
             ORDER BY r.campaign_id, r.id LIMIT 5000 FOR UPDATE OF r SKIP LOCKED",
        )
        .bind(now)
        .fetch_all(tx.conn())
        .await?;
        if due.is_empty() {
            return Ok(DuePass::default());
        }
        let who: Vec<Uuid> = due.iter().map(|d| d.2).collect();
        let mut contacts = Self::contacts(tx, &who).await?;
        let mut mails: HashMap<Uuid, Mail> = HashMap::new();
        let mut reminded: BTreeMap<Uuid, usize> = BTreeMap::new();
        let mut pass = DuePass::default();
        for (rid, campaign, m, sent, steps, escalate, due_at, lapsed) in due {
            if let std::collections::hash_map::Entry::Vacant(e) = mails.entry(campaign) {
                e.insert(Self::mail_vars(tx, campaign).await?);
            }
            let Some(mail) = mails.get(&campaign) else { continue };
            let link = mail.link(rid);
            if steps > sent && lapsed {
                // An expired announcement can no longer be acknowledged: the step is used up
                // without a reminder (A-32). The escalation below still records the miss.
                sqlx::query("UPDATE campaign_recipients SET reminders_sent = $2 WHERE id = $1")
                    .bind(rid)
                    .bind(steps)
                    .execute(tx.conn())
                    .await?;
            } else if steps > sent {
                // One reminder for the latest due step, even if the scheduler missed earlier ones.
                self.tell(
                    tx,
                    &mail.kind("reminder"),
                    m,
                    contacts.get(&m).map(|c| c.0.as_str()),
                    mail,
                    due_at,
                    &link,
                    ("campaign_recipient", rid),
                    None,
                    &format!("campaign:{rid}:reminder:{steps}"),
                )
                .await?;
                sqlx::query(
                    "UPDATE campaign_recipients SET reminders_sent = $2, last_reminded_at = $3 WHERE id = $1",
                )
                .bind(rid)
                .bind(steps)
                .bind(now)
                .execute(tx.conn())
                .await?;
                *reminded.entry(campaign).or_default() += 1;
                pass.reminders += 1;
            }
            if escalate {
                let manager = self.members.resolve_manager(tx, m).await?;
                if let Some(to) = manager.membership_id {
                    if !contacts.contains_key(&to) {
                        contacts.extend(Self::contacts(tx, &[to]).await?);
                    }
                    let name = contacts.get(&m).map(|c| c.1.clone()).unwrap_or_default();
                    self.tell(
                        tx,
                        &mail.kind("escalated"),
                        to,
                        contacts.get(&to).map(|c| c.0.as_str()),
                        mail,
                        due_at,
                        &format!("/members/{m}"),
                        ("campaign_recipient", rid),
                        Some(&name),
                        &format!("campaign:{rid}:escalation"),
                    )
                    .await?;
                }
                sqlx::query(
                    "UPDATE campaign_recipients SET escalated_at = $2, escalated_to = $3 WHERE id = $1",
                )
                .bind(rid)
                .bind(now)
                .bind(manager.membership_id)
                .execute(tx.conn())
                .await?;
                self.audit(
                    tx,
                    None,
                    Category::Content,
                    "campaign.escalated",
                    ("campaign_recipient", rid),
                    json!({"campaign_id": campaign, "membership_id": m, "manager_membership_id": manager.membership_id, "via": manager.via}),
                    (manager.membership_id.is_none()).then_some("no manager could be resolved"),
                )
                .await?;
                pass.escalations += 1;
            }
        }
        for (campaign, count) in reminded {
            self.audit(
                tx,
                None,
                Category::Content,
                "campaign.reminders_sent",
                ("campaign", campaign),
                json!({"recipients": count}),
                None,
            )
            .await?;
        }
        Ok(pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_cannot_start_formulas() {
        assert_eq!(cell("=HYPERLINK(\"x\")"), "'=HYPERLINK(\"x\")");
        assert_eq!(cell("@SUM"), "'@SUM");
        assert_eq!(cell("Ann Lee"), "Ann Lee");
    }

    #[test]
    fn human_dates_are_short_and_unpadded() {
        let t = time::macros::datetime!(2026-10-05 10:00 UTC);
        assert_eq!(human_date(t), "5 Oct 2026");
    }
}
