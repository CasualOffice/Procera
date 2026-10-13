//! Out of office and delegation (BPMS-086, docs/12 §9).
//!
//! A member who is away names a **delegate** for a date range (inclusive, UTC dates). While it is
//! active:
//! - new work items for them go to the delegate: the resolver substitutes at activation and records
//!   `{for, delegate, until}` in the work item's resolution snapshot ([`substitute`]);
//! - their existing open items stay with them, and the delegate may decide those too ([`acting_for`]);
//! - a decision by a delegate records `completed_by = delegate`, `completed_on_behalf_of = original`.
//!
//! Invariants: the chain is followed at most `access.delegation.max_depth` hops (default 1: no
//! chains); a cycle stops the chain; a delegate excluded by separation of duties (the initiator, or
//! an actor named by `excludeActorsOf`) is never substituted, and the original keeps the item; the
//! initiator's own steps (e.g. revising their request) are never handed to someone else; an
//! expired out-of-office no longer counts (fails closed).

use std::collections::{BTreeMap, BTreeSet};

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::forms::FieldError;
use crate::views::Person;
use crate::{Actor, RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// The longest out-of-office that can be set in one go.
const MAX_DAYS: i64 = 366;
const NOTE_MAX: usize = 500;

/// A member's out-of-office.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutOfOffice {
    pub member: Person,
    pub delegate: Person,
    #[serde(serialize_with = "iso")]
    pub starts_on: Date,
    #[serde(serialize_with = "iso")]
    pub ends_on: Date,
    pub note: Option<String>,
    /// Today is inside the range.
    pub active: bool,
    pub set_by: Person,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// What a member (or an admin for them) asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetOutOfOffice {
    pub starts_on: Date,
    pub ends_on: Date,
    pub delegate: Uuid,
    pub note: Option<String>,
}

/// Someone away, as shown next to their name: until when, and who decides for them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Away {
    pub id: Uuid,
    pub name: String,
    /// The last day away; `None` when they are back.
    #[serde(serialize_with = "iso_opt")]
    pub until: Option<Date>,
    pub delegate: Option<Person>,
}

/// Dates as `YYYY-MM-DD`.
pub(crate) fn iso<S: serde::Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.to_string())
}

fn iso_opt<S: serde::Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => s.serialize_str(&d.to_string()),
        None => s.serialize_none(),
    }
}

/// `access.delegation.max_depth`, clamped to 1..=3, read without a service handle (the resolver
/// runs inside the engine's effect port).
pub async fn max_depth(tx: &mut WsTx<'_>) -> Result<i64, String> {
    let v: Option<Value> =
        sqlx::query_scalar("SELECT value FROM workspace_settings WHERE key = 'access.delegation.max_depth'")
            .fetch_optional(tx.conn())
            .await
            .map_err(|e| e.to_string())?;
    Ok(v.and_then(|v| v.as_i64()).unwrap_or(1).clamp(1, 3))
}

/// The active delegate of `member` on `on`, if they are away and the delegate is an active member.
async fn delegate_of(tx: &mut WsTx<'_>, member: Uuid, on: Date) -> Result<Option<(Uuid, Date)>, String> {
    sqlx::query_as(
        "SELECT o.delegate_membership_id, o.ends_on FROM out_of_office o
           JOIN memberships d ON d.workspace_id = o.workspace_id AND d.id = o.delegate_membership_id
          WHERE o.membership_id = $1 AND o.starts_on <= $2 AND o.ends_on >= $2 AND d.status = 'active'",
    )
    .bind(member)
    .bind(on)
    .fetch_optional(tx.conn())
    .await
    .map_err(|e| e.to_string())
}

/// Out-of-office substitution at activation. Returns the candidates to offer and the substitution
/// notes for the snapshot (`{for, delegate, until}`, or `{for, skipped}` when SoD or a cycle stops it).
pub async fn substitute(
    tx: &mut WsTx<'_>,
    at: OffsetDateTime,
    candidates: Vec<Uuid>,
    excluded: &BTreeSet<Uuid>,
    initiator: Option<Uuid>,
) -> Result<(Vec<Uuid>, Vec<Value>), String> {
    let on = at.date();
    let depth = max_depth(tx).await?;
    let mut out = BTreeSet::new();
    let mut notes = Vec::new();
    for c in candidates {
        if Some(c) == initiator {
            out.insert(c);
            continue;
        }
        let (mut cur, mut until, mut seen) = (c, None, BTreeSet::from([c]));
        for _ in 0..depth {
            let Some((d, ends)) = delegate_of(tx, cur, on).await? else { break };
            let reason = if seen.contains(&d) {
                Some("cycle")
            } else if excluded.contains(&d) || Some(d) == initiator {
                Some("separation_of_duties")
            } else {
                None
            };
            if let Some(r) = reason {
                notes.push(json!({ "for": cur, "delegate": d, "skipped": r }));
                break;
            }
            seen.insert(d);
            until = Some(until.map_or(ends, |u: Date| u.min(ends)));
            cur = d;
        }
        if cur != c {
            notes.push(json!({ "for": c, "delegate": cur, "until": until.map(|u| u.to_string()) }));
        }
        out.insert(cur);
    }
    Ok((out.into_iter().collect(), notes))
}

/// Everyone `me` may currently act for (themselves included): members whose active out-of-office
/// chain reaches `me` within `access.delegation.max_depth` hops.
pub async fn acting_for(tx: &mut WsTx<'_>, me: Uuid, on: Date) -> R<Vec<Uuid>> {
    let depth = max_depth(tx).await.map_err(RequestError::Internal)?;
    let mut all = BTreeSet::from([me]);
    let mut frontier = vec![me];
    for _ in 0..depth {
        let next: Vec<Uuid> = sqlx::query_scalar(
            "SELECT o.membership_id FROM out_of_office o
              WHERE o.delegate_membership_id = ANY($1) AND o.starts_on <= $2 AND o.ends_on >= $2
                AND NOT (o.membership_id = ANY($3))",
        )
        .bind(&frontier)
        .bind(on)
        .bind(all.iter().copied().collect::<Vec<_>>())
        .fetch_all(tx.conn())
        .await?;
        if next.is_empty() {
            break;
        }
        all.extend(next.iter().copied());
        frontier = next;
    }
    Ok(all.into_iter().collect())
}

/// Who among `members` is away on `on`, with their delegate.
pub async fn away(tx: &mut WsTx<'_>, members: &[Uuid], on: Date) -> R<BTreeMap<Uuid, Away>> {
    let rows: Vec<(Uuid, String, Date, Uuid, String)> = sqlx::query_as(
        "SELECT o.membership_id, u.display_name, o.ends_on, o.delegate_membership_id, du.display_name
           FROM out_of_office o
           JOIN memberships m ON m.workspace_id = o.workspace_id AND m.id = o.membership_id JOIN users u ON u.id = m.user_id
           JOIN memberships d ON d.workspace_id = o.workspace_id AND d.id = o.delegate_membership_id JOIN users du ON du.id = d.user_id
          WHERE o.membership_id = ANY($1) AND o.starts_on <= $2 AND o.ends_on >= $2",
    )
    .bind(members)
    .bind(on)
    .fetch_all(tx.conn())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, until, d, dn)| {
            (id, Away { id, name, until: Some(until), delegate: Some(Person { id: d, name: dn }) })
        })
        .collect())
}

fn invalid(field: &str, code: &str, message: &str) -> RequestError {
    RequestError::Invalid(vec![FieldError::new(field, code, message)])
}

impl Requests {
    /// Today, as the out-of-office ranges count it (UTC).
    pub fn today(&self) -> Date {
        self.clock.now().date()
    }

    /// A member's out-of-office (current or upcoming), if set.
    pub async fn out_of_office(&self, tx: &mut WsTx<'_>, member: Uuid) -> R<Option<OutOfOffice>> {
        type Row = (Uuid, String, Uuid, String, Date, Date, Option<String>, Uuid, String, OffsetDateTime);
        let row: Option<Row> = sqlx::query_as(
            "SELECT o.membership_id, u.display_name, o.delegate_membership_id, du.display_name, o.starts_on, o.ends_on, o.note,
                    o.set_by, su.display_name, o.updated_at
               FROM out_of_office o
               JOIN memberships m ON m.workspace_id = o.workspace_id AND m.id = o.membership_id JOIN users u ON u.id = m.user_id
               JOIN memberships d ON d.workspace_id = o.workspace_id AND d.id = o.delegate_membership_id JOIN users du ON du.id = d.user_id
               JOIN memberships s ON s.workspace_id = o.workspace_id AND s.id = o.set_by JOIN users su ON su.id = s.user_id
              WHERE o.membership_id = $1",
        )
        .bind(member)
        .fetch_optional(tx.conn())
        .await?;
        let today = self.today();
        Ok(row.map(|r| OutOfOffice {
            member: Person { id: r.0, name: r.1 },
            delegate: Person { id: r.2, name: r.3 },
            starts_on: r.4,
            ends_on: r.5,
            note: r.6,
            active: r.4 <= today && today <= r.5,
            set_by: Person { id: r.7, name: r.8 },
            updated_at: r.9,
        }))
    }

    /// Sets (or replaces) `member`'s out-of-office; `actor` is the member or an admin/manager
    /// (authorised by the caller). Appends `member.ooo_set`.
    pub async fn set_out_of_office(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        member: Uuid,
        req: &SetOutOfOffice,
    ) -> R<OutOfOffice> {
        let mut errors = Vec::new();
        if req.ends_on < req.starts_on {
            errors.push(FieldError::new("ends_on", "before_start", "Must be on or after the first day"));
        } else if req.ends_on < self.today() {
            errors.push(FieldError::new("ends_on", "in_past", "Must be today or later"));
        } else if (req.ends_on - req.starts_on).whole_days() >= MAX_DAYS {
            errors.push(FieldError::new("ends_on", "too_long", "Set at most a year at a time"));
        }
        let note = req.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
        if note.is_some_and(|n| n.chars().count() > NOTE_MAX || n.chars().any(char::is_control)) {
            errors.push(FieldError::new("note", "too_long", "Use at most 500 characters on one line"));
        }
        if req.delegate == member {
            errors.push(FieldError::new("delegate", "self", "Choose someone else"));
        } else {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
            )
            .bind(req.delegate)
            .fetch_one(tx.conn())
            .await?;
            if !active {
                errors.push(FieldError::new(
                    "delegate",
                    "unknown_member",
                    "Choose an active member of this workspace",
                ));
            }
        }
        if !errors.is_empty() {
            return Err(RequestError::Invalid(errors));
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
        )
        .bind(member)
        .fetch_one(tx.conn())
        .await?;
        if !exists {
            return Err(RequestError::NotFound);
        }
        let now = self.clock.now();
        sqlx::query(
            "INSERT INTO out_of_office (workspace_id, membership_id, delegate_membership_id, starts_on, ends_on, note, set_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
             ON CONFLICT (workspace_id, membership_id) DO UPDATE SET delegate_membership_id = EXCLUDED.delegate_membership_id,
               starts_on = EXCLUDED.starts_on, ends_on = EXCLUDED.ends_on, note = EXCLUDED.note, set_by = EXCLUDED.set_by, updated_at = EXCLUDED.updated_at",
        )
        .bind(tx.ctx.workspace_id)
        .bind(member)
        .bind(req.delegate)
        .bind(req.starts_on)
        .bind(req.ends_on)
        .bind(note)
        .bind(actor.membership)
        .bind(now)
        .execute(tx.conn())
        .await?;
        let diff = json!({
            "member": member, "delegate": req.delegate, "starts_on": req.starts_on.to_string(),
            "ends_on": req.ends_on.to_string(), "by_someone_else": actor.membership != member,
        });
        self.ooo_audit(tx, actor, "member.ooo_set", member, diff).await?;
        self.out_of_office(tx, member).await?.ok_or(RequestError::NotFound)
    }

    /// Clears `member`'s out-of-office. Appends `member.ooo_cleared`; `NotFound` when none is set.
    pub async fn clear_out_of_office(&self, tx: &mut WsTx<'_>, actor: &Actor, member: Uuid) -> R<()> {
        let gone = sqlx::query("DELETE FROM out_of_office WHERE membership_id = $1")
            .bind(member)
            .execute(tx.conn())
            .await?
            .rows_affected();
        if gone == 0 {
            return Err(RequestError::NotFound);
        }
        let diff = json!({ "member": member, "by_someone_else": actor.membership != member });
        self.ooo_audit(tx, actor, "member.ooo_cleared", member, diff).await
    }

    async fn ooo_audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        action: &str,
        member: Uuid,
        diff: Value,
    ) -> R<()> {
        let mut ev = AuditEvent::new(
            Some(actor.subject_ref),
            ActorKind::User,
            Category::Access,
            action,
            Outcome::Allowed,
        )
        .object("membership", member);
        ev.diff = Some(diff);
        ev.ip = actor.ip.clone();
        ev.user_agent = actor.user_agent.clone();
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }
}

/// The ISO date of a request body field, or a field error.
pub fn parse_date(field: &str, v: &str) -> R<Date> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(v, fmt).map_err(|_| invalid(field, "invalid", "Must be a date (YYYY-MM-DD)"))
}

/// Whom `me` may decide a work item for: themselves (a candidate in their own right), or an away
/// member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    Own,
    For(Uuid),
}

/// The member a substitution gave this item to `me` for, unless `me` was resolved directly too.
fn substituted_for(snapshot: &Value, me: Uuid) -> Option<Uuid> {
    let me_s = me.to_string();
    let mine = |v: &Value| v.as_str() == Some(me_s.as_str());
    let direct = snapshot["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|s| s["members"].as_array().is_some_and(|m| m.iter().any(mine)));
    if direct {
        return None;
    }
    snapshot["substitutions"].as_array().into_iter().flatten().find_map(|s| {
        if mine(&s["delegate"]) && s.get("skipped").is_none() {
            s["for"].as_str().and_then(|f| Uuid::parse_str(f).ok())
        } else {
            None
        }
    })
}

/// `me`'s standing on a work item; `None` = neither a candidate nor acting for one. `principals`
/// is [`acting_for`]`(me)`. Never for the initiator: their own steps are not handed on, and never
/// for an acknowledgement: only the person themselves can confirm it.
pub(crate) async fn standing(
    tx: &mut WsTx<'_>,
    work_id: Uuid,
    me: Uuid,
    principals: &[Uuid],
    initiator: Uuid,
) -> R<Option<Standing>> {
    let (candidates, snapshot, kind): (Vec<Uuid>, Value, String) = sqlx::query_as(
        "SELECT coalesce((SELECT array_agg(c.membership_id ORDER BY c.membership_id) FROM work_item_candidates c WHERE c.work_item_id = w.id), '{}'),
                w.resolution, w.kind
           FROM work_items w WHERE w.id = $1",
    )
    .bind(work_id)
    .fetch_one(tx.conn())
    .await?;
    if kind == "acknowledge" {
        return Ok(candidates.contains(&me).then_some(Standing::Own));
    }
    if candidates.contains(&me) {
        return Ok(Some(substituted_for(&snapshot, me).map_or(Standing::Own, Standing::For)));
    }
    // With no chains (depth 1), a candidate who holds the item as someone's delegate cannot pass
    // it on again.
    let chains = max_depth(tx).await.map_err(RequestError::Internal)? > 1;
    Ok(candidates
        .iter()
        .find(|c| {
            **c != me
                && **c != initiator
                && principals.contains(c)
                && (chains || substituted_for(&snapshot, **c).is_none())
        })
        .map(|c| Standing::For(*c)))
}
