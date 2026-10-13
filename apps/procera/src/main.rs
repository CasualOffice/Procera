//! `procera` binary entry point.

use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

mod announcement_ack;
mod audit_exports;
mod bus;
mod http;
mod reack;
mod retention;

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
        /// Reverse proxies in front of the server that append to X-Forwarded-For (0 = ignore the header).
        #[arg(long, env = "PROCERA_TRUSTED_PROXY_HOPS", default_value_t = 1)]
        trusted_proxy_hops: usize,
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
        /// SMTP relay, e.g. smtp://user:pass@host:587?tls=required or smtps://host:465. Wins over the
        /// email settings saved in the system console; with neither, emails stay queued.
        #[arg(long, env = "PROCERA_SMTP_URL", hide_env_values = true)]
        smtp_url: Option<String>,
        /// Sender address used with PROCERA_SMTP_URL (the console's settings carry their own).
        #[arg(long, env = "PROCERA_MAIL_FROM", default_value = "Procera <no-reply@localhost>")]
        mail_from: String,
    },
    /// Print a new random master key (base64) for PROCERA_MASTER_KEY.
    Keygen,
    /// Exit 0 if the server answers `GET /healthz` with 200, else 1. For container health checks
    /// (the runtime image has no shell or curl).
    Healthcheck {
        /// Address of the running server.
        #[arg(long, env = "PROCERA_BIND", default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
    },
    /// Apply database migrations. Run with the `procera_migrator` role, never the app role.
    Migrate {
        #[arg(long, env = "PROCERA_MIGRATOR_URL")]
        migrator_url: String,
    },
    /// Re-seal every value sealed under the master key with a new one, in one transaction. Stop the
    /// server first; afterwards set PROCERA_MASTER_KEY to the new key and start it again.
    RotateMasterKey {
        /// PostgreSQL URL for the runtime role `procera_app`.
        #[arg(long, env = "PROCERA_DATABASE_URL", hide_env_values = true)]
        database_url: String,
        /// The key the server runs with today.
        #[arg(long, env = "PROCERA_MASTER_KEY", hide_env_values = true)]
        master_key: String,
        /// The new key (`procera keygen`). Prefer the environment variable: a flag shows in the process list.
        #[arg(long = "new-key", env = "PROCERA_NEW_MASTER_KEY", hide_env_values = true)]
        new_key: String,
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
        Command::Healthcheck { bind } => {
            // Probe over loopback when bound to every interface.
            let addr = if bind.ip().is_unspecified() {
                SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), bind.port())
            } else {
                bind
            };
            if !healthcheck(addr).await {
                std::process::exit(1);
            }
        }
        Command::Migrate { migrator_url } => {
            procera_db::migrate(&migrator_url).await?;
            tracing::info!("migrations applied");
        }
        Command::RotateMasterKey { database_url, master_key, new_key } => {
            rotate_master_key(&database_url, &master_key, &new_key).await?;
        }
        Command::Serve {
            bind,
            all,
            cache_url,
            database_url,
            master_key,
            insecure_cookies,
            trusted_proxy_hops,
            login_rate_per_min,
            public_url,
            webauthn_rp_id,
            smtp_url,
            mail_from,
        } => {
            // An empty variable (e.g. `PROCERA_SMTP_URL=` in a compose file) means "not set".
            let unset_empty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
            let (cache_url, database_url, master_key) =
                (unset_empty(cache_url), unset_empty(database_url), unset_empty(master_key));
            let (webauthn_rp_id, smtp_url) = (unset_empty(webauthn_rp_id), unset_empty(smtp_url));
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
            let _ = http::auth::TRUSTED_PROXY_HOPS.set(trusted_proxy_hops);
            if insecure_cookies {
                tracing::warn!("insecure cookies enabled: local HTTP development / E2E only");
            }
            let mut state = base
                .with_identity(identity, !insecure_cookies)
                .with_services(keys, &public_url)
                .map_err(|e| anyhow::anyhow!("services: {e}"))?
                .with_requests()
                .map_err(|e| anyhow::anyhow!("built-in request types: {e}"))?;
            state.login_rate_per_min = login_rate_per_min;
            state.platform_info.public_url_source = if std::env::var("PROCERA_PUBLIC_URL").is_ok() {
                "environment"
            } else if public_url != "http://127.0.0.1:8080" {
                "command_line"
            } else {
                "default"
            };
            if let Some(url) = smtp_url.as_deref() {
                state = state.with_smtp_env(url, &mail_from)?;
            }
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
            // Email delivery (worker role): the transport is re-resolved on every pass, so settings saved
            // in the system console apply without a restart (BPMS-065). E2E builds deliver only through
            // PROCERA_SMTP_URL, so the shared queue that the packs read stays put (docs/23).
            let deliver = cfg!(not(feature = "e2e")) || smtp_url.is_some();
            if smtp_url.is_none() {
                tracing::info!(
                    "no PROCERA_SMTP_URL: using the email settings from the system console, if any"
                );
            }
            let mail_worker = match (&state.notify, &state.mailers, all && deliver) {
                (Some(n), Some(m), true) => {
                    let (n, m) = (n.clone(), m.clone());
                    let mut rx = stop_rx.clone();
                    tracing::info!("email sender started");
                    Some(tokio::spawn(async move {
                        let mut warned = false;
                        loop {
                            match m.current().await {
                                Ok(Some(mailer)) => {
                                    warned = false;
                                    if let Err(e) = n.send_due(&mailer).await {
                                        tracing::error!(error = %e, "email send pass failed");
                                    }
                                }
                                Ok(None) if !warned => {
                                    warned = true;
                                    tracing::warn!(
                                        "email delivery isn't set up: emails are queued but not sent"
                                    );
                                }
                                Ok(None) => {}
                                Err(e) => tracing::error!(error = %e, "email settings could not be read"),
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
                    let retention = state.retention.clone();
                    let keys = state.keys.clone();
                    let audit_exports = state.audit_exports.clone();
                    // Retention runs at most hourly (BPMS-066).
                    let mut last_retention: Option<std::time::Instant> = None;
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
                            if let Some(r) = retention.as_ref()
                                && last_retention
                                    .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(3600))
                            {
                                last_retention = Some(std::time::Instant::now());
                                match r.run().await {
                                    Ok(0) => {}
                                    Ok(n) => tracing::info!(removed = n, "retention removed records"),
                                    Err(e) => tracing::error!(error = %e, "retention pass failed"),
                                }
                            }
                            if let Some(x) = audit_exports.as_ref() {
                                match x.run_due().await {
                                    Ok(0) => {}
                                    Ok(n) => tracing::info!(jobs = n, "audit exports finished"),
                                    Err(e) => tracing::error!(error = %e, "audit export pass failed"),
                                }
                            }
                            if let Some(k) = keys.as_ref() {
                                match k.rewrap_due(500).await {
                                    Ok(0) => {}
                                    Ok(n) => {
                                        tracing::info!(rewrapped = n, "values rewrapped to the active key")
                                    }
                                    Err(e) => tracing::error!(error = %e, "key rewrap pass failed"),
                                }
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
            // Runtime scheduler (docs/04 §4.4): due timers and jobs every second. The lease owner names
            // this process so operators can tell workers apart.
            let runtime = match (&state.requests, all) {
                (Some(r), true) => {
                    let owner = format!("procera-{}", std::process::id());
                    let sched = procera_engine::Scheduler::new(r.engine().clone(), owner);
                    let mut rx = stop_rx.clone();
                    tracing::info!("runtime scheduler started");
                    Some(tokio::spawn(async move {
                        loop {
                            match sched.run_once().await {
                                Ok(p) if p.progressed() => tracing::debug!(
                                    timers = p.timers_fired,
                                    jobs = p.jobs_done,
                                    "runtime pass"
                                ),
                                Ok(_) => {}
                                Err(e) => tracing::error!(error = %e, "runtime scheduler pass failed"),
                            }
                            tokio::select! {
                                _ = rx.wait_for(|stop| *stop) => return,
                                () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                            }
                        }
                    }))
                }
                _ => None,
            };
            axum::serve(listener, http::router(state)).with_graceful_shutdown(shutdown_signal()).await?;
            let _ = stop_tx.send(true);
            for w in [worker, mail_worker, scheduler, runtime].into_iter().flatten() {
                let _ = w.await;
            }
        }
    }
    Ok(())
}

/// Stops on Ctrl-C (SIGINT) or SIGTERM, which `docker stop` and Kubernetes send.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "failed to listen for shutdown signal");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
    tracing::info!("shutting down");
}

async fn healthcheck(addr: SocketAddr) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        stream.write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n").await.ok()?;
        let mut head = [0u8; 12];
        stream.read_exact(&mut head).await.ok()?;
        Some(head.starts_with(b"HTTP/1.") && &head[9..12] == b"200")
    };
    matches!(tokio::time::timeout(std::time::Duration::from_secs(3), probe).await, Ok(Some(true)))
}

/// `procera rotate-master-key`: validates both keys (never printing them), re-seals every
/// root-sealed value in one verified transaction and says what changed and what to do next.
async fn rotate_master_key(database_url: &str, current: &str, new: &str) -> anyhow::Result<()> {
    use procera_crypto::LocalKey;
    let parse = |v: &str, what: &str| {
        LocalKey::from_base64(v).map_err(|_| {
            anyhow::anyhow!("{what} is not a valid master key: it must be 32 bytes, base64 (generate one with `procera keygen`)")
        })
    };
    let current_key = parse(current, "PROCERA_MASTER_KEY")?;
    let new_key = parse(new, "the new key")?;
    if current.trim() == new.trim() {
        anyhow::bail!(
            "the new key is the same as PROCERA_MASTER_KEY; generate a new one with `procera keygen`"
        );
    }
    let db = procera_db::Db::connect(database_url, 2).await?;
    let done = procera_keys::rotate_root(
        &db,
        &current_key,
        &new_key,
        &procera_kernel::SystemClock,
        &procera_kernel::UuidV7Gen,
    )
    .await?;
    tracing::info!(values = done.total(), workspaces = done.workspaces, "master key rotated");
    let report = format!(
        "Re-sealed {total} values under the new master key in one transaction; each one was opened with the new key before the commit.\n\
         \x20 workspace keys:              {wk} (across {ws} workspaces, including the system plane)\n\
         \x20 authenticator-app secrets:   {totp}\n\
         \x20 invitation tokens:           {inv}\n\
         \x20 older email bodies:          {mail}\n\
         Files, email bodies and the SMTP password are wrapped by the workspace keys and needed no change.\n\
         \n\
         Next: set PROCERA_MASTER_KEY to the new key (in .env or your secret store), then start the server.\n\
         When it starts:\n\
         \x20 - access tokens and half-finished two-step sign-ins signed with the old key stop working; people stay\n\
         \x20   signed in and the app fetches a fresh token with its refresh cookie;\n\
         \x20 - signed file download and preview links stop working; open the file again for a new link.\n\
         The old key can't read this database any more, but backups taken before now still need it: keep it with them.\n",
        total = done.total(),
        wk = done.workspace_keys,
        ws = done.workspaces,
        totp = done.totp_secrets,
        inv = done.invitation_tokens,
        mail = done.legacy_email_bodies,
    );
    std::io::Write::write_all(&mut std::io::stdout(), report.as_bytes())?;
    Ok(())
}
