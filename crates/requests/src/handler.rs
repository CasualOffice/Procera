//! The engine's effect port for requests (BPMS-085): runs inside every advance transaction.
//!
//! - **Start**: the `requests` row (display id from the per-workspace sequence, title, initiator).
//! - `create_work_item`: resolve the assignment (SoD, fallback), insert the work item and its
//!   candidates with a due date from the pinned business calendar, notify each candidate
//!   (`work_assigned`, in-app + email).
//! - `cancel_work_item`: the open item is cancelled.
//! - `notify`: `request_notice` to the resolved principals.
//! - After each stimulus: the request status and current step; the requester hears about a
//!   decision (`request_decided`) or the outcome (`request_completed`).
//!
//! Consumed effects are marked `done` in `instance_effects`. Programs that are not request types
//! pass through untouched.

use std::sync::Arc;

use procera_db::WsTx;
use procera_engine::{BoxFuture, BusinessDuration, EffectContext, EffectHandler, store};
use procera_kernel::IdGen;
use procera_notify::Notify;
use procera_notify::delivery::Urgency;
use procera_runtime::{Command, Event, InstanceStatus, TokenState, TraceEvent, WaitKind, WorkKind};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::catalogue::{Catalogue, RequestType};
use crate::resolve::{Site, resolve};

#[derive(Clone)]
pub struct Effects {
    pub(crate) catalogue: Arc<Catalogue>,
    pub(crate) notify: Notify,
    pub(crate) ids: Arc<dyn IdGen>,
    pub(crate) public_url: String,
}

impl std::fmt::Debug for Effects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("requests::Effects")
    }
}

impl EffectHandler for Effects {
    fn apply<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        cx: EffectContext<'a>,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(rt) = self.catalogue.get(&cx.program.process_key).cloned() else { return Ok(()) };
            self.run(tx, &rt, cx).await.map_err(|e| format!("requests: {e}"))
        })
    }
}

type R<T> = Result<T, String>;

fn db(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// `kind` of a work item as stored (docs/04 §2.6).
fn kind(k: WorkKind) -> &'static str {
    match k {
        WorkKind::Task => "task",
        WorkKind::Approval => "approval",
        WorkKind::Sign => "sign",
        WorkKind::Acknowledge => "acknowledge",
    }
}

/// The request status for an instance state (BPMS-103).
pub(crate) fn status(rt: &RequestType, st: &procera_runtime::InstanceState) -> &'static str {
    match st.status {
        InstanceStatus::Completed => st
            .trace
            .iter()
            .rev()
            .filter(|t| matches!(t.event, TraceEvent::NodeEntered))
            .find_map(|t| t.node.as_ref().and_then(|n| rt.end_status.get(n)))
            .copied()
            .unwrap_or("approved"),
        InstanceStatus::Terminated => "cancelled",
        _ if rt.revise_node.is_some_and(|r| st.tokens.values().any(|t| t.node == r)) => "changes_requested",
        _ => "in_progress",
    }
}

fn current_step(rt: &RequestType, st: &procera_runtime::InstanceState) -> Option<String> {
    let names: Vec<String> = st
        .tokens
        .values()
        .filter(|t| matches!(&t.state, TokenState::Waiting(w) if w.kind == WaitKind::WorkItem))
        .map(|t| rt.node_name(&t.node))
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

impl Effects {
    async fn run(&self, tx: &mut WsTx<'_>, rt: &RequestType, cx: EffectContext<'_>) -> R<()> {
        let st = cx.state;
        let iid = st.instance_id;
        let at = cx.stimulus.at;
        if cx.before.is_none() {
            self.insert_request(tx, rt, st, at).await?;
        }
        let (display_id, title, initiator): (String, String, Uuid) = sqlx::query_as(
            "SELECT display_id, title, initiator_membership_id FROM requests WHERE instance_id = $1",
        )
        .bind(iid)
        .fetch_one(tx.conn())
        .await
        .map_err(db)?;
        let link = format!("/requests/{iid}");
        let base = json!({ "display_id": display_id, "title": title, "type_name": rt.name });
        for cmd in cx.commands {
            match cmd {
                Command::CreateWorkItem {
                    key,
                    token,
                    node,
                    kind: k,
                    assignment,
                    form,
                    outcomes,
                    due,
                    ..
                } => {
                    let res = match assignment {
                        Some(a) => {
                            resolve(tx, &rt.exe, st, at, Site { node, field: "assignment" }, a, true).await?
                        }
                        None => crate::resolve::Resolution {
                            candidates: vec![],
                            snapshot: json!({ "sources": [] }),
                        },
                    };
                    let due_at = match due {
                        Some(d) => Some(self.due(tx, rt, at, &d.after.0, d.business == Some(true)).await?),
                        None => None,
                    };
                    let id = self.ids.new_id();
                    let (ex_init, ex_actors) = assignment
                        .as_ref()
                        .map(|a| {
                            (
                                a.exclude_initiator == Some(true),
                                a.exclude_actors_of.clone().unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default();
                    let inserted = sqlx::query(
                        "INSERT INTO work_items (workspace_id, id, work_key, instance_id, token_id, node_id, name, kind, state, outcomes, form,
                                                 resolution, unassigned, exclude_initiator, exclude_actors_of, due_at, created_at)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'offered',$9,$10,$11,$12,$13,$14,$15,$16) ON CONFLICT (workspace_id, work_key) DO NOTHING",
                    )
                    .bind(tx.ctx.workspace_id).bind(id).bind(key).bind(iid).bind(i64::try_from(token.0).unwrap_or(i64::MAX))
                    .bind(node).bind(rt.node_name(node)).bind(kind(*k)).bind(outcomes).bind(form)
                    .bind(&res.snapshot).bind(res.unassigned()).bind(ex_init).bind(&ex_actors).bind(due_at).bind(at)
                    .execute(tx.conn()).await.map_err(db)?.rows_affected();
                    if inserted == 1 {
                        for m in &res.candidates {
                            sqlx::query("INSERT INTO work_item_candidates (workspace_id, work_item_id, membership_id) VALUES ($1, $2, $3)")
                                .bind(tx.ctx.workspace_id).bind(id).bind(m).execute(tx.conn()).await.map_err(db)?;
                        }
                        let mut vars = base.clone();
                        vars["step"] = json!(rt.node_name(node));
                        vars["due"] = json!(due_at.map(human_date));
                        vars["initiator"] = json!(name(tx, initiator).await?);
                        for m in &res.candidates {
                            self.tell(
                                tx,
                                *m,
                                "work_assigned",
                                &vars,
                                &format!("/work/{id}"),
                                ("work_item", id),
                                &format!("work_assigned:{id}:{m}"),
                                Urgency::Urgent,
                            )
                            .await?;
                        }
                        if res.unassigned() {
                            tracing::warn!(instance = %iid, node, "no assignee resolved; work item left unassigned for admins");
                        }
                    }
                    done(tx, key, "create_work_item").await?;
                }
                Command::CancelWorkItem { key } => {
                    sqlx::query("UPDATE work_items SET state = 'cancelled', completed_at = $2, revision = revision + 1 WHERE work_key = $1 AND state IN ('offered', 'claimed')")
                        .bind(key).bind(at).execute(tx.conn()).await.map_err(db)?;
                    done(tx, key, "cancel_work_item").await?;
                }
                Command::Notify { key, node, to: Some(a), .. } => {
                    let res = resolve(tx, &rt.exe, st, at, Site { node, field: "to" }, a, false).await?;
                    let mut vars = base.clone();
                    vars["step"] = json!(rt.node_name(node));
                    for m in &res.candidates {
                        self.tell(
                            tx,
                            *m,
                            "request_notice",
                            &vars,
                            &link,
                            ("request", iid),
                            &format!("request_notice:{key}:{m}"),
                            Urgency::Normal,
                        )
                        .await?;
                    }
                    done(tx, key, "notify").await?;
                }
                _ => {}
            }
        }
        let status = status(rt, st);
        sqlx::query("UPDATE requests SET status = $2, current_step = $3, updated_at = $4, ended_at = $5 WHERE instance_id = $1")
            .bind(iid).bind(status).bind(current_step(rt, st)).bind(at).bind(st.ended_at)
            .execute(tx.conn()).await.map_err(db)?;
        // Tell the requester what happened, once per decision or outcome.
        let finished_now =
            st.status == InstanceStatus::Completed && cx.before.is_some_and(|b| !b.status.is_final());
        if finished_now {
            let mut vars = base.clone();
            vars["status"] = json!(status);
            self.tell(
                tx,
                initiator,
                "request_completed",
                &vars,
                &link,
                ("request", iid),
                &format!("request_completed:{iid}"),
                Urgency::Normal,
            )
            .await?;
        } else if let Event::WorkItemCompleted { key, outcome, .. } = &cx.stimulus.event
            && !st.status.is_final()
        {
            let row: Option<(String, String, Option<Uuid>, Option<Value>)> = sqlx::query_as(
                "SELECT name, kind, completed_by, decision FROM work_items WHERE work_key = $1",
            )
            .bind(key)
            .fetch_optional(tx.conn())
            .await
            .map_err(db)?;
            if let Some((step, k, Some(by), decision)) = row
                && k == "approval"
                && by != initiator
            {
                let mut vars = base.clone();
                vars["step"] = json!(step);
                vars["outcome"] = json!(outcome);
                vars["decided_by"] = json!(name(tx, by).await?);
                vars["comment"] = decision.and_then(|d| d.get("comment").cloned()).unwrap_or(Value::Null);
                self.tell(
                    tx,
                    initiator,
                    "request_decided",
                    &vars,
                    &link,
                    ("request", iid),
                    &format!("request_decided:{key}"),
                    Urgency::Normal,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn insert_request(
        &self,
        tx: &mut WsTx<'_>,
        rt: &RequestType,
        st: &procera_runtime::InstanceState,
        at: OffsetDateTime,
    ) -> R<()> {
        let initiator = st
            .initiator
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("the initiator has no member id")?;
        let year = at.year();
        let n: i64 = sqlx::query_scalar(
            "INSERT INTO request_sequences (workspace_id, prefix, year, last) VALUES ($1, $2, $3, 1)
             ON CONFLICT (workspace_id, prefix, year) DO UPDATE SET last = request_sequences.last + 1 RETURNING last",
        )
        .bind(tx.ctx.workspace_id).bind(rt.prefix).bind(year)
        .fetch_one(tx.conn()).await.map_err(db)?;
        let display_id = format!("{}-{year}-{n:06}", rt.prefix);
        sqlx::query(
            "INSERT INTO requests (workspace_id, instance_id, display_id, process_key, title, initiator_membership_id, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'in_progress', $7, $7)",
        )
        .bind(tx.ctx.workspace_id).bind(st.instance_id).bind(display_id).bind(rt.key).bind(rt.title(&st.variables)).bind(initiator).bind(at)
        .execute(tx.conn()).await.map_err(db)?;
        Ok(())
    }

    /// A step's due date: business durations on the pinned calendar, others as wall time.
    async fn due(
        &self,
        tx: &mut WsTx<'_>,
        rt: &RequestType,
        from: OffsetDateTime,
        after: &str,
        business: bool,
    ) -> R<OffsetDateTime> {
        let d = BusinessDuration::parse(after).map_err(db)?;
        if business {
            let cref = rt.exe.calendar_ref(None).ok_or("no default calendar")?;
            let cal = store::calendar_by_ref(tx, &cref).await.map_err(db)?;
            cal.calendar.add(from, d).map_err(db)
        } else {
            let secs = d.weeks * 7 * 86_400 + d.days * 86_400 + d.seconds;
            Ok(from + time::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX)))
        }
    }

    /// In-app notification and email to one member (idempotent on `dedupe`).
    #[allow(clippy::too_many_arguments)]
    async fn tell(
        &self,
        tx: &mut WsTx<'_>,
        to: Uuid,
        kind: &str,
        vars: &Value,
        link: &str,
        reference: (&str, Uuid),
        dedupe: &str,
        urgency: Urgency,
    ) -> R<()> {
        let mut vars = vars.clone();
        vars["link"] = json!(link);
        self.notify
            .notify_member(tx, to, kind, &vars, Some(link), Some(reference), dedupe)
            .await
            .map_err(db)?;
        let email: Option<String> = sqlx::query_scalar(
            "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
        )
        .bind(to)
        .fetch_optional(tx.conn())
        .await
        .map_err(db)?;
        if let Some(email) = email {
            vars["link"] = json!(format!("{}{link}", self.public_url));
            self.notify.email_member(tx, to, kind, &email, &vars, dedupe, urgency).await.map_err(db)?;
        }
        Ok(())
    }
}

async fn done(tx: &mut WsTx<'_>, key: &str, command: &str) -> R<()> {
    sqlx::query("UPDATE instance_effects SET state = 'done' WHERE effect_key = $1 AND command = $2 AND state = 'pending'")
        .bind(key).bind(command).execute(tx.conn()).await.map_err(db)?;
    Ok(())
}

/// A member's display name.
pub(crate) async fn name(tx: &mut WsTx<'_>, m: Uuid) -> R<String> {
    let n: Option<String> = sqlx::query_scalar(
        "SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
    )
    .bind(m)
    .fetch_optional(tx.conn())
    .await
    .map_err(db)?;
    Ok(n.unwrap_or_else(|| "A former member".to_owned()))
}

pub(crate) fn human_date(t: OffsetDateTime) -> String {
    let fmt = time::macros::format_description!("[day padding:none] [month repr:short] [year]");
    t.format(fmt).unwrap_or_default()
}
