//! Request and work-item use cases. Every method runs in the caller's workspace transaction, so the
//! HTTP layer can put the idempotency claim, the change and its audit record in one commit.

use std::collections::BTreeMap;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use procera_engine::{EngineError, put_calendar_if_absent};
use procera_runtime::Event;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::catalogue::{Form, RequestType};
use crate::forms::{self, FieldError};
use crate::views::{
    Decided, Person, RequestDetail, RequestSummary, Sla, Started, Step, TimelineEntry, WorkRequest, WorkView,
};
use crate::{Actor, DEFAULT_CALENDAR, RequestError, Requests, default_calendar};

type R<T> = Result<T, RequestError>;

const LIST_LIMIT: i64 = 200;

/// Which requests a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Started by this member.
    Mine(Uuid),
    All,
}

/// Which work items a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Open items offered to the member.
    Open,
    /// Items the member decided.
    Done,
    /// Open items nobody could be resolved for (admins).
    Unassigned,
}

const SUMMARY: &str = "SELECT r.instance_id, r.display_id, r.process_key, r.title, r.status, r.current_step, r.created_at, r.updated_at,
       r.initiator_membership_id, u.display_name,
       coalesce((SELECT array_agg(DISTINCT u2.display_name ORDER BY u2.display_name) FROM work_items w
                 JOIN work_item_candidates c ON c.workspace_id = w.workspace_id AND c.work_item_id = w.id
                 JOIN memberships m2 ON m2.workspace_id = c.workspace_id AND m2.id = c.membership_id
                 JOIN users u2 ON u2.id = m2.user_id
                 WHERE w.instance_id = r.instance_id AND w.state IN ('offered', 'claimed')), '{}'),
       r.ended_at
  FROM requests r JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.initiator_membership_id
  JOIN users u ON u.id = m.user_id";

type SummaryRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Option<String>,
    OffsetDateTime,
    OffsetDateTime,
    Uuid,
    String,
    Vec<String>,
    Option<OffsetDateTime>,
);

#[derive(sqlx::FromRow)]
struct WorkListRow {
    id: Uuid,
    node_id: String,
    name: String,
    kind: String,
    outcomes: Vec<String>,
    due_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    revision: i64,
    unassigned: bool,
    outcome: Option<String>,
    completed_at: Option<OffsetDateTime>,
    instance_id: Uuid,
    display_id: String,
    process_key: String,
    title: String,
    initiator_membership_id: Uuid,
    initiator_name: String,
    variables: Value,
    state: String,
}

/// What the decision path reads under the row lock.
type WorkRow = (Uuid, String, String, String, Vec<String>, i64, bool, Vec<String>, String);

impl Requests {
    fn type_of(&self, key: &str) -> R<std::sync::Arc<RequestType>> {
        self.catalogue.get(key).cloned().ok_or_else(|| RequestError::UnknownType(key.to_owned()))
    }

    fn summary(&self, r: SummaryRow) -> RequestSummary {
        let type_name = self.catalogue.get(&r.2).map_or_else(|| r.2.clone(), |t| t.name.to_owned());
        RequestSummary {
            id: r.0,
            display_id: r.1,
            ty: r.2,
            type_name,
            title: r.3,
            status: r.4,
            current_step: r.5,
            assignees: r.10,
            created_at: r.6,
            updated_at: r.7,
            initiator: Person { id: r.8, name: r.9 },
        }
    }

    /// A principal from the directory (docs/11 §5, A-26): `{id, email, department, manager}`, the
    /// manager shaped one level further so `x.manager.manager` resolves.
    pub async fn principal(&self, tx: &mut WsTx<'_>, m: Uuid) -> R<Value> {
        let mut p = self.basic(tx, m).await?;
        if let Some(mgr) = self.members.resolve_manager(tx, m).await?.membership_id {
            let mut mp = self.basic(tx, mgr).await?;
            if let Some(mm) = self.members.resolve_manager(tx, mgr).await?.membership_id {
                mp["manager"] = json!({ "id": mm.to_string() });
            }
            p["manager"] = mp;
        }
        Ok(p)
    }

    async fn basic(&self, tx: &mut WsTx<'_>, m: Uuid) -> R<Value> {
        let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT u.email::text, m.department_id FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(m)
        .fetch_optional(tx.conn())
        .await?;
        let (email, dept) = row.ok_or(RequestError::NotFound)?;
        Ok(json!({ "id": m.to_string(), "email": email, "department": dept.map(|d| d.to_string()) }))
    }

    /// Validates a form and shapes its principal fields from the directory. Errors point under `member`.
    async fn values(
        &self,
        tx: &mut WsTx<'_>,
        form: &Form,
        input: &Map<String, Value>,
        partial: bool,
        member: &str,
    ) -> R<BTreeMap<String, Value>> {
        let under =
            |e: Vec<FieldError>| RequestError::Invalid(e.into_iter().map(|e| e.under(member)).collect());
        let mut vals = forms::validate(form, input, partial).map_err(under)?;
        let mut errors = Vec::new();
        for f in form.fields.iter().filter(|f| f.ty == "principal") {
            let Some(id) = vals.get(&f.name).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
            )
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
            if active {
                vals.insert(f.name.clone(), self.principal(tx, id).await?);
            } else {
                errors.push(FieldError::new(
                    &f.name,
                    "unknown_member",
                    "Choose an active member of this workspace",
                ));
            }
        }
        if errors.is_empty() { Ok(vals) } else { Err(under(errors)) }
    }

    /// Starts a request of type `key` for `actor` (BPMS-103). Appends `request.started`.
    pub async fn start(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        key: &str,
        input: &Map<String, Value>,
    ) -> R<Started> {
        let rt = self.type_of(key)?;
        let vars = self.values(tx, &rt.start_form, input, false, "input").await?;
        if let Some(check) = rt.check {
            let errs = check(&vars);
            if !errs.is_empty() {
                return Err(RequestError::Invalid(errs.into_iter().map(|e| e.under("input")).collect()));
            }
        }
        let initiator = self.principal(tx, actor.membership).await?;
        put_calendar_if_absent(
            tx,
            self.clock.as_ref(),
            self.ids.as_ref(),
            DEFAULT_CALENDAR,
            "1",
            &default_calendar(),
        )
        .await?;
        let who = procera_engine::Actor { subject_ref: actor.subject_ref };
        let a = self.engine.start_in(tx, &rt.program, initiator, vars, Some(who)).await?;
        let started = self.started(tx, a.instance_id).await?;
        self.audit(
            tx,
            actor,
            "request.started",
            "request",
            a.instance_id,
            json!({ "type": key, "display_id": started.display_id }),
        )
        .await?;
        Ok(started)
    }

    async fn started(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Started> {
        let (display_id, status): (String, String) =
            sqlx::query_as("SELECT display_id, status FROM requests WHERE instance_id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
        Ok(Started { id, display_id, status })
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        action: &str,
        kind: &str,
        id: Uuid,
        diff: Value,
    ) -> R<()> {
        let mut ev = AuditEvent::new(
            Some(actor.subject_ref),
            ActorKind::User,
            Category::Runtime,
            action,
            Outcome::Allowed,
        )
        .object(kind, id);
        ev.diff = Some(diff);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// Requests, newest first (at most 200).
    pub async fn list(
        &self,
        tx: &mut WsTx<'_>,
        scope: Scope,
        status: Option<&str>,
    ) -> R<Vec<RequestSummary>> {
        let mine = match scope {
            Scope::Mine(m) => Some(m),
            Scope::All => None,
        };
        let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
            "{SUMMARY} WHERE ($1::uuid IS NULL OR r.initiator_membership_id = $1) AND ($2::text IS NULL OR r.status = $2)
             ORDER BY r.created_at DESC, r.instance_id DESC LIMIT $3"
        ))
        .bind(mine)
        .bind(status)
        .bind(LIST_LIMIT)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|r| self.summary(r)).collect())
    }

    /// Whether `m` started the request, is offered one of its steps or decided one.
    pub async fn involved(&self, tx: &mut WsTx<'_>, id: Uuid, m: Uuid) -> R<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM requests WHERE instance_id = $1 AND initiator_membership_id = $2)
                 OR EXISTS (SELECT 1 FROM work_items w WHERE w.instance_id = $1 AND (w.completed_by = $2 OR EXISTS
                            (SELECT 1 FROM work_item_candidates c WHERE c.work_item_id = w.id AND c.membership_id = $2)))",
        )
        .bind(id)
        .bind(m)
        .fetch_one(tx.conn())
        .await?)
    }

    /// The request initiator, or `NotFound`.
    pub async fn initiator(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Uuid> {
        sqlx::query_scalar("SELECT initiator_membership_id FROM requests WHERE instance_id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(RequestError::NotFound)
    }

    /// Principals as `{id, name}` for display; other values as stored.
    async fn display(
        &self,
        tx: &mut WsTx<'_>,
        form: &Form,
        vars: &BTreeMap<String, Value>,
    ) -> R<Map<String, Value>> {
        let mut out = Map::new();
        for f in &form.fields {
            let Some(v) = vars.get(&f.name) else { continue };
            let shown = match (
                f.ty,
                v.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok()),
            ) {
                ("principal", Some(id)) => {
                    json!({ "id": id, "name": crate::handler::name(tx, id).await.map_err(RequestError::Internal)? })
                }
                _ => v.clone(),
            };
            out.insert(f.name.clone(), shown);
        }
        Ok(out)
    }

    pub async fn detail(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<RequestDetail> {
        let row: Option<SummaryRow> = sqlx::query_as(&format!("{SUMMARY} WHERE r.instance_id = $1"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
        let row = row.ok_or(RequestError::NotFound)?;
        let ended_at = row.11;
        let summary = self.summary(row);
        let rt = self.type_of(&summary.ty)?;
        let state: Value = sqlx::query_scalar("SELECT state FROM process_instances WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
        let vars: BTreeMap<String, Value> = serde_json::from_value(state["variables"].clone())?;
        let input = self.display(tx, &rt.start_form, &vars).await?;
        type StepRow = (
            Uuid,
            String,
            String,
            String,
            String,
            bool,
            Option<String>,
            Option<Value>,
            Option<OffsetDateTime>,
            Option<OffsetDateTime>,
            OffsetDateTime,
            Option<String>,
            Vec<String>,
        );
        let rows: Vec<StepRow> = sqlx::query_as(
            "SELECT w.id, w.node_id, w.name, w.kind, w.state, w.unassigned, w.outcome, w.decision, w.completed_at, w.due_at, w.created_at,
                    (SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = w.completed_by),
                    coalesce((SELECT array_agg(u.display_name ORDER BY u.display_name) FROM work_item_candidates c
                              JOIN memberships m ON m.workspace_id = c.workspace_id AND m.id = c.membership_id JOIN users u ON u.id = m.user_id
                              WHERE c.work_item_id = w.id), '{}')
               FROM work_items w WHERE w.instance_id = $1 ORDER BY w.created_at, w.work_key",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let mut timeline = vec![TimelineEntry {
            at: summary.created_at,
            kind: "submitted".into(),
            text: format!("Submitted by {}", summary.initiator.name),
            actor: Some(summary.initiator.name.clone()),
        }];
        let mut steps = Vec::new();
        for r in rows {
            let comment =
                r.7.as_ref().and_then(|d| d.get("comment")).and_then(Value::as_str).map(str::to_owned);
            if let (Some(outcome), Some(at)) = (&r.6, r.8) {
                let who = r.11.clone().unwrap_or_else(|| "Someone".into());
                timeline.push(TimelineEntry {
                    at,
                    kind: "decided".into(),
                    text: format!("{}: {} by {who}", r.2, outcome.replace('_', " ")),
                    actor: r.11.clone(),
                });
            }
            steps.push(Step {
                id: r.0,
                node: r.1,
                name: r.2,
                kind: r.3,
                state: r.4,
                assignees: r.12,
                unassigned: r.5,
                decided_by: r.11,
                outcome: r.6,
                comment,
                decided_at: r.8,
                due_at: r.9,
                created_at: r.10,
            });
        }
        let events: Vec<(String, OffsetDateTime, Value)> = sqlx::query_as(
            "SELECT event_type, happened_at, payload FROM instance_events WHERE instance_id = $1 AND event_type IN ('cancel', 'timer_fired') ORDER BY seq",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        for (kind, at, payload) in events {
            let text = match kind.as_str() {
                "cancel" => format!("Cancelled: {}", payload["event"]["reason"].as_str().unwrap_or_default()),
                _ => "A deadline passed".to_owned(),
            };
            timeline.push(TimelineEntry {
                at,
                kind: if kind == "cancel" { "cancelled".into() } else { "timer".into() },
                text,
                actor: None,
            });
        }
        if let (Some(at), "approved" | "rejected") = (ended_at, summary.status.as_str()) {
            timeline.push(TimelineEntry {
                at,
                kind: "completed".into(),
                text: format!("Request {}", summary.status),
                actor: None,
            });
        }
        timeline.sort_by_key(|t| t.at);
        let sla: Option<(OffsetDateTime, Option<OffsetDateTime>, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT due_at, breached_at, stopped_at FROM sla_clocks WHERE instance_id = $1 AND node_id IS NULL ORDER BY started_at LIMIT 1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        Ok(RequestDetail {
            summary,
            ended_at,
            fields: rt.start_form.fields.clone(),
            input,
            steps,
            timeline,
            sla: sla.map(|(due_at, b, s)| Sla { due_at, breached: b.is_some(), stopped: s.is_some() }),
        })
    }

    /// Cancels a running request (Event::Cancel). Appends `request.cancelled`.
    pub async fn cancel(&self, tx: &mut WsTx<'_>, actor: Actor, id: Uuid, reason: &str) -> R<Started> {
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(RequestError::Invalid(vec![FieldError::new(
                "reason",
                "required",
                "Give a reason (up to 500 characters)",
            )]));
        }
        let status: String = sqlx::query_scalar("SELECT status FROM requests WHERE instance_id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(RequestError::NotFound)?;
        if !matches!(status.as_str(), "in_progress" | "changes_requested") {
            return Err(RequestError::Conflict(format!("the request is already {status}")));
        }
        let stimulus = self.engine.stamp(Event::Cancel { reason: reason.to_owned() });
        let who = procera_engine::Actor { subject_ref: actor.subject_ref };
        self.engine.apply_in_as(tx, id, &stimulus, None, Some(who)).await.map_err(|e| match e {
            EngineError::Rejected(_) => RequestError::Conflict("the request has already ended".into()),
            e => e.into(),
        })?;
        self.audit(tx, actor, "request.cancelled", "request", id, json!({ "reason": reason })).await?;
        self.started(tx, id).await
    }

    /// Work items for `m`.
    pub async fn work(&self, tx: &mut WsTx<'_>, m: Uuid, state: WorkState) -> R<Vec<WorkView>> {
        let filter = match state {
            WorkState::Open => {
                "w.state IN ('offered', 'claimed') AND EXISTS (SELECT 1 FROM work_item_candidates c WHERE c.work_item_id = w.id AND c.membership_id = $1)"
            }
            WorkState::Done => "w.state = 'completed' AND w.completed_by = $1",
            WorkState::Unassigned => "w.state IN ('offered', 'claimed') AND w.unassigned AND $1 = $1",
        };
        let rows: Vec<WorkListRow> = sqlx::query_as(&format!(
            "SELECT w.id, w.node_id, w.name, w.kind, w.outcomes, w.due_at, w.created_at, w.revision, w.unassigned, w.outcome, w.completed_at,
                    r.instance_id, r.display_id, r.process_key, r.title, r.initiator_membership_id, u.display_name AS initiator_name,
                    p.state->'variables' AS variables, w.state
               FROM work_items w JOIN requests r ON r.workspace_id = w.workspace_id AND r.instance_id = w.instance_id
               JOIN process_instances p ON p.workspace_id = w.workspace_id AND p.id = w.instance_id
               JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.initiator_membership_id JOIN users u ON u.id = m.user_id
              WHERE {filter} ORDER BY w.due_at NULLS LAST, w.created_at DESC LIMIT {LIST_LIMIT}"
        ))
        .bind(m)
        .fetch_all(tx.conn())
        .await?;
        let mut out = Vec::new();
        for r in rows {
            let rt = self.type_of(&r.process_key)?;
            let form = rt.task_forms.get(&r.node_id).cloned().unwrap_or_default();
            let vars: BTreeMap<String, Value> =
                serde_json::from_value(r.variables.clone()).unwrap_or_default();
            let values = self.display(tx, &form, &vars).await?;
            out.push(WorkView {
                id: r.id,
                request: WorkRequest {
                    id: r.instance_id,
                    display_id: r.display_id,
                    ty: r.process_key.clone(),
                    type_name: rt.name.to_owned(),
                    title: r.title,
                    initiator: Person { id: r.initiator_membership_id, name: r.initiator_name },
                },
                step: r.name,
                kind: r.kind,
                state: r.state,
                outcomes: r.outcomes,
                due_at: r.due_at,
                created_at: r.created_at,
                revision: r.revision,
                fields: form.fields,
                values,
                unassigned: r.unassigned,
                outcome: r.outcome,
                completed_at: r.completed_at,
            });
        }
        Ok(out)
    }

    /// Decides a work item (docs/08 §7.2). Exactly one decision wins: the instance row is locked
    /// first (docs/04 §4.3 lock order), then the item; a decided item is `AlreadyDecided`.
    /// Candidacy and SoD are re-checked here because the directory may have changed (docs/12 §10).
    #[allow(clippy::too_many_arguments)]
    pub async fn decide(
        &self,
        tx: &mut WsTx<'_>,
        actor: Actor,
        work_id: Uuid,
        if_match: i64,
        outcome: &str,
        comment: Option<&str>,
        fields: &Map<String, Value>,
    ) -> R<Decided> {
        let instance: Uuid = sqlx::query_scalar("SELECT instance_id FROM work_items WHERE id = $1")
            .bind(work_id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(RequestError::NotFound)?;
        let candidate: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM work_item_candidates WHERE work_item_id = $1 AND membership_id = $2)",
        )
        .bind(work_id)
        .bind(actor.membership)
        .fetch_one(tx.conn())
        .await?;
        if !candidate {
            return Err(if self.involved(tx, instance, actor.membership).await? {
                RequestError::Forbidden("this step is not offered to you".into())
            } else {
                RequestError::NotFound
            });
        }
        sqlx::query("SELECT 1 FROM process_instances WHERE id = $1 FOR UPDATE")
            .bind(instance)
            .execute(tx.conn())
            .await?;
        let (_, key, node, state, outcomes, revision, ex_init, ex_actors, process_key): WorkRow = sqlx::query_as(
            "SELECT w.instance_id, w.work_key, w.node_id, w.state, w.outcomes, w.revision, w.exclude_initiator, w.exclude_actors_of, r.process_key
               FROM work_items w JOIN requests r ON r.workspace_id = w.workspace_id AND r.instance_id = w.instance_id
              WHERE w.id = $1 FOR UPDATE OF w",
        )
        .bind(work_id)
        .fetch_one(tx.conn())
        .await?;
        if !matches!(state.as_str(), "offered" | "claimed") {
            return Err(RequestError::AlreadyDecided);
        }
        if if_match != revision {
            return Err(RequestError::Stale { current: revision });
        }
        let initiator = self.initiator(tx, instance).await?;
        let excluded = crate::resolve::excluded(tx, instance, Some(initiator), ex_init, &ex_actors)
            .await
            .map_err(RequestError::Internal)?;
        if excluded.contains(&actor.membership) {
            return Err(RequestError::SoD(if actor.membership == initiator {
                "you started this request".into()
            } else {
                "you decided an earlier step of this request".into()
            }));
        }
        let mut errors = Vec::new();
        if !outcomes.iter().any(|o| o == outcome) {
            errors.push(FieldError::new(
                "outcome",
                "invalid_option",
                &format!("Must be one of: {}", outcomes.join(", ")),
            ));
        }
        let comment = comment.map(str::trim).filter(|c| !c.is_empty());
        if comment.is_none() && matches!(outcome, "rejected" | "changes_requested") {
            errors.push(FieldError::new("comment", "required", "Say why, so the requester knows what to do"));
        }
        if comment.is_some_and(|c| c.chars().count() > 2_000) {
            errors.push(FieldError::new("comment", "too_long", "Must be at most 2000 characters"));
        }
        if !errors.is_empty() {
            return Err(RequestError::Invalid(errors));
        }
        let rt = self.type_of(&process_key)?;
        let form = rt.task_forms.get(&node).cloned().unwrap_or_default();
        let vars = self.values(tx, &form, fields, true, "fields").await?;
        let now = self.clock.now();
        let decision = json!({ "comment": comment, "fields": fields });
        sqlx::query(
            "UPDATE work_items SET state = 'completed', outcome = $2, decision = $3, completed_at = $4, completed_by = $5, revision = revision + 1
             WHERE id = $1",
        )
        .bind(work_id)
        .bind(outcome)
        .bind(&decision)
        .bind(now)
        .bind(actor.membership)
        .execute(tx.conn())
        .await?;
        let stimulus =
            self.engine.stamp(Event::WorkItemCompleted { key, outcome: outcome.to_owned(), variables: vars });
        let who = procera_engine::Actor { subject_ref: actor.subject_ref };
        self.engine.apply_in_as(tx, instance, &stimulus, None, Some(who)).await.map_err(|e| match e {
            EngineError::Rejected(_) => RequestError::AlreadyDecided,
            e => e.into(),
        })?;
        self.audit(
            tx,
            actor,
            "work_item.decided",
            "work_item",
            work_id,
            json!({ "outcome": outcome, "request": instance, "step": node }),
        )
        .await?;
        Ok(Decided {
            id: work_id,
            revision: revision + 1,
            outcome: outcome.to_owned(),
            request: self.started(tx, instance).await?,
        })
    }
}
