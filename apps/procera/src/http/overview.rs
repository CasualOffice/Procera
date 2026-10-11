//! Admin overview and people & access monitor (BPMS-063; FR-A01, FR-A06, docs/05 §3.13).
//!
//! Read-only aggregates over the modules that exist today (library, acknowledgement campaigns,
//! announcements, members, sign-ins). Modules that have not shipped yet (requests, files) are
//! reported as `null` so the UI shows "no data yet" guidance instead of an empty tile.
//! Both endpoints need `monitoring:view_dashboards`; the people monitor also needs `member:view`.
//!
//! Definitions (docs/08 "Admin"):
//! - *Dormant*: an active member with no sign-in to this workspace in 90 days, or never signed in
//!   and joined more than 30 days ago.
//! - *Privileged*: holds `workspace.owner`, `workspace.admin` or `security.admin` (directly or via
//!   a group).
//! - *Expiring grant*: a time-boxed role grant ending within 14 days.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use procera_access::Resource;
use serde::Deserialize;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/overview", get(overview)).route("/admin/people", get(people))
}

const PRIVILEGED: &[&str] = &["workspace.owner", "workspace.admin", "security.admin"];
const DORMANT_AFTER: Duration = Duration::days(90);
const NEVER_ACTIVE_GRACE: Duration = Duration::days(30);
const EXPIRING_WITHIN: Duration = Duration::days(14);

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "overview api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn invalid(m: &str) -> ApiError {
    ApiError::new(422, "validation_failed", "Validation failed").detail(m)
}

#[derive(Debug, Deserialize)]
struct RangeQuery {
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
    /// Range length ending now on the server's clock (the browser's clock may differ).
    days: Option<i64>,
    department: Option<Uuid>,
}

/// Members in scope: active, and in the department subtree when filtered (`$1` = department).
const IN_SCOPE: &str = "m.status = 'active' AND ($1::uuid IS NULL OR EXISTS (SELECT 1 FROM departments d WHERE d.id = m.department_id AND $1 = ANY(d.path)))";

async fn overview(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<RangeQuery>,
) -> Result<Json<Value>, ApiError> {
    let now = s.clock.now();
    let to = q.to.unwrap_or(now);
    let from = q.from.unwrap_or(to - Duration::days(q.days.unwrap_or(7).clamp(1, 366)));
    if from >= to || to - from > Duration::days(366) {
        return Err(invalid("from must be before to, at most 366 days apart"));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "monitoring:view_dashboards", Resource::Workspace).await?;
    let dept = q.department;
    let c = tx.conn();

    // Needs attention ------------------------------------------------------------------------
    let overdue_acks: Vec<(Uuid, String, i64)> = sqlx::query_as(&format!(
        "SELECT c.id, c.title, count(*) FROM campaign_recipients r JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
         JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
         WHERE c.state = 'active' AND r.state = 'pending' AND r.due_at < $2 AND {IN_SCOPE}
         GROUP BY c.id, c.title ORDER BY count(*) DESC, c.title"
    ))
    .bind(dept)
    .bind(now)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let overdue_reviews: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT id, title, next_review_at FROM artifacts WHERE current_published_version_id IS NOT NULL AND next_review_at <= $1 ORDER BY next_review_at, lower(title)",
    )
    .bind(now)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let in_review: Vec<(Uuid, Uuid, String, i32)> = sqlx::query_as(
        "SELECT a.id, v.id, a.title, v.version_no FROM artifact_versions v JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
         WHERE v.lifecycle = 'in_review' ORDER BY v.submitted_at NULLS LAST, lower(a.title)",
    )
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let pending_announcements: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, title FROM announcements WHERE state = 'pending_approval' ORDER BY lower(title)",
    )
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let privileged_without_mfa = privileged_members(&mut tx, dept, true).await?;

    let mut attention: Vec<Value> = vec![];
    for (id, title, n) in &overdue_acks {
        attention.push(json!({"kind": "overdue_acknowledgements", "severity": "high", "count": n, "title": format!("{n} overdue acknowledgement{} for {title}", if *n == 1 { "" } else { "s" }), "link": format!("/admin/campaigns/{id}")}));
    }
    for (id, title, _) in &privileged_without_mfa {
        attention.push(json!({"kind": "privileged_without_mfa", "severity": "high", "count": 1, "title": format!("{title} holds an admin role without two-step verification"), "link": format!("/admin/people?filter=no_mfa#{id}")}));
    }
    for (id, title, due) in &overdue_reviews {
        attention.push(json!({"kind": "overdue_review", "severity": "medium", "count": 1, "title": format!("{title} is past its review date"), "since": due.format(&time::format_description::well_known::Rfc3339).ok(), "link": format!("/library/{id}")}));
    }
    for (artifact, _version, title, no) in &in_review {
        attention.push(json!({"kind": "pending_review", "severity": "medium", "count": 1, "title": format!("{title} v{no} is waiting for review"), "link": format!("/library/{artifact}")}));
    }
    for (id, title) in &pending_announcements {
        attention.push(json!({"kind": "pending_announcement", "severity": "low", "count": 1, "title": format!("{title} is waiting for approval"), "link": format!("/admin/announcements?id={id}")}));
    }

    // Metrics in range ------------------------------------------------------------------------
    let c = tx.conn();
    let (assigned, completed): (i64, i64) = sqlx::query_as(&format!(
        "SELECT count(*), count(*) FILTER (WHERE r.state = 'completed') FROM campaign_recipients r
         JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
         WHERE r.assigned_at >= $2 AND r.assigned_at < $3 AND {IN_SCOPE}"
    ))
    .bind(dept)
    .bind(from)
    .bind(to)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;
    let pending_acks: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM campaign_recipients r JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
         JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
         WHERE c.state = 'active' AND r.state = 'pending' AND {IN_SCOPE}"
    ))
    .bind(dept)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;
    // One point per day of the range (at most 31 buckets: longer ranges bucket by week).
    let days = ((to - from).whole_days()).max(1);
    let bucket = if days > 31 { "7 days" } else { "1 day" };
    let published: Vec<(OffsetDateTime, i64)> = sqlx::query_as(
        "SELECT g, (SELECT count(*) FROM artifact_versions v WHERE v.published_at >= g AND v.published_at < g + $3::interval)
         FROM generate_series($1::timestamptz, $2::timestamptz - interval '1 second', $3::interval) g ORDER BY g",
    )
    .bind(from)
    .bind(to)
    .bind(bucket)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    let announcements_published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM announcements WHERE published_at >= $1 AND published_at < $2",
    )
    .bind(from)
    .bind(to)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;

    // Security strip --------------------------------------------------------------------------
    let failed_logins: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(f.failures), 0)::bigint FROM discovery.member_login_failures($1, $2) f",
    )
    .bind(from)
    .bind(to)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;
    let without_mfa: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM memberships m WHERE {IN_SCOPE}
           AND NOT EXISTS (SELECT 1 FROM user_credentials uc WHERE uc.user_id = m.user_id AND uc.kind = 'totp' AND uc.confirmed)"
    ))
    .bind(dept)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;
    // From the audit chain (app clock, authoritative): privileged roles granted in the range.
    let new_admins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'role.granted' AND result = 'allowed'
           AND diff->>'role' = ANY($1) AND occurred_at >= $2 AND occurred_at <= $3",
    )
    .bind(PRIVILEGED.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>())
    .bind(from)
    .bind(to)
    .fetch_one(&mut *c)
    .await
    .map_err(internal)?;

    let departments: Vec<(Uuid, String, i64, i64)> = sqlx::query_as(
        "SELECT d.id, d.name,
                (SELECT count(*) FROM memberships m WHERE m.status = 'active' AND m.department_id = d.id),
                (SELECT count(*) FROM campaign_recipients r JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
                   JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
                  WHERE c.state = 'active' AND r.state = 'pending' AND r.due_at < $1 AND m.department_id = d.id)
         FROM departments d WHERE d.parent_id IS NULL ORDER BY lower(d.name)",
    )
    .bind(now)
    .fetch_all(&mut *c)
    .await
    .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;

    let overdue_total: i64 = overdue_acks.iter().map(|x| x.2).sum();
    let fmt =
        |t: OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    Ok(Json(json!({
        "range": {"from": fmt(from), "to": fmt(to), "department": dept},
        "summary": {
            "overdue_acknowledgements": overdue_total,
            "overdue_reviews": overdue_reviews.len(),
            "pending_reviews": in_review.len(),
            "pending_announcements": pending_announcements.len(),
        },
        "attention": attention,
        "metrics": {
            "acknowledgements": {"assigned": assigned, "completed": completed, "pending": pending_acks,
                                  "completion_rate": if assigned == 0 { Value::Null } else { json!((completed as f64) / (assigned as f64)) }},
            "published_versions": {"total": published.iter().map(|p| p.1).sum::<i64>(), "bucket": if days > 31 { "week" } else { "day" },
                                    "series": published.iter().map(|(t, n)| json!({"at": fmt(*t), "count": n})).collect::<Vec<_>>()},
            "announcements_published": announcements_published,
        },
        "security": {"failed_logins": failed_logins, "members_without_mfa": without_mfa, "new_admins": new_admins, "break_glass_sessions": 0},
        "departments": departments.iter().map(|(id, name, members, overdue)| json!({"id": id, "name": name, "members": members, "overdue_acknowledgements": overdue})).collect::<Vec<_>>(),
        // Modules that have not shipped yet: the UI shows guidance instead of a tile.
        "requests": Value::Null,
        "storage": Value::Null,
    })))
}

/// Privileged members (id, name, has MFA), optionally only those without MFA.
async fn privileged_members(
    tx: &mut procera_db::WsTx<'_>,
    dept: Option<Uuid>,
    without_mfa_only: bool,
) -> Result<Vec<(Uuid, String, bool)>, ApiError> {
    sqlx::query_as(&format!(
        "SELECT m.id, u.display_name, EXISTS (SELECT 1 FROM user_credentials uc WHERE uc.user_id = m.user_id AND uc.kind = 'totp' AND uc.confirmed) AS mfa
         FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE {IN_SCOPE} AND EXISTS (
           SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
           WHERE r.key = ANY($2) AND (g.expires_at IS NULL OR g.expires_at > now())
             AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id)))
           AND (NOT $3 OR NOT EXISTS (SELECT 1 FROM user_credentials uc WHERE uc.user_id = m.user_id AND uc.kind = 'totp' AND uc.confirmed))
         ORDER BY lower(u.display_name)"
    ))
    .bind(dept)
    .bind(PRIVILEGED.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>())
    .bind(without_mfa_only)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)
}

#[derive(Debug, Deserialize)]
struct PeopleQuery {
    /// all | no_mfa | dormant | privileged | expiring
    filter: Option<String>,
    department: Option<Uuid>,
}

type PersonRow =
    (Uuid, String, String, Option<String>, Vec<String>, bool, Option<OffsetDateTime>, OffsetDateTime, i64);

async fn people(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<PeopleQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = q.filter.as_deref().unwrap_or("all");
    if !["all", "no_mfa", "dormant", "privileged", "expiring"].contains(&filter) {
        return Err(invalid("filter must be one of all, no_mfa, dormant, privileged, expiring"));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "monitoring:view_dashboards", Resource::Workspace).await?;
    me.require(m, &mut tx, "member:view", Resource::Workspace).await?;
    let now = s.clock.now();
    let ws = tx.ctx.workspace_id;
    let privileged: Vec<String> = PRIVILEGED.iter().map(|k| (*k).to_owned()).collect();
    let rows: Vec<PersonRow> = sqlx::query_as(&format!(
        "SELECT m.id, u.display_name, u.email::text, d.name,
                coalesce(ARRAY(SELECT DISTINCT r.key FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
                               WHERE (g.expires_at IS NULL OR g.expires_at > $3)
                                 AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id))
                               ORDER BY r.key), '{{}}') AS roles,
                EXISTS (SELECT 1 FROM user_credentials uc WHERE uc.user_id = m.user_id AND uc.kind = 'totp' AND uc.confirmed) AS mfa,
                (SELECT max(t.created_at) FROM refresh_tokens t WHERE t.user_id = m.user_id AND t.active_workspace_id = $2) AS last_active,
                m.joined_at,
                coalesce((SELECT f.failures FROM discovery.member_login_failures($3 - interval '30 days', $3) f WHERE f.subject_ref = u.subject_ref), 0)::bigint
         FROM memberships m JOIN users u ON u.id = m.user_id LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
         WHERE {IN_SCOPE}
         ORDER BY lower(u.display_name)"
    ))
    .bind(q.department)
    .bind(ws)
    .bind(now)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?;
    let expiring: Vec<(Uuid, String, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT m.id, u.display_name, r.key, g.expires_at FROM role_grants g
         JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
         JOIN memberships m ON m.workspace_id = g.workspace_id AND m.id = g.membership_id
         JOIN users u ON u.id = m.user_id
         WHERE g.expires_at > $1 AND g.expires_at <= $2 AND m.status = 'active' ORDER BY g.expires_at",
    )
    .bind(now)
    .bind(now + EXPIRING_WITHIN)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?;
    let invites: Vec<(Uuid, String, OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
        "SELECT id, email::text, created_at, expires_at FROM invitations
         WHERE accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $1 ORDER BY created_at DESC",
    )
    .bind(now)
    .fetch_all(tx.conn())
    .await
    .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;

    let fmt =
        |t: OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let expiring_ids: Vec<Uuid> = expiring.iter().map(|e| e.0).collect();
    let people: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, email, department, roles, mfa, last_active, joined, failures)| {
            let is_privileged = roles.iter().any(|r| privileged.contains(r));
            let dormant = match last_active {
                Some(t) => now - t > DORMANT_AFTER,
                None => now - joined > NEVER_ACTIVE_GRACE,
            };
            json!({
                "membership_id": id, "name": name, "email": email, "department": department, "roles": roles,
                "mfa": mfa, "last_active_at": last_active.map(fmt), "joined_at": fmt(joined),
                "privileged": is_privileged, "dormant": dormant, "failed_logins_30d": failures,
                "grant_expiring": expiring_ids.contains(&id),
            })
        })
        .collect();
    let count = |f: &dyn Fn(&Value) -> bool| people.iter().filter(|p| f(p)).count();
    let counts = json!({
        "members": people.len(),
        "no_mfa": count(&|p| p["mfa"] == false),
        "dormant": count(&|p| p["dormant"] == true),
        "privileged": count(&|p| p["privileged"] == true),
        "expiring": expiring.len(),
        "pending_invites": invites.len(),
    });
    let items: Vec<&Value> = people
        .iter()
        .filter(|p| match filter {
            "no_mfa" => p["mfa"] == false,
            "dormant" => p["dormant"] == true,
            "privileged" => p["privileged"] == true,
            "expiring" => p["grant_expiring"] == true,
            _ => true,
        })
        .collect();
    Ok(Json(json!({
        "filter": filter,
        "counts": counts,
        "items": items,
        "expiring_grants": expiring.iter().map(|(id, name, role, at)| json!({"membership_id": id, "name": name, "role": role, "expires_at": fmt(*at)})).collect::<Vec<_>>(),
        "pending_invites": invites.iter().map(|(id, email, at, exp)| json!({"id": id, "email": email, "invited_at": fmt(*at), "expires_at": fmt(*exp)})).collect::<Vec<_>>(),
    })))
}
