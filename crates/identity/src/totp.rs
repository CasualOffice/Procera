//! RFC 6238 TOTP (SHA-1, 6 digits, 30 s, ±1 step) with replay protection (docs/06 §3.6).

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;

pub const STEP_SECS: i64 = 30;

pub fn new_secret() -> Vec<u8> {
    procera_crypto::random_bytes::<20>().to_vec()
}

pub fn secret_base32(secret: &[u8]) -> String {
    BASE32_NOPAD.encode(secret)
}

pub fn otpauth_uri(secret: &[u8], account: &str, issuer: &str) -> String {
    let enc = |s: &str| s.replace('%', "%25").replace(' ', "%20").replace(':', "%3A").replace('@', "%40");
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
        enc(issuer),
        enc(account),
        secret_base32(secret),
        enc(issuer)
    )
}

/// RFC 4226 HOTP value for counter `step`.
pub fn code_at(secret: &[u8], step: i64) -> String {
    let Ok(mut mac) = <Hmac<Sha1> as Mac>::new_from_slice(secret) else { return String::new() };
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = usize::from(h[19] & 0x0f);
    let bin = (u32::from(h[off] & 0x7f) << 24)
        | (u32::from(h[off + 1]) << 16)
        | (u32::from(h[off + 2]) << 8)
        | u32::from(h[off + 3]);
    format!("{:06}", bin % 1_000_000)
}

/// Verifies `code` at unix time `now` within ±1 step. Returns the matched step, which must be greater
/// than `last_used_step` (a code can be used once).
pub fn verify(secret: &[u8], code: &str, now_unix: i64, last_used_step: Option<i64>) -> Option<i64> {
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let current = now_unix.div_euclid(STEP_SECS);
    (current - 1..=current + 1)
        .filter(|s| last_used_step.is_none_or(|last| *s > last))
        .find(|s| code_at(secret, *s).as_bytes().ct_eq(code.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_vectors() {
        // RFC 6238 appendix B, 8-digit vectors truncated to 6 digits.
        let secret = b"12345678901234567890";
        assert_eq!(code_at(secret, 59 / 30), "287082");
        assert_eq!(code_at(secret, 1_111_111_109 / 30), "081804");
        assert_eq!(code_at(secret, 1_234_567_890 / 30), "005924");
    }

    #[test]
    fn window_and_replay() {
        let s = new_secret();
        let now = 1_800_000_000;
        let code = code_at(&s, now / STEP_SECS - 1);
        let step = verify(&s, &code, now, None);
        assert_eq!(step, Some(now / STEP_SECS - 1));
        assert_eq!(verify(&s, &code, now, step), None, "same code twice is rejected");
        assert_eq!(verify(&s, &code_at(&s, now / STEP_SECS + 3), now, None), None, "outside the window");
        assert_eq!(verify(&s, "12a456", now, None), None);
    }
}
