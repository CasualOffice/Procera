//! Workload (FR-A03, docs/05 §3.18): per person, the open steps offered to them, how many are
//! overdue or due within a week, the oldest one waiting, and whether they are away. A team is the
//! viewer's reporting line (direct and indirect reports, active members); "all" is everyone active.
//! Someone away with open steps is a coverage gap: their delegate can decide them, but nobody else
//! will see them in their own list.

use procera_db::WsTx;
use serde::Serialize;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// Whose workload a view covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The viewer's direct and indirect reports.
    Team(Uuid),
    /// Every active member.
    All,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkloadAway {
    pub until: String,
    pub delegate: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkloadRow {
    pub id: Uuid,
    pub name: String,
    pub title: Option<String>,
    pub open: i64,
    pub overdue: i64,
    pub due_soon: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub oldest: Option<OffsetDateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub away: Option<WorkloadAway>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Workload {
    pub people: Vec<WorkloadRow>,
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Totals {
    pub people: usize,
    pub open: i64,
    pub overdue: i64,
    pub due_soon: i64,
    /// Away members who still hold open steps.
    pub coverage_gaps: usize,
}

type Row =
    (Uuid, String, Option<String>, i64, i64, i64, Option<OffsetDateTime>, Option<String>, Option<String>);

const TEAM: &str = "WITH RECURSIVE people AS (
        SELECT id FROM memberships WHERE manager_membership_id = $1 AND status = 'active'
        UNION SELECT m.id FROM memberships m JOIN people p ON m.manager_membership_id = p.id WHERE m.status = 'active')";
const ALL: &str = "WITH people AS (SELECT id FROM memberships WHERE status = 'active' AND $1 = $1)";

impl Requests {
    /// The workload of a team or the whole workspace, busiest first.
    pub async fn workload(&self, tx: &mut WsTx<'_>, reach: Reach) -> R<Workload> {
        let (with, root) = match reach {
            Reach::Team(m) => (TEAM, m),
            Reach::All => (ALL, Uuid::nil()),
        };
        let now = self.clock.now();
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "{with}
             SELECT m.id, u.display_name, m.title,
                    count(w.id), count(w.id) FILTER (WHERE w.due_at < $2),
                    count(w.id) FILTER (WHERE w.due_at >= $2 AND w.due_at < $3), min(w.created_at),
                    o.ends_on::text, du.display_name
               FROM people p JOIN memberships m ON m.id = p.id JOIN users u ON u.id = m.user_id
               LEFT JOIN work_item_candidates c ON c.workspace_id = m.workspace_id AND c.membership_id = m.id
               LEFT JOIN work_items w ON w.workspace_id = c.workspace_id AND w.id = c.work_item_id
                    AND w.state IN ('offered', 'claimed') AND NOT w.unassigned
               LEFT JOIN out_of_office o ON o.workspace_id = m.workspace_id AND o.membership_id = m.id
                    AND $4::date BETWEEN o.starts_on AND o.ends_on
               LEFT JOIN memberships dm ON dm.workspace_id = o.workspace_id AND dm.id = o.delegate_membership_id
               LEFT JOIN users du ON du.id = dm.user_id
              GROUP BY m.id, u.display_name, m.title, o.ends_on, du.display_name
              ORDER BY count(w.id) FILTER (WHERE w.due_at < $2) DESC, count(w.id) DESC, u.display_name"
        ))
        .bind(root)
        .bind(now)
        .bind(now + Duration::days(7))
        .bind(now.date())
        .fetch_all(tx.conn())
        .await?;
        let people: Vec<WorkloadRow> = rows
            .into_iter()
            .map(|r| WorkloadRow {
                id: r.0,
                name: r.1,
                title: r.2,
                open: r.3,
                overdue: r.4,
                due_soon: r.5,
                oldest: r.6,
                away: r.7.zip(r.8).map(|(until, delegate)| WorkloadAway { until, delegate }),
            })
            .collect();
        let totals = Totals {
            people: people.len(),
            open: people.iter().map(|p| p.open).sum(),
            overdue: people.iter().map(|p| p.overdue).sum(),
            due_soon: people.iter().map(|p| p.due_soon).sum(),
            coverage_gaps: people.iter().filter(|p| p.away.is_some() && p.open > 0).count(),
        };
        Ok(Workload { people, totals })
    }

    /// Whether `m` manages anyone active (has a team).
    pub async fn manages(&self, tx: &mut WsTx<'_>, m: Uuid) -> R<bool> {
        let (yes,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM memberships WHERE manager_membership_id = $1 AND status = 'active')",
        )
        .bind(m)
        .fetch_one(tx.conn())
        .await?;
        Ok(yes)
    }

    /// Whether a manager may move this open step within their team without `work:reassign`
    /// (docs/12 §6 "Team Manager (own department)"): everyone holding it is in their reporting
    /// line, and so is the new person (or it is the manager themselves).
    pub async fn may_reassign_within_team(
        &self,
        tx: &mut WsTx<'_>,
        manager: Uuid,
        item: Uuid,
        to: Uuid,
    ) -> R<bool> {
        let holders: Vec<(Uuid,)> =
            sqlx::query_as("SELECT membership_id FROM work_item_candidates WHERE work_item_id = $1")
                .bind(item)
                .fetch_all(tx.conn())
                .await?;
        if holders.is_empty() {
            return Ok(false);
        }
        for (h,) in holders {
            if !self.reports_to(tx, h, manager).await? {
                return Ok(false);
            }
        }
        Ok(to == manager || self.reports_to(tx, to, manager).await?)
    }

    /// Whether `member` is in `manager`'s reporting line.
    pub async fn reports_to(&self, tx: &mut WsTx<'_>, member: Uuid, manager: Uuid) -> R<bool> {
        let (yes,): (bool,) =
            sqlx::query_as(&format!("{TEAM} SELECT EXISTS (SELECT 1 FROM people WHERE id = $2)"))
                .bind(manager)
                .bind(member)
                .fetch_one(tx.conn())
                .await?;
        Ok(yes)
    }
}
