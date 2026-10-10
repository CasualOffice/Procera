//! Argon2id password hashing and policy (docs/06 §3.6).

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::{Algorithm, Argon2, Params, Version};

/// Hashing cost. Production: m = 64 MiB, t = 3, p = 1. Tests may use [`HashCost::FAST_FOR_TESTS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashCost {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl HashCost {
    pub const PRODUCTION: Self = Self { m_kib: 64 * 1024, t: 3, p: 1 };
    pub const FAST_FOR_TESTS: Self = Self { m_kib: 8 * 1024, t: 1, p: 1 };

    fn argon(self) -> Argon2<'static> {
        let params = Params::new(self.m_kib, self.t, self.p, None).unwrap_or_default();
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("password must be at least {0} characters")]
    TooShort(usize),
    #[error("password must be at most 128 characters")]
    TooLong,
    #[error("this password is too common; choose another")]
    Common,
    #[error("password must not contain your email address")]
    ContainsEmail,
}

/// A short offline list of the most common long passwords. A full HIBP k-anonymity prefix file can be
/// plugged in later (docs/06 §3.6); this list blocks the obvious ones without any network call.
const COMMON: &[&str] = &[
    "123456789012",
    "1234567890123",
    "password1234",
    "password12345",
    "passwordpassword",
    "qwertyuiop12",
    "qwertyuiopasdf",
    "iloveyou1234",
    "letmein12345",
    "welcome12345",
    "administrator",
    "changeme1234",
    "abcdefghijkl",
    "111111111111",
    "000000000000",
    "aaaaaaaaaaaa",
    "trustno11234",
    "procera12345",
];

pub fn check_policy(password: &str, email: &str, min_len: usize) -> Result<(), PolicyError> {
    let n = password.chars().count();
    if n < min_len {
        return Err(PolicyError::TooShort(min_len));
    }
    if n > 128 {
        return Err(PolicyError::TooLong);
    }
    let lower = password.to_lowercase();
    if COMMON.iter().any(|c| lower == *c) {
        return Err(PolicyError::Common);
    }
    let local = email.split('@').next().unwrap_or_default().to_lowercase();
    if local.len() >= 4 && lower.contains(&local) {
        return Err(PolicyError::ContainsEmail);
    }
    Ok(())
}

pub fn hash(password: &str, cost: HashCost) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(cost.argon().hash_password(password.as_bytes(), &salt)?.to_string())
}

/// Returns `(valid, needs_rehash)`.
pub fn verify(password: &str, phc: &str, cost: HashCost) -> (bool, bool) {
    let Ok(parsed) = PasswordHash::new(phc) else { return (false, false) };
    let ok = Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok();
    let current = format!("m={},t={},p={}", cost.m_kib, cost.t, cost.p);
    let stale = !phc.contains(&current);
    (ok, ok && stale)
}

/// Burns the same CPU as a real verification so unknown accounts don't answer faster (user enumeration).
pub fn dummy_verify(password: &str, cost: HashCost) {
    let _ = hash(password, cost);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        assert_eq!(check_policy("short", "a@b.c", 12), Err(PolicyError::TooShort(12)));
        assert_eq!(check_policy("password1234", "a@b.c", 12), Err(PolicyError::Common));
        assert_eq!(
            check_policy("my-alice.smith-secret", "alice.smith@acme.com", 12),
            Err(PolicyError::ContainsEmail)
        );
        assert_eq!(check_policy("correct horse battery staple", "alice@acme.com", 12), Ok(()));
    }

    #[test]
    fn hash_verify_and_rehash_detection() -> Result<(), argon2::password_hash::Error> {
        let h = hash("correct horse battery staple", HashCost::FAST_FOR_TESTS)?;
        assert!(h.starts_with("$argon2id$"));
        assert_eq!(verify("correct horse battery staple", &h, HashCost::FAST_FOR_TESTS), (true, false));
        assert_eq!(verify("wrong", &h, HashCost::FAST_FOR_TESTS), (false, false));
        assert_eq!(
            verify("correct horse battery staple", &h, HashCost { m_kib: 16 * 1024, t: 1, p: 1 }),
            (true, true)
        );
        Ok(())
    }
}
