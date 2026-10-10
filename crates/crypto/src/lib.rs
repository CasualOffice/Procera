//! Secrets-at-rest primitives (BPMS-030, first slice).
//!
//! - [`KeyProvider`]: where key material comes from. This slice ships [`LocalKey`] (a 256-bit key
//!   from env/file, for self-hosted installs); KMS / OpenBao Transit adapters implement the same trait.
//! - [`seal`] / [`open`]: AES-256-GCM with a random 96-bit nonce and **AAD that binds the ciphertext to
//!   its context** (e.g. `user_credentials:<id>:totp`), so a ciphertext copied to another row fails.
//! - [`random_token`] / [`hash_token`]: 256-bit opaque tokens; only hashes are stored (docs/06 §3.2, §3.7).

use aes_gcm::aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("key material invalid: {0}")]
    Key(String),
    #[error("ciphertext invalid or bound to a different context")]
    Open,
}

/// Source of the data-encryption key for a given purpose.
pub trait KeyProvider: Send + Sync + std::fmt::Debug {
    /// Returns the 32-byte key for `purpose` (e.g. `"secrets"`). Implementations may derive per purpose.
    fn key(&self, purpose: &str) -> Result<Zeroizing<[u8; 32]>, CryptoError>;
}

/// A 256-bit root key held locally (env `PROCERA_MASTER_KEY`, base64), with per-purpose derivation
/// `HMAC-SHA256(root, purpose)`. Suitable for single-node/self-hosted; production should prefer KMS.
pub struct LocalKey {
    root: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalKey(**redacted**)")
    }
}

impl LocalKey {
    pub fn from_base64(b64: &str) -> Result<Self, CryptoError> {
        let raw = Zeroizing::new(STANDARD.decode(b64.trim()).map_err(|e| CryptoError::Key(e.to_string()))?);
        let root: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::Key("PROCERA_MASTER_KEY must be 32 bytes (base64)".into()))?;
        Ok(Self { root: Zeroizing::new(root) })
    }

    /// Generates a new random root key, base64-encoded (for `procera keygen`).
    pub fn generate_base64() -> String {
        let mut k = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(k.as_mut());
        STANDARD.encode(k.as_ref())
    }
}

impl KeyProvider for LocalKey {
    fn key(&self, purpose: &str) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(self.root.as_ref())
            .map_err(|e| CryptoError::Key(e.to_string()))?;
        mac.update(b"procera:kdf:v1:");
        mac.update(purpose.as_bytes());
        let out: [u8; 32] = mac.finalize().into_bytes().into();
        Ok(Zeroizing::new(out))
    }
}

/// Encrypts `plaintext` bound to `context`. Output: `version ‖ nonce(12) ‖ ciphertext+tag`.
pub fn seal(
    keys: &dyn KeyProvider,
    purpose: &str,
    context: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let key = keys.key(purpose)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| CryptoError::Key(e.to_string()))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: context.as_bytes() })
        .map_err(|_| CryptoError::Open)?;
    let mut out = Vec::with_capacity(1 + 12 + ct.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypts a value produced by [`seal`] with the same purpose and context.
pub fn open(
    keys: &dyn KeyProvider,
    purpose: &str,
    context: &str,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if sealed.len() < 1 + 12 + 16 || sealed[0] != VERSION {
        return Err(CryptoError::Open);
    }
    let key = keys.key(purpose)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| CryptoError::Key(e.to_string()))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(&sealed[1..13]), Payload { msg: &sealed[13..], aad: context.as_bytes() })
        .map_err(|_| CryptoError::Open)?;
    Ok(Zeroizing::new(pt))
}

/// A 256-bit random token, base64url (43 chars). Give it to the client; store only [`hash_token`].
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

/// SHA-256 of a token, for storage and lookup.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Random bytes from the OS CSPRNG.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> LocalKey {
        LocalKey::from_base64(&LocalKey::generate_base64()).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn seal_open_round_trip() -> Result<(), CryptoError> {
        let k = keys();
        let s = seal(&k, "secrets", "user_credentials:1:totp", b"JBSWY3DPEHPK3PXP")?;
        assert_eq!(open(&k, "secrets", "user_credentials:1:totp", &s)?.as_slice(), b"JBSWY3DPEHPK3PXP");
        Ok(())
    }

    #[test]
    fn ciphertext_is_bound_to_context_purpose_and_key() -> Result<(), CryptoError> {
        let k = keys();
        let s = seal(&k, "secrets", "user_credentials:1:totp", b"x")?;
        assert!(open(&k, "secrets", "user_credentials:2:totp", &s).is_err(), "moved to another row");
        assert!(open(&k, "other", "user_credentials:1:totp", &s).is_err(), "different purpose key");
        assert!(open(&keys(), "secrets", "user_credentials:1:totp", &s).is_err(), "different root key");
        let mut tampered = s.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open(&k, "secrets", "user_credentials:1:totp", &tampered).is_err(), "tampered");
        Ok(())
    }

    #[test]
    fn tokens_are_256_bit_and_unique() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert_eq!(hash_token(&a).len(), 32);
    }

    #[test]
    fn rejects_short_root_key() {
        assert!(LocalKey::from_base64(&STANDARD.encode([0u8; 16])).is_err());
    }
}
