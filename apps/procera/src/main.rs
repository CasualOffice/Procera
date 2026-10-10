//! `procera` binary entry point.

use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

mod http;

#[derive(Debug, Parser)]
#[command(name = "procera", version, about = "Procera — turn processes into progress.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the server. `--all` runs api, worker and scheduler in one process (trial topology).
    Serve {
        /// Address to bind the HTTP API to.
        #[arg(long, env = "PROCERA_BIND", default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
        /// Run every role in-process (docs/03 §3).
        #[arg(long)]
        all: bool,
        /// Valkey/Redis URL for caches and rate limits; omit for the in-process cache (ADR-0019).
        #[arg(long, env = "PROCERA_CACHE_URL")]
        cache_url: Option<String>,
        /// PostgreSQL URL for the runtime role `procera_app` (ADR-0002).
        #[arg(long, env = "PROCERA_DATABASE_URL")]
        database_url: Option<String>,
        /// 32-byte base64 root key for secrets at rest and token signing (`procera keygen`). Required with a database.
        #[arg(long, env = "PROCERA_MASTER_KEY", hide_env_values = true)]
        master_key: Option<String>,
        /// Use non-Secure cookies without the __Host- prefix. ONLY for local HTTP development and E2E.
        #[arg(long, env = "PROCERA_INSECURE_COOKIES", default_value_t = false)]
        insecure_cookies: bool,
        /// Per-IP login attempts per minute (docs/06 §3.6). Raise only for test environments behind one IP.
        #[arg(long, env = "PROCERA_LOGIN_RATE_PER_MIN", default_value_t = 30)]
        login_rate_per_min: u32,
    },
    /// Print a new random master key (base64) for PROCERA_MASTER_KEY.
    Keygen,
    /// Apply database migrations. Run with the `procera_migrator` role, never the app role.
    Migrate {
        #[arg(long, env = "PROCERA_MIGRATOR_URL")]
        migrator_url: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("PROCERA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .json()
        .with_writer(std::io::stderr) // stdout is reserved for command output (e.g. `procera keygen`)
        .init();

    match Cli::parse().command {
        Command::Keygen => {
            let key = procera_crypto::LocalKey::generate_base64();
            tracing::info!(
                "generated a new master key; store it in your secret manager as PROCERA_MASTER_KEY"
            );
            std::io::Write::write_all(&mut std::io::stdout(), format!("{key}\n").as_bytes())?;
        }
        Command::Migrate { migrator_url } => {
            procera_db::migrate(&migrator_url).await?;
            tracing::info!("migrations applied");
        }
        Command::Serve {
            bind,
            all,
            cache_url,
            database_url,
            master_key,
            insecure_cookies,
            login_rate_per_min,
        } => {
            let db = match database_url.as_deref() {
                Some(url) => {
                    let db = procera_db::Db::connect(url, 20).await?;
                    // Refuse to serve if any workspace table lacks forced RLS (docs/03 §7 layer 3).
                    db.assert_rls_guard().await?;
                    let created = db.ensure_audit_partitions(2).await?;
                    tracing::info!(created, "database ready; RLS guard clean");
                    Some(db)
                }
                None => {
                    tracing::warn!(
                        "no PROCERA_DATABASE_URL: running without a database (trial smoke mode only)"
                    );
                    None
                }
            };
            tracing::info!(%bind, all, "starting procera");
            let cache = match procera_cache::Cache::from_url(cache_url.as_deref()).await {
                Ok(c) => c,
                Err(err) => {
                    // Advisory cache: never block startup on it (ADR-0019, docs/03 §8).
                    tracing::warn!(%err, "cache unavailable; falling back to the in-process cache");
                    procera_cache::Cache::Memory(procera_cache::MemoryCache::default())
                }
            };
            tracing::info!(backend = cache.backend(), "cache ready");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            #[cfg(feature = "e2e")]
            tracing::warn!(
                "built with the `e2e` feature: test-only controls are enabled — never use in production"
            );
            // One state, one clock: identity, platform and test controls must share it (E2E time travel).
            let base = http::AppState::new(cache).with_db(db.clone());
            let identity = match (&db, master_key.as_deref()) {
                (Some(db), Some(k)) => {
                    let keys = std::sync::Arc::new(procera_crypto::LocalKey::from_base64(k)?);
                    Some(procera_identity::Identity::new(
                        db.clone(),
                        keys,
                        base.clock.clone(),
                        std::sync::Arc::new(procera_kernel::UuidV7Gen),
                        procera_identity::IdentityConfig::default(),
                    )?)
                }
                (Some(_), None) => anyhow::bail!(
                    "PROCERA_MASTER_KEY is required when a database is configured (generate one with `procera keygen`)"
                ),
                _ => None,
            };
            if insecure_cookies {
                tracing::warn!("insecure cookies enabled: local HTTP development / E2E only");
            }
            let mut state = base.with_identity(identity, !insecure_cookies);
            state.login_rate_per_min = login_rate_per_min;
            // FR-S01: until a system owner exists, print a one-time bootstrap token (24 h) to the log.
            if let Some(p) = &state.platform
                && let Some(token) = p.issue_bootstrap_token().await?
            {
                tracing::warn!(bootstrap_token = %token, "platform not bootstrapped: POST /sysapi/v1/bootstrap with this one-time token (valid 24 h)");
            }
            axum::serve(listener, http::router(state)).with_graceful_shutdown(shutdown_signal()).await?;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(%err, "failed to listen for shutdown signal");
    }
}
