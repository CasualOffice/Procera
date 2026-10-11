//! Retention API (BPMS-066, docs/14 §4): the classes, what each covers, its period and job, the
//! drafts due in the next 30 days and the last run. Periods are changed through
//! `PUT /settings/{key}` (reason, step-up, floors), so this module is read-only.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use procera_access::Resource;
use serde_json::{Value, json};
use time::Duration;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/retention", get(overview))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "retention api internal error");
    ApiError::new(500, "internal", "Internal error")
}

async fn overview(State(s): State<AppState>, me: WsMember) -> Result<Json<Value>, ApiError> {
    let r = s.retention.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    me.require(m, &mut tx, "workspace:settings.view", Resource::Workspace).await?;
    let p = r.periods(&mut tx).await.map_err(internal)?;
    let due = r.due_drafts(&mut tx, &p, Duration::days(30)).await.map_err(internal)?;
    let last = r.last_run(&mut tx).await.map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let fmt = |t: time::OffsetDateTime| {
        t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
    };
    Ok(Json(json!({
        "classes": [
            {"class": "draft", "label": "Drafts", "covers": "Unpublished document versions", "mode": "automatic",
             "keep_days": p.draft_inactive_after.whole_days(), "measured_from": "the last edit", "setting": "retention.draft.inactive_after",
             "notice_days": p.draft_notice_before.whole_days(), "notice_setting": "retention.draft.notice_before", "due_30d": due.len()},
            {"class": "operational", "label": "Delivery logs", "covers": "Sent emails, read notifications and sent digests", "mode": "automatic",
             "keep_days": p.operational_keep_for.whole_days(), "measured_from": "creation", "setting": "retention.operational.keep_for"},
            {"class": "governance_record", "label": "Governance records", "covers": "Published versions, reviews, acknowledgements and their evidence", "mode": "never",
             "keep_days": p.governance_keep_for.whole_days(), "measured_from": "retirement", "setting": "retention.governance_record.keep_for"},
            {"class": "audit", "label": "Audit trail", "covers": "Every audit event and the hash chain", "mode": "never",
             "keep_days": Value::Null, "measured_from": Value::Null, "setting": Value::Null},
        ],
        "due_drafts": due,
        "last_run": last.map(|(at, purged)| json!({"at": fmt(at), "purged": purged})),
    })))
}
