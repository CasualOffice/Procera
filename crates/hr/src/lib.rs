//! HR time and attendance (docs/25). Slice 1 (BPMS-222): work locations (IANA time zone, work
//! week, holiday calendar), holiday calendars with holidays by year (optional ones too), import from
//! CSV or iCal, copying a year, each member's HR record, and a person's holidays. Day counting for
//! leave lives in [`days`].
//!
//! Audit is the caller's: each mutation returns what changed so the HTTP layer records it in the
//! same transaction.

pub mod dates;
pub mod days;
pub mod import;

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_db::WsTx;
use procera_kernel::{Clock, IdGen};
use serde::{Deserialize, Serialize};
use time::{Date, Month};
use uuid::Uuid;

pub use days::{Calendar, Half, Quote, WorkWeek};

#[derive(Debug, thiserror::Error)]
pub enum HrError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for HrError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                Self::Conflict("That name is already used.".into())
            }
            _ => Self::Internal(e.to_string()),
        }
    }
}

type R<T> = Result<T, HrError>;

/// A work location.
#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    pub work_week: Vec<i16>,
    pub day_hours: f64,
    pub holiday_calendar_id: Option<Uuid>,
    pub holiday_calendar: Option<String>,
    pub members: i64,
    pub archived: bool,
    pub revision: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationInput {
    pub name: String,
    pub timezone: String,
    #[serde(default = "standard_week")]
    pub work_week: Vec<i16>,
    #[serde(default)]
    pub day_hours: Option<f64>,
    #[serde(default)]
    pub holiday_calendar_id: Option<Uuid>,
    #[serde(default)]
    pub archived: Option<bool>,
}

fn standard_week() -> Vec<i16> {
    vec![1, 2, 3, 4, 5]
}

/// A holiday calendar with what uses it.
#[derive(Debug, Clone, Serialize)]
pub struct HolidayCalendar {
    pub id: Uuid,
    pub name: String,
    pub revision: i32,
    pub locations: Vec<String>,
    pub years: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holiday {
    #[serde(with = "dates::iso")]
    pub date: Date,
    pub name: String,
    #[serde(default)]
    pub optional: bool,
}

/// A member's HR record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemberHr {
    #[serde(default)]
    pub employee_no: Option<String>,
    #[serde(default)]
    pub employment_type: Option<String>,
    #[serde(default)]
    pub work_location_id: Option<Uuid>,
    #[serde(default, with = "dates::iso_opt")]
    pub start_date: Option<Date>,
    #[serde(default, with = "dates::iso_opt")]
    pub probation_end: Option<Date>,
}

/// What an import did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub added: u32,
    pub updated: u32,
    pub skipped: Vec<import::Skipped>,
}

pub const EMPLOYMENT_TYPES: &[&str] = &["full_time", "part_time", "contract", "intern"];

#[derive(Clone)]
pub struct Hr {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Hr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hr")
    }
}

fn name_of(raw: &str, what: &str) -> R<String> {
    let n = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() || n.chars().count() > 120 {
        return Err(HrError::Invalid(format!("{what} needs a name of 1 to 120 characters.")));
    }
    Ok(n)
}

impl Hr {
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { clock, ids }
    }

    // ---------------------------------------------------------------- locations

    pub async fn locations(&self, tx: &mut WsTx<'_>, include_archived: bool) -> R<Vec<Location>> {
        let rows: Vec<(Uuid, String, String, Vec<i16>, f64, Option<Uuid>, Option<String>, i64, bool, i32)> = sqlx::query_as(
            "SELECT l.id, l.name, l.timezone, l.work_week, l.day_hours::float8, l.holiday_calendar_id, c.name,
                    (SELECT count(*) FROM membership_hr h JOIN memberships m ON m.workspace_id = h.workspace_id AND m.id = h.membership_id
                      WHERE h.work_location_id = l.id AND m.status = 'active'),
                    l.archived_at IS NOT NULL, l.revision
               FROM work_locations l LEFT JOIN holiday_calendars c ON c.workspace_id = l.workspace_id AND c.id = l.holiday_calendar_id
              WHERE $1 OR l.archived_at IS NULL
              ORDER BY lower(l.name)",
        )
        .bind(include_archived)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Location {
                id: r.0,
                name: r.1,
                timezone: r.2,
                work_week: r.3,
                day_hours: r.4,
                holiday_calendar_id: r.5,
                holiday_calendar: r.6,
                members: r.7,
                archived: r.8,
                revision: r.9,
            })
            .collect())
    }

    pub async fn location(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Location> {
        self.locations(tx, true).await?.into_iter().find(|l| l.id == id).ok_or(HrError::NotFound)
    }

    async fn check_location(&self, tx: &mut WsTx<'_>, i: &LocationInput) -> R<(String, Vec<i16>, f64)> {
        let name = name_of(&i.name, "A location")?;
        let known: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)")
                .bind(&i.timezone)
                .fetch_one(tx.conn())
                .await?;
        if !known {
            return Err(HrError::Invalid(format!(
                "{} isn't a time zone. Use a name like Asia/Kolkata or Europe/London.",
                i.timezone
            )));
        }
        let week = WorkWeek::from_iso(&i.work_week)
            .ok_or_else(|| HrError::Invalid("Choose at least one working day.".into()))?;
        let hours = i.day_hours.unwrap_or(8.0);
        if !(hours > 0.0 && hours <= 24.0) {
            return Err(HrError::Invalid("A working day is between 0 and 24 hours.".into()));
        }
        if let Some(c) = i.holiday_calendar_id {
            let ok: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM holiday_calendars WHERE id = $1)")
                    .bind(c)
                    .fetch_one(tx.conn())
                    .await?;
            if !ok {
                return Err(HrError::Invalid("That holiday calendar doesn't exist.".into()));
            }
        }
        Ok((name, week.iso_days(), (hours * 4.0).round() / 4.0))
    }

    pub async fn create_location(&self, tx: &mut WsTx<'_>, i: &LocationInput) -> R<Location> {
        let (name, week, hours) = self.check_location(tx, i).await?;
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO work_locations (workspace_id, id, name, timezone, work_week, day_hours, holiday_calendar_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6::float8::numeric, $7, $8)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(&name)
        .bind(&i.timezone)
        .bind(&week)
        .bind(hours)
        .bind(i.holiday_calendar_id)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        self.location(tx, id).await
    }

    /// Changes a location (`If-Match` revision). A rename renames it on its members' profiles too.
    pub async fn update_location(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        rev: Option<i32>,
        i: &LocationInput,
    ) -> R<Location> {
        let before = self.location(tx, id).await?;
        if rev.is_some_and(|r| r != before.revision) {
            return Err(HrError::Conflict("stale".into()));
        }
        let (name, week, hours) = self.check_location(tx, i).await?;
        let archived = i.archived.unwrap_or(before.archived);
        if archived && !before.archived && before.members > 0 {
            let who = if before.members == 1 {
                "1 person works".to_string()
            } else {
                format!("{} people work", before.members)
            };
            return Err(HrError::Conflict(format!(
                "{who} at {}. Move them to another location first.",
                before.name
            )));
        }
        sqlx::query(
            "UPDATE work_locations SET name = $2, timezone = $3, work_week = $4, day_hours = $5::float8::numeric,
                    holiday_calendar_id = $6, archived_at = CASE WHEN $7 THEN coalesce(archived_at, $8) END, revision = revision + 1
              WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(&i.timezone)
        .bind(&week)
        .bind(hours)
        .bind(i.holiday_calendar_id)
        .bind(archived)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        if name != before.name {
            sqlx::query(
                "UPDATE memberships m SET location = $2 FROM membership_hr h
                  WHERE h.workspace_id = m.workspace_id AND h.membership_id = m.id AND h.work_location_id = $1",
            )
            .bind(id)
            .bind(&name)
            .execute(tx.conn())
            .await?;
        }
        self.location(tx, id).await
    }

    // ---------------------------------------------------------------- calendars

    pub async fn calendars(&self, tx: &mut WsTx<'_>) -> R<Vec<HolidayCalendar>> {
        let rows: Vec<(Uuid, String, i32, Vec<String>, Vec<i32>)> = sqlx::query_as(
            "SELECT c.id, c.name, c.revision,
                    coalesce((SELECT array_agg(l.name ORDER BY lower(l.name)) FROM work_locations l
                               WHERE l.holiday_calendar_id = c.id AND l.archived_at IS NULL), '{}'),
                    coalesce((SELECT array_agg(DISTINCT extract(year FROM h.date)::int) FROM holidays h WHERE h.calendar_id = c.id), '{}')
               FROM holiday_calendars c ORDER BY lower(c.name)",
        )
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| HolidayCalendar { id: r.0, name: r.1, revision: r.2, locations: r.3, years: r.4 })
            .collect())
    }

    pub async fn calendar(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<HolidayCalendar> {
        self.calendars(tx).await?.into_iter().find(|c| c.id == id).ok_or(HrError::NotFound)
    }

    /// A new calendar, optionally starting as a copy of another's holidays.
    pub async fn create_calendar(
        &self,
        tx: &mut WsTx<'_>,
        name: &str,
        copy_from: Option<Uuid>,
    ) -> R<HolidayCalendar> {
        let name = name_of(name, "A holiday calendar")?;
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO holiday_calendars (workspace_id, id, name, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(&name)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        if let Some(src) = copy_from {
            self.calendar(tx, src).await?;
            sqlx::query(
                "INSERT INTO holidays (workspace_id, calendar_id, date, name, optional)
                 SELECT workspace_id, $2, date, name, optional FROM holidays WHERE calendar_id = $1",
            )
            .bind(src)
            .bind(id)
            .execute(tx.conn())
            .await?;
        }
        self.calendar(tx, id).await
    }

    pub async fn rename_calendar(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        rev: Option<i32>,
        name: &str,
    ) -> R<HolidayCalendar> {
        let before = self.calendar(tx, id).await?;
        if rev.is_some_and(|r| r != before.revision) {
            return Err(HrError::Conflict("stale".into()));
        }
        let name = name_of(name, "A holiday calendar")?;
        sqlx::query("UPDATE holiday_calendars SET name = $2, revision = revision + 1 WHERE id = $1")
            .bind(id)
            .bind(&name)
            .execute(tx.conn())
            .await?;
        self.calendar(tx, id).await
    }

    /// Deletes a calendar no location uses (its holidays go with it).
    pub async fn delete_calendar(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<HolidayCalendar> {
        let c = self.calendar(tx, id).await?;
        let used: i64 =
            sqlx::query_scalar("SELECT count(*) FROM work_locations WHERE holiday_calendar_id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
        if used > 0 {
            return Err(HrError::Conflict(format!(
                "{} uses this calendar. Choose another calendar for it first.",
                if c.locations.is_empty() {
                    "An archived location".to_string()
                } else {
                    c.locations.join(", ")
                }
            )));
        }
        sqlx::query("DELETE FROM holiday_calendars WHERE id = $1").bind(id).execute(tx.conn()).await?;
        Ok(c)
    }

    async fn touch(&self, tx: &mut WsTx<'_>, calendar: Uuid) -> R<()> {
        sqlx::query("UPDATE holiday_calendars SET revision = revision + 1 WHERE id = $1")
            .bind(calendar)
            .execute(tx.conn())
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------- holidays

    pub async fn holidays(&self, tx: &mut WsTx<'_>, calendar: Uuid, from: Date, to: Date) -> R<Vec<Holiday>> {
        self.calendar(tx, calendar).await?;
        let rows: Vec<(Date, String, bool)> = sqlx::query_as(
            "SELECT date, name, optional FROM holidays WHERE calendar_id = $1 AND date BETWEEN $2 AND $3 ORDER BY date",
        )
        .bind(calendar)
        .bind(from)
        .bind(to)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|r| Holiday { date: r.0, name: r.1, optional: r.2 }).collect())
    }

    /// Adds or renames the holiday on a date. Returns whether it was new.
    pub async fn put_holiday(&self, tx: &mut WsTx<'_>, calendar: Uuid, h: &Holiday) -> R<bool> {
        self.calendar(tx, calendar).await?;
        let name = name_of(&h.name, "A holiday")?;
        let added: bool = sqlx::query_scalar(
            "INSERT INTO holidays (workspace_id, calendar_id, date, name, optional) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (workspace_id, calendar_id, date) DO UPDATE SET name = EXCLUDED.name, optional = EXCLUDED.optional
             RETURNING (xmax = 0)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(calendar)
        .bind(h.date)
        .bind(&name)
        .bind(h.optional)
        .fetch_one(tx.conn())
        .await?;
        self.touch(tx, calendar).await?;
        Ok(added)
    }

    pub async fn delete_holiday(&self, tx: &mut WsTx<'_>, calendar: Uuid, date: Date) -> R<Holiday> {
        let row: Option<(String, bool)> = sqlx::query_as(
            "DELETE FROM holidays WHERE calendar_id = $1 AND date = $2 RETURNING name, optional",
        )
        .bind(calendar)
        .bind(date)
        .fetch_optional(tx.conn())
        .await?;
        let (name, optional) = row.ok_or(HrError::NotFound)?;
        self.touch(tx, calendar).await?;
        Ok(Holiday { date, name, optional })
    }

    /// Imports holidays from CSV (`date,name[,optional]`) or iCal text. Existing dates are renamed.
    pub async fn import(&self, tx: &mut WsTx<'_>, calendar: Uuid, text: &str) -> R<ImportReport> {
        self.calendar(tx, calendar).await?;
        let (found, skipped) = import::parse(text);
        if found.is_empty() && skipped.is_empty() {
            return Err(HrError::Invalid(
                "No holidays found. Paste CSV lines like 2026-10-02,Gandhi Jayanti, or an iCal file.".into(),
            ));
        }
        if found.len() > 400 {
            return Err(HrError::Invalid("Import at most 400 holidays at a time.".into()));
        }
        let mut report = ImportReport { skipped, ..Default::default() };
        for h in &found {
            if self.put_holiday(tx, calendar, h).await? { report.added += 1 } else { report.updated += 1 }
        }
        Ok(report)
    }

    /// Copies a year's holidays to another year on the same month and day (29 February is skipped).
    /// Holidays that move every year (lunar calendars) need checking afterwards.
    pub async fn copy_year(&self, tx: &mut WsTx<'_>, calendar: Uuid, from: i32, to: i32) -> R<ImportReport> {
        if from == to || !(2000..=2100).contains(&from) || !(2000..=2100).contains(&to) {
            return Err(HrError::Invalid("Choose two different years between 2000 and 2100.".into()));
        }
        let start =
            |y| Date::from_calendar_date(y, Month::January, 1).map_err(|e| HrError::Internal(e.to_string()));
        let end = |y| {
            Date::from_calendar_date(y, Month::December, 31).map_err(|e| HrError::Internal(e.to_string()))
        };
        let source = self.holidays(tx, calendar, start(from)?, end(from)?).await?;
        if source.is_empty() {
            return Err(HrError::Invalid(format!("{from} has no holidays to copy.")));
        }
        let existing: BTreeMap<Date, Holiday> = self
            .holidays(tx, calendar, start(to)?, end(to)?)
            .await?
            .into_iter()
            .map(|h| (h.date, h))
            .collect();
        let mut report = ImportReport::default();
        for h in source {
            let Ok(date) = Date::from_calendar_date(to, h.date.month(), h.date.day()) else {
                report.skipped.push(import::Skipped {
                    line: 0,
                    text: h.name.clone(),
                    reason: format!("{to} has no {}", h.date),
                });
                continue;
            };
            if existing.contains_key(&date) {
                report.skipped.push(import::Skipped {
                    line: 0,
                    text: h.name.clone(),
                    reason: format!("{date} already has a holiday"),
                });
                continue;
            }
            self.put_holiday(tx, calendar, &Holiday { date, name: h.name, optional: h.optional }).await?;
            report.added += 1;
        }
        Ok(report)
    }

    // ---------------------------------------------------------------- people

    pub async fn member_hr(&self, tx: &mut WsTx<'_>, membership: Uuid) -> R<MemberHr> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1)")
            .bind(membership)
            .fetch_one(tx.conn())
            .await?;
        if !exists {
            return Err(HrError::NotFound);
        }
        let row: Option<(Option<String>, Option<String>, Option<Uuid>, Option<Date>, Option<Date>)> = sqlx::query_as(
            "SELECT employee_no, employment_type, work_location_id, start_date, probation_end FROM membership_hr WHERE membership_id = $1",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?;
        Ok(row
            .map(|r| MemberHr {
                employee_no: r.0,
                employment_type: r.1,
                work_location_id: r.2,
                start_date: r.3,
                probation_end: r.4,
            })
            .unwrap_or_default())
    }

    /// Sets a member's HR record. Choosing a work location also sets their profile's location.
    pub async fn set_member_hr(&self, tx: &mut WsTx<'_>, membership: Uuid, i: &MemberHr) -> R<MemberHr> {
        self.member_hr(tx, membership).await?;
        let employee_no = i.employee_no.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if employee_no.is_some_and(|n| n.chars().count() > 40) {
            return Err(HrError::Invalid("An employee number is at most 40 characters.".into()));
        }
        if i.employment_type.as_deref().is_some_and(|t| !EMPLOYMENT_TYPES.contains(&t)) {
            return Err(HrError::Invalid(
                "Employment type is full_time, part_time, contract or intern.".into(),
            ));
        }
        if let (Some(s), Some(p)) = (i.start_date, i.probation_end)
            && p < s
        {
            return Err(HrError::Invalid("Probation can't end before the start date.".into()));
        }
        let location_name: Option<String> = match i.work_location_id {
            Some(l) => Some(
                sqlx::query_scalar("SELECT name FROM work_locations WHERE id = $1 AND archived_at IS NULL")
                    .bind(l)
                    .fetch_optional(tx.conn())
                    .await?
                    .ok_or_else(|| HrError::Invalid("Choose a work location that exists.".into()))?,
            ),
            None => None,
        };
        sqlx::query(
            "INSERT INTO membership_hr (workspace_id, membership_id, employee_no, employment_type, work_location_id, start_date, probation_end, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (workspace_id, membership_id) DO UPDATE SET employee_no = $3, employment_type = $4, work_location_id = $5,
                    start_date = $6, probation_end = $7, updated_at = $8",
        )
        .bind(tx.ctx.workspace_id)
        .bind(membership)
        .bind(employee_no)
        .bind(&i.employment_type)
        .bind(i.work_location_id)
        .bind(i.start_date)
        .bind(i.probation_end)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| match HrError::from(e) {
            HrError::Conflict(_) => HrError::Conflict("Another member has that employee number.".into()),
            other => other,
        })?;
        sqlx::query("UPDATE memberships SET location = $2 WHERE id = $1 AND location IS DISTINCT FROM $2")
            .bind(membership)
            .bind(&location_name)
            .execute(tx.conn())
            .await?;
        self.member_hr(tx, membership).await
    }

    /// A member's work location (if any) and the holidays of its calendar between two dates.
    pub async fn holidays_for(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        from: Date,
        to: Date,
    ) -> R<(Option<Location>, Vec<Holiday>)> {
        let hr = self.member_hr(tx, membership).await?;
        let Some(lid) = hr.work_location_id else { return Ok((None, vec![])) };
        let loc = self.location(tx, lid).await?;
        let hols = match loc.holiday_calendar_id {
            Some(c) => self.holidays(tx, c, from, to).await?,
            None => vec![],
        };
        Ok((Some(loc), hols))
    }

    /// The day-counting calendar for a member between two dates (docs/25 §4). Rosters come with
    /// BPMS-226; until then the location's week and non-optional holidays decide.
    pub async fn day_calendar(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        from: Date,
        to: Date,
    ) -> R<Calendar> {
        let (loc, hols) = self.holidays_for(tx, membership, from, to).await?;
        Ok(Calendar {
            week: loc.and_then(|l| WorkWeek::from_iso(&l.work_week)),
            holidays: hols.into_iter().filter(|h| !h.optional).map(|h| (h.date, h.name)).collect(),
            roster: BTreeMap::new(),
        })
    }
}
