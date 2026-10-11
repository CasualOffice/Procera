//! `procera` binary entry point.

use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

mod bus;
mod http;
mod reack;

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
        /// Public base URL of the web app, used in email links.
        #[arg(long, env = "PROCERA_PUBLIC_URL", default_value = "http://127.0.0.1:8080")]
        public_url: String,
        /// WebAuthn relying party id (a registrable domain such as `example.com`). Defaults to the host
        /// of the public URL; a loopback IP maps to `localhost`, since browsers reject IP addresses.
        #[arg(long, env = "PROCERA_WEBAUTHN_RP_ID")]
        webauthn_rp_id: Option<String>,
        /// SMTP relay, e.g. smtp://user:pass@host:587?tls=required or smtps://host:465. Without it, emails stay queued.
        #[arg(long, env = "PROCERA_SMTP_URL", hide_env_values = true)]
        smtp_url: Option<String>,
        /// Sender address for platform email.
        #[arg(long, env = "PROCERA_MAIL_FROM", default_value = "Procera <no-reply@localhost>")]
        mail_from: String,
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
            public_url,
            webauthn_rp_id,
            smtp_url,
            mail_from,
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
            let keys: Option<std::sync::Arc<dyn procera_crypto::KeyProvider>> = match master_key.as_deref() {
                Some(k) => Some(std::sync::Arc::new(procera_crypto::LocalKey::from_base64(k)?)),
                None => None,
            };
            let identity = match (&db, keys.clone()) {
                (Some(db), Some(keys)) => Some(procera_identity::Identity::new(
                    db.clone(),
                    keys,
                    base.clock.clone(),
                    std::sync::Arc::new(procera_kernel::UuidV7Gen),
                    procera_identity::IdentityConfig {
                        relying_party: procera_identity::webauthn::RelyingParty::from_public_url(
                            &public_url,
                            webauthn_rp_id.as_deref(),
                        ),
                        ..procera_identity::IdentityConfig::default()
                    },
                )?),
                (Some(_), None) => anyhow::bail!(
                    "PROCERA_MASTER_KEY is required when a database is configured (generate one with `procera keygen`)"
                ),
                _ => None,
            };
            if insecure_cookies {
                tracing::warn!("insecure cookies enabled: local HTTP development / E2E only");
            }
            let mut state = base.with_identity(identity, !insecure_cookies).with_services(keys, &public_url);
            state.login_rate_per_min = login_rate_per_min;
            // FR-S01: until a system owner exists, print a one-time bootstrap token (24 h) to the log.
            if let Some(p) = &state.platform
                && let Some(token) = p.issue_bootstrap_token().await?
            {
                tracing::warn!(bootstrap_token = %token, "platform not bootstrapped: POST /sysapi/v1/bootstrap with this one-time token (valid 24 h)");
            }
            // Worker role (docs/03 §3): the outbox dispatcher runs in-process with `--all`.
            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            let worker = match (&state.dispatcher, all) {
                (Some(d), true) => {
                    let d = d.clone();
                    let mut rx = stop_rx.clone();
                    tracing::info!("outbox dispatcher started");
                    Some(tokio::spawn(async move {
                        d.run(async move {
                            let _ = rx.wait_for(|stop| *stop).await;
                        })
                        .await
                    }))
                }
                _ => None,
            };
            let mailer = match smtp_url.as_deref() {
                Some(url) => Some(procera_notify::SmtpMailer::from_url(url, &mail_from)?),
                None => {
                    tracing::warn!("no PROCERA_SMTP_URL: emails are queued but not sent");
                    None
                }
            };
            let mail_worker = match (&state.notify, mailer, all) {
                (Some(n), Some(m), true) => {
                    let n = n.clone();
                    let mut rx = stop_rx.clone();
                    tracing::info!("email sender started");
                    Some(tokio::spawn(async move {
                        loop {
                            if let Err(e) = n.send_due(&m).await {
                                tracing::error!(error = %e, "email send pass failed");
                            }
                            tokio::select! {
                                _ = rx.wait_for(|stop| *stop) => return,
                                () = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                            }
                        }
                    }))
                }
                _ => None,
            };
            // Scheduler role: publish scheduled library versions and announcements when due, flag artifacts
            // whose review date has passed, and send
            // due campaign reminders and manager escalations.
            let scheduler = match (&state.library, &state.campaigns, &state.announcements, all) {
                (Some(l), Some(c), Some(a), true) => {
                    let (l, c, a) = (l.clone(), c.clone(), a.clone());
                    let notify = state.notify.clone();
                    let mut rx = stop_rx.clone();
                    Some(tokio::spawn(async move {
                        loop {
                            match l.publish_due().await {
                                Ok(0) => {}
                                Ok(n) => tracing::info!(published = n, "scheduled versions published"),
                                Err(e) => tracing::error!(error = %e, "scheduled publication pass failed"),
                            }
                            match l.flag_overdue().await {
                                Ok(0) => {}
                                Ok(n) => tracing::info!(flagged = n, "artifacts flagged for overdue review"),
                                Err(e) => tracing::error!(error = %e, "overdue review pass failed"),
                            }
                            match c.run_due().await {
                                Ok(p) if p.reminders + p.escalations == 0 => {}
                                Ok(p) => tracing::info!(
                                    reminders = p.reminders,
                                    escalations = p.escalations,
                                    "campaign reminders sent"
                                ),
                                Err(e) => tracing::error!(error = %e, "campaign reminder pass failed"),
                            }
                            if let Some(n) = notify.as_ref() {
                                match n.send_digests().await {
                                    Ok(0) => {}
                                    Ok(k) => tracing::info!(digests = k, "notification digests queued"),
                                    Err(e) => tracing::error!(error = %e, "digest pass failed"),
                                }
                            }
                            match a.publish_due().await {
                                Ok(0) => {}
                                Ok(n) => tracing::info!(published = n, "scheduled announcements published"),
                                Err(e) => tracing::error!(error = %e, "scheduled announcement pass failed"),
                            }
                            tokio::select! {
                                _ = rx.wait_for(|stop| *stop) => return,
                                () = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                            }
                        }
                    }))
                }
                _ => None,
            };
            axum::serve(listener, http::router(state)).with_graceful_shutdown(shutdown_signal()).await?;
            let _ = stop_tx.send(true);
            for w in [worker, mail_worker, scheduler].into_iter().flatten() {
                let _ = w.await;
            }
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(%err, "failed to listen for shutdown signal");
    }
}
