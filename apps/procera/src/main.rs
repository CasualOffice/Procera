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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("PROCERA_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .json()
        .init();

    match Cli::parse().command {
        Command::Serve { bind, all } => {
            tracing::info!(%bind, all, "starting procera");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            #[cfg(feature = "e2e")]
            tracing::warn!(
                "built with the `e2e` feature: test-only controls are enabled — never use in production"
            );
            axum::serve(listener, http::router(http::AppState::new()))
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
