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
    #[cfg(feature = "e2e")]
    pub manual_clock: Arc<procera_kernel::clock::ManualClock>,
}

impl AppState {
    /// Production state: system clock.
    #[cfg(not(feature = "e2e"))]
    pub fn new() -> Self {
        Self { clock: Arc::new(procera_kernel::SystemClock) }
    }

    /// E2E state: a manual clock that starts at the real time and can only move forward.
    #[cfg(feature = "e2e")]
    pub fn new() -> Self {
        let manual = Arc::new(procera_kernel::clock::ManualClock::new(time::OffsetDateTime::now_utc()));
        Self { clock: manual.clone(), manual_clock: manual }
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

/// Readiness. Will check Postgres/object store once those adapters exist.
async fn readyz() -> Json<Health> {
    Json(Health { status: "ready" })
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
        let res = router(AppState::new()).oneshot(Request::get("/healthz").body(Body::empty())?).await?;
        assert_eq!(res.status(), 200);
        let body = res.into_body().collect().await?.to_bytes();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body)?["status"], "ok");
        Ok(())
    }

    #[tokio::test]
    async fn unknown_route_is_404() -> anyhow::Result<()> {
        let res = router(AppState::new()).oneshot(Request::get("/nope").body(Body::empty())?).await?;
        assert_eq!(res.status(), 404);
        Ok(())
    }

    #[cfg(not(feature = "e2e"))]
    #[tokio::test]
    async fn test_controls_absent_without_e2e_feature() -> anyhow::Result<()> {
        let res = router(AppState::new()).oneshot(Request::get("/__test/clock").body(Body::empty())?).await?;
        assert_eq!(res.status(), 404);
        Ok(())
    }
}
