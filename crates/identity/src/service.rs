//! Identity service: users, login, MFA, refresh rotation, revocation (BPMS-021/022, ADR-0020).
//! Every security-relevant action is audited in the system chain within the same transaction.

use std::sync::Arc;
use std::time::Duration;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{KeyProvider, hash_token, open, random_token, seal};
use procera_db::{Db, DbError, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::jwt::{AUDIENCE, Claims, ISSUER, JwtKeys};
use crate::password::{self, HashCost, PolicyError};
use crate::totp;
use crate::webauthn::{CredentialJson, RelyingParty};

mod account_admin;
mod passkeys;
mod recovery;
pub use passkeys::Passkey;
pub use recovery::{AccountNotice, CHANGE_TOKEN_TTL, RESET_TTL, RESETS_PER_HOUR, ResetLink};

const SECRETS: &str = "secrets";
const MAX_FAILURES_BEFORE_BACKOFF: i32 = 5;
const MAX_BACKOFF_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct IdentityConfig {
    pub cost: HashCost,
    pub min_password_len: usize,
    pub access_ttl: Duration,
    pub mfa_ttl: Duration,
    pub refresh_ttl: Duration,
    pub family_ttl: Duration,
    /// WebAuthn relying party (RP ID and allowed origins), derived from the public URL.
    pub relying_party: RelyingParty,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            cost: HashCost::PRODUCTION,
            min_password_len: 12,
            access_ttl: Duration::from_secs(10 * 60),
            mfa_ttl: Duration::from_secs(5 * 60),
            refresh_ttl: Duration::from_secs(12 * 3600),
            family_ttl: Duration::from_secs(30 * 86400),
            relying_party: RelyingParty::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("too many attempts; retry after {retry_after_secs}s")]
    Throttled { retry_after_secs: i64 },
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error("an account with this email already exists")]
    EmailTaken,
    #[error("invalid or expired code")]
    InvalidCode,
    #[error("invalid or expired token")]
    InvalidToken,
    #[error("refresh token reuse detected; all sessions of this sign-in were revoked")]
    RefreshReused,
    #[error("no pending MFA enrolment")]
    NoPendingEnrolment,
    /// A passkey assertion (sign-in or step-up) did not verify. The reason is safe to show.
    #[error("passkey rejected: {0}")]
    PasskeyRejected(String),
    /// A passkey registration response did not verify.
    #[error("passkey registration rejected: {0}")]
    PasskeyInvalid(String),
    #[error("this passkey is already registered")]
    PasskeyExists,
    #[error("not found")]
    NotFound,
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for IdentityError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for IdentityError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for IdentityError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_crypto::CryptoError> for IdentityError {
    fn from(e: procera_crypto::CryptoError) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub status: String,
    #[serde(skip)]
    pub subject_ref: Uuid,
    #[serde(skip)]
    pub token_version: i32,
    pub mfa_enabled: bool,
    /// Enrolled second factors: `totp`, `webauthn`.
    pub mfa_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tokens {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    /// Delivered to browsers as an HttpOnly cookie, never exposed to scripts.
    #[serde(skip)]
    pub refresh_token: String,
    pub user: User,
}

#[derive(Debug)]
pub enum LoginOutcome {
    Authenticated(Box<Tokens>),
    /// `methods`: what the account can answer with (`totp`, `webauthn`, `recovery_code`).
    MfaRequired {
        mfa_token: String,
        methods: Vec<&'static str>,
    },
    /// An admin required a new password (docs/06 §3.6.1): set it with the short-lived, single-use
    /// `change_token` through [`Identity::complete_expired_change`].
    PasswordChangeRequired {
        change_token: String,
    },
}

/// A second factor presented at sign-in or step-up.
#[derive(Debug, Clone, Copy)]
pub enum Factor<'a> {
    /// A TOTP code (or, at sign-in only, a recovery code).
    Code(&'a str),
    Passkey(&'a CredentialJson),
    /// Step-up for accounts without MFA.
    Password(&'a str),
}

#[derive(Debug, Clone, Serialize)]
pub struct TotpEnrollment {
    pub secret: String,
    pub otpauth_uri: String,
}

/// Request metadata recorded with security events.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Identity {
    db: Db,
    keys: Arc<dyn KeyProvider>,
    jwt: JwtKeys,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    cfg: IdentityConfig,
}

type UserRow = (Uuid, String, String, String, Uuid, i32);

impl Identity {
    pub fn new(
        db: Db,
        keys: Arc<dyn KeyProvider>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        cfg: IdentityConfig,
    ) -> Result<Self, IdentityError> {
        let secret = keys.key("jwt-access-v1")?;
        Ok(Self { db, jwt: JwtKeys::from_secret(secret.as_ref()), keys, clock, ids, cfg })
    }

    fn now(&self) -> OffsetDateTime {
        self.clock.now()
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        action: &str,
        result: Outcome,
        client: &ClientInfo,
        reason: Option<&str>,
    ) -> Result<(), IdentityError> {
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Auth,
            action,
            result,
        );
        ev.ip = client.ip.clone();
        ev.user_agent = client.user_agent.clone();
        ev.reason = reason.map(str::to_owned);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    async fn load_user(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Option<User>, IdentityError> {
        let row: Option<UserRow> = sqlx::query_as("SELECT id, email::text, display_name, status, subject_ref, token_version FROM users WHERE id = $1")
            .bind(id).fetch_optional(tx.conn()).await?;
        let Some((id, email, display_name, status, subject_ref, token_version)) = row else {
            return Ok(None);
        };
        let methods: Vec<String> = sqlx::query_scalar("SELECT DISTINCT kind FROM user_credentials WHERE user_id = $1 AND kind IN ('totp', 'webauthn') AND confirmed ORDER BY kind")
            .bind(id).fetch_all(tx.conn()).await?;
        Ok(Some(User {
            id,
            email,
            display_name,
            status,
            subject_ref,
            token_version,
            mfa_enabled: !methods.is_empty(),
            mfa_methods: methods,
        }))
    }

    /// Whether `create_user` would accept this email and password, without creating anything (so a
    /// one-time bootstrap token is not spent on an account that would be refused).
    pub async fn check_new_account(&self, email: &str, password: &str) -> Result<(), IdentityError> {
        let email = email.trim().to_lowercase();
        password::check_policy(password, &email, self.cfg.min_password_len)?;
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = $1)")
            .bind(&email)
            .fetch_one(self.db.pool())
            .await
            .map_err(|e| IdentityError::Internal(e.to_string()))?;
        if taken { Err(IdentityError::EmailTaken) } else { Ok(()) }
    }

    /// Creates a user with a password (bootstrap, invites, admin creation).
    pub async fn create_user(
        &self,
        email: &str,
        display_name: &str,
        password: &str,
        client: &ClientInfo,
    ) -> Result<User, IdentityError> {
        let email = email.trim().to_lowercase();
        password::check_policy(password, &email, self.cfg.min_password_len)?;
        let phc =
            password::hash(password, self.cfg.cost).map_err(|e| IdentityError::Internal(e.to_string()))?;
        let mut tx = self.db.begin_system().await?;
        let id = self.ids.new_id();
        let subject = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4) ON CONFLICT (email) DO NOTHING")
            .bind(id).bind(&email).bind(display_name.trim()).bind(subject).execute(tx.conn()).await?.rows_affected();
        if inserted == 0 {
            return Err(IdentityError::EmailTaken);
        }
        sqlx::query(
            "INSERT INTO user_credentials (id, user_id, kind, secret) VALUES ($1, $2, 'password', $3)",
        )
        .bind(self.ids.new_id())
        .bind(id)
        .bind(phc.as_bytes())
        .execute(tx.conn())
        .await?;
        self.audit(&mut tx, Some(subject), "user.created", Outcome::Allowed, client, None).await?;
        let user = self.load_user(&mut tx, id).await?.ok_or(IdentityError::InvalidCredentials)?;
        tx.commit().await?;
        Ok(user)
    }

    async fn throttled(&self, tx: &mut WsTx<'_>, keys: &[String]) -> Result<Option<i64>, IdentityError> {
        let until: Option<OffsetDateTime> =
            sqlx::query_scalar("SELECT max(blocked_until) FROM auth_throttle WHERE key = ANY($1)")
                .bind(keys)
                .fetch_one(tx.conn())
                .await?;
        Ok(until.filter(|u| *u > self.now()).map(|u| (u - self.now()).whole_seconds().max(1)))
    }

    async fn record_failure(&self, keys: &[String]) -> Result<(), IdentityError> {
        // Separate transaction: failures must be recorded even though the login transaction rolls back.
        let mut tx = self.db.begin_system().await?;
        for k in keys {
            let failures: i32 = sqlx::query_scalar(
                "INSERT INTO auth_throttle (key, failures, updated_at) VALUES ($1, 1, $2)
                 ON CONFLICT (key) DO UPDATE SET failures = auth_throttle.failures + 1, updated_at = $2 RETURNING failures",
            ).bind(k).bind(self.now()).fetch_one(tx.conn()).await?;
            if failures >= MAX_FAILURES_BEFORE_BACKOFF {
                let secs = 2i64
                    .saturating_pow(u32::try_from(failures - MAX_FAILURES_BEFORE_BACKOFF).unwrap_or(30))
                    .min(MAX_BACKOFF_SECS);
                sqlx::query("UPDATE auth_throttle SET blocked_until = $2 WHERE key = $1")
                    .bind(k)
                    .bind(self.now() + time::Duration::seconds(secs))
                    .execute(tx.conn())
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn clear_failures(&self, tx: &mut WsTx<'_>, keys: &[String]) -> Result<(), IdentityError> {
        sqlx::query("DELETE FROM auth_throttle WHERE key = ANY($1)").bind(keys).execute(tx.conn()).await?;
        Ok(())
    }

    fn throttle_keys(email: &str, client: &ClientInfo) -> Vec<String> {
        let mut k = vec![format!("email:{}", email.trim().to_lowercase())];
        if let Some(ip) = &client.ip {
            k.push(format!("ip:{ip}"));
        }
        k
    }

    /// Password sign-in. Unknown users, wrong passwords and disabled accounts look identical.
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        client: &ClientInfo,
    ) -> Result<LoginOutcome, IdentityError> {
        let keys = Self::throttle_keys(email, client);
        let mut tx = self.db.begin_system().await?;
        if let Some(retry_after_secs) = self.throttled(&mut tx, &keys).await? {
            self.audit(&mut tx, None, "login.throttled", Outcome::Denied, client, None).await?;
            tx.commit().await?;
            return Err(IdentityError::Throttled { retry_after_secs });
        }
        let found: Option<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT u.id, c.secret FROM users u JOIN user_credentials c ON c.user_id = u.id AND c.kind = 'password' WHERE u.email = $1",
        ).bind(email.trim().to_lowercase()).fetch_optional(tx.conn()).await?;
        let Some((user_id, phc)) = found else {
            password::dummy_verify(password, self.cfg.cost);
            self.audit(&mut tx, None, "login.failed", Outcome::Denied, client, Some("unknown account"))
                .await?;
            tx.commit().await?;
            self.record_failure(&keys).await?;
            return Err(IdentityError::InvalidCredentials);
        };
        let phc = String::from_utf8(phc).unwrap_or_default();
        let (ok, rehash) = password::verify(password, &phc, self.cfg.cost);
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidCredentials)?;
        if !ok || user.status != "active" {
            self.audit(
                &mut tx,
                Some(user.subject_ref),
                "login.failed",
                Outcome::Denied,
                client,
                Some(if ok { "account not active" } else { "bad password" }),
            )
            .await?;
            tx.commit().await?;
            self.record_failure(&keys).await?;
            return Err(IdentityError::InvalidCredentials);
        }
        self.clear_failures(&mut tx, &keys).await?;
        if rehash && let Ok(new) = password::hash(password, self.cfg.cost) {
            sqlx::query("UPDATE user_credentials SET secret = $2 WHERE user_id = $1 AND kind = 'password'")
                .bind(user.id)
                .bind(new.as_bytes())
                .execute(tx.conn())
                .await?;
        }
        if self.must_change_password(&mut tx, user.id).await? {
            // An admin required a new password: no session until it is set (docs/06 §3.6.1).
            let change_token = self.change_token(&user)?;
            self.audit(
                &mut tx,
                Some(user.subject_ref),
                "login.password_change_required",
                Outcome::Allowed,
                client,
                None,
            )
            .await?;
            tx.commit().await?;
            return Ok(LoginOutcome::PasswordChangeRequired { change_token });
        }
        let outcome = self.after_password(&mut tx, &user, client).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    /// After a verified password: the MFA step when the account has a second factor, else a session.
    async fn after_password(
        &self,
        tx: &mut WsTx<'_>,
        user: &User,
        client: &ClientInfo,
    ) -> Result<LoginOutcome, IdentityError> {
        if user.mfa_enabled {
            let mfa_token = self.sign(
                user,
                Uuid::nil(),
                "workspace",
                None,
                &["pwd"],
                self.now(),
                "mfa",
                self.cfg.mfa_ttl,
            )?;
            self.audit(
                tx,
                Some(user.subject_ref),
                "login.password_ok_mfa_required",
                Outcome::Allowed,
                client,
                None,
            )
            .await?;
            let methods = self.mfa_methods(tx, user).await?;
            return Ok(LoginOutcome::MfaRequired { mfa_token, methods });
        }
        let tokens = self.issue(tx, user, None, "workspace", None, &["pwd"], self.now(), client).await?;
        self.audit(tx, Some(user.subject_ref), "login.succeeded", Outcome::Allowed, client, None).await?;
        Ok(LoginOutcome::Authenticated(Box::new(tokens)))
    }

    /// Second factor: TOTP code or a single-use recovery code.
    pub async fn complete_mfa(
        &self,
        mfa_token: &str,
        code: &str,
        client: &ClientInfo,
    ) -> Result<Tokens, IdentityError> {
        self.complete_mfa_with(mfa_token, Factor::Code(code), client).await
    }

    /// Second factor at sign-in: a code, or a passkey assertion (amr `hwk`).
    pub async fn complete_mfa_with(
        &self,
        mfa_token: &str,
        factor: Factor<'_>,
        client: &ClientInfo,
    ) -> Result<Tokens, IdentityError> {
        let claims = self.jwt.verify(mfa_token, "mfa").ok_or(IdentityError::InvalidToken)?;
        let keys = vec![format!("mfa:{}", claims.sub)];
        let mut tx = self.db.begin_system().await?;
        if let Some(retry_after_secs) = self.throttled(&mut tx, &keys).await? {
            return Err(IdentityError::Throttled { retry_after_secs });
        }
        let user = self.load_user(&mut tx, claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" || user.token_version != claims.tv {
            return Err(IdentityError::InvalidToken);
        }
        let (failure, method) = match factor {
            Factor::Code(code) => {
                let ok = self.check_totp(&mut tx, user.id, code).await?
                    || self.use_recovery_code(&mut tx, user.id, code).await?;
                ((!ok).then(|| "code".to_owned()), "otp")
            }
            Factor::Passkey(cred) => (self.check_passkey(&mut tx, user.id, "mfa", cred).await?, "hwk"),
            Factor::Password(_) => (Some("password is not a second factor".to_owned()), "pwd"),
        };
        if let Some(reason) = failure {
            self.audit(&mut tx, Some(user.subject_ref), "mfa.failed", Outcome::Denied, client, Some(&reason))
                .await?;
            tx.commit().await?;
            self.record_failure(&keys).await?;
            return Err(if method == "hwk" {
                IdentityError::PasskeyRejected(reason)
            } else {
                IdentityError::InvalidCode
            });
        }
        self.clear_failures(&mut tx, &keys).await?;
        let auth_time = OffsetDateTime::from_unix_timestamp(claims.auth_time).unwrap_or_else(|_| self.now());
        let tokens =
            self.issue(&mut tx, &user, None, "workspace", None, &["pwd", method], auth_time, client).await?;
        self.audit(&mut tx, Some(user.subject_ref), "login.succeeded", Outcome::Allowed, client, Some("mfa"))
            .await?;
        tx.commit().await?;
        Ok(tokens)
    }

    /// The second factors an account can answer with at sign-in.
    async fn mfa_methods(&self, tx: &mut WsTx<'_>, user: &User) -> Result<Vec<&'static str>, IdentityError> {
        let mut m: Vec<&'static str> = Vec::new();
        if user.mfa_methods.iter().any(|k| k == "totp") {
            m.push("totp");
        }
        if user.mfa_methods.iter().any(|k| k == "webauthn") {
            m.push("webauthn");
        }
        let codes: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM user_credentials WHERE user_id = $1 AND kind = 'recovery_code' AND used_at IS NULL)")
            .bind(user.id).fetch_one(tx.conn()).await?;
        if codes {
            m.push("recovery_code");
        }
        Ok(m)
    }

    async fn check_totp(&self, tx: &mut WsTx<'_>, user: Uuid, code: &str) -> Result<bool, IdentityError> {
        let row: Option<(Uuid, Vec<u8>, Option<i64>)> = sqlx::query_as(
            "SELECT id, secret, last_used_step FROM user_credentials WHERE user_id = $1 AND kind = 'totp' AND confirmed FOR UPDATE",
        ).bind(user).fetch_optional(tx.conn()).await?;
        let Some((cred, sealed, last)) = row else { return Ok(false) };
        let secret = open(self.keys.as_ref(), SECRETS, &format!("user_credentials:{cred}:totp"), &sealed)?;
        match totp::verify(&secret, code, self.now().unix_timestamp(), last) {
            Some(step) => {
                sqlx::query(
                    "UPDATE user_credentials SET last_used_step = $2, last_used_at = $3 WHERE id = $1",
                )
                .bind(cred)
                .bind(step)
                .bind(self.now())
                .execute(tx.conn())
                .await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn use_recovery_code(
        &self,
        tx: &mut WsTx<'_>,
        user: Uuid,
        code: &str,
    ) -> Result<bool, IdentityError> {
        let h = Sha256::digest(code.trim().to_lowercase().as_bytes()).to_vec();
        let n = sqlx::query("UPDATE user_credentials SET used_at = $3 WHERE user_id = $1 AND kind = 'recovery_code' AND secret = $2 AND used_at IS NULL")
            .bind(user).bind(h).bind(self.now()).execute(tx.conn()).await?.rows_affected();
        Ok(n == 1)
    }

    #[allow(clippy::too_many_arguments)]
    fn sign(
        &self,
        user: &User,
        sid: Uuid,
        plane: &str,
        ws: Option<Uuid>,
        amr: &[&str],
        auth_time: OffsetDateTime,
        typ: &str,
        ttl: Duration,
    ) -> Result<String, IdentityError> {
        let now = self.now().unix_timestamp();
        let claims = Claims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: user.id,
            sid,
            plane: plane.into(),
            ws,
            amr: amr.iter().map(|s| (*s).to_owned()).collect(),
            auth_time: auth_time.unix_timestamp(),
            tv: user.token_version,
            typ: typ.into(),
            iat: now,
            exp: now + i64::try_from(ttl.as_secs()).unwrap_or(600),
            jti: Uuid::new_v4(),
        };
        self.jwt.sign(&claims).map_err(|e| IdentityError::Internal(e.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    async fn issue(
        &self,
        tx: &mut WsTx<'_>,
        user: &User,
        family: Option<(Uuid, OffsetDateTime)>,
        plane: &str,
        ws: Option<Uuid>,
        amr: &[&str],
        auth_time: OffsetDateTime,
        client: &ClientInfo,
    ) -> Result<Tokens, IdentityError> {
        let now = self.now();
        let (family_id, family_exp) = family.unwrap_or((Uuid::new_v4(), now + self.cfg.family_ttl));
        let refresh = random_token();
        let exp = (now + self.cfg.refresh_ttl).min(family_exp);
        sqlx::query(
            "INSERT INTO refresh_tokens (token_hash, family_id, user_id, plane, active_workspace_id, auth_time, amr, ip, user_agent, expires_at, family_expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::inet, $9, $10, $11)",
        )
        .bind(hash_token(&refresh)).bind(family_id).bind(user.id).bind(plane).bind(ws).bind(auth_time)
        .bind(amr.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()).bind(&client.ip).bind(&client.user_agent).bind(exp).bind(family_exp)
        .execute(tx.conn()).await?;
        let access_token =
            self.sign(user, family_id, plane, ws, amr, auth_time, "access", self.cfg.access_ttl)?;
        Ok(Tokens {
            access_token,
            token_type: "Bearer",
            expires_in: self.cfg.access_ttl.as_secs(),
            refresh_token: refresh,
            user: user.clone(),
        })
    }

    /// Rotates a refresh token. Reusing an already-rotated token revokes the whole family (SEC-14).
    pub async fn refresh(&self, refresh_token: &str, client: &ClientInfo) -> Result<Tokens, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        type Row = (
            Uuid,
            Uuid,
            String,
            Option<Uuid>,
            OffsetDateTime,
            Vec<String>,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
            Option<OffsetDateTime>,
        );
        let row: Option<Row> = sqlx::query_as(
            "SELECT family_id, user_id, plane, active_workspace_id, auth_time, amr, expires_at, family_expires_at, used_at, revoked_at
             FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
        ).bind(hash_token(refresh_token)).fetch_optional(tx.conn()).await?;
        let Some((family, user_id, plane, ws, auth_time, amr, exp, family_exp, used, revoked)) = row else {
            return Err(IdentityError::InvalidToken);
        };
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        if used.is_some() || revoked.is_some() {
            sqlx::query(
                "UPDATE refresh_tokens SET revoked_at = coalesce(revoked_at, $2) WHERE family_id = $1",
            )
            .bind(family)
            .bind(self.now())
            .execute(tx.conn())
            .await?;
            self.audit(
                &mut tx,
                Some(user.subject_ref),
                "security.token_reuse",
                Outcome::Denied,
                client,
                Some("rotated refresh token presented again"),
            )
            .await?;
            tx.commit().await?;
            return Err(IdentityError::RefreshReused);
        }
        if exp <= self.now() || family_exp <= self.now() || user.status != "active" {
            return Err(IdentityError::InvalidToken);
        }
        sqlx::query("UPDATE refresh_tokens SET used_at = $2 WHERE token_hash = $1")
            .bind(hash_token(refresh_token))
            .bind(self.now())
            .execute(tx.conn())
            .await?;
        let amr: Vec<&str> = amr.iter().map(String::as_str).collect();
        let tokens = self
            .issue(&mut tx, &user, Some((family, family_exp)), &plane, ws, &amr, auth_time, client)
            .await?;
        tx.commit().await?;
        Ok(tokens)
    }

    /// Ends one sign-in (revokes its refresh family).
    pub async fn logout(&self, refresh_token: &str, client: &ClientInfo) -> Result<(), IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let row: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT family_id, user_id FROM refresh_tokens WHERE token_hash = $1")
                .bind(hash_token(refresh_token))
                .fetch_optional(tx.conn())
                .await?;
        if let Some((family, user_id)) = row {
            sqlx::query(
                "UPDATE refresh_tokens SET revoked_at = coalesce(revoked_at, $2) WHERE family_id = $1",
            )
            .bind(family)
            .bind(self.now())
            .execute(tx.conn())
            .await?;
            let subject = self.load_user(&mut tx, user_id).await?.map(|u| u.subject_ref);
            self.audit(&mut tx, subject, "logout", Outcome::Allowed, client, None).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// "Sign out everywhere": revokes every refresh family and invalidates outstanding access tokens.
    pub async fn revoke_all(
        &self,
        user_id: Uuid,
        reason: &str,
        client: &ClientInfo,
    ) -> Result<(), IdentityError> {
        self.revoke_all_sessions(user_id, None, reason, client).await.map(|_| ())
    }

    /// Disables an account: no new sign-ins, refresh fails, access tokens die on next check (SEC-04).
    pub async fn deactivate(&self, user_id: Uuid, client: &ClientInfo) -> Result<(), IdentityError> {
        let mut tx = self.db.begin_system().await?;
        sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        tx.commit().await?;
        self.revoke_all(user_id, "account deactivated", client).await
    }

    /// Issues an access token scoped to workspace `ws` (or the user's home view when `None`) for an
    /// existing sign-in, and records the workspace on the refresh family so refresh keeps it. The caller
    /// must have verified an active membership first.
    pub async fn issue_workspace_token(
        &self,
        claims: &Claims,
        ws: Option<Uuid>,
        client: &ClientInfo,
    ) -> Result<(String, u64), IdentityError> {
        let user = self.me(claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" || user.token_version != claims.tv {
            return Err(IdentityError::InvalidToken);
        }
        let mut tx = self.db.begin_system().await?;
        sqlx::query(
            "UPDATE refresh_tokens SET active_workspace_id = $2 WHERE family_id = $1 AND revoked_at IS NULL",
        )
        .bind(claims.sid)
        .bind(ws)
        .execute(tx.conn())
        .await?;
        let reason = ws.map(|w| format!("workspace {w}"));
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "session.workspace_selected",
            Outcome::Allowed,
            client,
            reason.as_deref(),
        )
        .await?;
        tx.commit().await?;
        let amr: Vec<&str> = claims.amr.iter().map(String::as_str).collect();
        let auth_time = OffsetDateTime::from_unix_timestamp(claims.auth_time).unwrap_or_else(|_| self.now());
        let token =
            self.sign(&user, claims.sid, &claims.plane, ws, &amr, auth_time, "access", self.cfg.access_ttl)?;
        Ok((token, self.cfg.access_ttl.as_secs()))
    }

    /// Step-up (docs/12 §11): re-verifies the signed-in user and re-issues the access token with a fresh
    /// `auth_time`. Users with MFA must give a current TOTP code or a passkey (recovery codes are for
    /// sign-in only); users without MFA give their password. Throttled like sign-in.
    pub async fn step_up(
        &self,
        claims: &Claims,
        code: Option<&str>,
        password: Option<&str>,
        client: &ClientInfo,
    ) -> Result<(String, u64), IdentityError> {
        let factor = match (code, password) {
            (Some(c), _) => Some(Factor::Code(c)),
            (None, Some(p)) => Some(Factor::Password(p)),
            (None, None) => None,
        };
        self.step_up_with(claims, factor, client).await
    }

    /// Step-up with any factor, including a passkey assertion (amr `hwk`).
    pub async fn step_up_with(
        &self,
        claims: &Claims,
        factor: Option<Factor<'_>>,
        client: &ClientInfo,
    ) -> Result<(String, u64), IdentityError> {
        let keys = vec![format!("stepup:{}", claims.sub)];
        let mut tx = self.db.begin_system().await?;
        if let Some(retry_after_secs) = self.throttled(&mut tx, &keys).await? {
            return Err(IdentityError::Throttled { retry_after_secs });
        }
        let user = self.load_user(&mut tx, claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" || user.token_version != claims.tv {
            return Err(IdentityError::InvalidToken);
        }
        let (failure, method) = match (user.mfa_enabled, factor) {
            (true, Some(Factor::Code(c))) => {
                ((!self.check_totp(&mut tx, user.id, c).await?).then(|| "code".to_owned()), "otp")
            }
            (true, Some(Factor::Passkey(cred))) => {
                (self.check_passkey(&mut tx, user.id, "step_up", cred).await?, "hwk")
            }
            (true, _) => (Some("a second factor is required".to_owned()), "otp"),
            (false, factor) => {
                let phc: Option<Vec<u8>> = sqlx::query_scalar(
                    "SELECT secret FROM user_credentials WHERE user_id = $1 AND kind = 'password'",
                )
                .bind(user.id)
                .fetch_optional(tx.conn())
                .await?;
                let ok = match (factor, phc) {
                    (Some(Factor::Password(pw)), Some(phc)) => {
                        password::verify(pw, &String::from_utf8(phc).unwrap_or_default(), self.cfg.cost).0
                    }
                    (Some(Factor::Password(pw)), None) => {
                        password::dummy_verify(pw, self.cfg.cost);
                        false
                    }
                    _ => false,
                };
                ((!ok).then(|| "password".to_owned()), "pwd")
            }
        };
        if let Some(reason) = failure {
            self.audit(
                &mut tx,
                Some(user.subject_ref),
                "session.step_up_failed",
                Outcome::Denied,
                client,
                Some(&format!("{method}: {reason}")),
            )
            .await?;
            tx.commit().await?;
            self.record_failure(&keys).await?;
            return Err(match method {
                "hwk" => IdentityError::PasskeyRejected(reason),
                "otp" => IdentityError::InvalidCode,
                _ => IdentityError::InvalidCredentials,
            });
        }
        self.clear_failures(&mut tx, &keys).await?;
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "session.stepped_up",
            Outcome::Allowed,
            client,
            Some(method),
        )
        .await?;
        tx.commit().await?;
        let mut amr: Vec<&str> = claims.amr.iter().map(String::as_str).collect();
        if !amr.contains(&method) {
            amr.push(method);
        }
        let token = self.sign(
            &user,
            claims.sid,
            &claims.plane,
            claims.ws,
            &amr,
            self.now(),
            "access",
            self.cfg.access_ttl,
        )?;
        Ok((token, self.cfg.access_ttl.as_secs()))
    }

    /// Re-signs `claims` with a different `auth_time` (E2E controls only: simulates an old sign-in).
    pub fn resign_with_auth_time(
        &self,
        claims: &Claims,
        auth_time: OffsetDateTime,
    ) -> Result<String, IdentityError> {
        let user = User {
            id: claims.sub,
            email: String::new(),
            display_name: String::new(),
            status: "active".into(),
            subject_ref: Uuid::nil(),
            token_version: claims.tv,
            mfa_enabled: false,
            mfa_methods: Vec::new(),
        };
        let amr: Vec<&str> = claims.amr.iter().map(String::as_str).collect();
        self.sign(&user, claims.sid, &claims.plane, claims.ws, &amr, auth_time, "access", self.cfg.access_ttl)
    }

    /// The WebAuthn relying party passkeys are bound to.
    pub fn relying_party(&self) -> &RelyingParty {
        &self.cfg.relying_party
    }

    /// Verifies an access token's signature and claims (no database). Pair with [`Identity::token_state`].
    pub fn verify_access(&self, token: &str) -> Option<Claims> {
        self.jwt.verify(token, "access")
    }

    /// Current `(status, token_version)` of a user, for the per-request revocation check (cached by callers).
    pub async fn token_state(&self, user_id: Uuid) -> Result<Option<(String, i32)>, IdentityError> {
        Ok(sqlx::query_as("SELECT status, token_version FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(self.db.pool())
            .await?)
    }

    pub async fn me(&self, user_id: Uuid) -> Result<Option<User>, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let u = self.load_user(&mut tx, user_id).await?;
        tx.rollback().await?;
        Ok(u)
    }

    /// Starts TOTP enrolment: stores a sealed, unconfirmed secret and returns it once for the authenticator app.
    pub async fn totp_begin(
        &self,
        user_id: Uuid,
        client: &ClientInfo,
    ) -> Result<TotpEnrollment, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        sqlx::query("DELETE FROM user_credentials WHERE user_id = $1 AND kind = 'totp' AND NOT confirmed")
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        let cred = self.ids.new_id();
        let secret = totp::new_secret();
        let sealed = seal(self.keys.as_ref(), SECRETS, &format!("user_credentials:{cred}:totp"), &secret)?;
        sqlx::query("INSERT INTO user_credentials (id, user_id, kind, secret, confirmed) VALUES ($1, $2, 'totp', $3, false)")
            .bind(cred).bind(user_id).bind(sealed).execute(tx.conn()).await?;
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "mfa.totp_enrollment_started",
            Outcome::Allowed,
            client,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(TotpEnrollment {
            secret: totp::secret_base32(&secret),
            otpauth_uri: totp::otpauth_uri(&secret, &user.email, "Procera"),
        })
    }

    /// Confirms enrolment with a first code; activates TOTP and returns 10 single-use recovery codes.
    pub async fn totp_confirm(
        &self,
        user_id: Uuid,
        code: &str,
        client: &ClientInfo,
    ) -> Result<Vec<String>, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let row: Option<(Uuid, Vec<u8>)> = sqlx::query_as("SELECT id, secret FROM user_credentials WHERE user_id = $1 AND kind = 'totp' AND NOT confirmed FOR UPDATE")
            .bind(user_id).fetch_optional(tx.conn()).await?;
        let Some((cred, sealed)) = row else { return Err(IdentityError::NoPendingEnrolment) };
        let secret = open(self.keys.as_ref(), SECRETS, &format!("user_credentials:{cred}:totp"), &sealed)?;
        let step = totp::verify(&secret, code, self.now().unix_timestamp(), None)
            .ok_or(IdentityError::InvalidCode)?;
        sqlx::query("DELETE FROM user_credentials WHERE user_id = $1 AND kind IN ('totp', 'recovery_code') AND id <> $2").bind(user_id).bind(cred).execute(tx.conn()).await?;
        sqlx::query("UPDATE user_credentials SET confirmed = true, last_used_step = $2 WHERE id = $1")
            .bind(cred)
            .bind(step)
            .execute(tx.conn())
            .await?;
        let codes = self.issue_recovery_codes(&mut tx, user_id).await?;
        let subject = self.load_user(&mut tx, user_id).await?.map(|u| u.subject_ref);
        self.audit(&mut tx, subject, "mfa.totp_enabled", Outcome::Allowed, client, None).await?;
        tx.commit().await?;
        Ok(codes)
    }

    /// Replaces the account's recovery codes with 10 new single-use codes (stored hashed).
    async fn issue_recovery_codes(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
    ) -> Result<Vec<String>, IdentityError> {
        sqlx::query("DELETE FROM user_credentials WHERE user_id = $1 AND kind = 'recovery_code'")
            .bind(user_id)
            .execute(tx.conn())
            .await?;
        let mut codes = Vec::with_capacity(10);
        for _ in 0..10 {
            let raw = data_encoding::BASE32_NOPAD.encode(&procera_crypto::random_bytes::<5>()).to_lowercase();
            let code = format!("{}-{}", &raw[..4], &raw[4..8]);
            sqlx::query("INSERT INTO user_credentials (id, user_id, kind, secret) VALUES ($1, $2, 'recovery_code', $3)")
                .bind(self.ids.new_id()).bind(user_id).bind(Sha256::digest(code.as_bytes()).to_vec()).execute(tx.conn()).await?;
            codes.push(code);
        }
        Ok(codes)
    }
}
