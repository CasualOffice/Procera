//! A software ES256 authenticator for tests (BPMS-022): produces registration and assertion
//! responses in the browser's JSON shape, signed with a fresh P-256 key.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ciborium::Value;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

pub const UP: u8 = 0x01;
pub const UV: u8 = 0x04;
pub const AT: u8 = 0x40;

pub struct SoftKey {
    kp: EcdsaKeyPair,
    rng: SystemRandom,
    pub id: Vec<u8>,
    pub counter: u32,
    pub rp_id: String,
    pub origin: String,
    pub flags: u8,
}

fn b64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

impl SoftKey {
    pub fn new(rp_id: &str, origin: &str) -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let kp = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
        let id = Sha256::digest(pkcs8.as_ref()).to_vec();
        Self { kp, rng, id, counter: 0, rp_id: rp_id.into(), origin: origin.into(), flags: UP | UV }
    }

    pub fn cose(&self) -> Vec<u8> {
        let p = self.kp.public_key().as_ref();
        let key = Value::Map(vec![
            (Value::from(1), Value::from(2)),
            (Value::from(3), Value::from(-7)),
            (Value::from(-1), Value::from(1)),
            (Value::from(-2), Value::Bytes(p[1..33].to_vec())),
            (Value::from(-3), Value::Bytes(p[33..65].to_vec())),
        ]);
        let mut out = Vec::new();
        ciborium::ser::into_writer(&key, &mut out).unwrap();
        out
    }

    pub fn auth_data(&self, flags: u8, attested: bool) -> Vec<u8> {
        let mut d = Sha256::digest(self.rp_id.as_bytes()).to_vec();
        d.push(flags | if attested { AT } else { 0 });
        d.extend_from_slice(&self.counter.to_be_bytes());
        if attested {
            d.extend_from_slice(&[0u8; 16]);
            d.extend_from_slice(&u16::try_from(self.id.len()).unwrap().to_be_bytes());
            d.extend_from_slice(&self.id);
            d.extend_from_slice(&self.cose());
        }
        d
    }

    pub fn client_data(&self, typ: &str, challenge: &[u8]) -> Vec<u8> {
        serde_json::to_vec(
            &json!({"type": typ, "challenge": b64(challenge), "origin": self.origin, "crossOrigin": false}),
        )
        .unwrap()
    }

    /// `navigator.credentials.create()` → (clientDataJSON, attestationObject).
    pub fn register_raw(&self, challenge: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let att = Value::Map(vec![
            (Value::from("fmt"), Value::from("none")),
            (Value::from("attStmt"), Value::Map(vec![])),
            (Value::from("authData"), Value::Bytes(self.auth_data(self.flags, true))),
        ]);
        let mut out = Vec::new();
        ciborium::ser::into_writer(&att, &mut out).unwrap();
        (self.client_data("webauthn.create", challenge), out)
    }

    pub fn register(&self, challenge: &[u8]) -> Json {
        let (cd, att) = self.register_raw(challenge);
        json!({"id": b64(&self.id), "rawId": b64(&self.id), "type": "public-key",
               "response": {"clientDataJSON": b64(&cd), "attestationObject": b64(&att), "transports": ["internal"]}})
    }

    /// `navigator.credentials.get()` → (clientDataJSON, authenticatorData, signature). Bumps the counter.
    pub fn assert_raw(&mut self, challenge: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        self.counter += 1;
        let cd = self.client_data("webauthn.get", challenge);
        let ad = self.auth_data(self.flags, false);
        let mut msg = ad.clone();
        msg.extend_from_slice(&Sha256::digest(&cd));
        let sig = self.kp.sign(&self.rng, &msg).unwrap().as_ref().to_vec();
        (cd, ad, sig)
    }

    pub fn assert(&mut self, challenge: &[u8]) -> Json {
        let (cd, ad, sig) = self.assert_raw(challenge);
        json!({"id": b64(&self.id), "rawId": b64(&self.id), "type": "public-key",
               "response": {"clientDataJSON": b64(&cd), "authenticatorData": b64(&ad), "signature": b64(&sig)}})
    }
}

/// The challenge bytes from a `{public_key: {challenge}}` options response.
pub fn challenge_of(options: &Json) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(options["public_key"]["challenge"].as_str().unwrap()).unwrap()
}
