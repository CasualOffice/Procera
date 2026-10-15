//! Team calendar and org chart (BPMS-225, docs/25 §6, §8). Who's out, by team, department or
//! location, with holidays; what a viewer sees of someone else's leave depends on who they are:
//! themselves, their manager chain and HR see the type and pending requests; colleagues see only
//! approved time off, as "Out".

use std::collections::BTreeSet;

use procera_db::WsTx;
use serde::Serialize;
use time::Date;
use uuid::Uuid;

use crate::leave::days as to_days;
use crate::{Hr, HrError, R, dates};

/// The longest range one calendar request covers.
pub const MAX_DAYS: i64 = 92;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Me, my manager, people who share my manager, and my direct reports.
    Team,
    /// A department and its sub-departments (mine by default).
    Department(Option<Uuid>),
    /// A work location (mine by default).
    Location(Option<Uuid>),
    /// Everyone (HR).
    Everyone,
}

#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    pub title: Option<String>,
    pub department: Option<String>,
    pub location_id: Option<Uuid>,
    pub me: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Absence {
    pub membership_id: Uuid,
    #[serde(with = "dates::iso")]
    pub start_date: Date,
    pub start_half: String,
    #[serde(with = "dates::iso")]
    pub end_date: Date,
    pub end_half: String,
    pub state: String,
    /// The leave type's name, or "Out" when the viewer may not see it.
    pub label: String,
    pub colour: String,
    /// Set when the viewer may open the request.
    pub request_id: Option<Uuid>,
    pub days: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalHoliday {
    pub location_id: Uuid,
    #[serde(with = "dates::iso")]
    pub date: Date,
    pub name: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamCalendar {
    #[serde(with = "dates::iso")]
    pub from: Date,
    #[serde(with = "dates::iso")]
    pub to: Date,
    pub people: Vec<Person>,
    pub absences: Vec<Absence>,
    pub holidays: Vec<CalHoliday>,
    /// Work weeks by location (ISO weekdays), so weekends shade per person.
    pub work_weeks: Vec<(Uuid, Vec<i16>)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgNode {
    pub id: Uuid,
    pub name: String,
    pub title: Option<String>,
    pub department: Option<String>,
    pub location: Option<String>,
    pub manager_id: Option<Uuid>,
    /// Out today (approved leave), shown to everyone as a plain "Out".
    pub out_today: bool,
}

impl Hr {
    /// Everyone below `m` in the reporting line.
    async fn reports_of(&self, tx: &mut WsTx<'_>, m: Uuid) -> R<BTreeSet<Uuid>> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "WITH RECURSIVE r AS (
                SELECT id FROM memberships WHERE manager_membership_id = $1 AND status = 'active'
              UNION
                SELECT x.id FROM memberships x JOIN r ON x.manager_membership_id = r.id WHERE x.status = 'active'
             ) SELECT id FROM r",
        )
        .bind(m)
        .fetch_all(tx.conn())
        .await?;
        Ok(ids.into_iter().collect())
    }

    async fn scope_members(&self, tx: &mut WsTx<'_>, viewer: Uuid, scope: Scope) -> R<Vec<Uuid>> {
        let ids: Vec<Uuid> = match scope {
            Scope::Team => {
                sqlx::query_scalar(
                    "SELECT m.id FROM memberships m, (SELECT id, manager_membership_id AS boss FROM memberships WHERE id = $1) v
                      WHERE m.status = 'active'
                        AND (m.id = v.id OR m.manager_membership_id = v.id OR m.id = v.boss
                             OR (v.boss IS NOT NULL AND m.manager_membership_id = v.boss))",
                )
                .bind(viewer)
                .fetch_all(tx.conn())
                .await?
            }
            Scope::Department(d) => {
                let dept: Option<Uuid> = match d {
                    Some(d) => Some(d),
                    None => sqlx::query_scalar::<_, Option<Uuid>>("SELECT department_id FROM memberships WHERE id = $1")
                        .bind(viewer)
                        .fetch_one(tx.conn())
                        .await?,
                };
                let Some(dept) = dept else { return Ok(vec![viewer]) };
                let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM departments WHERE id = $1)")
                    .bind(dept)
                    .fetch_one(tx.conn())
                    .await?;
                if !exists {
                    return Err(HrError::NotFound);
                }
                sqlx::query_scalar(
                    "SELECT m.id FROM memberships m JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
                      WHERE m.status = 'active' AND (d.id = $1 OR $1 = ANY(d.path))",
                )
                .bind(dept)
                .fetch_all(tx.conn())
                .await?
            }
            Scope::Location(l) => {
                let loc = match l {
                    Some(l) => Some(l),
                    None => self.member_hr(tx, viewer).await?.work_location_id,
                };
                let Some(loc) = loc else { return Ok(vec![viewer]) };
                self.location(tx, loc).await?;
                sqlx::query_scalar(
                    "SELECT m.id FROM memberships m JOIN membership_hr h ON h.workspace_id = m.workspace_id AND h.membership_id = m.id
                      WHERE m.status = 'active' AND h.work_location_id = $1",
                )
                .bind(loc)
                .fetch_all(tx.conn())
                .await?
            }
            Scope::Everyone => sqlx::query_scalar("SELECT id FROM memberships WHERE status = 'active'").fetch_all(tx.conn()).await?,
        };
        Ok(ids)
    }

    /// The calendar for `viewer` between two dates (`view_all`: the viewer is HR).
    pub async fn team_calendar(
        &self,
        tx: &mut WsTx<'_>,
        viewer: Uuid,
        scope: Scope,
        from: Date,
        to: Date,
        view_all: bool,
    ) -> R<TeamCalendar> {
        if to < from || (to - from).whole_days() >= MAX_DAYS {
            return Err(HrError::Invalid(format!("Ask for at most {MAX_DAYS} days, from before to.")));
        }
        let ids = self.scope_members(tx, viewer, scope).await?;
        let reports = self.reports_of(tx, viewer).await?;
        type P = (Uuid, String, Option<String>, Option<String>, Option<Uuid>);
        let rows: Vec<P> = sqlx::query_as(
            "SELECT m.id, coalesce(u.display_name, u.email::text), m.title, d.name, h.work_location_id
               FROM memberships m JOIN users u ON u.id = m.user_id
               LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
               LEFT JOIN membership_hr h ON h.workspace_id = m.workspace_id AND h.membership_id = m.id
              WHERE m.id = ANY($1) ORDER BY (m.id = $2) DESC, lower(coalesce(u.display_name, u.email::text)) LIMIT 300",
        )
        .bind(&ids)
        .bind(viewer)
        .fetch_all(tx.conn())
        .await?;
        let people: Vec<Person> = rows
            .into_iter()
            .map(|r| Person {
                me: r.0 == viewer,
                id: r.0,
                name: r.1,
                title: r.2,
                department: r.3,
                location_id: r.4,
            })
            .collect();
        let shown: Vec<Uuid> = people.iter().map(|p| p.id).collect();
        type A = (Uuid, Uuid, Date, String, Date, String, String, String, String, i32);
        let leave: Vec<A> = sqlx::query_as(
            "SELECT l.membership_id, l.request_id, l.start_date, l.start_half, l.end_date, l.end_half, l.state, t.name, t.colour, l.halves
               FROM leave_requests l JOIN leave_types t ON t.workspace_id = l.workspace_id AND t.id = l.leave_type_id
              WHERE l.membership_id = ANY($1) AND l.state IN ('pending', 'approved') AND l.start_date <= $3 AND l.end_date >= $2
              ORDER BY l.start_date",
        )
        .bind(&shown)
        .bind(from)
        .bind(to)
        .fetch_all(tx.conn())
        .await?;
        let mut absences = Vec::new();
        for a in leave {
            let full = view_all || a.0 == viewer || reports.contains(&a.0);
            if !full && a.6 != "approved" {
                continue; // colleagues don't see pending requests
            }
            absences.push(Absence {
                membership_id: a.0,
                start_date: a.2,
                start_half: a.3,
                end_date: a.4,
                end_half: a.5,
                state: a.6,
                label: if full { a.7 } else { "Out".into() },
                colour: if full { a.8 } else { "gray".into() },
                request_id: full.then_some(a.1),
                days: full.then(|| to_days(a.9)),
            });
        }
        let locs: Vec<Uuid> =
            people.iter().filter_map(|p| p.location_id).collect::<BTreeSet<_>>().into_iter().collect();
        let holidays: Vec<(Uuid, Date, String, bool)> = sqlx::query_as(
            "SELECT l.id, h.date, h.name, h.optional FROM work_locations l
               JOIN holidays h ON h.workspace_id = l.workspace_id AND h.calendar_id = l.holiday_calendar_id
              WHERE l.id = ANY($1) AND h.date BETWEEN $2 AND $3 ORDER BY h.date",
        )
        .bind(&locs)
        .bind(from)
        .bind(to)
        .fetch_all(tx.conn())
        .await?;
        let weeks: Vec<(Uuid, Vec<i16>)> =
            sqlx::query_as("SELECT id, work_week FROM work_locations WHERE id = ANY($1)")
                .bind(&locs)
                .fetch_all(tx.conn())
                .await?;
        Ok(TeamCalendar {
            from,
            to,
            people,
            absences,
            holidays: holidays
                .into_iter()
                .map(|h| CalHoliday { location_id: h.0, date: h.1, name: h.2, optional: h.3 })
                .collect(),
            work_weeks: weeks,
        })
    }

    /// Everyone active, with their manager, for the org chart.
    pub async fn org_chart(&self, tx: &mut WsTx<'_>, today: Date) -> R<Vec<OrgNode>> {
        type N = (Uuid, String, Option<String>, Option<String>, Option<String>, Option<Uuid>, bool);
        let rows: Vec<N> = sqlx::query_as(
            "SELECT m.id, coalesce(u.display_name, u.email::text), m.title, d.name, m.location,
                    (SELECT b.id FROM memberships b WHERE b.id = m.manager_membership_id AND b.status = 'active'),
                    EXISTS (SELECT 1 FROM leave_requests l WHERE l.membership_id = m.id AND l.state = 'approved' AND $1 BETWEEN l.start_date AND l.end_date)
               FROM memberships m JOIN users u ON u.id = m.user_id
               LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
              WHERE m.status = 'active' ORDER BY lower(coalesce(u.display_name, u.email::text)) LIMIT 5000",
        )
        .bind(today)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| OrgNode {
                id: r.0,
                name: r.1,
                title: r.2,
                department: r.3,
                location: r.4,
                manager_id: r.5,
                out_today: r.6,
            })
            .collect())
    }
}
