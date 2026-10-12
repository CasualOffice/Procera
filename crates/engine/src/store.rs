//! Calendar versions (`calendar_versions`, docs/04 §2.12). Versions are immutable: a new version
//! is a new row with its own reference, and timers and SLA clocks pin the row they used.

use procera_db::WsTx;
use procera_kernel::{Clock, IdGen};
use serde_json::Value;
use time::Date;
use uuid::Uuid;

use crate::EngineError;
use crate::calendar::{Calendar, CalendarSpec};

/// A loaded calendar version.
#[derive(Debug, Clone)]
pub struct CalendarVersion {
    pub id: Uuid,
    pub calendar_ref: String,
    pub timezone: String,
    pub calendar: Calendar,
}

/// Stores a new calendar version after validating it. The reference (`calendar:in-blr@2026.2`) is
/// what IR documents name in `refs.calendars`.
pub async fn put_calendar(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    calendar_ref: &str,
    version_label: &str,
    spec: &CalendarSpec,
) -> Result<Uuid, EngineError> {
    Calendar::from_spec(spec)?;
    let id = ids.new_id();
    let ws = tx.ctx.workspace_id;
    sqlx::query(
        "INSERT INTO calendar_versions (workspace_id, id, calendar_ref, version_label, timezone, working_hours, holidays, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(ws)
    .bind(id)
    .bind(calendar_ref)
    .bind(version_label)
    .bind(&spec.timezone)
    .bind(serde_json::to_value(&spec.working_hours)?)
    .bind(&spec.holidays)
    .bind(clock.now())
    .execute(tx.conn())
    .await?;
    Ok(id)
}

/// Stores a calendar version unless one with this reference exists (seeding a workspace default
/// lazily; concurrent seeders wait on the unique reference and then do nothing). `true` when stored.
pub async fn put_calendar_if_absent(
    tx: &mut WsTx<'_>,
    clock: &dyn Clock,
    ids: &dyn IdGen,
    calendar_ref: &str,
    version_label: &str,
    spec: &CalendarSpec,
) -> Result<bool, EngineError> {
    Calendar::from_spec(spec)?;
    let n = sqlx::query(
        "INSERT INTO calendar_versions (workspace_id, id, calendar_ref, version_label, timezone, working_hours, holidays, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (workspace_id, calendar_ref) DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(ids.new_id())
    .bind(calendar_ref)
    .bind(version_label)
    .bind(&spec.timezone)
    .bind(serde_json::to_value(&spec.working_hours)?)
    .bind(&spec.holidays)
    .bind(clock.now())
    .execute(tx.conn())
    .await?
    .rows_affected();
    Ok(n == 1)
}

type Row = (Uuid, String, String, Value, Vec<Date>);

fn version(row: Row) -> Result<CalendarVersion, EngineError> {
    let (id, calendar_ref, timezone, hours, holidays) = row;
    let spec =
        CalendarSpec { timezone: timezone.clone(), working_hours: serde_json::from_value(hours)?, holidays };
    Ok(CalendarVersion { id, calendar_ref, timezone, calendar: Calendar::from_spec(&spec)? })
}

/// The calendar version a reference names, if stored.
pub async fn calendar_by_ref(tx: &mut WsTx<'_>, calendar_ref: &str) -> Result<CalendarVersion, EngineError> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, calendar_ref, timezone, working_hours, holidays FROM calendar_versions WHERE calendar_ref = $1",
    )
    .bind(calendar_ref)
    .fetch_optional(tx.conn())
    .await?;
    version(row.ok_or_else(|| EngineError::CalendarMissing(calendar_ref.to_owned()))?)
}

/// A calendar version by id (timers and SLA clocks pin it, so a due date can be recomputed in an audit).
pub async fn calendar_by_id(tx: &mut WsTx<'_>, id: Uuid) -> Result<CalendarVersion, EngineError> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, calendar_ref, timezone, working_hours, holidays FROM calendar_versions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(tx.conn())
    .await?;
    version(row.ok_or_else(|| EngineError::CalendarMissing(id.to_string()))?)
}
