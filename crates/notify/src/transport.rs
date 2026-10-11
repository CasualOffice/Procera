//! The SMTP transport in use, with hot reload (BPMS-065, CFG-006; docs/20 §3 "Email").
//!
//! Precedence: `PROCERA_SMTP_URL` (the environment) always wins; otherwise the configuration stored on
//! the system plane; otherwise nothing, and emails stay queued.
//!
//! Hot reload: [`Mailers::current`] reads the stored configuration's generation (row id + revision)
//! on every delivery pass and rebuilds the transport only when it changed. A save in any process is
//! therefore picked up by every worker within one pass, without a restart and without a message bus.

use std::sync::Arc;
use std::time::Duration;

use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::Serialize;
use uuid::Uuid;

use crate::smtp_config::{TlsMode, TransportConfig};
use crate::{Notify, NotifyError, SmtpMailer};

/// How long a test send may take before it is reported as a timeout.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the transport in use comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Environment,
    Database,
    None,
}

/// A failed test send, safe to show: a category and a plain-language message, never server output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestFailure {
    pub kind: &'static str,
    pub message: String,
}

impl SmtpMailer {
    /// Builds the transport from a stored configuration. `timeout` bounds each SMTP command.
    pub fn from_config(c: &TransportConfig, timeout: Option<Duration>) -> Result<Self, NotifyError> {
        let tls = match c.tls_mode {
            TlsMode::None => Tls::None,
            mode => {
                let params =
                    TlsParameters::new(c.host.clone()).map_err(|e| NotifyError::Smtp(e.to_string()))?;
                if mode == TlsMode::ImplicitTls { Tls::Wrapper(params) } else { Tls::Required(params) }
            }
        };
        let mut b =
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(c.host.clone()).port(c.port).tls(tls);
        if let Some((user, pass)) = &c.credentials {
            b = b.credentials(Credentials::new(user.clone(), pass.to_string()));
        }
        if timeout.is_some() {
            b = b.timeout(timeout);
        }
        Ok(Self { transport: b.build(), from: c.from.clone(), reply_to: c.reply_to.clone() })
    }

    /// `PROCERA_SMTP_URL` with a command timeout (for test sends).
    pub fn from_url_with_timeout(url: &str, from: &str, timeout: Duration) -> Result<Self, NotifyError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| NotifyError::Smtp(e.to_string()))?
            .timeout(Some(timeout))
            .build();
        let from = from
            .parse()
            .map_err(|e: lettre::address::AddressError| NotifyError::Smtp(format!("from address: {e}")))?;
        Ok(Self { transport, from, reply_to: None })
    }

    /// The sender address and name this transport uses.
    pub fn from_mailbox(&self) -> &Mailbox {
        &self.from
    }

    /// Sends one test message now and reports a safe, classified error.
    pub async fn send_test(&self, to: &str) -> Result<(), TestFailure> {
        let to: Mailbox = to.trim().parse().map_err(|_| TestFailure {
            kind: "invalid_recipient",
            message: "Enter a valid email address to send the test to.".into(),
        })?;
        let text = "This is a test email from Procera.\n\nIf you can read it, email delivery is set up correctly. \
                    A system administrator sent it from the system console.";
        let html = "<p>This is a test email from Procera.</p><p>If you can read it, email delivery is set up correctly. \
                    A system administrator sent it from the system console.</p>";
        let mut b = Message::builder().from(self.from.clone()).to(to).subject("Procera test email");
        if let Some(r) = &self.reply_to {
            b = b.reply_to(r.clone());
        }
        let msg = b
            .message_id(Some(format!("<{}@procera>", Uuid::now_v7())))
            .multipart(MultiPart::alternative_plain_html(text.to_owned(), html.to_owned()))
            .map_err(|_| TestFailure {
                kind: "message",
                message: "The test message could not be built.".into(),
            })?;
        match tokio::time::timeout(TEST_TIMEOUT + Duration::from_secs(2), self.transport.send(msg)).await {
            Err(_) => Err(timeout_failure()),
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(classify(&e)),
        }
    }
}

fn timeout_failure() -> TestFailure {
    TestFailure {
        kind: "timeout",
        message: format!(
            "The mail server didn’t answer within {} seconds. Check the host, the port and any firewall in between.",
            TEST_TIMEOUT.as_secs()
        ),
    }
}

/// Maps a transport error to a category and a message that names no secret and quotes no server text.
pub fn classify(e: &lettre::transport::smtp::Error) -> TestFailure {
    use std::error::Error as _;
    let io = {
        let mut src = e.source();
        let mut found = None;
        while let Some(s) = src {
            if let Some(io) = s.downcast_ref::<std::io::Error>() {
                found = Some(io.kind());
                break;
            }
            src = s.source();
        }
        found
    };
    let code = e.status().map(|c| c.to_string());
    if e.is_timeout() {
        return timeout_failure();
    }
    if e.is_tls() {
        return TestFailure {
            kind: "tls_failed",
            message: "The secure connection failed. Check the TLS mode (STARTTLS usually uses port 587, implicit TLS port 465) and that the server’s certificate is valid for this host name.".into(),
        };
    }
    if let Some(code) = code {
        if matches!(code.as_str(), "530" | "534" | "535" | "454") {
            return TestFailure {
                kind: "auth_failed",
                message: format!("The mail server refused the username or password ({code})."),
            };
        }
        return TestFailure {
            kind: if e.is_permanent() { "rejected" } else { "temporary_failure" },
            message: format!(
                "The mail server rejected the message ({code}). Check the sender address is allowed to send through this server."
            ),
        };
    }
    match io {
        Some(std::io::ErrorKind::ConnectionRefused) => TestFailure {
            kind: "connection_refused",
            message: "The mail server refused the connection. Check the host and port, and that the server is running.".into(),
        },
        Some(std::io::ErrorKind::TimedOut) => timeout_failure(),
        _ if e.is_client() && e.to_string().to_lowercase().contains("authentication") => TestFailure {
            kind: "auth_failed",
            message: "The mail server offers no sign-in method Procera supports. Check whether it needs a username and password.".into(),
        },
        _ => TestFailure {
            kind: "connection_failed",
            message: "Couldn’t connect to the mail server. Check the host name, the port and the TLS mode.".into(),
        },
    }
}

type Cached = Option<((Uuid, i64), Option<SmtpMailer>)>;

/// The transport in use, rebuilt when the stored configuration changes.
#[derive(Clone)]
pub struct Mailers {
    notify: Notify,
    env: Option<(SmtpMailer, String, String)>,
    cache: Arc<tokio::sync::Mutex<Cached>>,
}

impl std::fmt::Debug for Mailers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailers").field("source_env", &self.env.is_some()).finish_non_exhaustive()
    }
}

impl Mailers {
    /// `env` = (`PROCERA_SMTP_URL`, `PROCERA_MAIL_FROM`) when the URL is set.
    pub fn new(notify: Notify, env: Option<(&str, &str)>) -> Result<Self, NotifyError> {
        let env = match env {
            Some((url, from)) => Some((SmtpMailer::from_url(url, from)?, url.to_owned(), from.to_owned())),
            None => None,
        };
        Ok(Self { notify, env, cache: Arc::new(tokio::sync::Mutex::new(None)) })
    }

    pub fn notify(&self) -> &Notify {
        &self.notify
    }

    /// The `PROCERA_MAIL_FROM` sender when the environment provides the transport.
    pub fn environment_from(&self) -> Option<String> {
        self.env.as_ref().map(|(m, _, _)| m.from.to_string())
    }

    pub async fn source(&self) -> Result<Source, NotifyError> {
        if self.env.is_some() {
            return Ok(Source::Environment);
        }
        Ok(if self.notify.email_generation().await?.is_some() { Source::Database } else { Source::None })
    }

    /// The transport to deliver with now; `None` = not set up (emails stay queued). Rebuilt only when
    /// the stored configuration's generation changed since the last call.
    pub async fn current(&self) -> Result<Option<SmtpMailer>, NotifyError> {
        if let Some((m, _, _)) = &self.env {
            return Ok(Some(m.clone()));
        }
        let generation = self.notify.email_generation().await?;
        let mut cache = self.cache.lock().await;
        let Some(generation) = generation else {
            *cache = None;
            return Ok(None);
        };
        if let Some((g, m)) = cache.as_ref()
            && *g == generation
        {
            return Ok(m.clone());
        }
        let built = match self.notify.email_transport().await? {
            Some((g, c)) => {
                let m = SmtpMailer::from_config(&c, None)
                    .inspect_err(
                        |e| tracing::error!(error = %e, "stored email settings can't build a transport"),
                    )
                    .ok();
                tracing::info!(revision = g.1, "email transport loaded from the stored settings");
                (g, m)
            }
            None => {
                *cache = None;
                return Ok(None);
            }
        };
        let m = built.1.clone();
        *cache = Some(built);
        Ok(m)
    }

    /// A transport for a synchronous test send, with a short command timeout.
    pub async fn for_test(&self) -> Result<Option<SmtpMailer>, NotifyError> {
        if let Some((_, url, from)) = &self.env {
            return SmtpMailer::from_url_with_timeout(url, from, TEST_TIMEOUT).map(Some);
        }
        match self.notify.email_transport().await? {
            Some((_, c)) => SmtpMailer::from_config(&c, Some(TEST_TIMEOUT)).map(Some),
            None => Ok(None),
        }
    }
}
