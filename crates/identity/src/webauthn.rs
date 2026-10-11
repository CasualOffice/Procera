//! WebAuthn Level 3 relying-party checks for passkeys as a second factor and for step-up
//! (BPMS-022, docs/06 §3.6, docs/12 §3/§11).
//!
//! The ceremonies follow W3C WebAuthn L3 §7.1 (registration) and §7.2 (authentication). Signature
//! verification uses `ring` (already the JWT and TLS backend); CBOR decoding uses `ciborium`. Both
//! are permissively licensed (ADR-0014); `webauthn-rs` is MPL-2.0 and not on the allow list
//! (docs/22 A-16). Attestation is requested as `none` and not verified: Procera accepts any
//! authenticator (no AAGUID allow list at P0), so the attestation statement carries no trust.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ciborium::Value;
use ring::signature::{self, RsaPublicKeyComponents, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// COSE algorithm ids we accept, in preference order (IANA COSE Algorithms).
pub const ES256: i64 = -7;
pub const EDDSA: i64 = -8;
pub const RS256: i64 = -257;
pub const ALGORITHMS: [i64; 3] = [ES256, EDDSA, RS256];

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_BE: u8 = 0x08;
const FLAG_BS: u8 = 0x10;
const FLAG_AT: u8 = 0x40;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WebauthnError {
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("wrong ceremony type")]
    WrongType,
    #[error("challenge mismatch")]
    Challenge,
    #[error("origin not allowed")]
    Origin,
    #[error("relying party id mismatch")]
    RpId,
    #[error("user presence not asserted")]
    UserPresence,
    #[error("unsupported public key algorithm")]
    Algorithm,
    #[error("signature does not verify")]
    Signature,
    #[error("signature counter did not increase (possible cloned authenticator)")]
    Counter,
}

/// Who the browser must be talking to: the RP ID (a registrable domain) and the exact origins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelyingParty {
    pub id: String,
    pub name: String,
    pub origins: Vec<String>,
}

impl Default for RelyingParty {
    fn default() -> Self {
        Self::from_public_url("http://127.0.0.1:8080", None)
    }
}

impl RelyingParty {
    /// Derives the RP from the public web URL. Browsers refuse IP addresses as an RP ID, so a loopback
    /// IP (local development and E2E) maps to `localhost`, and the allowed origin follows it.
    pub fn from_public_url(public_url: &str, rp_id: Option<&str>) -> Self {
        let (scheme, rest) = public_url.split_once("://").unwrap_or(("https", public_url));
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            match v6.split_once(']') {
                Some((h, p)) => (format!("[{h}]"), p.strip_prefix(':').map(str::to_owned)),
                None => (authority.to_owned(), None),
            }
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                    (h.to_owned(), Some(p.to_owned()))
                }
                _ => (authority.to_owned(), None),
            }
        };
        let host = host.to_ascii_lowercase();
        let loopback = host == "127.0.0.1" || host == "[::1]";
        let host = if loopback { "localhost".to_owned() } else { host };
        let origin = match port {
            Some(p) => format!("{scheme}://{host}:{p}"),
            None => format!("{scheme}://{host}"),
        };
        Self { id: rp_id.map_or(host, str::to_owned), name: "Procera".into(), origins: vec![origin] }
    }

    fn rp_id_hash(&self) -> [u8; 32] {
        Sha256::digest(self.id.as_bytes()).into()
    }
}

/// A verified new credential, ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCredential {
    pub credential_id: Vec<u8>,
    /// COSE_Key bytes as sent by the authenticator.
    pub public_key: Vec<u8>,
    pub sign_count: u32,
    pub user_verified: bool,
    pub backup_eligible: bool,
    pub backup_state: bool,
}

/// The result of a verified assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assertion {
    pub sign_count: u32,
    pub user_verified: bool,
    pub backup_state: bool,
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Result<Vec<u8>, WebauthnError> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).map_err(|_| WebauthnError::Malformed("base64url"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientData {
    #[serde(rename = "type")]
    typ: String,
    challenge: String,
    origin: String,
    #[serde(default)]
    cross_origin: Option<bool>,
}

/// The challenge a client-data JSON claims to answer (used to find the stored, single-use challenge).
pub fn client_challenge(client_data_json: &[u8]) -> Result<Vec<u8>, WebauthnError> {
    let cd: ClientData =
        serde_json::from_slice(client_data_json).map_err(|_| WebauthnError::Malformed("clientDataJSON"))?;
    unb64(&cd.challenge)
}

fn check_client_data(
    rp: &RelyingParty,
    client_data_json: &[u8],
    typ: &str,
    challenge: &[u8],
) -> Result<(), WebauthnError> {
    let cd: ClientData =
        serde_json::from_slice(client_data_json).map_err(|_| WebauthnError::Malformed("clientDataJSON"))?;
    if cd.typ != typ {
        return Err(WebauthnError::WrongType);
    }
    if !bool::from(unb64(&cd.challenge)?.ct_eq(challenge)) {
        return Err(WebauthnError::Challenge);
    }
    if cd.cross_origin == Some(true) || !rp.origins.contains(&cd.origin) {
        return Err(WebauthnError::Origin);
    }
    Ok(())
}

struct AuthData<'a> {
    flags: u8,
    sign_count: u32,
    /// Attested credential data (AT flag): credential id and the COSE key bytes.
    attested: Option<(Vec<u8>, Vec<u8>)>,
    _raw: &'a [u8],
}

fn parse_auth_data<'a>(rp: &RelyingParty, raw: &'a [u8]) -> Result<AuthData<'a>, WebauthnError> {
    if raw.len() < 37 {
        return Err(WebauthnError::Malformed("authenticatorData"));
    }
    if !bool::from(raw[..32].ct_eq(&rp.rp_id_hash())) {
        return Err(WebauthnError::RpId);
    }
    let flags = raw[32];
    if flags & FLAG_UP == 0 {
        return Err(WebauthnError::UserPresence);
    }
    let sign_count = u32::from_be_bytes([raw[33], raw[34], raw[35], raw[36]]);
    let attested = if flags & FLAG_AT != 0 {
        let rest = &raw[37..];
        if rest.len() < 18 {
            return Err(WebauthnError::Malformed("attestedCredentialData"));
        }
        let id_len = usize::from(u16::from_be_bytes([rest[16], rest[17]]));
        let id = rest.get(18..18 + id_len).ok_or(WebauthnError::Malformed("credentialId"))?;
        let mut key_bytes = &rest[18 + id_len..];
        let before = key_bytes.len();
        // Read exactly one CBOR item (extensions may follow it).
        let _: Value = ciborium::de::from_reader(&mut key_bytes)
            .map_err(|_| WebauthnError::Malformed("credentialPublicKey"))?;
        let used = before - key_bytes.len();
        Some((id.to_vec(), rest[18 + id_len..18 + id_len + used].to_vec()))
    } else {
        None
    };
    Ok(AuthData { flags, sign_count, attested, _raw: raw })
}

fn map_get(map: &[(Value, Value)], key: i64) -> Option<&Value> {
    map.iter().find(|(k, _)| k.as_integer().and_then(|i| i64::try_from(i).ok()) == Some(key)).map(|(_, v)| v)
}

fn int(v: Option<&Value>) -> Option<i64> {
    v?.as_integer().and_then(|i| i64::try_from(i).ok())
}

fn bytes(v: Option<&Value>) -> Option<&[u8]> {
    v?.as_bytes().map(Vec::as_slice)
}

/// A credential public key we can verify with (ES256 P-256, EdDSA Ed25519, RS256 of 2048+ bits).
#[derive(Debug)]
enum CoseKey {
    Es256(Vec<u8>),
    Ed25519(Vec<u8>),
    Rs256 { n: Vec<u8>, e: Vec<u8> },
}

fn parse_cose(cose_key: &[u8]) -> Result<CoseKey, WebauthnError> {
    let key: Value =
        ciborium::de::from_reader(cose_key).map_err(|_| WebauthnError::Malformed("credentialPublicKey"))?;
    let map = key.as_map().ok_or(WebauthnError::Malformed("credentialPublicKey"))?;
    match (int(map_get(map, 1)), int(map_get(map, 3))) {
        (Some(2), Some(ES256)) => {
            match (int(map_get(map, -1)), bytes(map_get(map, -2)), bytes(map_get(map, -3))) {
                (Some(1), Some(x), Some(y)) if x.len() == 32 && y.len() == 32 => {
                    let mut point = Vec::with_capacity(65);
                    point.push(0x04);
                    point.extend_from_slice(x);
                    point.extend_from_slice(y);
                    Ok(CoseKey::Es256(point))
                }
                _ => Err(WebauthnError::Algorithm),
            }
        }
        (Some(1), Some(EDDSA)) => match (int(map_get(map, -1)), bytes(map_get(map, -2))) {
            (Some(6), Some(x)) if x.len() == 32 => Ok(CoseKey::Ed25519(x.to_vec())),
            _ => Err(WebauthnError::Algorithm),
        },
        (Some(3), Some(RS256)) => match (bytes(map_get(map, -1)), bytes(map_get(map, -2))) {
            (Some(n), Some(e)) if n.iter().skip_while(|b| **b == 0).count() >= 256 => {
                Ok(CoseKey::Rs256 { n: n.to_vec(), e: e.to_vec() })
            }
            _ => Err(WebauthnError::Algorithm),
        },
        _ => Err(WebauthnError::Algorithm),
    }
}

fn verify_cose(cose_key: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), WebauthnError> {
    let ok = match parse_cose(cose_key)? {
        CoseKey::Es256(point) => {
            UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, point).verify(msg, sig).is_ok()
        }
        CoseKey::Ed25519(x) => UnparsedPublicKey::new(&signature::ED25519, x).verify(msg, sig).is_ok(),
        CoseKey::Rs256 { n, e } => RsaPublicKeyComponents { n: &n, e: &e }
            .verify(&signature::RSA_PKCS1_2048_8192_SHA256, msg, sig)
            .is_ok(),
    };
    if ok { Ok(()) } else { Err(WebauthnError::Signature) }
}

/// Registration ceremony (§7.1): client data, RP ID hash, user presence, credential and key.
pub fn verify_registration(
    rp: &RelyingParty,
    challenge: &[u8],
    client_data_json: &[u8],
    attestation_object: &[u8],
) -> Result<NewCredential, WebauthnError> {
    check_client_data(rp, client_data_json, "webauthn.create", challenge)?;
    let att: Value = ciborium::de::from_reader(attestation_object)
        .map_err(|_| WebauthnError::Malformed("attestationObject"))?;
    let map = att.as_map().ok_or(WebauthnError::Malformed("attestationObject"))?;
    let auth_data = map
        .iter()
        .find(|(k, _)| k.as_text() == Some("authData"))
        .and_then(|(_, v)| v.as_bytes())
        .ok_or(WebauthnError::Malformed("authData"))?;
    let ad = parse_auth_data(rp, auth_data)?;
    let (credential_id, public_key) =
        ad.attested.ok_or(WebauthnError::Malformed("attestedCredentialData"))?;
    if credential_id.len() < 16 {
        return Err(WebauthnError::Malformed("credentialId"));
    }
    parse_cose(&public_key)?;
    Ok(NewCredential {
        credential_id,
        public_key,
        sign_count: ad.sign_count,
        user_verified: ad.flags & FLAG_UV != 0,
        backup_eligible: ad.flags & FLAG_BE != 0,
        backup_state: ad.flags & FLAG_BS != 0,
    })
}

/// Authentication ceremony (§7.2): client data, RP ID hash, user presence, signature over
/// `authenticatorData || SHA-256(clientDataJSON)`, and the signature counter (§6.1.1).
pub fn verify_assertion(
    rp: &RelyingParty,
    challenge: &[u8],
    client_data_json: &[u8],
    authenticator_data: &[u8],
    sig: &[u8],
    public_key: &[u8],
    stored_sign_count: u32,
) -> Result<Assertion, WebauthnError> {
    check_client_data(rp, client_data_json, "webauthn.get", challenge)?;
    let ad = parse_auth_data(rp, authenticator_data)?;
    let mut msg = authenticator_data.to_vec();
    msg.extend_from_slice(&Sha256::digest(client_data_json));
    verify_cose(public_key, &msg, sig)?;
    if (ad.sign_count != 0 || stored_sign_count != 0) && ad.sign_count <= stored_sign_count {
        return Err(WebauthnError::Counter);
    }
    Ok(Assertion {
        sign_count: ad.sign_count,
        user_verified: ad.flags & FLAG_UV != 0,
        backup_state: ad.flags & FLAG_BS != 0,
    })
}

/// A `PublicKeyCredential` serialized like the browser's `toJSON()` (binary fields base64url).
/// Unknown fields are tolerated here: browsers add members over time.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialJson {
    pub id: String,
    #[serde(default)]
    pub raw_id: Option<String>,
    #[serde(rename = "type")]
    pub typ: String,
    pub response: ResponseJson,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseJson {
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    #[serde(default)]
    pub attestation_object: Option<String>,
    #[serde(default)]
    pub authenticator_data: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub transports: Option<Vec<String>>,
}

impl CredentialJson {
    /// The credential id (raw bytes), checked against `rawId` when both are present.
    pub fn credential_id(&self) -> Result<Vec<u8>, WebauthnError> {
        if self.typ != "public-key" {
            return Err(WebauthnError::Malformed("credential type"));
        }
        let id = unb64(&self.id)?;
        if let Some(raw) = &self.raw_id
            && unb64(raw)? != id
        {
            return Err(WebauthnError::Malformed("rawId"));
        }
        Ok(id)
    }
}

#[cfg(test)]
#[path = "../tests/support/passkey.rs"]
mod soft;

#[cfg(test)]
mod tests {
    use super::soft::{SoftKey, UP, UV};
    use super::*;
    use proptest::prelude::*;

    fn rp() -> RelyingParty {
        RelyingParty::from_public_url("http://127.0.0.1:14173", None)
    }

    fn registered(key: &SoftKey) -> NewCredential {
        let (cd, att) = key.register_raw(b"0123456789abcdef0123456789abcdef");
        verify_registration(&rp(), b"0123456789abcdef0123456789abcdef", &cd, &att).expect("registers")
    }

    #[test]
    fn rp_from_public_url() {
        assert_eq!(rp().id, "localhost");
        assert_eq!(rp().origins, ["http://localhost:14173"]);
        let prod = RelyingParty::from_public_url("https://Procera.Example.com/app", None);
        assert_eq!(
            (prod.id.as_str(), prod.origins[0].as_str()),
            ("procera.example.com", "https://procera.example.com")
        );
        let custom = RelyingParty::from_public_url("https://app.example.com:8443", Some("example.com"));
        assert_eq!(
            (custom.id.as_str(), custom.origins[0].as_str()),
            ("example.com", "https://app.example.com:8443")
        );
        assert_eq!(RelyingParty::from_public_url("http://[::1]:80", None).origins, ["http://localhost:80"]);
    }

    #[test]
    fn registration_then_assertion() {
        let mut key = SoftKey::new("localhost", "http://localhost:14173");
        let cred = registered(&key);
        assert_eq!(cred.credential_id, key.id);
        assert!(cred.user_verified);
        let (cd, ad, sig) = key.assert_raw(b"challenge-two-challenge-two-1234");
        let a =
            verify_assertion(&rp(), b"challenge-two-challenge-two-1234", &cd, &ad, &sig, &cred.public_key, 0)
                .expect("asserts");
        assert_eq!(a.sign_count, 1);
        // The same response again: the counter did not move (and the server consumes challenges).
        assert_eq!(
            verify_assertion(&rp(), b"challenge-two-challenge-two-1234", &cd, &ad, &sig, &cred.public_key, 1),
            Err(WebauthnError::Counter)
        );
    }

    #[test]
    fn registration_rejections() {
        let ch = b"0123456789abcdef0123456789abcdef";
        let key = SoftKey::new("localhost", "http://localhost:14173");
        let (cd, att) = key.register_raw(ch);
        assert_eq!(
            verify_registration(&rp(), b"another-challenge-another-chall", &cd, &att),
            Err(WebauthnError::Challenge)
        );
        let evil = SoftKey::new("localhost", "http://evil.test");
        let (cd2, att2) = evil.register_raw(ch);
        assert_eq!(verify_registration(&rp(), ch, &cd2, &att2), Err(WebauthnError::Origin));
        let other_rp = SoftKey::new("example.com", "http://localhost:14173");
        let (cd3, att3) = other_rp.register_raw(ch);
        assert_eq!(verify_registration(&rp(), ch, &cd3, &att3), Err(WebauthnError::RpId));
        let mut absent = SoftKey::new("localhost", "http://localhost:14173");
        absent.flags = UV;
        let (cd4, att4) = absent.register_raw(ch);
        assert_eq!(verify_registration(&rp(), ch, &cd4, &att4), Err(WebauthnError::UserPresence));
        let get = key.client_data("webauthn.get", ch);
        assert_eq!(verify_registration(&rp(), ch, &get, &att), Err(WebauthnError::WrongType));
    }

    #[test]
    fn assertion_rejections() {
        let ch = b"0123456789abcdef0123456789abcdef";
        let mut key = SoftKey::new("localhost", "http://localhost:14173");
        let cred = registered(&key);
        let (cd, ad, mut sig) = key.assert_raw(ch);
        let mut tampered = ad.clone();
        tampered[32] |= 0x10;
        assert_eq!(
            verify_assertion(&rp(), ch, &cd, &tampered, &sig, &cred.public_key, 0),
            Err(WebauthnError::Signature)
        );
        let other = SoftKey::new("localhost", "http://localhost:14173");
        assert_eq!(
            verify_assertion(&rp(), ch, &cd, &ad, &sig, &other.cose(), 0),
            Err(WebauthnError::Signature)
        );
        let last = sig.len() - 1;
        sig[last] ^= 1;
        assert_eq!(
            verify_assertion(&rp(), ch, &cd, &ad, &sig, &cred.public_key, 0),
            Err(WebauthnError::Signature)
        );
        key.flags = UP; // no user verification: still a valid possession factor
        let (cd, ad, sig) = key.assert_raw(ch);
        let a =
            verify_assertion(&rp(), ch, &cd, &ad, &sig, &cred.public_key, 1).expect("UP only is accepted");
        assert!(!a.user_verified);
    }

    #[test]
    fn credential_json_ids() {
        let key = SoftKey::new("localhost", "http://localhost:14173");
        let c: CredentialJson = serde_json::from_value(key.register(b"0123456789abcdef")).expect("json");
        assert_eq!(c.credential_id(), Ok(key.id.clone()));
        let mut bad = c.clone();
        bad.raw_id = Some(b64(b"something-else-entirely"));
        assert!(bad.credential_id().is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_input_never_panics(cd in proptest::collection::vec(any::<u8>(), 0..300),
                                        blob in proptest::collection::vec(any::<u8>(), 0..300),
                                        sig in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = verify_registration(&rp(), b"c", &cd, &blob);
            let _ = verify_assertion(&rp(), b"c", &cd, &blob, &sig, &blob, 0);
            let _ = client_challenge(&cd);
        }

        #[test]
        fn mutated_authenticator_data_is_rejected(i in 0usize..37, bit in 0u8..8) {
            let ch = b"0123456789abcdef0123456789abcdef";
            let mut key = SoftKey::new("localhost", "http://localhost:14173");
            let cred = registered(&key);
            let (cd, mut ad, sig) = key.assert_raw(ch);
            ad[i] ^= 1 << bit;
            prop_assert!(verify_assertion(&rp(), ch, &cd, &ad, &sig, &cred.public_key, 0).is_err());
        }
    }
}
