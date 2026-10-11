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

// ---- Envelope encryption under a workspace KEK (BPMS-030, docs/06 §4) ----
//
// `0x02 ‖ kek_version u32 BE ‖ wrapped_dek (12 nonce + 32 key + 16 tag = 60) ‖ nonce 12 ‖ ciphertext+tag`
// The DEK is random per value and wrapped by the KEK with AAD "dek"; the data is sealed by the DEK with
// the caller's context as AAD. Rotation rewraps the DEK only; the ciphertext never changes.

const ENVELOPE: u8 = 2;
const WRAPPED_DEK: usize = 12 + 32 + 16;
const HEADER: usize = 1 + 4 + WRAPPED_DEK;

fn gcm(key: &[u8]) -> Result<Aes256Gcm, CryptoError> {
    Aes256Gcm::new_from_slice(key).map_err(|e| CryptoError::Key(e.to_string()))
}

fn gcm_seal(key: &[u8], aad: &[u8], pt: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = gcm(key)?
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: pt, aad })
        .map_err(|_| CryptoError::Open)?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn gcm_open(key: &[u8], aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if sealed.len() < 12 + 16 {
        return Err(CryptoError::Open);
    }
    let pt = gcm(key)?
        .decrypt(Nonce::from_slice(&sealed[..12]), Payload { msg: &sealed[12..], aad })
        .map_err(|_| CryptoError::Open)?;
    Ok(Zeroizing::new(pt))
}

/// Whether `sealed` is an envelope (as opposed to a legacy [`seal`] value).
pub fn is_envelope(sealed: &[u8]) -> bool {
    sealed.len() > HEADER && sealed[0] == ENVELOPE
}

/// The KEK version an envelope's DEK is wrapped with.
pub fn envelope_version(sealed: &[u8]) -> Option<u32> {
    is_envelope(sealed).then(|| u32::from_be_bytes([sealed[1], sealed[2], sealed[3], sealed[4]]))
}

/// Seals `plaintext` under a fresh DEK wrapped by `kek` (version `version`), bound to `context`.
pub fn envelope_seal(
    kek: &[u8; 32],
    version: u32,
    context: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let dek: Zeroizing<[u8; 32]> = Zeroizing::new(random_bytes());
    let wrapped = gcm_seal(kek, b"dek", dek.as_ref())?;
    let body = gcm_seal(dek.as_ref(), context.as_bytes(), plaintext)?;
    let mut out = Vec::with_capacity(HEADER + body.len());
    out.push(ENVELOPE);
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&wrapped);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Opens an envelope with the KEK of its version (looked up by `kek_for`).
pub fn envelope_open(
    kek_for: impl FnOnce(u32) -> Result<Zeroizing<[u8; 32]>, CryptoError>,
    context: &str,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let version = envelope_version(sealed).ok_or(CryptoError::Open)?;
    let kek = kek_for(version)?;
    let dek = gcm_open(kek.as_ref(), b"dek", &sealed[5..HEADER])?;
    gcm_open(&dek, context.as_bytes(), &sealed[HEADER..])
}

/// Rewraps an envelope's DEK from `old` to `new` (version `new_version`). The ciphertext is unchanged.
pub fn envelope_rewrap(
    sealed: &[u8],
    old: &[u8; 32],
    new: &[u8; 32],
    new_version: u32,
) -> Result<Vec<u8>, CryptoError> {
    if !is_envelope(sealed) {
        return Err(CryptoError::Open);
    }
    let dek = gcm_open(old, b"dek", &sealed[5..HEADER])?;
    let wrapped = gcm_seal(new, b"dek", &dek)?;
    let mut out = Vec::with_capacity(sealed.len());
    out.push(ENVELOPE);
    out.extend_from_slice(&new_version.to_be_bytes());
    out.extend_from_slice(&wrapped);
    out.extend_from_slice(&sealed[HEADER..]);
    Ok(out)
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

    #[test]
    fn envelopes_open_with_their_kek_rewrap_without_reencrypting_and_shred_with_the_kek() {
        let (k1, k2) = (random_bytes::<32>(), random_bytes::<32>());
        let e = envelope_seal(&k1, 1, "email_messages:1", b"hello").expect("seals");
        assert!(is_envelope(&e) && envelope_version(&e) == Some(1));
        let key = |k: [u8; 32]| {
            move |_v: u32| -> Result<Zeroizing<[u8; 32]>, CryptoError> { Ok(Zeroizing::new(k)) }
        };
        assert_eq!(envelope_open(key(k1), "email_messages:1", &e).expect("opens").as_slice(), b"hello");
        assert!(envelope_open(key(k1), "email_messages:2", &e).is_err(), "context is bound");
        assert!(envelope_open(key(k2), "email_messages:1", &e).is_err(), "another KEK cannot open it");
        let r = envelope_rewrap(&e, &k1, &k2, 2).expect("rewraps");
        assert_eq!(envelope_version(&r), Some(2));
        assert_eq!(&r[HEADER..], &e[HEADER..], "the ciphertext is unchanged");
        assert_eq!(envelope_open(key(k2), "email_messages:1", &r).expect("opens").as_slice(), b"hello");
        let mut tampered = r.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(envelope_open(key(k2), "email_messages:1", &tampered).is_err(), "tampering is detected");
    }
}
