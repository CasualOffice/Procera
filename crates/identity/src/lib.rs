//! Identity (BPMS-021/022, ADR-0020): Argon2id passwords, JWT access tokens + rotating refresh
//! tokens, login throttling, TOTP and WebAuthn passkey MFA with recovery codes. Audited in the system chain.

pub mod jwt;
pub mod password;
mod service;
pub mod totp;
pub mod webauthn;

pub use service::{
    ClientInfo, Factor, Identity, IdentityConfig, IdentityError, LoginOutcome, Passkey, Tokens,
    TotpEnrollment, User,
};
