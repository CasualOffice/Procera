//! Test-only controls for the E2E harness (docs/23 §4). Compiled only with the
//! `e2e` feature; the release pipeline asserts these routes are absent.

use super::AppState;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use procera_kernel::Problem;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/clock", get(read_clock).post(advance_clock))
        .route("/drain", post(drain))
        .route("/fault", post(fault))
        .route("/audit/verify", get(audit_verify))
}

#[derive(Debug, Serialize)]
struct ClockState {
    #[serde(with = "time::serde::rfc3339")]
    now: time::OffsetDateTime,
}

async fn read_clock(State(s): State<AppState>) -> Json<ClockState> {
    Json(ClockState { now: s.clock.now() })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Advance {
    /// ISO-8601 duration, subset: PnW, PnD, PTnH, PTnM, PTnS and combinations (e.g. "P30D", "P1DT2H").
    advance: String,
}

async fn advance_clock(State(s): State<AppState>, Json(req): Json<Advance>) -> Response {
    match parse_duration(&req.advance) {
        Some(d) if d.is_positive() => Json(ClockState { now: s.manual_clock.advance(d) }).into_response(),
        _ => problem(
            Problem::new(422, "validation_failed", "Validation failed")
                .with_detail("advance must be a positive ISO-8601 duration such as P30D or PT2H"),
        ),
    }
}

#[derive(Debug, Serialize)]
struct Drained {
    idle: bool,
    pending_jobs: u64,
    pending_timers: u64,
    pending_outbox: u64,
}

/// Waits until outbox, jobs and timers are idle. Nothing is asynchronous yet, so
/// this is immediately idle; BPMS-029 and BPMS-083 wire the real queues in.
async fn drain() -> Json<Drained> {
    Json(Drained { idle: true, pending_jobs: 0, pending_timers: 0, pending_outbox: 0 })
}

/// Fault injection arrives with the runtime (BPMS-089).
async fn fault() -> Response {
    problem(
        Problem::new(501, "not_implemented", "Not implemented")
            .with_detail("fault injection lands with the runtime (BPMS-089)"),
    )
}

/// Chain verification arrives with the audit chain (BPMS-026).
async fn audit_verify() -> Response {
    problem(
        Problem::new(501, "not_implemented", "Not implemented")
            .with_detail("audit verification lands with the audit chain (BPMS-026)"),
    )
}

fn problem(p: Problem) -> Response {
    let status = StatusCode::from_u16(p.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, [(axum::http::header::CONTENT_TYPE, "application/problem+json")], Json(p)).into_response()
}

/// Minimal ISO-8601 duration parser for test controls. Months and years are
/// rejected because their length is ambiguous.
fn parse_duration(s: &str) -> Option<time::Duration> {
    let rest = s.strip_prefix('P')?;
    let (date, clock) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    if clock == Some("") {
        return None;
    }
    let mut total = time::Duration::ZERO;
    let mut any = false;
    for (part, units) in [(date, "WD"), (clock.unwrap_or(""), "HMS")] {
        let mut num = String::new();
        for ch in part.chars() {
            if ch.is_ascii_digit() {
                num.push(ch);
                continue;
            }
            if !units.contains(ch) || num.is_empty() {
                return None;
            }
            let n: i64 = num.parse().ok()?;
            total += match ch {
                'W' => time::Duration::weeks(n),
                'D' => time::Duration::days(n),
                'H' => time::Duration::hours(n),
                'M' => time::Duration::minutes(n),
                _ => time::Duration::seconds(n),
            };
            num.clear();
            any = true;
        }
        if !num.is_empty() {
            return None;
        }
    }
    any.then_some(total)
}

#[cfg(test)]
mod tests {
    use super::parse_duration;
    use time::Duration;

    #[test]
    fn parses_supported_durations() {
        assert_eq!(parse_duration("P30D"), Some(Duration::days(30)));
        assert_eq!(parse_duration("P1DT2H"), Some(Duration::days(1) + Duration::hours(2)));
        assert_eq!(parse_duration("PT15M"), Some(Duration::minutes(15)));
        assert_eq!(parse_duration("P2W"), Some(Duration::weeks(2)));
    }

    #[test]
    fn rejects_invalid_durations() {
        for bad in ["", "P", "PT", "30D", "P1Y", "P1M", "PxD", "P1DT", "P1H"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }
}
