//! Access tokens: HS256 JWTs (ADR-0020). The algorithm is pinned; anything else is rejected.

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ISSUER: &str = "procera";
pub const AUDIENCE: &str = "procera-api";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    /// User id.
    pub sub: Uuid,
    /// Refresh family (sign-in) id; lets logout and revocation be audited per sign-in.
    pub sid: Uuid,
    /// `workspace` or `system`.
    pub plane: String,
    /// Active workspace (authoritative tenant for the request); absent for the system plane.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ws: Option<Uuid>,
    pub amr: Vec<String>,
    pub auth_time: i64,
    /// User token version; a bump (deactivation, password reset) invalidates outstanding tokens.
    pub tv: i32,
    /// `access` or `mfa` (a short-lived token that only allows completing MFA).
    pub typ: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: Uuid,
}

#[derive(Clone)]
pub struct JwtKeys {
    enc: EncodingKey,
    dec: DecodingKey,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JwtKeys(**redacted**)")
    }
}

impl JwtKeys {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self { enc: EncodingKey::from_secret(secret), dec: DecodingKey::from_secret(secret) }
    }

    pub fn sign(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::new(Algorithm::HS256), claims, &self.enc)
    }

    /// Verifies signature, `exp` (≤ 60 s leeway), issuer, audience and token type.
    pub fn verify(&self, token: &str, typ: &str) -> Option<Claims> {
        let mut v = Validation::new(Algorithm::HS256);
        v.leeway = 60;
        v.set_issuer(&[ISSUER]);
        v.set_audience(&[AUDIENCE]);
        v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let c = decode::<Claims>(token, &self.dec, &v).ok()?.claims;
        (c.typ == typ).then_some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(typ: &str, exp: i64) -> Claims {
        Claims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: Uuid::now_v7(),
            sid: Uuid::now_v7(),
            plane: "workspace".into(),
            ws: None,
            amr: vec!["pwd".into()],
            auth_time: 0,
            tv: 1,
            typ: typ.into(),
            iat: 0,
            exp,
            jti: Uuid::now_v7(),
        }
    }

    #[test]
    fn sign_verify_and_rejections() -> Result<(), jsonwebtoken::errors::Error> {
        let k = JwtKeys::from_secret(&[7u8; 32]);
        let future = time::OffsetDateTime::now_utc().unix_timestamp() + 600;
        let t = k.sign(&claims("access", future))?;
        assert!(k.verify(&t, "access").is_some());
        assert!(k.verify(&t, "mfa").is_none(), "an access token is not an MFA token");
        assert!(JwtKeys::from_secret(&[8u8; 32]).verify(&t, "access").is_none(), "wrong key");
        assert!(k.verify(&k.sign(&claims("access", 1_000))?, "access").is_none(), "expired");
        // alg=none / tampered tokens are rejected.
        let parts: Vec<&str> = t.split('.').collect();
        let none_header = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
        assert!(k.verify(&format!("{none_header}.{}.", parts[1]), "access").is_none());
        Ok(())
    }
}
