//! Leave requests (BPMS-224, docs/25 §4, §7). A leave request is an `hr.leave-request` request; HR
//! checks it before it starts (type, dates, notice, overlap, balance), holds the days while it is
//! decided, and takes or releases them when it ends. Workspaces without leave types keep the plain
//! request (the four original types, no balances).

use std::collections::BTreeMap;

use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::days::{self, Half, Line, Why};
use crate::leave::{LeaveType, add_months, days as to_days, year_start};
use crate::{Hr, HrError, R, dates};

/// The original types, used while a workspace has no leave types of its own.
pub const LEGACY_TYPES: &[&str] = &["annual", "sick", "unpaid", "other"];

/// A problem with a request, on one of its fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub field: &'static str,
    pub code: &'static str,
    pub message: String,
}

fn problem(field: &'static str, code: &'static str, message: impl Into<String>) -> Problem {
    Problem { field, code, message: message.into() }
}

/// What a leave request asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub leave_type: String,
    pub start: Date,
    pub start_half: Half,
    pub end: Date,
    pub end_half: Half,
    pub has_note: bool,
}

fn half_of(v: Option<&Value>) -> Half {
    match v.and_then(Value::as_str) {
        Some("second_half") => Half::SecondHalf,
        Some("first_half") => Half::FirstHalf,
        _ => Half::Full,
    }
}

fn half_name(h: Half) -> &'static str {
    match h {
        Half::Full => "full",
        Half::FirstHalf => "first_half",
        Half::SecondHalf => "second_half",
    }
}

/// Reads a request's variables (`None` when the dates or type are missing: the form reports that).
pub fn ask_from(vars: &BTreeMap<String, Value>) -> Option<Ask> {
    let d = |k: &str| vars.get(k).and_then(Value::as_str).and_then(dates::parse);
    Some(Ask {
        leave_type: vars.get("leave_type")?.as_str()?.trim().to_string(),
        start: d("start_date")?,
        start_half: half_of(vars.get("start_half")),
        end: d("end_date")?,
        end_half: half_of(vars.get("end_half")),
        has_note: vars.get("supporting_document").is_some_and(|v| !v.is_null() && v != &json!([])),
    })
}

/// A leave request counted and checked.
#[derive(Debug, Clone, Serialize)]
pub struct LeaveQuote {
    pub leave_type: Option<Value>,
    pub days: f64,
    #[serde(skip)]
    pub halves: i32,
    pub lines: Vec<Line>,
    /// Available before this request (limited types; pending requests already held).
    pub available: Option<f64>,
    pub after: Option<f64>,
    pub problems: Vec<Problem>,
    #[serde(skip)]
    pub type_id: Option<Uuid>,
    #[serde(skip)]
    pub leave_year: Option<Date>,
}

/// A request as people see it in their time off.
#[derive(Debug, Clone, Serialize)]
pub struct LeaveRequestView {
    pub request_id: Uuid,
    pub display_id: String,
    pub leave_type: Value,
    #[serde(with = "dates::iso")]
    pub start_date: Date,
    pub start_half: String,
    #[serde(with = "dates::iso")]
    pub end_date: Date,
    pub end_half: String,
    pub days: f64,
    pub state: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

fn fmt_days(h: i32) -> String {
    let d = to_days(h);
    let n = if d.fract() == 0.0 { format!("{d:.0}") } else { format!("{d:.1}") };
    format!(
        "{n} {}",
        if (d - 1.0).abs() < f64::EPSILON || (d - 0.5).abs() < f64::EPSILON { "day" } else { "days" }
    )
}

impl Hr {
    /// Whether the workspace runs time off (has leave types); otherwise leave requests stay plain.
    pub async fn time_off_enabled(&self, tx: &mut WsTx<'_>) -> R<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM leave_types)").fetch_one(tx.conn()).await?)
    }

    async fn type_by_key(&self, tx: &mut WsTx<'_>, key: &str) -> R<Option<LeaveType>> {
        let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM leave_types WHERE key = $1")
            .bind(key)
            .fetch_optional(tx.conn())
            .await?;
        match id {
            Some(id) => Ok(Some(self.leave_type(tx, id).await?)),
            None => Ok(None),
        }
    }

    /// Counts and checks a leave request for `m` (`exclude`: the request itself, when re-checked).
    pub async fn quote_leave(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        ask: &Ask,
        today: Date,
        exclude: Option<Uuid>,
    ) -> R<LeaveQuote> {
        let mut q = LeaveQuote {
            leave_type: None,
            days: 0.0,
            halves: 0,
            lines: vec![],
            available: None,
            after: None,
            problems: vec![],
            type_id: None,
            leave_year: None,
        };
        let Some(t) = self.type_by_key(tx, &ask.leave_type).await?.filter(|t| !t.archived) else {
            q.problems.push(problem("leave_type", "unknown", "Choose a type of leave."));
            return Ok(q);
        };
        q.leave_type = Some(
            json!({ "id": t.id, "key": t.key, "name": t.name, "colour": t.colour, "limited": t.limited, "sensitive": t.sensitive }),
        );
        q.type_id = Some(t.id);
        let pol = self.policy_of(tx, m).await?;
        let rules = match &pol {
            Some(p) => self.policy(tx, p.id).await?.rules,
            None => vec![],
        };
        if !rules.iter().any(|r| r.leave_type_id == t.id) {
            q.problems.push(problem(
                "leave_type",
                "not_in_policy",
                format!("Your leave policy doesn't include {}.", t.name),
            ));
            return Ok(q);
        }
        if !t.half_days && (ask.start_half != Half::Full || ask.end_half != Half::Full) {
            q.problems.push(problem(
                "start_half",
                "whole_days",
                format!("{} is taken in whole days.", t.name),
            ));
        }
        // Count the days.
        let pad_from = ask.start.min(ask.end);
        let pad_to = ask.end.max(ask.start);
        if t.optional_holidays_only {
            let (_, hols) = self.holidays_for(tx, m, pad_from, pad_to).await?;
            let optional: BTreeMap<Date, String> =
                hols.into_iter().filter(|h| h.optional).map(|h| (h.date, h.name)).collect();
            if ask.end < ask.start || (ask.end - ask.start).whole_days() >= days::MAX_RANGE_DAYS {
                q.problems.push(problem(
                    "end_date",
                    "dates",
                    "Choose a last day on or after the first, within a year.",
                ));
                return Ok(q);
            }
            let mut d = ask.start;
            loop {
                let (why, h) = match optional.get(&d) {
                    Some(n) => (Why::Holiday { name: n.clone() }, 2u8),
                    None => (Why::Working, 0u8),
                };
                q.halves += i32::from(h);
                q.lines.push(Line { date: d, why, halves: h });
                match d.next_day() {
                    Some(n) if d < ask.end => d = n,
                    _ => break,
                }
            }
            if q.halves == 0 {
                q.problems.push(problem(
                    "start_date",
                    "not_optional_holiday",
                    format!("{} can only be taken on an optional holiday.", t.name),
                ));
            }
        } else {
            let cal = self.day_calendar(tx, m, pad_from, pad_to).await?;
            match days::quote(
                &cal,
                ask.start,
                ask.start_half,
                ask.end,
                ask.end_half,
                t.counts_non_working_days,
            ) {
                Ok(c) => {
                    q.halves = c.halves as i32;
                    q.lines = c.lines;
                    if q.halves == 0 {
                        q.problems.push(problem(
                            "end_date",
                            "no_working_days",
                            "Those dates are all weekends or holidays, so there's nothing to take.",
                        ));
                    }
                }
                Err(e) => {
                    let field = if matches!(e, days::RangeError::WrongHalf | days::RangeError::BothHalves) {
                        "start_half"
                    } else {
                        "end_date"
                    };
                    q.problems.push(problem(field, "dates", e.to_string()));
                    return Ok(q);
                }
            }
        }
        q.days = to_days(q.halves);
        // Notice, length and a note.
        let notice = (ask.start - today).whole_days();
        if t.min_notice_days > 0 && notice < i64::from(t.min_notice_days) {
            q.problems.push(problem(
                "start_date",
                "notice",
                format!("{} needs {} days' notice.", t.name, t.min_notice_days),
            ));
        }
        if let Some(max) = t.max_consecutive_days
            && q.halves > i32::from(max) * 2
        {
            q.problems.push(problem(
                "end_date",
                "too_long",
                format!("{} can be at most {max} days in a row.", t.name),
            ));
        }
        if let Some(after) = t.needs_attachment_after_days
            && q.halves > i32::from(after) * 2
            && !ask.has_note
        {
            q.problems.push(problem(
                "supporting_document",
                "note_required",
                format!("Attach a note for more than {after} days of {}.", t.name),
            ));
        }
        // Overlap with other time off.
        let clash: Option<(Date, Date)> = sqlx::query_as(
            "SELECT start_date, end_date FROM leave_requests
              WHERE membership_id = $1 AND state IN ('pending', 'approved') AND start_date <= $3 AND end_date >= $2
                AND ($4::uuid IS NULL OR request_id <> $4) ORDER BY start_date LIMIT 1",
        )
        .bind(m)
        .bind(ask.start)
        .bind(ask.end)
        .bind(exclude)
        .fetch_optional(tx.conn())
        .await?;
        if let Some((a, b)) = clash {
            let when = if a == b { a.to_string() } else { format!("{a} to {b}") };
            q.problems.push(problem(
                "start_date",
                "overlap",
                format!("You already have time off on {when}."),
            ));
        }
        // The balance of the leave year the request starts in.
        let ysm = pol.as_ref().map_or(1, |p| p.year_start_month);
        let year = year_start(ask.start, ysm);
        q.leave_year = Some(year);
        if ask.start >= add_months(year, 24) || ask.start < add_months(year_start(today, ysm), -12) {
            q.problems.push(problem(
                "start_date",
                "out_of_range",
                "Choose dates in the previous, this or next leave year.",
            ));
        }
        if t.limited {
            let (_, bals) = self.balances(tx, m, today, Some(ask.start)).await?;
            let mut avail =
                bals.iter().find(|b| b.leave_type["id"] == json!(t.id)).map_or(0.0, |b| b.available);
            if let Some(ex) = exclude {
                // When re-checking, this request's own hold is not "used".
                let held: Option<i64> = sqlx::query_scalar("SELECT -sum(halves) FROM leave_ledger WHERE request_id = $1 AND kind IN ('hold', 'release')")
                    .bind(ex)
                    .fetch_one(tx.conn())
                    .await?;
                avail += to_days(held.unwrap_or(0) as i32);
            }
            let after = avail - q.days;
            q.available = Some(avail);
            q.after = Some(after);
            if after < -t.negative_limit_days {
                q.problems.push(problem(
                    "leave_type",
                    "balance",
                    format!(
                        "Not enough {}: {} available, this needs {}.",
                        t.name,
                        fmt_days((avail * 2.0) as i32),
                        fmt_days(q.halves)
                    ),
                ));
            }
        }
        Ok(q)
    }

    #[allow(clippy::too_many_arguments)]
    async fn ledger(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        t: Uuid,
        year: Date,
        kind: &str,
        h: i32,
        request: Uuid,
    ) -> R<()> {
        if h == 0 {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO leave_ledger (workspace_id, id, membership_id, leave_type_id, leave_year, kind, halves, request_id, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(self.ids.new_id())
        .bind(m)
        .bind(t)
        .bind(year)
        .bind(kind)
        .bind(h)
        .bind(request)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        Ok(())
    }

    /// A leave request started (or was sent again after changes): records it and holds its days.
    pub async fn place_leave(
        &self,
        tx: &mut WsTx<'_>,
        request: Uuid,
        m: Uuid,
        ask: &Ask,
        today: Date,
    ) -> R<()> {
        let existing: Option<(Uuid, Date, i32)> = sqlx::query_as(
            "SELECT leave_type_id, leave_year, halves FROM leave_requests WHERE request_id = $1 AND state = 'pending'",
        )
        .bind(request)
        .fetch_optional(tx.conn())
        .await?;
        if let Some((t, y, h)) = existing {
            self.ledger(tx, m, t, y, "release", h, request).await?;
        }
        let q = self.quote_leave(tx, m, ask, today, Some(request)).await?;
        let (Some(t), Some(year)) = (q.type_id, q.leave_year) else {
            return Err(HrError::Invalid("The leave type isn't known.".into()));
        };
        let detail = serde_json::to_value(&q.lines).map_err(|e| HrError::Internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO leave_requests (workspace_id, request_id, membership_id, leave_type_id, leave_year, start_date, start_half, end_date, end_half, halves, detail, state, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'pending',$12)
             ON CONFLICT (workspace_id, request_id) DO UPDATE SET leave_type_id = $4, leave_year = $5, start_date = $6, start_half = $7,
                    end_date = $8, end_half = $9, halves = $10, detail = $11, state = 'pending', decided_at = NULL",
        )
        .bind(tx.ctx.workspace_id)
        .bind(request)
        .bind(m)
        .bind(t)
        .bind(year)
        .bind(ask.start)
        .bind(half_name(ask.start_half))
        .bind(ask.end)
        .bind(half_name(ask.end_half))
        .bind(q.halves)
        .bind(detail)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        self.ledger(tx, m, t, year, "hold", -q.halves, request).await
    }

    /// A leave request ended: approved takes the held days, anything else releases them.
    pub async fn settle_leave(&self, tx: &mut WsTx<'_>, request: Uuid, outcome: &str) -> R<()> {
        let row: Option<(Uuid, Uuid, Date, i32)> = sqlx::query_as(
            "SELECT membership_id, leave_type_id, leave_year, halves FROM leave_requests WHERE request_id = $1 AND state = 'pending'",
        )
        .bind(request)
        .fetch_optional(tx.conn())
        .await?;
        let Some((m, t, year, h)) = row else { return Ok(()) };
        self.ledger(tx, m, t, year, "release", h, request).await?;
        let state = match outcome {
            "approved" | "completed" => {
                self.ledger(tx, m, t, year, "taken", -h, request).await?;
                "approved"
            }
            "rejected" => "rejected",
            _ => "cancelled",
        };
        sqlx::query("UPDATE leave_requests SET state = $2, decided_at = $3 WHERE request_id = $1")
            .bind(request)
            .bind(state)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        Ok(())
    }

    /// A member's leave requests, newest first (the last two years and anything ahead).
    pub async fn leave_requests_of(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        today: Date,
    ) -> R<Vec<LeaveRequestView>> {
        type Row = (
            Uuid,
            String,
            Uuid,
            String,
            String,
            String,
            bool,
            Date,
            String,
            Date,
            String,
            i32,
            String,
            OffsetDateTime,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT l.request_id, r.display_id, t.id, t.key, t.name, t.colour, t.sensitive, l.start_date, l.start_half, l.end_date, l.end_half, l.halves, l.state, l.created_at
               FROM leave_requests l JOIN requests r ON r.workspace_id = l.workspace_id AND r.instance_id = l.request_id
               JOIN leave_types t ON t.workspace_id = l.workspace_id AND t.id = l.leave_type_id
              WHERE l.membership_id = $1 AND l.end_date >= $2 ORDER BY l.start_date DESC LIMIT 200",
        )
        .bind(m)
        .bind(add_months(today, -24))
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| LeaveRequestView {
                request_id: r.0,
                display_id: r.1,
                leave_type: json!({ "id": r.2, "key": r.3, "name": r.4, "colour": r.5, "sensitive": r.6 }),
                start_date: r.7,
                start_half: r.8,
                end_date: r.9,
                end_half: r.10,
                days: to_days(r.11),
                state: r.12,
                created_at: r.13,
            })
            .collect())
    }

    /// The choices for a leave request's type: the workspace's active types (`[(key, name)]`), or
    /// the original four while it has none.
    pub async fn leave_choices(&self, tx: &mut WsTx<'_>) -> R<Vec<(String, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, name FROM leave_types WHERE archived_at IS NULL ORDER BY created_at")
                .fetch_all(tx.conn())
                .await?;
        if rows.is_empty() {
            return Ok(LEGACY_TYPES
                .iter()
                .map(|k| ((*k).to_string(), format!("{}{}", k[..1].to_uppercase(), &k[1..])))
                .collect());
        }
        Ok(rows)
    }
}
