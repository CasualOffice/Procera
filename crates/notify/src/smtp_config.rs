//! Platform email delivery settings (BPMS-065, CFG-006; docs/20 §3 "Email", docs/06 §4).
//!
//! - One configuration for the whole platform, kept on the system plane (`platform_email_config`,
//!   nil workspace id, forced RLS). `PROCERA_SMTP_URL` still wins when it is set ([`crate::Mailers`]).
//! - The password is envelope-sealed under the system workspace's KEK and is never returned: reads
//!   carry `password_set` only. A blank password on save keeps the stored one; clearing the username
//!   removes it.
//! - Every change appends `platform.email_changed` to the system chain with a diff that never contains
//!   the password (only whether it was set, kept, changed or removed).

use lettre::Address;
use lettre::message::Mailbox;
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsTx;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{Notify, NotifyError};

/// How the connection to the relay is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// Plain connection upgraded with STARTTLS; delivery fails if the server does not offer it.
    StarttlsRequired,
    /// TLS from the first byte (usually port 465).
    ImplicitTls,
    /// No encryption. Accepted only for a relay on this machine.
    None,
}

impl TlsMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::StarttlsRequired => "starttls_required",
            Self::ImplicitTls => "implicit_tls",
            Self::None => "none",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "implicit_tls" => Self::ImplicitTls,
            "none" => Self::None,
            _ => Self::StarttlsRequired,
        }
    }
}

/// The stored configuration as the API returns it: never the password.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub tls_mode: TlsMode,
    pub username: Option<String>,
    pub password_set: bool,
    pub from_address: String,
    pub from_name: Option<String>,
    pub reply_to: Option<String>,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A save request. `password`: blank or absent keeps the stored one.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailConfigInput {
    pub host: String,
    pub port: u16,
    pub tls_mode: TlsMode,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    pub from_address: String,
    #[serde(default)]
    pub from_name: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    pub reason: String,
}

impl std::fmt::Debug for EmailConfigInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailConfigInput")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls_mode", &self.tls_mode)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("from_address", &self.from_address)
            .finish_non_exhaustive()
    }
}

/// What the transport needs, with the opened password. Lives only in memory.
#[derive(Clone)]
pub struct TransportConfig {
    pub host: String,
    pub port: u16,
    pub tls_mode: TlsMode,
    pub credentials: Option<(String, Zeroizing<String>)>,
    pub from: Mailbox,
    pub reply_to: Option<Mailbox>,
}

impl std::fmt::Debug for TransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls_mode", &self.tls_mode)
            .finish_non_exhaustive()
    }
}

/// Whether `s` is a single email address.
pub fn is_address(s: &str) -> bool {
    s.trim().parse::<Address>().is_ok()
}

/// Hosts on which an unencrypted connection is accepted.
pub fn is_local_host(host: &str) -> bool {
    matches!(host.trim().to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn trimmed(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn no_newlines(field: &str, v: &str) -> Result<(), NotifyError> {
    if v.contains(['\r', '\n']) {
        return Err(NotifyError::Invalid(format!("{field} must be on one line")));
    }
    Ok(())
}

/// Checks a save request (the stored values are trimmed).
pub fn validate(i: &EmailConfigInput) -> Result<(), NotifyError> {
    let host = i.host.trim();
    if host.is_empty() || host.len() > 253 {
        return Err(NotifyError::Invalid("host is required (at most 253 characters)".into()));
    }
    if host.contains(|c: char| c.is_whitespace() || matches!(c, '/' | '@' | '?' | '#')) {
        return Err(NotifyError::Invalid(
            "host must be a server name or address, such as smtp.example.com".into(),
        ));
    }
    if i.port == 0 {
        return Err(NotifyError::Invalid("port must be between 1 and 65535".into()));
    }
    if i.tls_mode == TlsMode::None && !is_local_host(host) {
        return Err(NotifyError::Invalid(
            "tls_mode none is allowed only for a relay on this machine (localhost); use STARTTLS or implicit TLS"
                .into(),
        ));
    }
    let username = trimmed(&i.username);
    if username.as_ref().is_some_and(|u| u.len() > 320) {
        return Err(NotifyError::Invalid("username must be at most 320 characters".into()));
    }
    if let Some(p) = i.password.as_deref().filter(|p| !p.is_empty()) {
        if username.is_none() {
            return Err(NotifyError::Invalid("username is required with a password".into()));
        }
        if p.len() > 1024 {
            return Err(NotifyError::Invalid("password must be at most 1024 characters".into()));
        }
        no_newlines("password", p)?;
    }
    i.from_address
        .trim()
        .parse::<Address>()
        .map_err(|_| NotifyError::Invalid("from_address must be an email address".into()))?;
    if let Some(n) = trimmed(&i.from_name) {
        if n.chars().count() > 100 {
            return Err(NotifyError::Invalid("from_name must be at most 100 characters".into()));
        }
        no_newlines("from_name", &n)?;
    }
    if let Some(r) = trimmed(&i.reply_to) {
        r.parse::<Address>().map_err(|_| NotifyError::Invalid("reply_to must be an email address".into()))?;
    }
    if i.reason.trim().is_empty() || i.reason.len() > 500 {
        return Err(NotifyError::Invalid("reason is required (at most 500 characters)".into()));
    }
    Ok(())
}

type Row = (
    Uuid,
    String,
    i32,
    String,
    Option<String>,
    Option<Vec<u8>>,
    String,
    Option<String>,
    Option<String>,
    i64,
    OffsetDateTime,
);

const SELECT: &str = "SELECT id, host, port, tls_mode, username, password_sealed, from_address, from_name, reply_to, revision, updated_at FROM platform_email_config";

fn to_config(r: &Row) -> EmailConfig {
    EmailConfig {
        host: r.1.clone(),
        port: u16::try_from(r.2).unwrap_or(25),
        tls_mode: TlsMode::parse(&r.3),
        username: r.4.clone(),
        password_set: r.5.is_some(),
        from_address: r.6.clone(),
        from_name: r.7.clone(),
        reply_to: r.8.clone(),
        revision: r.9,
        updated_at: r.10,
    }
}

/// The audited view of a configuration: everything but the password.
fn audit_view(c: &EmailConfig) -> Value {
    json!({
        "host": c.host, "port": c.port, "tls_mode": c.tls_mode, "username": c.username,
        "from_address": c.from_address, "from_name": c.from_name, "reply_to": c.reply_to,
    })
}

fn aad(id: Uuid) -> String {
    format!("platform_email_config:{id}")
}

impl Notify {
    async fn email_row(&self, tx: &mut WsTx<'_>, lock: bool) -> Result<Option<Row>, NotifyError> {
        let sql = if lock { format!("{SELECT} FOR UPDATE") } else { SELECT.to_owned() };
        Ok(sqlx::query_as(&sql).fetch_optional(tx.conn()).await?)
    }

    /// The stored configuration, if any (never the password).
    pub async fn email_config(&self) -> Result<Option<EmailConfig>, NotifyError> {
        let mut tx = self.db.begin_system().await?;
        let row = self.email_row(&mut tx, false).await?;
        tx.rollback().await?;
        Ok(row.as_ref().map(to_config))
    }

    /// The stored configuration's generation (row id and revision), for hot reload.
    pub(crate) async fn email_generation(&self) -> Result<Option<(Uuid, i64)>, NotifyError> {
        let mut tx = self.db.begin_system().await?;
        let g: Option<(Uuid, i64)> = sqlx::query_as("SELECT id, revision FROM platform_email_config")
            .fetch_optional(tx.conn())
            .await?;
        tx.rollback().await?;
        Ok(g)
    }

    /// The stored configuration with its opened password, for building the transport.
    pub(crate) async fn email_transport(
        &self,
    ) -> Result<Option<((Uuid, i64), TransportConfig)>, NotifyError> {
        let mut tx = self.db.begin_system().await?;
        let Some(r) = self.email_row(&mut tx, false).await? else {
            tx.rollback().await?;
            return Ok(None);
        };
        let password = match &r.5 {
            Some(sealed) => {
                let pt = self
                    .workspace_keys
                    .open(&mut tx, &aad(r.0), sealed)
                    .await
                    .map_err(|e| NotifyError::Internal(e.to_string()))?;
                Some(Zeroizing::new(
                    String::from_utf8(pt.to_vec())
                        .map_err(|_| NotifyError::Internal("stored password is not text".into()))?,
                ))
            }
            None => None,
        };
        tx.rollback().await?;
        let c = to_config(&r);
        let address: Address =
            c.from_address.parse().map_err(|_| NotifyError::Smtp("stored from address is invalid".into()))?;
        let from = Mailbox::new(c.from_name.clone().filter(|n| !n.trim().is_empty()), address);
        let reply_to =
            c.reply_to.as_deref().and_then(|r| r.parse::<Address>().ok()).map(|a| Mailbox::new(None, a));
        let credentials = match (c.username.clone(), password) {
            (Some(u), Some(p)) => Some((u, p)),
            (Some(u), None) => Some((u, Zeroizing::new(String::new()))),
            _ => None,
        };
        Ok(Some((
            (r.0, r.9),
            TransportConfig { host: c.host, port: c.port, tls_mode: c.tls_mode, credentials, from, reply_to },
        )))
    }

    /// Creates or replaces the configuration. `expected_revision` is the If-Match value: 0 when none
    /// exists yet. Audited `platform.email_changed` on the system chain, in the same transaction.
    pub async fn set_email_config(
        &self,
        actor_user: Uuid,
        expected_revision: i64,
        input: &EmailConfigInput,
        client: (Option<String>, Option<String>),
    ) -> Result<EmailConfig, NotifyError> {
        validate(input)?;
        let mut tx = self.db.begin_system().await?;
        let before = self.email_row(&mut tx, true).await?;
        let current = before.as_ref().map_or(0, |r| r.9);
        if expected_revision != current {
            return Err(NotifyError::Stale { expected: expected_revision, current });
        }
        let id = before.as_ref().map_or_else(|| self.ids.new_id(), |r| r.0);
        let username = trimmed(&input.username);
        let new_password = input.password.as_deref().filter(|p| !p.is_empty());
        let (sealed, password_change) =
            match (&username, new_password, before.as_ref().and_then(|r| r.5.clone())) {
                (None, _, old) => (None, if old.is_some() { "removed" } else { "none" }),
                (Some(_), Some(p), old) => (
                    Some(
                        self.workspace_keys
                            .seal(&mut tx, &aad(id), p.as_bytes())
                            .await
                            .map_err(|e| NotifyError::Internal(e.to_string()))?,
                    ),
                    if old.is_some() { "changed" } else { "set" },
                ),
                (Some(_), None, Some(old)) => (Some(old), "kept"),
                (Some(_), None, None) => (None, "none"),
            };
        let now = self.clock.now();
        let row: Row = sqlx::query_as(
            "INSERT INTO platform_email_config (workspace_id, id, host, port, tls_mode, username, password_sealed, from_address, from_name, reply_to, revision, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 1, $11, $12)
             ON CONFLICT (workspace_id) DO UPDATE SET host = EXCLUDED.host, port = EXCLUDED.port, tls_mode = EXCLUDED.tls_mode,
               username = EXCLUDED.username, password_sealed = EXCLUDED.password_sealed, from_address = EXCLUDED.from_address,
               from_name = EXCLUDED.from_name, reply_to = EXCLUDED.reply_to, revision = platform_email_config.revision + 1,
               updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at
             RETURNING id, host, port, tls_mode, username, password_sealed, from_address, from_name, reply_to, revision, updated_at",
        )
        .bind(procera_db::SYSTEM_WORKSPACE)
        .bind(id)
        .bind(input.host.trim())
        .bind(i32::from(input.port))
        .bind(input.tls_mode.as_str())
        .bind(&username)
        .bind(sealed)
        .bind(input.from_address.trim())
        .bind(trimmed(&input.from_name))
        .bind(trimmed(&input.reply_to))
        .bind(actor_user)
        .bind(now)
        .fetch_one(tx.conn())
        .await?;
        let after = to_config(&row);
        let diff = json!({
            "before": before.as_ref().map(|r| audit_view(&to_config(r))),
            "after": audit_view(&after),
            "password": password_change,
        });
        self.audit_system(
            &mut tx,
            actor_user,
            "platform.email_changed",
            diff,
            Some(input.reason.trim()),
            client,
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }

    /// Records a test send on the system chain: the recipient's domain and the outcome, never the body.
    pub async fn record_email_test(
        &self,
        actor_user: Uuid,
        to: &str,
        outcome: &Value,
        client: (Option<String>, Option<String>),
    ) -> Result<(), NotifyError> {
        let domain = to.rsplit_once('@').map_or("", |(_, d)| d).to_lowercase();
        let mut tx = self.db.begin_system().await?;
        let diff = json!({"recipient_domain": domain, "result": outcome});
        self.audit_system(&mut tx, actor_user, "platform.email_tested", diff, None, client).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn audit_system(
        &self,
        tx: &mut WsTx<'_>,
        actor_user: Uuid,
        action: &str,
        diff: Value,
        reason: Option<&str>,
        client: (Option<String>, Option<String>),
    ) -> Result<(), NotifyError> {
        let subject: Option<Uuid> = sqlx::query_scalar("SELECT subject_ref FROM users WHERE id = $1")
            .bind(actor_user)
            .fetch_optional(tx.conn())
            .await?;
        let mut ev = AuditEvent::new(subject, ActorKind::User, Category::Config, action, Outcome::Allowed);
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        (ev.ip, ev.user_agent) = client;
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev)
            .await
            .map_err(|e| NotifyError::Internal(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> EmailConfigInput {
        EmailConfigInput {
            host: "smtp.example.com".into(),
            port: 587,
            tls_mode: TlsMode::StarttlsRequired,
            username: Some("mailer".into()),
            password: Some("s3cret".into()),
            from_address: "no-reply@example.com".into(),
            from_name: Some("Acme".into()),
            reply_to: None,
            reason: "set up email".into(),
        }
    }

    #[test]
    fn validation() {
        assert!(validate(&input()).is_ok());
        let bad = |f: fn(&mut EmailConfigInput)| {
            let mut i = input();
            f(&mut i);
            validate(&i).is_err()
        };
        assert!(bad(|i| i.host = " ".into()));
        assert!(bad(|i| i.host = "smtp://x".into()));
        assert!(bad(|i| i.port = 0));
        assert!(bad(|i| i.tls_mode = TlsMode::None), "plain text only to localhost");
        assert!(bad(|i| i.username = None), "a password needs a username");
        assert!(bad(|i| i.from_address = "nope".into()));
        assert!(bad(|i| i.from_name = Some("a\nBcc: x@y.z".into())));
        assert!(bad(|i| i.reply_to = Some("x".into())));
        assert!(bad(|i| i.reason = "".into()));
        let mut local = input();
        local.host = "localhost".into();
        local.tls_mode = TlsMode::None;
        assert!(validate(&local).is_ok());
    }

    #[test]
    fn debug_never_prints_the_password() {
        assert!(!format!("{:?}", input()).contains("s3cret"));
    }
}
