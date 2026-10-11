//! Test-only controls for the E2E harness (docs/23 §4). Compiled only with the
//! `e2e` feature; the release pipeline asserts these routes are absent.

use super::AppState;
use axum::{
    Json, Router,
    extract::{Query, State},
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
        .route("/audit/contract", get(audit_contract))
        .route("/audit/events", get(audit_events))
        .route("/users", post(create_user))
        .route("/bootstrap-token", post(bootstrap_token))
        .route("/system-roles", post(grant_system_role))
        .route("/invitations/token", get(invitation_token))
        .route("/session/backdate", post(backdate_session))
        .route("/mail", get(mail))
}

/// Issues a bootstrap token if the platform is not bootstrapped yet (E2E only; prod prints it to the log).
async fn bootstrap_token(State(s): State<AppState>) -> Response {
    let Some(p) = s.platform.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match p.issue_bootstrap_token().await {
        Ok(Some(t)) => Json(serde_json::json!({"token": t})).into_response(),
        Ok(None) => {
            problem(Problem::new(409, "already_bootstrapped", "The platform is already bootstrapped"))
        }
        Err(e) => super::error::ApiError::from(e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantReq {
    email: String,
    role: String,
}

/// Grants a system role to an existing user (E2E only), so suites don't depend on bootstrap order.
async fn grant_system_role(State(s): State<AppState>, Json(r): Json<GrantReq>) -> Response {
    let (Some(p), Some(db)) = (s.platform.as_ref(), s.db.as_ref()) else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let user: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(r.email.to_lowercase())
        .fetch_optional(db.pool())
        .await
        .ok()
        .flatten();
    let Some(user) = user else { return problem(Problem::not_found()) };
    match p.grant_system_role(None, user, &r.role, &procera_identity::ClientInfo::default()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => super::error::ApiError::from(e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewUser {
    email: String,
    display_name: String,
    password: String,
}

/// Seeds a user directly (E2E only). Real onboarding goes through bootstrap/invites (BPMS-020/023).
async fn create_user(State(s): State<AppState>, Json(u): Json<NewUser>) -> Response {
    let Some(id) = s.identity.as_ref() else {
        return problem(
            Problem::new(503, "unavailable", "Service unavailable").with_detail("database is not configured"),
        );
    };
    match id
        .create_user(&u.email, &u.display_name, &u.password, &procera_identity::ClientInfo::default())
        .await
    {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(e) => super::error::ApiError::from(e).into_response(),
    }
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
/// Drains the outbox (jobs and timers arrive with the runtime) and reports what is still pending.
async fn drain(State(s): State<AppState>) -> Response {
    let Some(d) = s.dispatcher.as_ref() else {
        return Json(Drained { idle: true, pending_jobs: 0, pending_timers: 0, pending_outbox: 0 })
            .into_response();
    };
    // Parallel tests keep writing events; retry a few passes before reporting what is left.
    let mut pending = Ok(0);
    for _ in 0..5 {
        pending = match d.drain().await {
            Ok(_) => d.pending().await,
            Err(e) => Err(e),
        };
        if matches!(pending, Ok(0) | Err(_)) {
            break;
        }
    }
    match pending {
        Ok(n) => {
            let n = n as u64;
            Json(Drained { idle: n == 0, pending_jobs: 0, pending_timers: 0, pending_outbox: n })
                .into_response()
        }
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
}

/// Fault injection arrives with the runtime (BPMS-089).
async fn fault() -> Response {
    problem(
        Problem::new(501, "not_implemented", "Not implemented")
            .with_detail("fault injection lands with the runtime (BPMS-089)"),
    )
}

/// Verifies the system-plane audit chain (`?workspace=<uuid>` for a workspace chain).
async fn audit_verify(
    State(s): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(
            Problem::new(503, "unavailable", "Service unavailable").with_detail("database is not configured"),
        );
    };
    let ws = q.get("workspace").and_then(|w| w.parse().ok()).unwrap_or(procera_db::SYSTEM_WORKSPACE);
    match procera_audit::verify(db, procera_db::WsCtx { workspace_id: ws }).await {
        Ok(r) => Json(serde_json::json!({"checked": r.checked, "first_broken": r.first_broken, "ok": r.first_broken.is_none()})).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
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

#[derive(Debug, Deserialize)]
struct InvitationQuery {
    workspace: uuid::Uuid,
    email: String,
}

/// Reads the plaintext token of the latest pending invitation (stands in for the mailbox until BPMS-048).
async fn invitation_token(State(s): State<AppState>, Query(q): Query<InvitationQuery>) -> Response {
    let Some(m) = s.members.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match m.pending_invitation_token(procera_db::WsCtx { workspace_id: q.workspace }, &q.email).await {
        Ok(Some(token)) => Json(serde_json::json!({ "token": token })).into_response(),
        Ok(None) => problem(Problem::not_found()),
        Err(e) => super::error::ApiError::from(e).into_response(),
    }
}

async fn audit_contract() -> Json<&'static [super::audit_contract::Entry]> {
    Json(super::audit_contract::CONTRACT)
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    workspace: Option<uuid::Uuid>,
    user_agent: String,
}

/// Audit events from one client (matched by User-Agent) on the system chain and, optionally, a
/// workspace chain. Lets the audit-coverage pack attribute events without racing parallel tests.
async fn audit_events(State(s): State<AppState>, Query(q): Query<EventsQuery>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let mut chains = vec![("system", procera_db::SYSTEM_WORKSPACE)];
    if let Some(ws) = q.workspace {
        chains.push(("workspace", ws));
    }
    let mut out = vec![];
    for (chain, ws) in chains {
        let rows: Result<Vec<(i64, String)>, String> = async {
            let mut tx =
                db.begin_ws(procera_db::WsCtx { workspace_id: ws }).await.map_err(|e| e.to_string())?;
            let rows =
                sqlx::query_as("SELECT seq, action FROM audit_events WHERE user_agent = $1 ORDER BY seq")
                    .bind(&q.user_agent)
                    .fetch_all(tx.conn())
                    .await
                    .map_err(|e| e.to_string())?;
            tx.rollback().await.map_err(|e| e.to_string())?;
            Ok(rows)
        }
        .await;
        match rows {
            Ok(rows) => out.extend(
                rows.into_iter()
                    .map(|(seq, action)| serde_json::json!({"chain": chain, "seq": seq, "action": action})),
            ),
            Err(e) => return problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
        }
    }
    Json(serde_json::json!({ "items": out })).into_response()
}

#[derive(Debug, Deserialize)]
struct Backdate {
    seconds: i64,
}

/// Re-signs the caller's access token with `auth_time` moved `seconds` into the past, so step-up
/// expiry can be tested without moving the shared clock.
async fn backdate_session(
    State(s): State<AppState>,
    super::auth::AuthUser(c): super::auth::AuthUser,
    Json(b): Json<Backdate>,
) -> Response {
    let Some(id) = s.identity.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let at = time::OffsetDateTime::from_unix_timestamp(c.auth_time - b.seconds)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    match id.resign_with_auth_time(&c, at) {
        Ok(token) => Json(serde_json::json!({ "access_token": token })).into_response(),
        Err(e) => super::error::ApiError::from(e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct MailQuery {
    workspace: uuid::Uuid,
    to: String,
}

/// Emails queued or sent to `to` in a workspace, decrypted (the local stand-in for Mailpit).
async fn mail(State(s): State<AppState>, Query(q): Query<MailQuery>) -> Response {
    let Some(n) = s.notify.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match n.emails_to(procera_db::WsCtx { workspace_id: q.workspace }, &q.to).await {
        Ok(items) => Json(serde_json::json!({ "items": items })).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
}
