//! Email-actionable approvals (BPMS-145, FR-N03): the confirm page's two calls. The token in the
//! path is the credential (single use, at most seven days, until the step closes), so there is no
//! session: `GET` only reads what the page shows, `POST` decides once, as the member the link was
//! sent to. Tokens are 256-bit random values (only their SHA-256 is stored), so they can't be guessed.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::AppState;
use super::error::ApiError;
use super::requests::requests;

// One route per line: the audit contract test scans these lines.
#[rustfmt::skip]
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/email-actions/{token}", get(view).post(decide))
}

/// `GET /email-actions/{token}`: what the confirm page shows (no session; the link is the key):
/// `{state: open|used|expired|closed, deciding_as, workspace, step, outcomes, request {display_id,
/// title, type_name, initiator, submitted_at, due_at}, answers [{label, type, value}], expires_at}`.
/// Answers marked confidential or restricted are left out. 404 for an unknown link.
async fn view(State(s): State<AppState>, Path(token): Path<String>) -> Result<Response, ApiError> {
    let v = requests(&s)?.email_action(&token).await?;
    Ok(Json(v).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideReq {
    outcome: String,
    #[serde(default)]
    comment: Option<String>,
}

/// `POST /email-actions/{token}`: decides once through the link: `outcome` `approved`, or `rejected`
/// with a `comment`. Candidacy, separation of duties and "one decision wins" apply as in My work.
/// 409 when the link was used or expired or the step is already decided. Returns
/// `{outcome, display_id, status}`.
async fn decide(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
    Json(r): Json<DecideReq>,
) -> Result<Response, ApiError> {
    let c = super::auth::client_info(&headers);
    let v = requests(&s)?
        .email_action_decide(&token, &r.outcome, r.comment.as_deref(), c.ip, c.user_agent)
        .await?;
    Ok(Json(v).into_response())
}
