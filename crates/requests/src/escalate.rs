//! Work-item reminders, escalations and reassignment (BPMS-085, docs/11 §3.2, docs/19 §5.6).
//!
//! - **Arming.** When a work item is created, each of its node's `escalations` becomes a handler
//!   timer `<work key>/esc/<n>` (purpose `reminder` for `remind`, else `escalation`). `at` counts
//!   from activation, in business time when the step's `due` is business time, else wall time.
//! - **Cancelling.** A decision or a cancelled step cancels every timer under `<work key>/esc/`.
//! - **Firing** ([`Effects::fire_timer`]), only while the item is still open:
//!   - `remind`: the current candidates hear `work_reminder`;
//!   - `notify`: the `to` principals hear `work_escalated`;
//!   - `reassign`: the candidates become `to` (SoD still applies), audited `work_item.reassigned`
//!     by the system; when `to` resolves to nobody the step stays where it is;
//!   - `outcome`: the item completes with that outcome and the instance advances (only when the
//!     IR says so; controlled processes cannot, IR-E030).
//! - Every action is kept in the item's `resolution.history`, which the request timeline shows.

use std::collections::{BTreeMap, BTreeSet};

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use procera_engine::{BusinessDuration, FiredTimer, HandlerTimer, arm_timer, cancel_timers_under, store};
use procera_ir::model::{Due, Escalation, EscalationAction};
use procera_notify::delivery::Urgency;
use procera_runtime::{Event, InstanceState, Stimulus, TimerPurpose};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::catalogue::RequestType;
use crate::handler::{Effects, human_date, name};

type R<T> = Result<T, String>;

fn db(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Separates a work key from an escalation index in a timer key.
pub(crate) const ESC: &str = "/esc/";

/// What a reminder, escalation or reassignment needs to know about an open work item.
pub(crate) struct Item {
    pub id: Uuid,
    pub key: String,
    pub instance: Uuid,
    pub node: String,
    pub open: bool,
    pub exclude_initiator: bool,
    pub exclude_actors_of: Vec<String>,
    pub process_key: String,
    pub initiator: Uuid,
    /// display_id, title, type_name, step, due, initiator: the common template variables.
    pub vars: Value,
}

type ItemRow = (
    Uuid,
    String,
    Uuid,
    String,
    String,
    String,
    Option<OffsetDateTime>,
    bool,
    Vec<String>,
    String,
    Uuid,
    String,
    String,
);

/// Loads a work item (locked) with its request, by id or by work key.
pub(crate) async fn item(
    tx: &mut WsTx<'_>,
    rt_name: impl Fn(&str) -> Option<String>,
    by: Result<Uuid, &str>,
) -> R<Option<Item>> {
    let (id, key) = match by {
        Ok(id) => (Some(id), None),
        Err(k) => (None, Some(k)),
    };
    let row: Option<ItemRow> = sqlx::query_as(
        "SELECT w.id, w.work_key, w.instance_id, w.node_id, w.name, w.state, w.due_at, w.exclude_initiator, w.exclude_actors_of,
                r.process_key, r.initiator_membership_id, r.display_id, r.title
           FROM work_items w JOIN requests r ON r.workspace_id = w.workspace_id AND r.instance_id = w.instance_id
          WHERE ($1::uuid IS NULL OR w.id = $1) AND ($2::text IS NULL OR w.work_key = $2) FOR UPDATE OF w",
    )
    .bind(id)
    .bind(key)
    .fetch_optional(tx.conn())
    .await
    .map_err(db)?;
    let Some(r) = row else { return Ok(None) };
    let initiator_name = name(tx, r.10).await?;
    let vars = json!({
        "display_id": r.11, "title": r.12, "type_name": rt_name(&r.9).unwrap_or_else(|| r.9.clone()),
        "step": r.4, "due": r.6.map(human_date), "initiator": initiator_name,
    });
    Ok(Some(Item {
        id: r.0,
        key: r.1,
        instance: r.2,
        node: r.3,
        open: matches!(r.5.as_str(), "offered" | "claimed"),
        exclude_initiator: r.7,
        exclude_actors_of: r.8,
        process_key: r.9,
        initiator: r.10,
        vars,
    }))
}

pub(crate) async fn candidates(tx: &mut WsTx<'_>, item: Uuid) -> R<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT membership_id FROM work_item_candidates WHERE work_item_id = $1 ORDER BY membership_id",
    )
    .bind(item)
    .fetch_all(tx.conn())
    .await
    .map_err(db)
}

async fn names(tx: &mut WsTx<'_>, ms: &[Uuid]) -> R<Vec<String>> {
    let mut out = Vec::new();
    for m in ms {
        out.push(name(tx, *m).await?);
    }
    Ok(out)
}

/// Appends one entry to the item's `resolution.history`.
pub(crate) async fn history(tx: &mut WsTx<'_>, item: Uuid, entry: Value) -> R<()> {
    sqlx::query(
        "UPDATE work_items SET resolution = jsonb_set(resolution, '{history}', coalesce(resolution->'history', '[]'::jsonb) || $2::jsonb)
         WHERE id = $1",
    )
    .bind(item)
    .bind(json!([entry]))
    .execute(tx.conn())
    .await
    .map_err(db)?;
    Ok(())
}

/// Cancels a work item's reminders and escalations.
pub(crate) async fn cancel(tx: &mut WsTx<'_>, work_key: &str) -> R<()> {
    cancel_timers_under(tx, &format!("{work_key}{ESC}")).await.map_err(db)?;
    Ok(())
}

impl Effects {
    /// Arms the escalations of a work item just created.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn arm_escalations(
        &self,
        tx: &mut WsTx<'_>,
        rt: &RequestType,
        instance: Uuid,
        work_key: &str,
        node: &str,
        escalations: &[Escalation],
        due: Option<&Due>,
        from: OffsetDateTime,
    ) -> R<()> {
        if escalations.is_empty() {
            return Ok(());
        }
        let cal = if due.is_some_and(|d| d.business == Some(true)) {
            let cref = rt.exe.calendar_ref(None).ok_or("no default calendar")?;
            Some(store::calendar_by_ref(tx, &cref).await.map_err(db)?)
        } else {
            None
        };
        for (n, e) in escalations.iter().enumerate() {
            let d = BusinessDuration::parse(&e.at.0).map_err(db)?;
            let (due_at, rule) = match &cal {
                Some(c) => (c.calendar.add(from, d).map_err(db)?, format!("business {} from {from}", e.at.0)),
                None => {
                    let secs = d.weeks * 7 * 86_400 + d.days * 86_400 + d.seconds;
                    let at = from + time::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX));
                    (at, format!("{} from {from}", e.at.0))
                }
            };
            let key = format!("{work_key}{ESC}{n}");
            let purpose = if e.action == EscalationAction::Remind {
                TimerPurpose::Reminder
            } else {
                TimerPurpose::Escalation
            };
            let t = HandlerTimer {
                key: &key,
                instance,
                node,
                purpose,
                due_at,
                calendar_version: cal.as_ref().map(|c| c.id),
                source_tz: cal.as_ref().map(|c| c.timezone.as_str()),
                rule: &rule,
            };
            arm_timer(tx, self.ids.as_ref(), from, t).await.map_err(db)?;
        }
        Ok(())
    }

    /// Runs a due reminder or escalation (see the module docs).
    pub(crate) async fn fire_timer(&self, tx: &mut WsTx<'_>, t: FiredTimer<'_>) -> R<Option<Stimulus>> {
        let Some((work_key, n)) = t.key.rsplit_once(ESC) else { return Ok(None) };
        let Ok(n) = n.parse::<usize>() else { return Ok(None) };
        let cat = self.catalogue.clone();
        let Some(it) = item(tx, |k| cat.get(k).map(|r| r.name.to_owned()), Err(work_key)).await? else {
            return Ok(None);
        };
        let Some(rt) = self.catalogue.get(&it.process_key).cloned() else { return Ok(None) };
        let esc = rt
            .exe
            .program
            .ir()
            .nodes
            .iter()
            .find(|x| x.id == it.node)
            .and_then(|x| x.escalations.as_ref())
            .and_then(|e| e.get(n))
            .cloned();
        let (Some(esc), true) = (esc, it.open) else { return Ok(None) };
        let link = format!("/work/{}", it.id);
        match esc.action {
            EscalationAction::Remind => {
                let to = candidates(tx, it.id).await?;
                for m in &to {
                    let dedupe = format!("work_reminder:{}:{m}", t.key);
                    let r = ("work_item", it.id);
                    self.tell(tx, *m, "work_reminder", &it.vars, &link, r, &dedupe, Urgency::Urgent).await?;
                }
                let to = names(tx, &to).await?;
                history(tx, it.id, json!({ "action": "reminded", "at": at(t.at), "to": to })).await?;
            }
            EscalationAction::Notify => {
                let to = self.targets(tx, &rt, &it, n, &esc, t.at, false).await?;
                let mut vars = it.vars.clone();
                vars["reason"] = json!(esc.reason);
                for m in &to {
                    let dedupe = format!("work_escalated:{}:{m}", t.key);
                    let link = format!("/requests/{}", it.instance);
                    let r = ("request", it.instance);
                    self.tell(tx, *m, "work_escalated", &vars, &link, r, &dedupe, Urgency::Urgent).await?;
                }
                let to = names(tx, &to).await?;
                history(tx, it.id, json!({ "action": "notified", "at": at(t.at), "to": to })).await?;
            }
            EscalationAction::Reassign => {
                let to = self.targets(tx, &rt, &it, n, &esc, t.at, true).await?;
                if to.is_empty() {
                    let entry = json!({ "action": "escalation_skipped", "at": at(t.at), "reason": "nobody to reassign to" });
                    history(tx, it.id, entry).await?;
                } else {
                    let reason = esc.reason.as_deref().unwrap_or("escalated after due");
                    let from = self.reassign(tx, &it, &to, None, reason, t.at, t.key).await?;
                    let mut ev = AuditEvent::new(
                        None,
                        ActorKind::System,
                        Category::Runtime,
                        "work_item.reassigned",
                        Outcome::Allowed,
                    )
                    .object("work_item", it.id);
                    ev.diff = Some(
                        json!({ "request": it.instance, "step": it.node, "from": from, "to": to, "reason": reason }),
                    );
                    append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await.map_err(db)?;
                }
            }
            EscalationAction::Outcome => {
                let Some(outcome) = esc.outcome.clone() else { return Ok(None) };
                let reason = esc.reason.clone().unwrap_or_else(|| "no decision before the deadline".into());
                sqlx::query(
                    "UPDATE work_items SET state = 'completed', outcome = $2, decision = $3, completed_at = $4, revision = revision + 1 WHERE id = $1",
                )
                .bind(it.id)
                .bind(&outcome)
                .bind(json!({ "comment": reason, "auto": true }))
                .bind(t.at)
                .execute(tx.conn())
                .await
                .map_err(db)?;
                let entry =
                    json!({ "action": "auto_outcome", "at": at(t.at), "outcome": outcome, "reason": reason });
                history(tx, it.id, entry).await?;
                let mut ev = AuditEvent::new(
                    None,
                    ActorKind::System,
                    Category::Runtime,
                    "work_item.decided",
                    Outcome::Allowed,
                )
                .object("work_item", it.id);
                ev.diff = Some(
                    json!({ "outcome": outcome, "request": it.instance, "step": it.node, "auto": true }),
                );
                append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await.map_err(db)?;
                let event =
                    Event::WorkItemCompleted { key: it.key.clone(), outcome, variables: BTreeMap::new() };
                return Ok(Some(Stimulus { at: t.at, event }));
            }
        }
        Ok(None)
    }

    /// The members an escalation's `to` names now, minus SoD exclusions when `sod`.
    #[allow(clippy::too_many_arguments)]
    async fn targets(
        &self,
        tx: &mut WsTx<'_>,
        rt: &RequestType,
        it: &Item,
        n: usize,
        esc: &Escalation,
        now: OffsetDateTime,
        sod: bool,
    ) -> R<Vec<Uuid>> {
        let Some(to) = &esc.to else { return Ok(vec![]) };
        let state: Value = sqlx::query_scalar("SELECT state FROM process_instances WHERE id = $1")
            .bind(it.instance)
            .fetch_one(tx.conn())
            .await
            .map_err(db)?;
        let st: InstanceState = serde_json::from_value(state).map_err(db)?;
        let excluded = if sod {
            crate::resolve::excluded(
                tx,
                it.instance,
                Some(it.initiator),
                it.exclude_initiator,
                &it.exclude_actors_of,
            )
            .await?
        } else {
            BTreeSet::new()
        };
        let field = format!("escalations/{n}/to/ref");
        crate::resolve::resolve_candidate(tx, &rt.exe, &st, now, &it.node, &field, to, &excluded).await
    }

    /// Replaces an open item's candidates with `to` and tells everyone concerned: the new
    /// candidates (`work_assigned`) and the ones taken off (`work_reassigned`). `by` is the admin
    /// (membership) or `None` for an escalation. Returns the previous candidates. The caller audits.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn reassign(
        &self,
        tx: &mut WsTx<'_>,
        it: &Item,
        to: &[Uuid],
        by: Option<Uuid>,
        reason: &str,
        now: OffsetDateTime,
        tag: &str,
    ) -> R<Vec<Uuid>> {
        let from = candidates(tx, it.id).await?;
        sqlx::query("DELETE FROM work_item_candidates WHERE work_item_id = $1")
            .bind(it.id)
            .execute(tx.conn())
            .await
            .map_err(db)?;
        for m in to {
            sqlx::query("INSERT INTO work_item_candidates (workspace_id, work_item_id, membership_id) VALUES ($1, $2, $3)")
                .bind(tx.ctx.workspace_id)
                .bind(it.id)
                .bind(m)
                .execute(tx.conn())
                .await
                .map_err(db)?;
        }
        sqlx::query("UPDATE work_items SET unassigned = false, revision = revision + 1 WHERE id = $1")
            .bind(it.id)
            .execute(tx.conn())
            .await
            .map_err(db)?;
        let by_name = match by {
            Some(b) => Some(name(tx, b).await?),
            None => None,
        };
        let (from_names, to_names) = (names(tx, &from).await?, names(tx, to).await?);
        let entry = json!({ "action": "reassigned", "at": at(now), "by": by_name, "reason": reason, "from": from_names, "to": to_names });
        history(tx, it.id, entry).await?;
        let link = format!("/work/{}", it.id);
        for m in to.iter().filter(|m| !from.contains(m)) {
            let dedupe = format!("work_assigned:{}:{m}:{tag}", it.id);
            let r = ("work_item", it.id);
            self.tell(tx, *m, "work_assigned", &it.vars, &link, r, &dedupe, Urgency::Urgent).await?;
        }
        let mut vars = it.vars.clone();
        vars["to"] = json!(to_names.join(", "));
        vars["reason"] = json!(reason);
        for m in from.iter().filter(|m| !to.contains(m)) {
            let dedupe = format!("work_reassigned:{}:{m}:{tag}", it.id);
            let link = format!("/requests/{}", it.instance);
            let r = ("request", it.instance);
            self.tell(tx, *m, "work_reassigned", &vars, &link, r, &dedupe, Urgency::Normal).await?;
        }
        Ok(from)
    }
}

fn at(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}
