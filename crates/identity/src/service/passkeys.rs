//! WebAuthn passkeys as a second factor and for step-up (BPMS-022, docs/06 §3.6, docs/12 §3/§11).
//! Ceremony options carry a single-use challenge stored in `webauthn_challenges`; the browser's
//! response is verified by [`crate::webauthn`]. Every change is audited in the system chain.

use procera_audit::Outcome;
use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ClientInfo, Identity, IdentityError};
use crate::jwt::Claims;
use crate::webauthn::{self, ALGORITHMS, CredentialJson, b64, unb64};

const CHALLENGE_TTL: time::Duration = time::Duration::minutes(5);
/// Ceremony timeout hint for the browser, in milliseconds (matches the challenge lifetime).
const TIMEOUT_MS: u64 = 300_000;

/// A registered passkey as shown to its owner. The key material is never returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Passkey {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    /// Synced between devices by its provider (backup state flag).
    pub synced: bool,
    pub user_verified: bool,
    pub transports: Vec<String>,
}

type PasskeyRow =
    (Uuid, String, OffsetDateTime, Option<OffsetDateTime>, Option<bool>, Option<bool>, Option<Vec<String>>);

fn passkey(r: PasskeyRow) -> Passkey {
    Passkey {
        id: r.0,
        name: r.1,
        created_at: r.2,
        last_used_at: r.3,
        synced: r.4.unwrap_or(false),
        user_verified: r.5.unwrap_or(false),
        transports: r.6.unwrap_or_default(),
    }
}

const PASSKEY_COLS: &str = "id, label, created_at, last_used_at, backup_state, user_verified, transports";

fn descriptor(id: &[u8], transports: Option<Vec<String>>) -> Value {
    match transports {
        Some(t) if !t.is_empty() => json!({"type": "public-key", "id": b64(id), "transports": t}),
        _ => json!({"type": "public-key", "id": b64(id)}),
    }
}

impl Identity {
    async fn new_challenge(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
        purpose: &str,
    ) -> Result<Vec<u8>, IdentityError> {
        sqlx::query("DELETE FROM webauthn_challenges WHERE user_id = $1 AND (used_at IS NOT NULL OR expires_at <= $2)")
            .bind(user_id).bind(self.now()).execute(tx.conn()).await?;
        let challenge = procera_crypto::random_bytes::<32>().to_vec();
        sqlx::query("INSERT INTO webauthn_challenges (id, user_id, purpose, challenge, created_at, expires_at) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(self.ids.new_id()).bind(user_id).bind(purpose).bind(&challenge).bind(self.now()).bind(self.now() + CHALLENGE_TTL)
            .execute(tx.conn()).await?;
        Ok(challenge)
    }

    /// Consumes the stored challenge the client data answers. `None` when it is unknown, belongs to
    /// another user or ceremony, expired, or was already used.
    async fn take_challenge(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
        purpose: &str,
        client_data_json: &[u8],
    ) -> Result<Option<Vec<u8>>, IdentityError> {
        let Ok(claimed) = webauthn::client_challenge(client_data_json) else { return Ok(None) };
        Ok(sqlx::query_scalar(
            "UPDATE webauthn_challenges SET used_at = $4 WHERE user_id = $1 AND purpose = $2 AND challenge = $3 AND used_at IS NULL AND expires_at > $4 RETURNING challenge",
        )
        .bind(user_id).bind(purpose).bind(claimed).bind(self.now()).fetch_optional(tx.conn()).await?)
    }

    async fn credential_descriptors(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
    ) -> Result<Vec<Value>, IdentityError> {
        let rows: Vec<(Vec<u8>, Option<Vec<String>>)> = sqlx::query_as(
            "SELECT credential_id, transports FROM user_credentials WHERE user_id = $1 AND kind = 'webauthn' ORDER BY created_at",
        )
        .bind(user_id)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(|(id, t)| descriptor(&id, t)).collect())
    }

    fn request_options(&self, challenge: &[u8], allow: Vec<Value>) -> Value {
        json!({"public_key": {
            "challenge": b64(challenge),
            "rpId": self.cfg.relying_party.id,
            "allowCredentials": allow,
            "userVerification": "preferred",
            "timeout": TIMEOUT_MS,
        }})
    }

    /// Options for `navigator.credentials.create()`: a new challenge, the account, and the passkeys
    /// already registered (so the same authenticator is not registered twice).
    pub async fn passkey_registration_options(&self, user_id: Uuid) -> Result<Value, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        let exclude = self.credential_descriptors(&mut tx, user_id).await?;
        let challenge = self.new_challenge(&mut tx, user_id, "register").await?;
        tx.commit().await?;
        let rp = &self.cfg.relying_party;
        Ok(json!({"public_key": {
            "rp": {"id": rp.id, "name": rp.name},
            "user": {"id": b64(user.id.as_bytes()), "name": user.email, "displayName": user.display_name},
            "challenge": b64(&challenge),
            "pubKeyCredParams": ALGORITHMS.iter().map(|a| json!({"type": "public-key", "alg": a})).collect::<Vec<_>>(),
            "timeout": TIMEOUT_MS,
            "excludeCredentials": exclude,
            "authenticatorSelection": {"residentKey": "preferred", "userVerification": "preferred"},
            "attestation": "none",
        }}))
    }

    /// Verifies a registration response and stores the passkey. When it is the account's first second
    /// factor, 10 recovery codes are issued and returned once (as with TOTP enrolment).
    pub async fn passkey_register(
        &self,
        user_id: Uuid,
        name: &str,
        cred: &CredentialJson,
        client: &ClientInfo,
    ) -> Result<(Passkey, Option<Vec<String>>), IdentityError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(IdentityError::Invalid("name must be 1 to 64 characters".into()));
        }
        let invalid = |e: webauthn::WebauthnError| IdentityError::PasskeyInvalid(e.to_string());
        let id = cred.credential_id().map_err(invalid)?;
        let client_data = unb64(&cred.response.client_data_json).map_err(invalid)?;
        let attestation = cred
            .response
            .attestation_object
            .as_deref()
            .ok_or_else(|| IdentityError::PasskeyInvalid("missing attestationObject".into()))
            .and_then(|a| unb64(a).map_err(invalid))?;
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        let challenge = self
            .take_challenge(&mut tx, user_id, "register", &client_data)
            .await?
            .ok_or_else(|| IdentityError::PasskeyInvalid("challenge expired or already used".into()))?;
        let new =
            webauthn::verify_registration(&self.cfg.relying_party, &challenge, &client_data, &attestation)
                .map_err(invalid)?;
        if new.credential_id != id {
            return Err(IdentityError::PasskeyInvalid("credential id mismatch".into()));
        }
        let transports: Vec<String> = cred
            .response
            .transports
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.len() <= 32)
            .take(8)
            .collect();
        let row = self.ids.new_id();
        let inserted = sqlx::query(
            "INSERT INTO user_credentials (id, user_id, kind, secret, label, credential_id, sign_count, transports, backup_eligible, backup_state, user_verified, created_at)
             VALUES ($1, $2, 'webauthn', $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (credential_id) WHERE kind = 'webauthn' DO NOTHING",
        )
        .bind(row).bind(user_id).bind(&new.public_key).bind(name).bind(&new.credential_id).bind(i64::from(new.sign_count))
        .bind(&transports).bind(new.backup_eligible).bind(new.backup_state).bind(new.user_verified).bind(self.now())
        .execute(tx.conn()).await?.rows_affected();
        if inserted == 0 {
            return Err(IdentityError::PasskeyExists);
        }
        let codes =
            if user.mfa_enabled { None } else { Some(self.issue_recovery_codes(&mut tx, user_id).await?) };
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "mfa.webauthn_added",
            Outcome::Allowed,
            client,
            codes.as_ref().map(|_| "first second factor; recovery codes issued"),
        )
        .await?;
        let pk: PasskeyRow =
            sqlx::query_as(&format!("SELECT {PASSKEY_COLS} FROM user_credentials WHERE id = $1"))
                .bind(row)
                .fetch_one(tx.conn())
                .await?;
        tx.commit().await?;
        Ok((passkey(pk), codes))
    }

    pub async fn passkeys(&self, user_id: Uuid) -> Result<Vec<Passkey>, IdentityError> {
        let rows: Vec<PasskeyRow> = sqlx::query_as(&format!(
            "SELECT {PASSKEY_COLS} FROM user_credentials WHERE user_id = $1 AND kind = 'webauthn' ORDER BY created_at"
        ))
        .bind(user_id)
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows.into_iter().map(passkey).collect())
    }

    /// Removes a passkey (the caller enforces step-up). Removing the last second factor also removes
    /// the recovery codes, which only make sense alongside one.
    pub async fn passkey_remove(
        &self,
        user_id: Uuid,
        id: Uuid,
        client: &ClientInfo,
    ) -> Result<(), IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let n =
            sqlx::query("DELETE FROM user_credentials WHERE id = $1 AND user_id = $2 AND kind = 'webauthn'")
                .bind(id)
                .bind(user_id)
                .execute(tx.conn())
                .await?
                .rows_affected();
        if n == 0 {
            return Err(IdentityError::NotFound);
        }
        let user = self.load_user(&mut tx, user_id).await?.ok_or(IdentityError::InvalidToken)?;
        let last = !user.mfa_enabled;
        if last {
            sqlx::query("DELETE FROM user_credentials WHERE user_id = $1 AND kind = 'recovery_code'")
                .bind(user_id)
                .execute(tx.conn())
                .await?;
        }
        self.audit(
            &mut tx,
            Some(user.subject_ref),
            "mfa.webauthn_removed",
            Outcome::Allowed,
            client,
            last.then_some("last second factor; recovery codes removed"),
        )
        .await?;
        tx.commit().await.map_err(IdentityError::from)
    }

    /// Options for `navigator.credentials.get()` during sign-in, after the password step.
    pub async fn mfa_passkey_options(&self, mfa_token: &str) -> Result<Value, IdentityError> {
        let claims = self.jwt.verify(mfa_token, "mfa").ok_or(IdentityError::InvalidToken)?;
        self.assertion_options(&claims, "mfa").await
    }

    /// Options for `navigator.credentials.get()` during step-up.
    pub async fn step_up_passkey_options(&self, claims: &Claims) -> Result<Value, IdentityError> {
        self.assertion_options(claims, "step_up").await
    }

    async fn assertion_options(&self, claims: &Claims, purpose: &str) -> Result<Value, IdentityError> {
        let mut tx = self.db.begin_system().await?;
        let user = self.load_user(&mut tx, claims.sub).await?.ok_or(IdentityError::InvalidToken)?;
        if user.status != "active" || user.token_version != claims.tv {
            return Err(IdentityError::InvalidToken);
        }
        let allow = self.credential_descriptors(&mut tx, user.id).await?;
        if allow.is_empty() {
            return Err(IdentityError::PasskeyRejected("no passkey is registered for this account".into()));
        }
        let challenge = self.new_challenge(&mut tx, user.id, purpose).await?;
        tx.commit().await?;
        Ok(self.request_options(&challenge, allow))
    }

    /// Verifies an assertion against the user's passkeys and a stored challenge for `purpose`,
    /// and advances the signature counter. Returns the failure reason, or `None` when it verified.
    pub(super) async fn check_passkey(
        &self,
        tx: &mut WsTx<'_>,
        user_id: Uuid,
        purpose: &str,
        cred: &CredentialJson,
    ) -> Result<Option<String>, IdentityError> {
        let parts = (|| {
            let id = cred.credential_id()?;
            let cd = unb64(&cred.response.client_data_json)?;
            let ad = unb64(cred.response.authenticator_data.as_deref().unwrap_or_default())?;
            let sig = unb64(cred.response.signature.as_deref().unwrap_or_default())?;
            Ok::<_, webauthn::WebauthnError>((id, cd, ad, sig))
        })();
        let (id, cd, ad, sig) = match parts {
            Ok(p) => p,
            Err(e) => return Ok(Some(e.to_string())),
        };
        let Some(challenge) = self.take_challenge(tx, user_id, purpose, &cd).await? else {
            return Ok(Some("challenge expired or already used".into()));
        };
        let row: Option<(Uuid, Vec<u8>, i64)> = sqlx::query_as(
            "SELECT id, secret, sign_count FROM user_credentials WHERE user_id = $1 AND kind = 'webauthn' AND credential_id = $2 FOR UPDATE",
        )
        .bind(user_id)
        .bind(&id)
        .fetch_optional(tx.conn())
        .await?;
        let Some((row, key, count)) = row else { return Ok(Some("unknown passkey".into())) };
        let stored = u32::try_from(count).unwrap_or(u32::MAX);
        match webauthn::verify_assertion(&self.cfg.relying_party, &challenge, &cd, &ad, &sig, &key, stored) {
            Ok(a) => {
                sqlx::query("UPDATE user_credentials SET sign_count = $2, last_used_at = $3, backup_state = $4 WHERE id = $1")
                    .bind(row).bind(i64::from(a.sign_count)).bind(self.now()).bind(a.backup_state)
                    .execute(tx.conn()).await?;
                Ok(None)
            }
            Err(e) => Ok(Some(e.to_string())),
        }
    }
}
