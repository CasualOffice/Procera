//! OpenID Connect single sign-on, the relying party (BPMS-140, FR-P02, INT-001, docs/06 §3.3).
//!
//! - **Configuration** per workspace: the provider's issuer, a client id and secret (sealed under
//!   the workspace key), the email domains it vouches for, on/off, and whether SSO is required
//!   (enforced by the HTTP layer for everyone but owners, who keep a password as break-glass).
//! - **Start**: the provider's discovery document (cached) gives the authorization endpoint; a
//!   one-time `state` (its SHA-256 stored, prefixed with the workspace id so the callback knows
//!   where to look), a `nonce` and a PKCE S256 verifier are kept for ten minutes.
//! - **Callback**: the state is spent once; the code is exchanged (client secret, PKCE verifier);
//!   the ID token's signature is checked against the provider's JWKS (cached, refetched for an
//!   unknown key id), then `iss`, `aud` (and `azp`), `exp`/`iat` with 60 s leeway and the `nonce`.
//! - **Who it is**: an earlier sign-in links the issuer and subject to a user; otherwise a verified
//!   email in one of the workspace's domains that belongs to an active member is linked now.
//!   Nobody is created here (accounts arrive through invitations or SCIM).
//!
//! Every call to the provider goes through the egress guard (public https only, unless the
//! operator allows an internal range with `PROCERA_EGRESS_ALLOW`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use procera_db::{Db, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_keys::Keys;
use procera_webhooks::egress;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const STATE_TTL: Duration = Duration::minutes(10);
const CACHE_TTL: StdDuration = StdDuration::from_secs(600);
const LEEWAY: u64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum SsoError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    /// A sign-in that can't complete; the text is safe to show.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Internal(String),
}

type R<T> = Result<T, SsoError>;

fn internal(e: impl std::fmt::Display) -> SsoError {
    SsoError::Internal(e.to_string())
}

impl From<sqlx::Error> for SsoError {
    fn from(e: sqlx::Error) -> Self {
        internal(e)
    }
}

/// A workspace's provider, as admins see it (never the secret).
#[derive(Debug, Clone, Serialize)]
pub struct Config {
    pub issuer: String,
    pub client_id: String,
    pub has_secret: bool,
    pub domains: Vec<String>,
    pub enabled: bool,
    pub required: bool,
    pub revision: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// What an admin sets.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigInput {
    pub issuer: String,
    pub client_id: String,
    /// Absent keeps the stored secret; empty clears it (public clients with PKCE only).
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
}

fn yes() -> bool {
    true
}

/// The provider's discovery document (the parts used).
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// A verified sign-in.
#[derive(Debug, Clone)]
pub struct Verified {
    pub workspace: Uuid,
    pub user_id: Uuid,
    pub return_to: Option<String>,
    /// `amr` from the provider (e.g. `pwd`, `otp`, `mfa`), to record with the session.
    pub amr: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IdClaims {
    sub: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default)]
    amr: Option<Vec<String>>,
}

#[derive(Default)]
struct Cache {
    discovery: HashMap<String, (Instant, Discovery)>,
    jwks: HashMap<String, (Instant, JwkSet)>,
}

#[derive(Clone)]
pub struct Sso {
    db: Db,
    keys: Keys,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    policy: egress::Policy,
    cache: Arc<Mutex<Cache>>,
}

impl std::fmt::Debug for Sso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sso")
    }
}

fn sha(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

fn random() -> String {
    URL_SAFE_NO_PAD.encode(procera_crypto::random_bytes::<32>())
}

fn domain_of(email: &str) -> Option<String> {
    email.rsplit_once('@').map(|(_, d)| d.trim().to_lowercase()).filter(|d| !d.is_empty())
}

impl Sso {
    pub fn new(db: Db, keys: Keys, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self {
            db,
            keys,
            clock,
            ids,
            policy: egress::Policy::from_env(),
            cache: Arc::new(Mutex::new(Cache::default())),
        }
    }

    // ---- configuration ----

    pub async fn config(&self, tx: &mut WsTx<'_>) -> R<Option<Config>> {
        let r: Option<(String, String, bool, Vec<String>, bool, bool, i32, OffsetDateTime)> = sqlx::query_as(
            "SELECT issuer, client_id, client_secret_sealed IS NOT NULL, domains, enabled, required, revision, updated_at FROM sso_configs",
        )
        .fetch_optional(tx.conn())
        .await?;
        Ok(r.map(|r| Config {
            issuer: r.0,
            client_id: r.1,
            has_secret: r.2,
            domains: r.3,
            enabled: r.4,
            required: r.5,
            revision: r.6,
            updated_at: r.7,
        }))
    }

    /// Saves the configuration after checking the issuer's discovery document answers.
    pub async fn save(
        &self,
        tx: &mut WsTx<'_>,
        by: Uuid,
        revision: Option<i32>,
        input: &ConfigInput,
    ) -> R<Config> {
        let issuer = input.issuer.trim().trim_end_matches('/').to_owned();
        let client_id = input.client_id.trim().to_owned();
        if client_id.is_empty() {
            return Err(SsoError::Invalid("Enter the client id from your identity provider.".into()));
        }
        let mut domains: Vec<String> = input
            .domains
            .iter()
            .map(|d| d.trim().trim_start_matches('@').to_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        domains.sort();
        domains.dedup();
        if domains.iter().any(|d| !d.contains('.') || d.contains(' ') || d.len() > 253) {
            return Err(SsoError::Invalid("Domains look like example.com.".into()));
        }
        if input.required && !input.enabled {
            return Err(SsoError::Invalid("SSO can only be required while it's on.".into()));
        }
        // The issuer must answer with a matching discovery document before it is saved.
        let d = self.discovery(&issuer).await?;
        if d.issuer.trim_end_matches('/') != issuer {
            return Err(SsoError::Invalid(format!(
                "The provider says its issuer is {}, not {issuer}.",
                d.issuer
            )));
        }
        let current = self.config(tx).await?;
        if let (Some(c), Some(r)) = (&current, revision)
            && c.revision != r
        {
            return Err(SsoError::Invalid("stale".into()));
        }
        let ws = tx.ctx.workspace_id;
        let secret: Option<Option<Vec<u8>>> = match input.client_secret.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(s) => {
                Some(Some(self.keys.seal(tx, &format!("sso:{ws}"), s.as_bytes()).await.map_err(internal)?))
            }
        };
        let now = self.clock.now();
        match current {
            None => {
                sqlx::query("INSERT INTO sso_configs (workspace_id, issuer, client_id, client_secret_sealed, domains, enabled, required, updated_by, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                    .bind(ws).bind(&issuer).bind(&client_id).bind(secret.flatten()).bind(&domains).bind(input.enabled).bind(input.required).bind(by).bind(now)
                    .execute(tx.conn()).await?;
            }
            Some(_) => {
                sqlx::query("UPDATE sso_configs SET issuer = $1, client_id = $2, client_secret_sealed = CASE WHEN $3 THEN $4 ELSE client_secret_sealed END, domains = $5, enabled = $6, required = $7, updated_by = $8, updated_at = $9, revision = revision + 1")
                    .bind(&issuer).bind(&client_id).bind(secret.is_some()).bind(secret.flatten()).bind(&domains).bind(input.enabled).bind(input.required).bind(by).bind(now)
                    .execute(tx.conn()).await?;
            }
        }
        self.config(tx).await?.ok_or_else(|| internal("saved configuration missing"))
    }

    pub async fn remove(&self, tx: &mut WsTx<'_>) -> R<()> {
        sqlx::query("DELETE FROM sso_states").execute(tx.conn()).await?;
        sqlx::query("DELETE FROM sso_configs").execute(tx.conn()).await?;
        Ok(())
    }

    /// Whether this workspace requires SSO (cheap; the HTTP layer caches it).
    pub async fn required(&self, tx: &mut WsTx<'_>) -> R<bool> {
        Ok(sqlx::query_scalar::<_, bool>("SELECT enabled AND required FROM sso_configs")
            .fetch_optional(tx.conn())
            .await?
            .unwrap_or(false))
    }

    /// Workspaces whose provider vouches for this email's domain.
    pub async fn workspaces_for(&self, email: &str) -> R<Vec<(Uuid, String, String)>> {
        let Some(d) = domain_of(email) else { return Ok(vec![]) };
        Ok(sqlx::query_as("SELECT workspace_id, slug, name FROM discovery.sso_workspaces_for_domain($1)")
            .bind(d)
            .fetch_all(self.db.pool())
            .await?)
    }

    // ---- the provider ----

    async fn get_json(&self, url: &str) -> R<Value> {
        let t = egress::parse(url)
            .map_err(|e| SsoError::Invalid(format!("The provider's address isn't allowed: {e}")))?;
        let addr = egress::resolve(&t, &self.policy)
            .await
            .map_err(|e| SsoError::Invalid(format!("The provider couldn't be reached: {e}")))?;
        let res = egress::get(&t, addr)
            .await
            .map_err(|e| SsoError::Invalid(format!("The provider couldn't be reached: {e}")))?;
        if !(200..300).contains(&res.status) {
            return Err(SsoError::Invalid(format!("The provider answered {} for {url}.", res.status)));
        }
        serde_json::from_slice(&res.body)
            .map_err(|_| SsoError::Invalid(format!("The provider's answer from {url} isn't JSON.")))
    }

    pub async fn discovery(&self, issuer: &str) -> R<Discovery> {
        if let Some((at, d)) = self.cache.lock().map_err(internal)?.discovery.get(issuer)
            && at.elapsed() < CACHE_TTL
        {
            return Ok(d.clone());
        }
        let v = self.get_json(&format!("{issuer}/.well-known/openid-configuration")).await?;
        let d: Discovery = serde_json::from_value(v).map_err(|_| {
            SsoError::Invalid("The provider's discovery document is missing endpoints.".into())
        })?;
        self.cache.lock().map_err(internal)?.discovery.insert(issuer.to_owned(), (Instant::now(), d.clone()));
        Ok(d)
    }

    async fn jwks(&self, uri: &str, kid: Option<&str>) -> R<JwkSet> {
        if let Some((at, set)) = self.cache.lock().map_err(internal)?.jwks.get(uri)
            && at.elapsed() < CACHE_TTL
            && kid.is_none_or(|k| set.find(k).is_some())
        {
            return Ok(set.clone());
        }
        let v = self.get_json(uri).await?;
        let set: JwkSet = serde_json::from_value(v)
            .map_err(|_| SsoError::Refused("The provider's signing keys couldn't be read.".into()))?;
        self.cache.lock().map_err(internal)?.jwks.insert(uri.to_owned(), (Instant::now(), set.clone()));
        Ok(set)
    }

    // ---- sign-in ----

    /// The provider URL to send the browser to. `redirect_uri` is Procera's callback.
    pub async fn start(&self, ws: Uuid, redirect_uri: &str, return_to: Option<&str>) -> R<String> {
        let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await.map_err(internal)?;
        let row: Option<(String, String, bool)> =
            sqlx::query_as("SELECT issuer, client_id, enabled FROM sso_configs")
                .fetch_optional(tx.conn())
                .await?;
        let Some((issuer, client_id, true)) = row else { return Err(SsoError::NotFound) };
        let d = self.discovery(&issuer).await.map_err(|e| SsoError::Refused(e.to_string()))?;
        let state = format!("{ws}.{}", random());
        let nonce = random();
        let verifier = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let now = self.clock.now();
        let return_to = return_to.filter(|r| r.starts_with('/') && !r.starts_with("//"));
        sqlx::query("INSERT INTO sso_states (workspace_id, id, state_hash, code_verifier, nonce, return_to, created_at, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(ws).bind(self.ids.new_id()).bind(sha(&state)).bind(&verifier).bind(&nonce).bind(return_to).bind(now).bind(now + STATE_TTL)
            .execute(tx.conn()).await?;
        sqlx::query("DELETE FROM sso_states WHERE expires_at < $1")
            .bind(now - Duration::hours(1))
            .execute(tx.conn())
            .await?;
        tx.commit().await.map_err(internal)?;
        let mut url = reqwest::Url::parse(&d.authorization_endpoint)
            .map_err(|_| SsoError::Refused("The provider's sign-in address is invalid.".into()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", "openid email profile")
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.to_string())
    }

    /// Completes a sign-in from the provider's redirect.
    pub async fn callback(&self, state: &str, code: &str, redirect_uri: &str) -> R<Verified> {
        let refuse = |m: &str| SsoError::Refused(m.to_owned());
        let ws: Uuid = state
            .split_once('.')
            .and_then(|(w, _)| w.parse().ok())
            .ok_or_else(|| refuse("This sign-in link isn't valid. Start again."))?;
        let ctx = WsCtx { workspace_id: ws };
        let mut tx = self.db.begin_ws(ctx).await.map_err(internal)?;
        let st: Option<(Uuid, String, String, Option<String>, OffsetDateTime, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT id, code_verifier, nonce, return_to, expires_at, used_at FROM sso_states WHERE state_hash = $1 FOR UPDATE",
        )
        .bind(sha(state))
        .fetch_optional(tx.conn())
        .await?;
        let (sid, verifier, nonce, return_to, expires, used) =
            st.ok_or_else(|| refuse("This sign-in link isn't valid. Start again."))?;
        if used.is_some() || expires <= self.clock.now() {
            return Err(refuse("This sign-in link was already used or has expired. Start again."));
        }
        sqlx::query("UPDATE sso_states SET used_at = $2 WHERE id = $1")
            .bind(sid)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        let (issuer, client_id, sealed, domains): (String, String, Option<Vec<u8>>, Vec<String>) =
            sqlx::query_as(
                "SELECT issuer, client_id, client_secret_sealed, domains FROM sso_configs WHERE enabled",
            )
            .fetch_optional(tx.conn())
            .await?
            .ok_or_else(|| refuse("Single sign-on is off for this workspace."))?;
        let secret = match sealed {
            Some(s) => Some(
                String::from_utf8(
                    self.keys.open(&mut tx, &format!("sso:{ws}"), &s).await.map_err(internal)?.to_vec(),
                )
                .map_err(internal)?,
            ),
            None => None,
        };
        tx.commit().await.map_err(internal)?; // the state is spent whatever happens next
        let d = self.discovery(&issuer).await.map_err(|e| SsoError::Refused(e.to_string()))?;

        // Exchange the code (client_secret_post; PKCE always).
        let mut form = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", redirect_uri.to_owned()),
            ("client_id", client_id.clone()),
            ("code_verifier", verifier),
        ];
        if let Some(s) = secret {
            form.push(("client_secret", s));
        }
        let body = form
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencode(v)))
            .collect::<Vec<_>>()
            .join("&")
            .into_bytes();
        let t = egress::parse(&d.token_endpoint).map_err(|e| SsoError::Refused(e.to_string()))?;
        let addr = egress::resolve(&t, &self.policy).await.map_err(|e| SsoError::Refused(e.to_string()))?;
        let headers = vec![
            ("content-type".to_owned(), "application/x-www-form-urlencoded".to_owned()),
            ("accept".to_owned(), "application/json".to_owned()),
        ];
        let res = egress::post_full(&t, addr, &headers, body)
            .await
            .map_err(|e| SsoError::Refused(format!("The provider couldn't be reached: {e}")))?;
        if !(200..300).contains(&res.status) {
            return Err(refuse(
                "The provider didn't accept the sign-in. Try again, or ask your administrator.",
            ));
        }
        let tokens: Value = serde_json::from_slice(&res.body)
            .map_err(|_| refuse("The provider's answer couldn't be read."))?;
        let id_token =
            tokens["id_token"].as_str().ok_or_else(|| refuse("The provider didn't send an ID token."))?;

        // Verify the ID token.
        let header =
            decode_header(id_token).map_err(|_| refuse("The provider's ID token couldn't be read."))?;
        if !matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::PS256
        ) {
            return Err(refuse("The provider signed with an algorithm Procera doesn't accept."));
        }
        let set = self.jwks(&d.jwks_uri, header.kid.as_deref()).await?;
        let jwk = match header.kid.as_deref() {
            Some(k) => set.find(k),
            None => set.keys.first(),
        }
        .ok_or_else(|| refuse("The provider's signing key wasn't found."))?;
        let key =
            DecodingKey::from_jwk(jwk).map_err(|_| refuse("The provider's signing key couldn't be used."))?;
        let mut v = Validation::new(header.alg);
        v.set_issuer(&[issuer.as_str(), format!("{issuer}/").as_str()]);
        v.set_audience(&[client_id.as_str()]);
        v.leeway = LEEWAY;
        v.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
        let claims = decode::<IdClaims>(id_token, &key, &v)
            .map_err(|e| SsoError::Refused(format!("The provider's ID token was refused ({e}).")))?
            .claims;
        if claims.nonce.as_deref() != Some(nonce.as_str()) {
            return Err(refuse("The provider's ID token was refused (nonce)."));
        }
        if claims.azp.as_deref().is_some_and(|a| a != client_id) {
            return Err(refuse("The provider's ID token was meant for another app."));
        }

        // Who is it?
        let mut tx = self.db.begin_ws(ctx).await.map_err(internal)?;
        let linked: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sso_identities WHERE issuer = $1 AND subject = $2")
                .bind(&issuer)
                .bind(&claims.sub)
                .fetch_optional(tx.conn())
                .await?;
        let user = match linked {
            Some(u) => u,
            None => {
                let email = claims.email.as_deref().map(|e| e.trim().to_lowercase()).filter(|_| claims.email_verified == Some(true))
                    .ok_or_else(|| refuse("Your provider didn't share a verified email, so Procera can't tell who you are. Ask your administrator."))?;
                let d = domain_of(&email).unwrap_or_default();
                if !domains.contains(&d) {
                    return Err(refuse(
                        "Your email's domain isn't one this workspace trusts its provider for. Ask your administrator.",
                    ));
                }
                let u: Option<Uuid> = sqlx::query_scalar("SELECT u.id FROM users u JOIN memberships m ON m.user_id = u.id WHERE lower(u.email::text) = $1 AND m.status = 'active' AND u.status = 'active'")
                    .bind(&email).fetch_optional(tx.conn()).await?;
                let u = u.ok_or_else(|| {
                    refuse("You're not a member of this workspace yet. Ask an administrator to invite you.")
                })?;
                sqlx::query("INSERT INTO sso_identities (workspace_id, issuer, subject, user_id, email, created_at) VALUES ($1,$2,$3,$4,$5,$6)")
                    .bind(ws).bind(&issuer).bind(&claims.sub).bind(u).bind(&email).bind(self.clock.now()).execute(tx.conn()).await?;
                u
            }
        };
        let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.user_id = $1 AND m.status = 'active' AND u.status = 'active')")
            .bind(user).fetch_one(tx.conn()).await?;
        if !active {
            return Err(refuse("Your access to this workspace has ended. Ask an administrator."));
        }
        sqlx::query("UPDATE sso_identities SET last_login_at = $3 WHERE issuer = $1 AND subject = $2")
            .bind(&issuer)
            .bind(&claims.sub)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        tx.commit().await.map_err(internal)?;
        Ok(Verified { workspace: ws, user_id: user, return_to, amr: claims.amr.unwrap_or_default() })
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(domain_of("Ann@Example.COM"), Some("example.com".into()));
        assert_eq!(domain_of("nope"), None);
        assert_eq!(urlencode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
