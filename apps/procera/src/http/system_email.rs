//! Email delivery and platform basics on the system plane (BPMS-065, CFG-006; docs/20 §3, docs/08 §5).
//!
//! - `GET/PUT /email`: the SMTP relay. `PROCERA_SMTP_URL` wins when set (`source: environment`, and
//!   saving is refused); otherwise the stored settings (`database`); otherwise `none` and emails stay
//!   queued. The password is write-only: reads carry `password_set`.
//! - `POST /email/test`: one synchronous test message through the transport in use, short timeout;
//!   the outcome (sent, or a safe error category) is the 200 body and is audited either way.
//! - `GET /platform`: the public URL, the sender name, storage and malware scanning, read-only.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use procera_notify::smtp_config::EmailConfigInput;
use procera_notify::{Mailers, Source};
use procera_platform::authz::SystemAction;
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use super::auth::client_info;
use super::error::ApiError;
use super::system::{SystemUser, etag, if_match, platform};

pub fn router() -> Router<AppState> {
    Router::new()
        // One route per line: the audit contract test reads these lines.
        .route("/email", get(email).put(set_email))
        .route("/email/test", post(test_email))
        .route("/platform", get(platform_basics))
}

fn mailers(s: &AppState) -> Result<&Mailers, ApiError> {
    s.mailers.as_ref().ok_or_else(|| ApiError::unavailable("database"))
}

/// The page's view: source, stored settings (never the password), the environment's sender and the queue.
async fn email_view(s: &AppState) -> Result<(Value, i64), ApiError> {
    let m = mailers(s)?;
    let source = m.source().await?;
    let config = m.notify().email_config().await?;
    let q = platform(s)?.queues().await?;
    let revision = config.as_ref().map_or(0, |c| c.revision);
    Ok((
        json!({
            "source": source,
            "config": config,
            "environment": m.environment_from().map(|from| json!({"from": from})),
            "queue": {"queued": q.mail_queued, "failed": q.mail_failed},
        }),
        revision,
    ))
}

async fn email(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfigRead)?;
    let (body, revision) = email_view(&s).await?;
    Ok(etag(Json(body).into_response(), revision))
}

async fn set_email(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Json(input): Json<EmailConfigInput>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfig)?;
    u.require_step_up(&s)?;
    let expected = if_match(&headers).ok_or_else(|| {
        ApiError::new(428, "precondition_required", "If-Match is required")
            .detail("send the revision from GET /sysapi/v1/email (\"0\" when nothing is stored yet)")
    })?;
    let m = mailers(&s)?;
    if m.source().await? == Source::Environment {
        return Err(ApiError::new(409, "set_by_environment", "Email delivery is set by the environment")
            .detail("PROCERA_SMTP_URL is set and takes precedence; change it there, or unset it to use stored settings"));
    }
    let c = client_info(&headers);
    m.notify().set_email_config(u.user_id, expected, &input, (c.ip, c.user_agent)).await?;
    let (body, revision) = email_view(&s).await?;
    Ok(etag((StatusCode::OK, Json(body)).into_response(), revision))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestReq {
    to: String,
}

async fn test_email(
    State(s): State<AppState>,
    headers: HeaderMap,
    u: SystemUser,
    Json(r): Json<TestReq>,
) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfig)?;
    let to = r.to.trim();
    if to.len() > 320 || !procera_notify::smtp_config::is_address(to) {
        return Err(ApiError::new(422, "validation_failed", "Validation failed")
            .detail("to must be an email address"));
    }
    let m = mailers(&s)?;
    let Some(mailer) = m.for_test().await.map_err(|e| {
        tracing::warn!(error = %e, "email transport could not be built for a test send");
        ApiError::new(409, "email_not_configured", "Email delivery isn't set up")
            .detail("the stored settings can't build a transport; check them and save again")
    })?
    else {
        return Err(ApiError::new(409, "email_not_configured", "Email delivery isn't set up")
            .detail("save the email settings first; until then emails stay queued"));
    };
    let source = m.source().await?;
    let outcome = match mailer.send_test(to).await {
        Ok(()) => json!({"sent": true, "source": source}),
        Err(f) => json!({"sent": false, "source": source, "error": f}),
    };
    let audited = json!({"sent": outcome["sent"], "kind": outcome["error"]["kind"]});
    let c = client_info(&headers);
    m.notify().record_email_test(u.user_id, to, &audited, (c.ip, c.user_agent)).await?;
    Ok(Json(outcome).into_response())
}

/// Read-only deployment facts: nothing here is configurable from the console yet.
async fn platform_basics(State(s): State<AppState>, u: SystemUser) -> Result<Response, ApiError> {
    u.require(SystemAction::PlatformConfigRead)?;
    let m = mailers(&s)?;
    let stored = m.notify().email_config().await?;
    let sender_name = match m.source().await? {
        Source::Environment => m.current().await?.and_then(|x| x.from_mailbox().name.clone()),
        _ => stored.and_then(|c| c.from_name),
    };
    let i = &s.platform_info;
    Ok(Json(json!({
        "public_url": s.public_url,
        "public_url_source": i.public_url_source,
        "sender_name": sender_name,
        "storage": {"kind": if i.files_dir_source == "s3" { "s3" } else { "local" }, "path": i.files_dir, "source": i.files_dir_source},
        "scanning": match &i.clamd_addr {
            Some(addr) => json!({"kind": "clamd", "address": addr}),
            None => json!({"kind": "eicar_only", "address": null}),
        },
    }))
    .into_response())
}
