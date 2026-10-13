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
pub mod delegation;
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
pub mod requests;
pub mod retention;
pub mod search;
pub mod settings;
pub mod system;
pub mod system_console;
pub mod system_email;
pub mod templates;
#[cfg(feature = "e2e")]
pub mod test_controls;
pub mod views;
pub mod work;
pub mod workspace;

/// Deployment facts the system console shows read-only (docs/20 §3): where each comes from.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlatformInfo {
    /// `environment` (PROCERA_PUBLIC_URL), `command_line` (--public-url) or `default`.
    pub public_url_source: &'static str,
    /// Local directory holding stored files.
    pub files_dir: String,
    /// `environment` (PROCERA_FILES_DIR), `temporary` (a temporary directory: lost on reboot) or
    /// `s3` (an S3-compatible bucket, PROCERA_S3_*).
    pub files_dir_source: &'static str,
    /// The clamd address (PROCERA_CLAMD_ADDR), or `None` for the built-in EICAR-only check.
    pub clamd_addr: Option<String>,
}

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
    /// Requests and approvals: built-in request types, the runtime engine and its work items
    /// (BPMS-083/085/103). Its scheduler runs in `serve --all` and in `/__test/drain`.
    pub requests: Option<procera_requests::Requests>,
    /// The SMTP transport in use: `PROCERA_SMTP_URL`, else the platform email settings (BPMS-065).
    pub mailers: Option<procera_notify::Mailers>,
    /// What the system console reports about storage, scanning and the public URL (read-only).
    pub platform_info: PlatformInfo,
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

    /// Uses `PROCERA_SMTP_URL` (with `PROCERA_MAIL_FROM`) as the transport: it wins over stored settings.
    pub fn with_smtp_env(mut self, url: &str, from: &str) -> Result<Self, procera_notify::NotifyError> {
        if let Some(n) = &self.notify {
            self.mailers = Some(procera_notify::Mailers::new(n.clone(), Some((url, from)))?);
        }
        Ok(self)
    }

    /// Wires members (and through it access) and the platform once the database, identity and keys exist.
    pub fn with_services(
        mut self,
        keys: Option<Arc<dyn procera_crypto::KeyProvider>>,
        public_url: &str,
    ) -> Result<Self, String> {
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
            // Files: storage (PROCERA_STORAGE=local|s3; PROCERA_FILES_DIR or PROCERA_S3_*) and malware
            // scanner (PROCERA_CLAMD_ADDR). A misconfigured S3 store stops the server at boot.
            let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
            let store = if env("PROCERA_STORAGE").as_deref() == Some("s3") {
                let c = procera_files::S3Config {
                    bucket: env("PROCERA_S3_BUCKET").ok_or("PROCERA_STORAGE=s3 needs PROCERA_S3_BUCKET")?,
                    region: env("PROCERA_S3_REGION"),
                    endpoint: env("PROCERA_S3_ENDPOINT"),
                    access_key_id: env("PROCERA_S3_ACCESS_KEY_ID"),
                    secret_access_key: env("PROCERA_S3_SECRET_ACCESS_KEY"),
                    allow_http: env("PROCERA_S3_ALLOW_HTTP").as_deref() == Some("true"),
                    path_style: env("PROCERA_S3_PATH_STYLE").as_deref() != Some("false"),
                    prefix: env("PROCERA_S3_PREFIX").unwrap_or_default(),
                };
                self.platform_info.files_dir_source = "s3";
                procera_files::Store::s3(&c)?
            } else {
                self.platform_info.files_dir_source =
                    if env("PROCERA_FILES_DIR").is_some() { "environment" } else { "temporary" };
                let files_dir = env("PROCERA_FILES_DIR").unwrap_or_else(|| {
                    let d = std::env::temp_dir().join("procera-files");
                    tracing::warn!(dir = %d.display(), "no PROCERA_FILES_DIR: files are stored in a temporary directory");
                    d.display().to_string()
                });
                procera_files::Store::local(files_dir)
            };
            let scanner = match std::env::var("PROCERA_CLAMD_ADDR") {
                Ok(addr) if !addr.trim().is_empty() => {
                    self.platform_info.clamd_addr = Some(addr.clone());
                    procera_files::Scanner::Clamd(addr)
                }
                _ => {
                    tracing::warn!(
                        "no PROCERA_CLAMD_ADDR: malware scanning detects only the EICAR test file"
                    );
                    procera_files::Scanner::EicarOnly
                }
            };
            self.platform_info.files_dir = store.describe();
            self.files = Some(procera_files::Files::new(
                db.clone(),
                keys.clone(),
                store,
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
            // Publishing a process compiles it and writes its release bundle (BPMS-084).
            let library = procera_library::Library::new(
                db.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            )
            .with_publish_hook(Arc::new(procera_processes::PublishProcesses::new(
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
            )));
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
            // The environment transport, if any, is attached by `with_smtp_env` (main).
            self.mailers = procera_notify::Mailers::new(notify.clone(), None).ok();
            self.notify = Some(notify);
            self.library = Some(library);
            self.announcements = Some(announcements);
            self.members = Some(members);
        }
        Ok(self)
    }

    /// Compiles the built-in request types and wires the runtime engine. A built-in that does not
    /// compile is fatal: the server refuses to start rather than offer a broken request type.
    pub fn with_requests(mut self) -> Result<Self, String> {
        if let (Some(db), Some(members), Some(notify)) = (&self.db, &self.members, &self.notify) {
            self.requests = Some(procera_requests::Requests::new(
                db.clone(),
                members.clone(),
                notify.clone(),
                self.clock.clone(),
                Arc::new(procera_kernel::UuidV7Gen),
                &self.public_url,
            )?);
        }
        Ok(self)
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
            requests: None,
            mailers: None,
            platform_info: PlatformInfo::default(),
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
            requests: None,
            mailers: None,
            platform_info: PlatformInfo::default(),
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
        .nest("/api/v1", requests::router())
        .nest("/api/v1", work::router())
        .nest("/api/v1", delegation::router())
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
