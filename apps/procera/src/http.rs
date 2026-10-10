//! HTTP surface. Only operational endpoints exist so far; product routes are
//! added by their tracker items under `/api/v1` (docs/08).

use axum::{Json, Router, extract::State, routing::get};
use procera_kernel::Clock;
use serde::Serialize;
use std::sync::Arc;

#[cfg(feature = "e2e")]
pub mod test_controls;

/// Shared application state. Grows as adapters (Postgres, object store, cache) arrive.
#[derive(Debug, Clone)]
pub struct AppState {
    pub clock: Arc<dyn Clock>,
    pub cache: procera_cache::Cache,
    pub db: Option<procera_db::Db>,
    #[cfg(feature = "e2e")]
    pub manual_clock: Arc<procera_kernel::clock::ManualClock>,
}

impl AppState {
    pub fn with_db(mut self, db: Option<procera_db::Db>) -> Self {
        self.db = db;
        self
    }

    /// Production state: system clock.
    #[cfg(not(feature = "e2e"))]
    pub fn new(cache: procera_cache::Cache) -> Self {
        Self { clock: Arc::new(procera_kernel::SystemClock), cache, db: None }
    }

    /// Test helper: in-process cache.
    #[cfg(all(test, not(feature = "e2e")))]
    pub fn for_tests() -> Self {
        Self::new(procera_cache::Cache::Memory(procera_cache::MemoryCache::default()))
    }

    /// E2E state: a manual clock that starts at the real time and can only move forward.
    #[cfg(feature = "e2e")]
    pub fn new(cache: procera_cache::Cache) -> Self {
        let manual = Arc::new(procera_kernel::clock::ManualClock::new(time::OffsetDateTime::now_utc()));
        Self { clock: manual.clone(), manual_clock: manual, cache, db: None }
    }

    /// Test helper: in-process cache.
    #[cfg(all(test, feature = "e2e"))]
    pub fn for_tests() -> Self {
        Self::new(procera_cache::Cache::Memory(procera_cache::MemoryCache::default()))
    }
}

pub fn router(state: AppState) -> Router {
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version));
    #[cfg(feature = "e2e")]
    let router = router.nest("/__test", test_controls::router());
    router.with_state(state)
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Debug, Serialize)]
struct Ready {
    status: &'static str,
    database: &'static str,
    cache: &'static str,
    cache_health: &'static str,
}

/// Readiness. The cache is advisory: a degraded cache reports `degraded` but stays ready
/// (ADR-0019). Postgres and object-store checks are added with their adapters.
async fn readyz(State(state): State<AppState>) -> Json<Ready> {
    let health = match state.cache.health().await {
        procera_cache::Health::Ok => "ok",
        procera_cache::Health::Degraded => "degraded",
    };
    let database = match &state.db {
        None => "none",
        Some(db) => match sqlx_ping(db).await {
            true => "ok",
            false => "unavailable",
        },
    };
    let status = if database == "unavailable" { "not_ready" } else { "ready" };
    Json(Ready { status, database, cache: state.cache.backend(), cache_health: health })
}

async fn sqlx_ping(db: &procera_db::Db) -> bool {
    db.ping().await
}

#[derive(Debug, Serialize)]
struct Version {
    name: &'static str,
    version: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    server_time: time::OffsetDateTime,
}

async fn version(State(state): State<AppState>) -> Json<Version> {
    Json(Version { name: "procera", version: env!("CARGO_PKG_VERSION"), server_time: state.clock.now() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() -> anyhow::Result<()> {
        let res =
            router(AppState::for_tests()).oneshot(Request::get("/healthz").body(Body::empty())?).await?;
        assert_eq!(res.status(), 200);
        let body = res.into_body().collect().await?.to_bytes();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body)?["status"], "ok");
        Ok(())
    }

    #[tokio::test]
    async fn readyz_reports_cache_backend() -> anyhow::Result<()> {
        let res = router(AppState::for_tests()).oneshot(Request::get("/readyz").body(Body::empty())?).await?;
        let body = res.into_body().collect().await?.to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(v["cache"], "memory");
        assert_eq!(v["cache_health"], "ok");
        Ok(())
    }

    #[tokio::test]
    async fn unknown_route_is_404() -> anyhow::Result<()> {
        let res = router(AppState::for_tests()).oneshot(Request::get("/nope").body(Body::empty())?).await?;
        assert_eq!(res.status(), 404);
        Ok(())
    }

    #[cfg(not(feature = "e2e"))]
    #[tokio::test]
    async fn test_controls_absent_without_e2e_feature() -> anyhow::Result<()> {
        let res =
            router(AppState::for_tests()).oneshot(Request::get("/__test/clock").body(Body::empty())?).await?;
        assert_eq!(res.status(), 404);
        Ok(())
    }
}
