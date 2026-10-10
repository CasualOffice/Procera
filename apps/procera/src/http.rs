//! HTTP surface. Only operational endpoints exist so far; product routes are
//! added by their tracker items under `/api/v1` (docs/08).

use axum::{Json, Router, routing::get};
use serde::Serialize;

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
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
}

async fn version() -> Json<Version> {
    Json(Version { name: "procera", version: env!("CARGO_PKG_VERSION") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() -> anyhow::Result<()> {
        let res = router().oneshot(Request::get("/healthz").body(Body::empty())?).await?;
        assert_eq!(res.status(), 200);
        let body = res.into_body().collect().await?.to_bytes();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body)?["status"], "ok");
        Ok(())
    }

    #[tokio::test]
    async fn unknown_route_is_404() -> anyhow::Result<()> {
        let res = router().oneshot(Request::get("/nope").body(Body::empty())?).await?;
        assert_eq!(res.status(), 404);
        Ok(())
    }
}
