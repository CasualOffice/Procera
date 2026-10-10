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
    },
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
        .init();

    match Cli::parse().command {
        Command::Migrate { migrator_url } => {
            procera_db::migrate(&migrator_url).await?;
            tracing::info!("migrations applied");
        }
        Command::Serve { bind, all, cache_url, database_url } => {
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
            axum::serve(listener, http::router(http::AppState::new(cache).with_db(db)))
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(%err, "failed to listen for shutdown signal");
    }
}
