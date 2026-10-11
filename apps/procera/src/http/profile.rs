//! Your own profile (Account › Profile): display name, time zone and language, used across every
//! workspace you belong to. Changes are audited on the system chain (accounts are global).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_kernel::UuidV7Gen;
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use super::auth::{AuthUser, client_info};
use super::error::ApiError;

pub fn router() -> Router<AppState> {
    Router::new()
        // One line per route: the audit contract test reads `.route(` lines from this file.
        .route("/me/profile", get(get_profile).patch(update_profile))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "profile api internal error");
    ApiError::new(500, "internal", "Internal error")
}

fn invalid(m: &str) -> ApiError {
    ApiError::new(422, "validation_failed", "Validation failed").detail(m.to_owned())
}

/// Languages the interface offers today (more arrive with localisation, CFG-008).
const LOCALES: &[&str] = &["en"];

async fn get_profile(State(s): State<AppState>, AuthUser(c): AuthUser) -> Result<Json<Value>, ApiError> {
    let db = s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let mut tx = db.begin_system().await.map_err(internal)?;
    let row: Option<(String, String, String, String)> =
        sqlx::query_as("SELECT email::text, display_name, timezone, locale FROM users WHERE id = $1")
            .bind(c.sub)
            .fetch_optional(tx.conn())
            .await
            .map_err(internal)?;
    tx.rollback().await.map_err(internal)?;
    let (email, display_name, timezone, locale) = row.ok_or_else(ApiError::not_found)?;
    Ok(Json(
        json!({ "email": email, "display_name": display_name, "timezone": timezone, "locale": locale, "locales": LOCALES }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileReq {
    display_name: Option<String>,
    /// An IANA time zone, e.g. "Asia/Kolkata".
    timezone: Option<String>,
    locale: Option<String>,
}

async fn update_profile(
    State(s): State<AppState>,
    headers: HeaderMap,
    AuthUser(c): AuthUser,
    Json(r): Json<ProfileReq>,
) -> Result<Json<Value>, ApiError> {
    let db = s.db.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
    let name = r.display_name.as_deref().map(str::trim);
    if let Some(n) = name
        && (n.is_empty() || n.chars().count() > 200 || n.chars().any(char::is_control))
    {
        return Err(invalid("your name must be 1 to 200 characters"));
    }
    if let Some(l) = r.locale.as_deref()
        && !LOCALES.contains(&l)
    {
        return Err(invalid("that language isn't available yet"));
    }
    let mut tx = db.begin_system().await.map_err(internal)?;
    if let Some(tz) = r.timezone.as_deref() {
        let known: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)")
                .bind(tz)
                .fetch_one(tx.conn())
                .await
                .map_err(internal)?;
        if !known {
            return Err(invalid("choose a time zone from the list"));
        }
    }
    let before: Option<(String, String, String, uuid::Uuid)> = sqlx::query_as(
        "SELECT display_name, timezone, locale, subject_ref FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(c.sub)
    .fetch_optional(tx.conn())
    .await
    .map_err(internal)?;
    let (old_name, old_tz, old_locale, subject) = before.ok_or_else(ApiError::not_found)?;
    sqlx::query(
        "UPDATE users SET display_name = coalesce($2, display_name), timezone = coalesce($3, timezone), locale = coalesce($4, locale) WHERE id = $1",
    )
    .bind(c.sub)
    .bind(name)
    .bind(r.timezone.as_deref())
    .bind(r.locale.as_deref())
    .execute(tx.conn())
    .await
    .map_err(internal)?;
    let client = client_info(&headers);
    let mut ev = AuditEvent::new(
        Some(subject),
        ActorKind::User,
        Category::Auth,
        "user.profile_updated",
        Outcome::Allowed,
    );
    ev.ip = client.ip;
    ev.user_agent = client.user_agent;
    ev.diff = Some(json!({
        "display_name": {"before": old_name, "after": name.unwrap_or(&old_name)},
        "timezone": {"before": old_tz, "after": r.timezone.as_deref().unwrap_or(&old_tz)},
        "locale": {"before": old_locale, "after": r.locale.as_deref().unwrap_or(&old_locale)},
    }));
    append(&mut tx, s.clock.as_ref(), &UuidV7Gen, &ev).await.map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    get_profile(State(s), AuthUser(c)).await
}
