//! HR time and attendance (docs/25), slice 1 (BPMS-222): work locations, holiday calendars and
//! holidays (read `directory:view`, change `calendar:manage`), a member's HR record (change
//! `directory:manage`) and a person's own holidays.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use procera_access::Resource;
use procera_hr::{Holiday, HrError, LocationInput, MemberHr};
use serde::Deserialize;
use serde_json::{Value, json};
use time::{Date, Duration, Month};
use uuid::Uuid;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/work-locations", get(locations).post(create_location))
        .route("/work-locations/{id}", get(location).patch(update_location))
        .route("/holiday-calendars", get(calendars).post(create_calendar))
        .route("/holiday-calendars/{id}", get(calendar).patch(rename_calendar).delete(delete_calendar))
        .route("/holiday-calendars/{id}/holidays", get(holidays))
        .route("/holiday-calendars/{id}/holidays/{date}", put(put_holiday).delete(delete_holiday))
        .route("/holiday-calendars/{id}/import", post(import))
        .route("/holiday-calendars/{id}/copy-year", post(copy_year))
        .route("/members/{id}/hr", get(member_hr).put(set_member_hr))
        .route("/me/holidays", get(my_holidays))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "hr api internal error");
    ApiError::new(500, "internal", "Internal error")
}

impl From<HrError> for ApiError {
    fn from(e: HrError) -> Self {
        match e {
            HrError::NotFound => Self::not_found(),
            HrError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            HrError::Conflict(m) if m == "stale" => {
                Self::new(412, "precondition_failed", "It changed since you loaded it")
                    .detail("Reload and try again.")
            }
            HrError::Conflict(m) => Self::new(409, "conflict", "That can't be done now").detail(m),
            HrError::Internal(m) => internal(m),
        }
    }
}

fn hr(s: &AppState) -> Result<&procera_hr::Hr, ApiError> {
    s.hr.as_ref().ok_or_else(|| ApiError::unavailable("time and attendance"))
}

fn if_match(headers: &HeaderMap) -> Option<i32> {
    headers.get(header::IF_MATCH)?.to_str().ok()?.trim().trim_matches('"').parse().ok()
}

fn with_etag(v: Value, rev: i32) -> Response {
    let mut res = Json(v).into_response();
    if let Ok(h) = HeaderValue::from_str(&format!("\"{rev}\"")) {
        res.headers_mut().insert(header::ETAG, h);
    }
    res
}

fn date(s: &str) -> Result<Date, ApiError> {
    procera_hr::dates::parse(s).ok_or_else(|| {
        ApiError::new(422, "validation_failed", "Validation failed").detail("Dates look like 2026-10-02.")
    })
}

fn year_bounds(year: i32) -> Result<(Date, Date), ApiError> {
    let bad = || {
        ApiError::new(422, "validation_failed", "Validation failed")
            .detail("Choose a year between 2000 and 2100.")
    };
    if !(2000..=2100).contains(&year) {
        return Err(bad());
    }
    Ok((
        Date::from_calendar_date(year, Month::January, 1).map_err(|_| bad())?,
        Date::from_calendar_date(year, Month::December, 31).map_err(|_| bad())?,
    ))
}

/// Opens the transaction, checks `perm` and runs `f`; commits when `write`.
macro_rules! audited {
    ($s:expr, $me:expr, $headers:expr, $perm:expr, |$tx:ident, $m:ident| $body:expr) => {{
        let $m = members(&$s)?;
        let mut $tx = $me.tx($m).await?;
        $me.require($m, &mut $tx, $perm, Resource::Workspace).await?;
        let out = $body;
        $tx.commit().await.map_err(internal)?;
        out
    }};
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    s: &AppState,
    me: &WsMember,
    tx: &mut procera_db::WsTx<'_>,
    headers: &HeaderMap,
    action: &str,
    kind: &str,
    id: Uuid,
    data: Value,
) -> Result<(), ApiError> {
    members(s)?
        .audit_object(
            tx,
            Some(me.actor.subject_ref),
            action,
            kind,
            id,
            data,
            &super::auth::client_info(headers),
        )
        .await?;
    Ok(())
}

// ------------------------------------------------------------------ locations

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    archived: bool,
}

/// `GET /work-locations?archived=`: locations with their time zone, work week, holiday calendar and
/// active member count (`directory:view`).
async fn locations(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let items = hr(&s)?.locations(&mut tx, q.archived).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `GET /work-locations/{id}`: one location, with an `ETag` of its revision (`directory:view`).
async fn location(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let l = hr(&s)?.location(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    let rev = l.revision;
    Ok(with_etag(json!(l), rev))
}

/// `POST /work-locations {name, timezone (IANA), work_week [1-7]?, day_hours?, holiday_calendar_id?}`:
/// a new location (`calendar:manage`). 201. Audited `work_location.created`.
async fn create_location(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<LocationInput>,
) -> Result<Response, ApiError> {
    let l = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let l = hr(&s)?.create_location(&mut tx, &r).await?;
        audit(&s, &me, &mut tx, &headers, "work_location.created", "work_location", l.id, json!({ "name": l.name, "timezone": l.timezone, "work_week": l.work_week, "holiday_calendar_id": l.holiday_calendar_id })).await?;
        l
    });
    Ok((StatusCode::CREATED, Json(l)).into_response())
}

/// `PATCH /work-locations/{id}` (the full location, as for create, plus `archived`): changes it
/// (`calendar:manage`; honours `If-Match`); a rename renames it on members' profiles; a location
/// people work at can't be archived (409). Audited `work_location.updated`.
async fn update_location(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<LocationInput>,
) -> Result<Response, ApiError> {
    let l = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let l = hr(&s)?.update_location(&mut tx, id, if_match(&headers), &r).await?;
        audit(&s, &me, &mut tx, &headers, "work_location.updated", "work_location", id, json!({ "name": l.name, "timezone": l.timezone, "work_week": l.work_week, "holiday_calendar_id": l.holiday_calendar_id, "archived": l.archived })).await?;
        l
    });
    let rev = l.revision;
    Ok(with_etag(json!(l), rev))
}

// ------------------------------------------------------------------ calendars

/// `GET /holiday-calendars`: calendars with the locations using them and the years they have
/// holidays in (`directory:view`).
async fn calendars(State(s): State<AppState>, me: WsMember) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let items = hr(&s)?.calendars(&mut tx).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

/// `GET /holiday-calendars/{id}`: one calendar, with an `ETag` of its revision (`directory:view`).
async fn calendar(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let c = hr(&s)?.calendar(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    let rev = c.revision;
    Ok(with_etag(json!(c), rev))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalendarReq {
    name: String,
    #[serde(default)]
    copy_from: Option<Uuid>,
}

/// `POST /holiday-calendars {name, copy_from?}`: a new calendar, empty or a copy of another's
/// holidays (`calendar:manage`). 201. Audited `holiday_calendar.created`.
async fn create_calendar(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Json(r): Json<CalendarReq>,
) -> Result<Response, ApiError> {
    let c = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let c = hr(&s)?.create_calendar(&mut tx, &r.name, r.copy_from).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday_calendar.created",
            "holiday_calendar",
            c.id,
            json!({ "name": c.name, "copy_from": r.copy_from }),
        )
        .await?;
        c
    });
    Ok((StatusCode::CREATED, Json(c)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameReq {
    name: String,
}

/// `PATCH /holiday-calendars/{id} {name}`: renames a calendar (`calendar:manage`; honours
/// `If-Match`). Audited `holiday_calendar.updated`.
async fn rename_calendar(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<RenameReq>,
) -> Result<Response, ApiError> {
    let c = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let c = hr(&s)?.rename_calendar(&mut tx, id, if_match(&headers), &r.name).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday_calendar.updated",
            "holiday_calendar",
            id,
            json!({ "name": c.name }),
        )
        .await?;
        c
    });
    let rev = c.revision;
    Ok(with_etag(json!(c), rev))
}

/// `DELETE /holiday-calendars/{id}`: deletes a calendar and its holidays when no location uses it
/// (409 otherwise; `calendar:manage`). 204. Audited `holiday_calendar.deleted`.
async fn delete_calendar(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    audited!(s, me, headers, "calendar:manage", |tx, m| {
        let c = hr(&s)?.delete_calendar(&mut tx, id).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday_calendar.deleted",
            "holiday_calendar",
            id,
            json!({ "name": c.name }),
        )
        .await?;
    });
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ------------------------------------------------------------------ holidays

#[derive(Debug, Deserialize)]
struct YearQuery {
    year: i32,
}

/// `GET /holiday-calendars/{id}/holidays?year=`: the year's holidays `[{date, name, optional}]`
/// (`directory:view`).
async fn holidays(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Response, ApiError> {
    let (from, to) = year_bounds(q.year)?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let items = hr(&s)?.holidays(&mut tx, id, from, to).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HolidayReq {
    name: String,
    #[serde(default)]
    optional: bool,
}

/// `PUT /holiday-calendars/{id}/holidays/{date} {name, optional?}`: adds the holiday on that date or
/// renames it (`calendar:manage`). 201 when new, 200 when changed. Audited `holiday.set`.
async fn put_holiday(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, day)): Path<(Uuid, String)>,
    Json(r): Json<HolidayReq>,
) -> Result<Response, ApiError> {
    let d = date(&day)?;
    let h = Holiday { date: d, name: r.name, optional: r.optional };
    let added = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let added = hr(&s)?.put_holiday(&mut tx, id, &h).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday.set",
            "holiday_calendar",
            id,
            json!({ "date": day, "name": h.name.trim(), "optional": h.optional, "added": added }),
        )
        .await?;
        added
    });
    Ok((if added { StatusCode::CREATED } else { StatusCode::OK }, Json(h)).into_response())
}

/// `DELETE /holiday-calendars/{id}/holidays/{date}`: removes it (`calendar:manage`). 204. Audited
/// `holiday.removed`.
async fn delete_holiday(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path((id, day)): Path<(Uuid, String)>,
) -> Result<Response, ApiError> {
    let d = date(&day)?;
    audited!(s, me, headers, "calendar:manage", |tx, m| {
        let h = hr(&s)?.delete_holiday(&mut tx, id, d).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday.removed",
            "holiday_calendar",
            id,
            json!({ "date": day, "name": h.name }),
        )
        .await?;
    });
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportReq {
    text: String,
}

/// `POST /holiday-calendars/{id}/import {text}`: imports CSV (`date,name[,optional]`) or iCal text,
/// at most 400 holidays and 1 MB (`calendar:manage`) → `{added, updated, skipped: [{line, text,
/// reason}]}`. Audited `holiday_calendar.imported`.
async fn import(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<ImportReq>,
) -> Result<Response, ApiError> {
    if r.text.len() > 1_000_000 {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("Import at most 1 MB at a time."));
    }
    let report = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let report = hr(&s)?.import(&mut tx, id, &r.text).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday_calendar.imported",
            "holiday_calendar",
            id,
            json!({ "added": report.added, "updated": report.updated, "skipped": report.skipped.len() }),
        )
        .await?;
        report
    });
    Ok(Json(report).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyReq {
    from: i32,
    to: i32,
}

/// `POST /holiday-calendars/{id}/copy-year {from, to}`: copies a year's holidays to the same month
/// and day of another year, skipping dates already taken (`calendar:manage`) → as import. Audited
/// `holiday_calendar.year_copied`.
async fn copy_year(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<CopyReq>,
) -> Result<Response, ApiError> {
    let report = audited!(s, me, headers, "calendar:manage", |tx, m| {
        let report = hr(&s)?.copy_year(&mut tx, id, r.from, r.to).await?;
        audit(
            &s,
            &me,
            &mut tx,
            &headers,
            "holiday_calendar.year_copied",
            "holiday_calendar",
            id,
            json!({ "from": r.from, "to": r.to, "added": report.added }),
        )
        .await?;
        report
    });
    Ok(Json(report).into_response())
}

// ------------------------------------------------------------------ people

/// `GET /members/{id}/hr`: `{employee_no, employment_type, work_location_id, start_date,
/// probation_end}` (`directory:view`).
async fn member_hr(
    State(s): State<AppState>,
    me: WsMember,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "directory:view", Resource::Workspace).await?;
    let h = hr(&s)?.member_hr(&mut tx, id).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(h).into_response())
}

/// `PUT /members/{id}/hr` (the whole record): sets it (`directory:manage`); the work location also
/// becomes the profile's location; 409 for a duplicate employee number. Audited `member.hr_updated`.
async fn set_member_hr(
    State(s): State<AppState>,
    headers: HeaderMap,
    me: WsMember,
    Path(id): Path<Uuid>,
    Json(r): Json<MemberHr>,
) -> Result<Response, ApiError> {
    let h = audited!(s, me, headers, "directory:manage", |tx, m| {
        let h = hr(&s)?.set_member_hr(&mut tx, id, &r).await?;
        audit(&s, &me, &mut tx, &headers, "member.hr_updated", "membership", id, json!(h)).await?;
        h
    });
    Ok(Json(h).into_response())
}

#[derive(Debug, Deserialize)]
struct RangeQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

/// `GET /me/holidays?from=&to=`: the caller's work location (or `null`) and its holidays in the
/// range (default: today and the next 366 days; at most 2 years).
async fn my_holidays(
    State(s): State<AppState>,
    me: WsMember,
    Query(q): Query<RangeQuery>,
) -> Result<Response, ApiError> {
    let today = s.clock.now().date();
    let from = q.from.as_deref().map(date).transpose()?.unwrap_or(today);
    let to = q.to.as_deref().map(date).transpose()?.unwrap_or(from + Duration::days(366));
    if to < from || (to - from).whole_days() > 731 {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("Ask for at most two years, from before to."));
    }
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let (loc, items) = hr(&s)?.holidays_for(&mut tx, me.actor.membership, from, to).await?;
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "location": loc, "items": items })).into_response())
}
