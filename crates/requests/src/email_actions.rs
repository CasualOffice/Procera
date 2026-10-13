//! Email-actionable approvals (BPMS-145, FR-N03, docs/05 §8 "Email-actionable approvals").
//!
//! A plain approval step (no fields to fill in) emails each candidate Approve and Reject buttons that
//! carry a single-use link. The link only opens a confirm page (reading it never decides); the page
//! shows who is deciding and what, and posting it decides as that member through the same path as
//! My work (candidacy, separation of duties and "one decision wins" all apply), marking the link used
//! in the same transaction. A link works once, until the step closes, and for at most seven days;
//! only its SHA-256 is stored. Workspaces can turn it off (`notify.email_actions.enabled`).

use procera_crypto::{hash_token, random_token};
use procera_db::{WsCtx, WsTx};
use serde_json::{Map, Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{Actor, RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// How long a link lives at most.
pub const LIFETIME: Duration = Duration::days(7);

fn iso(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Issues a link for `member` on `work_item`; returns the token (for the email only).
pub(crate) async fn issue(
    tx: &mut WsTx<'_>,
    id: Uuid,
    work_item: Uuid,
    member: Uuid,
    now: OffsetDateTime,
    due: Option<OffsetDateTime>,
) -> Result<String, sqlx::Error> {
    let token = random_token();
    let expires = due.map_or(now + LIFETIME, |d| d.max(now + Duration::hours(1)).min(now + LIFETIME));
    sqlx::query("INSERT INTO email_action_links (workspace_id, id, work_item_id, membership_id, token_hash, created_at, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(tx.ctx.workspace_id).bind(id).bind(work_item).bind(member).bind(hash_token(&token)).bind(now).bind(expires)
        .execute(tx.conn()).await?;
    Ok(token)
}

/// A link's row: id, work item, member, expiry, used.
type Link = (Uuid, Uuid, Uuid, OffsetDateTime, Option<OffsetDateTime>);

impl Requests {
    async fn link_tx(&self, token: &str) -> R<(WsTx<'static>, Link)> {
        let hash = hash_token(token.trim());
        let ws: Option<Uuid> = sqlx::query_scalar("SELECT discovery.email_action_workspace($1)")
            .bind(&hash)
            .fetch_one(self.db.pool())
            .await?;
        let ws = ws.ok_or(RequestError::NotFound)?;
        let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
        let link: Link = sqlx::query_as("SELECT id, work_item_id, membership_id, expires_at, used_at FROM email_action_links WHERE token_hash = $1 FOR UPDATE")
            .bind(&hash)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(RequestError::NotFound)?;
        Ok((tx, link))
    }

    /// What the confirm page shows. `state`: `open`, `used`, `expired` or `closed` (decided or
    /// cancelled already). Answers and fields above `internal` are left out.
    pub async fn email_action(&self, token: &str) -> R<Value> {
        let (mut tx, (_, work, member, expires, used)) = self.link_tx(token).await?;
        let row: Option<(String, String, Vec<String>, i64, Uuid, String, String, String, Value, OffsetDateTime, String, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT w.name, w.state, w.outcomes, w.revision, r.instance_id, r.display_id, r.title, r.process_key, COALESCE(p.state->'variables', '{}'::jsonb), r.created_at, ui.display_name, w.due_at
               FROM work_items w JOIN requests r ON r.workspace_id = w.workspace_id AND r.instance_id = w.instance_id
               JOIN process_instances p ON p.workspace_id = r.workspace_id AND p.id = r.instance_id
               JOIN memberships mi ON mi.workspace_id = r.workspace_id AND mi.id = r.initiator_membership_id JOIN users ui ON ui.id = mi.user_id
              WHERE w.id = $1",
        )
        .bind(work)
        .fetch_optional(tx.conn())
        .await?;
        let (
            step,
            wstate,
            outcomes,
            revision,
            iid,
            display_id,
            title,
            type_key,
            input,
            submitted,
            initiator,
            due,
        ) = row.ok_or(RequestError::NotFound)?;
        let (me, workspace): (String, String) = sqlx::query_as(
            "SELECT u.display_name, w.name FROM memberships m JOIN users u ON u.id = m.user_id JOIN workspaces w ON w.id = m.workspace_id WHERE m.id = $1",
        )
        .bind(member)
        .fetch_one(tx.conn())
        .await?;
        let now = self.clock.now();
        let state = if used.is_some() {
            "used"
        } else if !matches!(wstate.as_str(), "offered" | "claimed") {
            "closed"
        } else if expires <= now {
            "expired"
        } else {
            "open"
        };
        // The request's answers, as the approver sees them on the request: labelled, internal or public only.
        let mut answers = Vec::new();
        let rt = self.catalogue.resolve(&mut tx, &type_key).await.map_err(RequestError::Internal)?;
        if let Some(rt) = &rt {
            let input: Map<String, Value> = input.as_object().cloned().unwrap_or_default();
            for f in &rt.start_form.fields {
                let open = matches!(f.classification.as_deref(), None | Some("public" | "internal"));
                if let (true, Some(v)) = (open, input.get(&f.name)) {
                    answers
                        .push(json!({ "label": f.label, "type": f.ty, "value": v, "currency": f.currency }));
                }
            }
        }
        let type_name: Option<String> = rt.as_ref().map(|rt| rt.name.clone());
        tx.rollback().await?;
        Ok(json!({
            "state": state, "deciding_as": me, "workspace": workspace, "step": step, "outcomes": outcomes, "revision": revision,
            "request": { "id": iid, "display_id": display_id, "title": title, "type_name": type_name, "initiator": initiator, "submitted_at": iso(submitted), "due_at": due.map(iso) },
            "answers": answers, "expires_at": iso(expires),
        }))
    }

    /// Decides through a link (once): `approved`, or `rejected` with a comment.
    pub async fn email_action_decide(
        &self,
        token: &str,
        outcome: &str,
        comment: Option<&str>,
        ip: Option<String>,
        user_agent: Option<String>,
    ) -> R<Value> {
        let (mut tx, (id, work, member, expires, used)) = self.link_tx(token).await?;
        if used.is_some() {
            return Err(RequestError::Conflict(
                "This link has already been used. Open the request in Procera to see where it stands.".into(),
            ));
        }
        if expires <= self.clock.now() {
            return Err(RequestError::Conflict(
                "This link has expired. Open the request in Procera to decide.".into(),
            ));
        }
        if !matches!(outcome, "approved" | "rejected") {
            return Err(RequestError::Invalid(vec![crate::forms::FieldError::new(
                "/outcome",
                "invalid",
                "Approve or reject.",
            )]));
        }
        let comment = comment.map(str::trim).filter(|c| !c.is_empty());
        if outcome == "rejected" && comment.is_none() {
            return Err(RequestError::Invalid(vec![crate::forms::FieldError::new(
                "/comment",
                "required",
                "Say why you're rejecting it.",
            )]));
        }
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM work_items WHERE id = $1")
            .bind(work)
            .fetch_one(tx.conn())
            .await?;
        let subject: Uuid = sqlx::query_scalar(
            "SELECT u.subject_ref FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(member)
        .fetch_one(tx.conn())
        .await?;
        let actor = Actor { membership: member, subject_ref: subject, ip, user_agent };
        let decided = self.decide(&mut tx, &actor, work, revision, outcome, comment, &Map::new()).await?;
        sqlx::query("UPDATE email_action_links SET used_at = $2 WHERE id = $1")
            .bind(id)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        // Other links for this step can't be used any more either.
        sqlx::query("UPDATE email_action_links SET used_at = $2 WHERE work_item_id = $1 AND used_at IS NULL")
            .bind(work)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        self.audit(
            &mut tx,
            &actor,
            "work.decided_by_email",
            "work_item",
            work,
            json!({ "outcome": outcome, "link_id": id }),
        )
        .await?;
        tx.commit().await?;
        Ok(
            json!({ "outcome": outcome, "display_id": decided.request.display_id, "status": decided.request.status }),
        )
    }
}
