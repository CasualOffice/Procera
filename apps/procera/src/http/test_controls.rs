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
        .route("/webauthn", get(webauthn_rp))
        .route("/session/age", post(age_sign_ins))
        .route("/mail", get(mail))
        .route("/settings", post(set_workspace_setting))
        .route("/campaigns/backdate", post(backdate_campaign))
        .route("/work-items/backdate", post(backdate_work_item))
        .route("/envelopes/backdate", post(backdate_envelope))
        .route("/incidents", post(plant_incident))
        .route("/library/backdate", post(backdate_artifact))
        .route("/library/due", post(make_version_due))
        .route("/announcements/expire", post(expire_announcement))
        .route("/retention/backdate", post(backdate_retention))
        .route("/retention/run", post(run_retention))
        .route("/keys/rewrap", post(rewrap_keys))
        .route("/files/backdate", post(backdate_file))
        .route("/audit/exports/run", post(run_audit_exports))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampaignBackdate {
    workspace: uuid::Uuid,
    campaign: uuid::Uuid,
    seconds: i64,
}

/// Moves an active campaign and its open assignments `seconds` into the past, so reminder and
/// escalation timing can be tested without moving the shared clock (a clock jump breaks the step-up
/// window of every test running in parallel).
async fn backdate_campaign(State(s): State<AppState>, Json(b): Json<CampaignBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let by = b.seconds as f64;
        let n = sqlx::query("UPDATE campaigns SET launched_at = launched_at - make_interval(secs => $2), due_at = due_at - make_interval(secs => $2) WHERE id = $1 AND state = 'active'")
            .bind(b.campaign).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        sqlx::query("UPDATE campaign_recipients SET assigned_at = assigned_at - make_interval(secs => $2), due_at = due_at - make_interval(secs => $2) WHERE campaign_id = $1 AND state = 'pending'")
            .bind(b.campaign).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkItemBackdate {
    workspace: uuid::Uuid,
    work_item: uuid::Uuid,
    seconds: i64,
}

/// Moves an open work item's due date and its scheduled reminders and escalations `seconds` into
/// the past (BPMS-085), so `/drain` fires them without moving the shared clock.
async fn backdate_work_item(State(s): State<AppState>, Json(b): Json<WorkItemBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let by = b.seconds as f64;
        let n = sqlx::query("UPDATE work_items SET due_at = due_at - make_interval(secs => $2) WHERE id = $1 AND state IN ('offered', 'claimed')")
            .bind(b.work_item).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        sqlx::query("UPDATE timers t SET due_at = t.due_at - make_interval(secs => $2) FROM work_items w
                     WHERE w.id = $1 AND starts_with(t.timer_key, w.work_key || '/esc/') AND t.state = 'scheduled'")
            .bind(b.work_item).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
struct EnvelopeBackdate {
    workspace: uuid::Uuid,
    envelope: uuid::Uuid,
    seconds: i64,
}

/// Moves a sent envelope's times back by `seconds` (sent, expiry, when signers were told and
/// reminded), so a reminder or expiry falls due without moving the shared clock (BPMS-107).
async fn backdate_envelope(State(s): State<AppState>, Json(b): Json<EnvelopeBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let by = b.seconds as f64;
        let n = sqlx::query("UPDATE envelopes SET sent_at = sent_at - make_interval(secs => $2), expires_at = expires_at - make_interval(secs => $2) WHERE id = $1 AND state = 'sent'")
            .bind(b.envelope).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        sqlx::query("UPDATE envelope_recipients SET notified_at = notified_at - make_interval(secs => $2), reminded_at = reminded_at - make_interval(secs => $2) WHERE envelope_id = $1")
            .bind(b.envelope).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
struct PlantIncident {
    workspace: uuid::Uuid,
    request: uuid::Uuid,
}

/// Plants a dead-lettered advance job for a request (an incident for the Ops view): its payload
/// can't be read, so a retry fails again and is dead-lettered after its attempts. Returns its id.
async fn plant_incident(State(s): State<AppState>, Json(b): Json<PlantIncident>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let id = uuid::Uuid::now_v7();
    let r: Result<(), String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO jobs (workspace_id, id, kind, instance_id, payload, run_at, attempts, max_attempts, state, idempotency_key, created_at, finished_at, last_error)
                     VALUES ($1, $2, 'advance', $3, '{\"e2e\": \"unreadable\"}', now(), 10, 10, 'dead', $4, now(), now(), '{\"message\": \"e2e: the stimulus could not be read\"}')")
            .bind(b.workspace).bind(id).bind(b.request).bind(format!("e2e-incident-{id}"))
            .execute(tx.conn()).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }
    .await;
    match r {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
struct ArtifactBackdate {
    workspace: uuid::Uuid,
    artifact: uuid::Uuid,
    seconds: i64,
}

/// Moves an artifact's review period and verification `seconds` into the past, so overdue review
/// and verification expiry can be tested without advancing the clock past token lifetimes.
async fn backdate_artifact(State(s): State<AppState>, Json(b): Json<ArtifactBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let n = sqlx::query("UPDATE artifacts SET next_review_at = next_review_at - make_interval(secs => $2), last_reviewed_at = last_reviewed_at - make_interval(secs => $2), verified_at = verified_at - make_interval(secs => $2), verification_expires_at = verification_expires_at - make_interval(secs => $2) WHERE id = $1")
            .bind(b.artifact).bind(b.seconds as f64).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionDue {
    workspace: uuid::Uuid,
    version: uuid::Uuid,
}

/// Makes a scheduled version due now (its `effective_at` one second ago), so a test can watch it go
/// live without moving the shared clock under parallel tests.
async fn make_version_due(State(s): State<AppState>, Json(b): Json<VersionDue>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let n = sqlx::query(
            "UPDATE artifact_versions SET effective_at = $2 WHERE id = $1 AND lifecycle = 'scheduled'",
        )
        .bind(b.version)
        .bind(s.clock.now() - time::Duration::seconds(1))
        .execute(tx.conn())
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnouncementExpire {
    workspace: uuid::Uuid,
    announcement: uuid::Uuid,
}

/// Ends a published announcement's time in the feed now (`expires_at` one second ago).
async fn expire_announcement(State(s): State<AppState>, Json(b): Json<AnnouncementExpire>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let n = sqlx::query("UPDATE announcements SET expires_at = $2 WHERE id = $1 AND state = 'published' AND ack_by IS NULL")
            .bind(b.announcement)
            .bind(s.clock.now() - time::Duration::seconds(1))
            .execute(tx.conn())
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

#[derive(Debug, Deserialize)]
struct RetentionBackdate {
    workspace: uuid::Uuid,
    version: Option<uuid::Uuid>,
    notification: Option<uuid::Uuid>,
    seconds: i64,
}

/// Moves a draft's last activity or a notification's creation `seconds` into the past, so retention
/// classes can be tested without moving the shared clock (BPMS-066).
async fn backdate_retention(State(s): State<AppState>, Json(b): Json<RetentionBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let by = b.seconds as f64;
        let mut n = 0;
        if let Some(v) = b.version {
            n += sqlx::query("UPDATE artifact_versions SET last_activity_at = last_activity_at - make_interval(secs => $2) WHERE id = $1 AND lifecycle = 'draft'")
                .bind(v).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        }
        if let Some(id) = b.notification {
            n += sqlx::query("UPDATE notifications SET created_at = created_at - make_interval(secs => $2) WHERE id = $1")
                .bind(id).bind(by).execute(tx.conn()).await.map_err(|e| e.to_string())?.rows_affected();
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(0) => problem(Problem::not_found()),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

/// Runs the retention jobs now (BPMS-066). Separate from `/drain`: retention reads every workspace's
/// settings, which would slow the drain that every other test calls.
async fn run_retention(State(s): State<AppState>) -> Response {
    let Some(r) = s.retention.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match r.run().await {
        Ok(n) => Json(serde_json::json!({ "removed": n })).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

/// Runs the key rewrap job now (BPMS-030).
async fn rewrap_keys(State(s): State<AppState>) -> Response {
    let Some(k) = s.keys.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match k.rewrap_due(10_000).await {
        Ok(n) => Json(serde_json::json!({ "rewrapped": n })).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct FileBackdate {
    workspace: uuid::Uuid,
    file: uuid::Uuid,
    seconds: i64,
}

/// Moves a file's completion `seconds` into the past (orphan report, BPMS-062).
async fn backdate_file(State(s): State<AppState>, Json(b): Json<FileBackdate>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r: Result<u64, String> = async {
        let mut tx =
            db.begin_ws(procera_db::WsCtx { workspace_id: b.workspace }).await.map_err(|e| e.to_string())?;
        let n = sqlx::query(
            "UPDATE files SET completed_at = completed_at - make_interval(secs => $2) WHERE id = $1",
        )
        .bind(b.file)
        .bind(b.seconds as f64)
        .execute(tx.conn())
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
    .await;
    match r {
        Ok(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => problem(Problem::not_found()),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}

/// Runs queued background audit exports now (BPMS-064).
async fn run_audit_exports(State(s): State<AppState>) -> Response {
    let Some(x) = s.audit_exports.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    match x.run_due().await {
        Ok(n) => Json(serde_json::json!({ "finished": n })).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
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
    // Scheduled publications (library versions, announcements) that are due, then the outbox.
    if let Some(l) = s.library.as_ref()
        && let Err(e) = l.publish_due().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
    // Artifacts whose review date has passed (BPMS-044).
    if let Some(l) = s.library.as_ref()
        && let Err(e) = l.flag_overdue().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
    // Campaign reminder steps and manager escalations that are due.
    if let Some(c) = s.campaigns.as_ref()
        && let Err(e) = c.run_due().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
    // Signing reminders and expiries that are due (BPMS-107).
    if let Some(x) = s.signing.as_ref()
        && let Err(e) = x.run_due().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e));
    }
    // Notification digests that are due (BPMS-048).
    if let Some(n) = s.notify.as_ref()
        && let Err(e) = n.send_digests().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
    // Runtime timers and jobs that are due (request steps, deadlines, continuations).
    if let Some(r) = s.requests.as_ref()
        && let Err(e) = procera_engine::Scheduler::new(r.engine().clone(), "e2e-drain").drain().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
    // Scheduled announcements that are due.
    if let Some(a) = s.announcements.as_ref()
        && let Err(e) = a.publish_due().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string()));
    }
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
    // Webhook deliveries queued by the bus (BPMS-143), sent now.
    if let Some(w) = s.webhooks.as_ref()
        && let Err(e) = w.run_due().await
    {
        return problem(Problem::new(500, "internal", "Internal error").with_detail(e));
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
#[serde(deny_unknown_fields)]
struct SignInAge {
    email: String,
    seconds: i64,
}

/// Moves the sign-in time of every live sign-in of `email` `seconds` into the past, so a browser
/// session (whose access token lives in page memory) gets an old `auth_time` on its next refresh.
/// Lets browser journeys reach step-up without moving the shared clock.
async fn age_sign_ins(State(s): State<AppState>, Json(b): Json<SignInAge>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let r = sqlx::query(
        "UPDATE refresh_tokens SET auth_time = auth_time - make_interval(secs => $2)
         WHERE user_id = (SELECT id FROM users WHERE email = $1) AND revoked_at IS NULL",
    )
    .bind(b.email.trim().to_lowercase())
    .bind(b.seconds as f64)
    .execute(db.pool())
    .await;
    match r {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
}

/// The WebAuthn relying party (RP ID and allowed origins), so E2E authenticators answer for the
/// origin the server expects whatever `--public-url` it runs with.
async fn webauthn_rp(State(s): State<AppState>) -> Response {
    let Some(id) = s.identity.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let rp = id.relying_party();
    Json(serde_json::json!({ "rp_id": rp.id, "origins": rp.origins })).into_response()
}

#[derive(Debug, Deserialize)]
struct MailQuery {
    /// Omit for system emails (account recovery and security notices, docs/06 §3.6.1).
    #[serde(default)]
    workspace: Option<uuid::Uuid>,
    to: String,
}

/// Emails queued or sent to `to` in a workspace (or, without `workspace`, the system queue),
/// decrypted (the local stand-in for Mailpit).
async fn mail(State(s): State<AppState>, Query(q): Query<MailQuery>) -> Response {
    let Some(n) = s.notify.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let workspace_id = q.workspace.unwrap_or(procera_db::SYSTEM_WORKSPACE);
    match n.emails_to(procera_db::WsCtx { workspace_id }, &q.to).await {
        Ok(items) => Json(serde_json::json!({ "items": items })).into_response(),
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingReq {
    workspace: uuid::Uuid,
    key: String,
    value: serde_json::Value,
}

/// Writes a workspace setting directly (validated against the catalogue). For demos and test set-up
/// only — e.g. a demo workspace that doesn't require MFA, which no member could otherwise enter to change.
async fn set_workspace_setting(State(s): State<AppState>, Json(r): Json<SettingReq>) -> Response {
    let Some(db) = s.db.as_ref() else {
        return problem(Problem::new(503, "unavailable", "Service unavailable"));
    };
    let value = match procera_settings::def(&r.key).and_then(|d| procera_settings::validate(d, &r.value)) {
        Ok(v) => v,
        Err(e) => return super::error::ApiError::from(e).into_response(),
    };
    let res: Result<(), String> = async {
        let mut tx = db.begin_ws(procera_db::WsCtx { workspace_id: r.workspace }).await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO workspace_settings (workspace_id, key, value) VALUES ($1, $2, $3) ON CONFLICT (workspace_id, key) DO UPDATE SET value = EXCLUDED.value, revision = workspace_settings.revision + 1")
            .bind(r.workspace).bind(&r.key).bind(&value).execute(tx.conn()).await.map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }
    .await;
    match res {
        Ok(()) => {
            // Bypasses the service, so drop cached effective values here (CFG-004).
            super::settings::invalidate(&s, r.workspace).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => problem(Problem::new(500, "internal", "Internal error").with_detail(e)),
    }
}
