//! Identity (BPMS-021/022, ADR-0020): Argon2id passwords, JWT access tokens + rotating refresh
//! tokens, login throttling, TOTP and WebAuthn passkey MFA with recovery codes. Audited in the system chain.

pub mod jwt;
pub mod password;
mod service;
pub mod totp;
pub mod webauthn;

pub use service::{
    AccountNotice, CHANGE_TOKEN_TTL, ClientInfo, Factor, Identity, IdentityConfig, IdentityError,
    LoginOutcome, Passkey, RESET_TTL, RESETS_PER_HOUR, ResetLink, Tokens, TotpEnrollment, User,
};
