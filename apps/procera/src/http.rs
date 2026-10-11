//! HTTP surface. Only operational endpoints exist so far; product routes are
//! added by their tracker items under `/api/v1` (docs/08).

use axum::{Json, Router, extract::State, routing::get};
use procera_kernel::Clock;
use serde::Serialize;
use std::sync::Arc;

pub mod announcements;
pub mod audit;
/// Route → audit contract; consumed by its unit test and the e2e audit pack only.
#[cfg(any(test, feature = "e2e"))]
pub mod audit_contract;
pub mod auth;
pub mod campaigns;
pub mod directory;
pub mod encryption;
pub mod error;
pub mod files;
pub mod library;
pub mod notifications;
pub mod overview;
pub mod probe;
pub mod profile;
pub mod recovery;
pub mod retention;
pub mod search;
pub mod settings;
pub mod system;
pub mod system_console;
pub mod templates;
#[cfg(feature = "e2e")]
pub mod test_controls;
pub mod views;
pub mod workspace;

/// Shared application state. Grows as adapters (Postgres, object store, cache) arrive.
#[derive(Debug, Clone)]
pub struct AppState {
    pub clock: Arc<dyn Clock>,
    pub cache: procera_cache::Cache,
    pub db: Option<procera_db::Db>,
    pub identity: Option<procera_identity::Identity>,
    pub platform: Option<procera_platform::Platform>,
    pub members: Option<procera_members::Members>,
    /// Outbox dispatcher (BPMS-029); runs in the worker role (`serve --all`).
    pub dispatcher: Option<Arc<procera_db::dispatch::Dispatcher<crate::bus::AppBus>>>,
    pub notify: Option<procera_notify::Notify>,
    pub library: Option<procera_library::Library>,
    pub campaigns: Option<procera_campaigns::Campaigns>,
    pub announcements: Option<procera_announcements::Announcements>,
    /// Retention jobs (BPMS-066); run by the scheduler and by `/__test/drain`.
    pub retention: Option<crate::retention::Retention>,
    /// Workspace KEKs (BPMS-030).
    pub keys: Option<procera_keys::Keys>,
    /// Files pipeline (BPMS-060).
    pub files: Option<procera_files::Files>,
    /// Background audit exports (BPMS-064).
    pub audit_exports: Option<crate::audit_exports::AuditExports>,
    /// Public base URL used in email links (`--public-url`).
    pub public_url: String,
    /// `__Host-` prefixed Secure cookies (default). Disable only for local HTTP development and E2E.
    pub secure_cookies: bool,
    /// Per-IP login attempts per minute, in front of the per-account backoff (default 30).
    pub login_rate_per_min: u32,
    #[cfg(feature = "e2e")]
    pub manual_clock: Arc<procera_kernel::clock::ManualClock>,
}

impl AppState {
    pub fn with_db(mut self, db: Option<procera_db::Db>) -> Self {
        self.db = db;
        self
    }

    pub fn with_identity(
        mut self,
        identity: Option<procera_identity::Identity>,
        secure_cookies: bool,
    ) -> Self {
        self.identity = identity;
        self.secure_cookies = secure_cookies;
        self
    }

    /// Wires members (and through it access) and the platform once the database, identity and keys exist.
    pub fn with_services(
        mut self,
        keys: Option<Arc<dyn procera_crypto::KeyProvider>>,
        public_url: &str,
    ) -> Self {
        self.public_url = public_url.trim_end_matches('/').to_owned();
        if let (Some(db), Some(id), Some(keys)) = (&self.db, &self.identity, keys) {
            let ids: Arc<dyn procera_kernel::IdGen> = Arc::new(procera_kernel::UuidV7Gen);
            let members = procera_members::Members::new(
                db.clone(),
                id.clone(),
                procera_access::Access::new(),
                keys.clone(),
                self.clock.clone(),
                ids.clone(),
            );
            self.platform = Some(procera_platform::Platform::new(
                db.clone(),
                id.clone(),
                members.clone(),
                self.clock.clone(),
                ids,
            ));
            self.keys = Some(procera_keys::Keys::new(
                db.clone(),
                keys.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            ));
            // Files: storage directory (PROCERA_FILES_DIR) and malware scanner (PROCERA_CLAMD_ADDR).
            let files_dir = std::env::var("PROCERA_FILES_DIR").unwrap_or_else(|_| {
                let d = std::env::temp_dir().join("procera-files");
                tracing::warn!(dir = %d.display(), "no PROCERA_FILES_DIR: files are stored in a temporary directory");
                d.display().to_string()
            });
            let scanner = match std::env::var("PROCERA_CLAMD_ADDR") {
                Ok(addr) if !addr.trim().is_empty() => procera_files::Scanner::Clamd(addr),
                _ => {
                    tracing::warn!(
                        "no PROCERA_CLAMD_ADDR: malware scanning detects only the EICAR test file"
                    );
                    procera_files::Scanner::EicarOnly
                }
            };
            self.files = Some(procera_files::Files::new(
                db.clone(),
                keys.clone(),
                procera_files::LocalStore::new(files_dir),
                scanner,
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            ));
            let notify = procera_notify::Notify::new(
                db.clone(),
                keys,
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            );
            let campaigns = procera_campaigns::Campaigns::new(
                db.clone(),
                members.clone(),
                notify.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
                &self.public_url,
            );
            let announcements = procera_announcements::Announcements::new(
                db.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            );
            let library = procera_library::Library::new(
                db.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            );
            let bus = crate::bus::AppBus {
                db: db.clone(),
                members: members.clone(),
                notify: notify.clone(),
                campaigns: campaigns.clone(),
                announcements: announcements.clone(),
                library: library.clone(),
                settings: procera_settings::Settings::new(
                    self.clock.clone(),
                    Arc::new(procera_kernel::UuidV7Gen),
                ),
                public_url: self.public_url.clone(),
                settings_cache: procera_settings::cache::SettingsCache::shared(&self.cache),
            };
            self.campaigns = Some(campaigns);
            self.dispatcher = Some(Arc::new(procera_db::dispatch::Dispatcher::new(db.clone(), bus)));
            self.retention = Some(crate::retention::Retention::new(
                db.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
                notify.clone(),
            ));
            if let Some(files) = &self.files {
                self.audit_exports = Some(crate::audit_exports::AuditExports {
                    db: db.clone(),
                    files: files.clone(),
                    notify: notify.clone(),
                    clock: self.clock.clone(),
                    ids: Arc::new(procera_kernel::UuidV7Gen),
                });
            }
            self.notify = Some(notify);
            self.library = Some(library);
            self.announcements = Some(announcements);
            self.members = Some(members);
        }
        self
    }

    /// Production state: system clock.
    #[cfg(not(feature = "e2e"))]
    pub fn new(cache: procera_cache::Cache) -> Self {
        Self {
            clock: Arc::new(procera_kernel::SystemClock),
            cache,
            db: None,
            identity: None,
            platform: None,
            members: None,
            dispatcher: None,
            notify: None,
            library: None,
            campaigns: None,
            announcements: None,
            retention: None,
            keys: None,
            files: None,
            audit_exports: None,
            public_url: "http://127.0.0.1:8080".into(),
            secure_cookies: true,
            login_rate_per_min: 30,
        }
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
        Self {
            clock: manual.clone(),
            manual_clock: manual,
            cache,
            db: None,
            identity: None,
            platform: None,
            members: None,
            dispatcher: None,
            notify: None,
            library: None,
            campaigns: None,
            announcements: None,
            retention: None,
            keys: None,
            files: None,
            audit_exports: None,
            public_url: "http://127.0.0.1:8080".into(),
            secure_cookies: true,
            login_rate_per_min: 30,
        }
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
        .route("/version", get(version))
        .nest("/api/v1", auth::router())
        .nest("/api/v1", recovery::router())
        .nest("/api/v1", workspace::router())
        .nest("/api/v1", settings::router())
        .nest("/api/v1", directory::router())
        .nest("/api/v1", notifications::router())
        .nest("/api/v1", library::router())
        .nest("/api/v1", campaigns::router())
        .nest("/api/v1", templates::router())
        .nest("/api/v1", announcements::router())
        .nest("/api/v1", search::router())
        .nest("/api/v1", audit::router())
        .nest("/api/v1", overview::router())
        .nest("/api/v1", retention::router())
        .nest("/api/v1", encryption::router())
        .nest("/api/v1", files::router())
        .nest("/api/v1", views::router())
        .nest("/api/v1", profile::router())
        .nest("/sysapi/v1", system::router())
        .layer(axum::middleware::from_fn_with_state(state.clone(), probe::audit_not_found))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024));
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
