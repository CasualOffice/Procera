//! BPMS-021/022 integration tests against a throwaway Postgres (PROCERA_TEST_DATABASE_URL).

use std::sync::Arc;
use std::time::Duration;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{ClientInfo, Identity, IdentityConfig, IdentityError, LoginOutcome, totp};
use procera_kernel::{SystemClock, UuidV7Gen};

const PW: &str = "correct horse battery staple";

async fn setup() -> Option<anyhow::Result<(test_support::TestDb, Identity)>> {
    let t = match test_support::fresh().await? {
        Ok(t) => t,
        Err(e) => return Some(Err(e.into())),
    };
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).ok()?);
    let cfg = IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() };
    let id = Identity::new(t.db.clone(), keys, Arc::new(SystemClock), Arc::new(UuidV7Gen), cfg).ok()?;
    Some(Ok((t, id)))
}

macro_rules! setup_or_skip {
    () => {
        match setup().await {
            Some(r) => r?,
            None => return Ok(()),
        }
    };
}

fn client(ip: &str) -> ClientInfo {
    ClientInfo { ip: Some(ip.into()), user_agent: Some("test".into()) }
}

fn authed(o: LoginOutcome) -> procera_identity::Tokens {
    match o {
        LoginOutcome::Authenticated(t) => *t,
        _ => panic!("expected tokens"),
    }
}

#[tokio::test]
async fn signup_login_access_token_and_policy() -> anyhow::Result<()> {
    let (_t, id) = setup_or_skip!();
    assert!(matches!(
        id.create_user("a@acme.test", "A", "short", &client("10.0.0.1")).await,
        Err(IdentityError::Policy(_))
    ));
    let u = id.create_user("Alice@Acme.test", "Alice", PW, &client("10.0.0.1")).await?;
    assert_eq!(u.email, "alice@acme.test");
    assert!(matches!(
        id.create_user("alice@acme.test", "Dup", PW, &client("10.0.0.1")).await,
        Err(IdentityError::EmailTaken)
    ));
    let tokens = authed(id.login("ALICE@acme.test", PW, &client("10.0.0.1")).await?);
    let claims = id.verify_access(&tokens.access_token).expect("valid access token");
    assert_eq!(claims.sub, u.id);
    assert_eq!(claims.amr, vec!["pwd"]);
    assert_eq!(tokens.expires_in, 600);
    // Unknown user and wrong password are indistinguishable.
    let e1 = id.login("nobody@acme.test", PW, &client("10.0.0.2")).await.err().map(|e| e.to_string());
    let e2 = id
        .login("alice@acme.test", "wrong password!!", &client("10.0.0.3"))
        .await
        .err()
        .map(|e| e.to_string());
    assert_eq!(e1, e2);
    Ok(())
}

#[tokio::test]
async fn refresh_rotation_and_reuse_detection() -> anyhow::Result<()> {
    let (_t, id) = setup_or_skip!();
    id.create_user("bob@acme.test", "Bob", PW, &client("10.0.0.1")).await?;
    let t1 = authed(id.login("bob@acme.test", PW, &client("10.0.0.1")).await?);
    let t2 = id.refresh(&t1.refresh_token, &client("10.0.0.1")).await?;
    assert_ne!(t1.refresh_token, t2.refresh_token);
    // Reusing the rotated token revokes the family: even the newest token stops working.
    assert!(matches!(
        id.refresh(&t1.refresh_token, &client("10.0.0.9")).await,
        Err(IdentityError::RefreshReused)
    ));
    assert!(matches!(
        id.refresh(&t2.refresh_token, &client("10.0.0.1")).await,
        Err(IdentityError::RefreshReused)
    ));
    // Logout revokes a fresh family.
    let t3 = authed(id.login("bob@acme.test", PW, &client("10.0.0.1")).await?);
    id.logout(&t3.refresh_token, &client("10.0.0.1")).await?;
    assert!(id.refresh(&t3.refresh_token, &client("10.0.0.1")).await.is_err());
    Ok(())
}

#[tokio::test]
async fn throttling_after_repeated_failures() -> anyhow::Result<()> {
    let (_t, id) = setup_or_skip!();
    id.create_user("carol@acme.test", "Carol", PW, &client("10.0.0.1")).await?;
    for _ in 0..5 {
        assert!(matches!(
            id.login("carol@acme.test", "nope nope nope", &client("10.9.9.9")).await,
            Err(IdentityError::InvalidCredentials)
        ));
    }
    // Even the right password is refused while the backoff is active.
    assert!(matches!(
        id.login("carol@acme.test", PW, &client("10.9.9.9")).await,
        Err(IdentityError::Throttled { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn totp_enrolment_mfa_login_recovery_codes_and_replay() -> anyhow::Result<()> {
    let (_t, id) = setup_or_skip!();
    let u = id.create_user("dana@acme.test", "Dana", PW, &client("10.0.0.1")).await?;
    let enr = id.totp_begin(u.id, &ClientInfo::default()).await?;
    assert!(enr.otpauth_uri.starts_with("otpauth://totp/Procera:dana%40acme.test?secret="));
    let secret = data_encoding::BASE32_NOPAD.decode(enr.secret.as_bytes())?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let codes = id.totp_confirm(u.id, &totp::code_at(&secret, now / 30), &client("10.0.0.1")).await?;
    assert_eq!(codes.len(), 10);

    let mfa_token = match id.login("dana@acme.test", PW, &client("10.0.0.1")).await? {
        LoginOutcome::MfaRequired { mfa_token, .. } => mfa_token,
        _ => panic!("MFA must be required"),
    };
    assert!(id.verify_access(&mfa_token).is_none(), "an MFA token is not an access token");
    assert!(matches!(
        id.complete_mfa(&mfa_token, "000000", &client("10.0.0.1")).await,
        Err(IdentityError::InvalidCode)
    ));
    // The code used at enrolment cannot be replayed; the next step's code works.
    let next = totp::code_at(&secret, now / 30 + 1);
    let tokens = id.complete_mfa(&mfa_token, &next, &client("10.0.0.1")).await?;
    assert_eq!(
        id.verify_access(&tokens.access_token).map(|c| c.amr),
        Some(vec!["pwd".to_owned(), "otp".to_owned()])
    );
    assert!(id.complete_mfa(&mfa_token, &next, &client("10.0.0.1")).await.is_err(), "same TOTP code twice");
    // Recovery codes are single use.
    id.complete_mfa(&mfa_token, &codes[0], &client("10.0.0.1")).await?;
    assert!(matches!(
        id.complete_mfa(&mfa_token, &codes[0], &client("10.0.0.1")).await,
        Err(IdentityError::InvalidCode)
    ));
    Ok(())
}

#[tokio::test]
async fn deactivation_revokes_everything_and_is_audited() -> anyhow::Result<()> {
    let (t, id) = setup_or_skip!();
    let u = id.create_user("erin@acme.test", "Erin", PW, &client("10.0.0.1")).await?;
    let tokens = authed(id.login("erin@acme.test", PW, &client("10.0.0.1")).await?);
    let claims = id.verify_access(&tokens.access_token).expect("token");
    id.deactivate(u.id, &client("10.0.0.1")).await?;
    let (status, tv) = id.token_state(u.id).await?.expect("user");
    assert_eq!(status, "disabled");
    assert!(tv > claims.tv, "outstanding access tokens are invalidated by the version bump");
    assert!(id.refresh(&tokens.refresh_token, &client("10.0.0.1")).await.is_err());
    assert!(matches!(
        id.login("erin@acme.test", PW, &client("10.0.0.1")).await,
        Err(IdentityError::InvalidCredentials)
    ));
    // All of this is in the system audit chain, which still verifies.
    let report = procera_audit::verify(&t.db, WsCtx { workspace_id: procera_db::SYSTEM_WORKSPACE }).await?;
    assert_eq!(report.first_broken, None);
    assert!(report.checked >= 5);
    let _ = Duration::from_secs(1);
    Ok(())
}

#[tokio::test]
async fn step_up_uses_totp_when_enrolled_and_password_otherwise() -> anyhow::Result<()> {
    let (_t, id) = setup_or_skip!();
    let c = client("10.0.0.9");
    // Without MFA: the password confirms; a wrong one is rejected; auth_time is refreshed.
    id.create_user("erin@acme.test", "Erin", PW, &c).await?;
    let tokens = authed(id.login("erin@acme.test", PW, &c).await?);
    let claims = id.verify_access(&tokens.access_token).expect("access token");
    let old = id
        .verify_access(
            &id.resign_with_auth_time(&claims, time::OffsetDateTime::now_utc() - time::Duration::hours(1))?,
        )
        .expect("re-signed");
    assert!(matches!(
        id.step_up(&old, None, Some("wrong password!!"), &c).await,
        Err(IdentityError::InvalidCredentials)
    ));
    let (fresh, _) = id.step_up(&old, None, Some(PW), &c).await?;
    let fresh = id.verify_access(&fresh).expect("fresh token");
    assert!(fresh.auth_time > old.auth_time);
    assert_eq!((fresh.sid, fresh.ws), (old.sid, old.ws));

    // With TOTP: the password is not enough; a current code works.
    let u = id.create_user("finn@acme.test", "Finn", PW, &c).await?;
    let enr = id.totp_begin(u.id, &c).await?;
    let secret = data_encoding::BASE32_NOPAD.decode(enr.secret.as_bytes())?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    id.totp_confirm(u.id, &totp::code_at(&secret, now / 30 - 1), &c).await?;
    let mfa_token = match id.login("finn@acme.test", PW, &c).await? {
        LoginOutcome::MfaRequired { mfa_token, .. } => mfa_token,
        _ => panic!("MFA must be required"),
    };
    let t = id.complete_mfa(&mfa_token, &totp::code_at(&secret, now / 30), &c).await?;
    let claims = id.verify_access(&t.access_token).expect("access token");
    assert!(matches!(id.step_up(&claims, None, Some(PW), &c).await, Err(IdentityError::InvalidCode)));
    let (up, _) = id.step_up(&claims, Some(&totp::code_at(&secret, now / 30 + 1)), None, &c).await?;
    assert_eq!(id.verify_access(&up).map(|c| c.amr), Some(vec!["pwd".to_owned(), "otp".to_owned()]));
    Ok(())
}

mod support {
    pub mod passkey;
}

#[allow(clippy::expect_used)]
fn cred(v: serde_json::Value) -> procera_identity::webauthn::CredentialJson {
    serde_json::from_value(v).expect("credential json")
}

#[tokio::test]
async fn passkey_registration_mfa_step_up_and_removal() -> anyhow::Result<()> {
    use procera_identity::Factor;
    use support::passkey::{SoftKey, challenge_of};
    let (t, id) = setup_or_skip!();
    let c = client("10.0.0.7");
    let u = id.create_user("gwen@acme.test", "Gwen", PW, &c).await?;
    let mut key = SoftKey::new("localhost", "http://localhost:8080");

    // Register: the options carry the RP, the user handle and a fresh challenge.
    let opts = id.passkey_registration_options(u.id).await?;
    assert_eq!(opts["public_key"]["rp"]["id"], "localhost");
    assert_eq!(opts["public_key"]["attestation"], "none");
    let ch = challenge_of(&opts);
    let (pk, codes) = id.passkey_register(u.id, "  Laptop  ", &cred(key.register(&ch)), &c).await?;
    assert_eq!(pk.name, "Laptop");
    assert_eq!(codes.map(|c| c.len()), Some(10), "the first second factor issues recovery codes");
    // The challenge is single use, and the same authenticator cannot be registered twice.
    assert!(matches!(
        id.passkey_register(u.id, "Again", &cred(key.register(&ch)), &c).await,
        Err(IdentityError::PasskeyInvalid(_))
    ));
    let opts = id.passkey_registration_options(u.id).await?;
    assert_eq!(opts["public_key"]["excludeCredentials"].as_array().map(Vec::len), Some(1));
    assert!(matches!(
        id.passkey_register(u.id, "Again", &cred(key.register(&challenge_of(&opts))), &c).await,
        Err(IdentityError::PasskeyExists)
    ));
    assert!(matches!(
        id.passkey_register(u.id, "", &cred(key.register(&challenge_of(&opts))), &c).await,
        Err(IdentityError::Invalid(_))
    ));
    assert_eq!(id.me(u.id).await?.map(|u| u.mfa_methods), Some(vec!["webauthn".to_owned()]));

    // Sign-in now needs the passkey; the methods say so.
    let (mfa_token, methods) = match id.login("gwen@acme.test", PW, &c).await? {
        LoginOutcome::MfaRequired { mfa_token, methods } => (mfa_token, methods),
        _ => panic!("MFA must be required"),
    };
    assert_eq!(methods, ["webauthn", "recovery_code"]);
    // A response made for another origin (phishing) is rejected, and the failure spends the challenge.
    let ch = challenge_of(&id.mfa_passkey_options(&mfa_token).await?);
    key.origin = "http://evil.test".into();
    assert!(matches!(
        id.complete_mfa_with(&mfa_token, Factor::Passkey(&cred(key.assert(&ch))), &c).await,
        Err(IdentityError::PasskeyRejected(r)) if r.contains("origin")
    ));
    key.origin = "http://localhost:8080".into();
    assert!(matches!(
        id.complete_mfa_with(&mfa_token, Factor::Passkey(&cred(key.assert(&ch))), &c).await,
        Err(IdentityError::PasskeyRejected(r)) if r.contains("challenge")
    ));
    let ch = challenge_of(&id.mfa_passkey_options(&mfa_token).await?);
    let tokens = id.complete_mfa_with(&mfa_token, Factor::Passkey(&cred(key.assert(&ch))), &c).await?;
    let claims = id.verify_access(&tokens.access_token).expect("access token");
    assert_eq!(claims.amr, ["pwd", "hwk"]);

    // Step-up with the passkey; a sign-in challenge cannot answer a step-up.
    let ch = challenge_of(&id.mfa_passkey_options(&mfa_token).await?);
    assert!(matches!(
        id.step_up_with(&claims, Some(Factor::Passkey(&cred(key.assert(&ch)))), &c).await,
        Err(IdentityError::PasskeyRejected(_))
    ));
    let ch = challenge_of(&id.step_up_passkey_options(&claims).await?);
    let (up, _) = id.step_up_with(&claims, Some(Factor::Passkey(&cred(key.assert(&ch)))), &c).await?;
    assert!(id.verify_access(&up).is_some_and(|u| u.auth_time >= claims.auth_time));
    // A cloned authenticator (counter going backwards) is refused.
    key.counter = 0;
    let ch = challenge_of(&id.step_up_passkey_options(&claims).await?);
    assert!(matches!(
        id.step_up_with(&claims, Some(Factor::Passkey(&cred(key.assert(&ch)))), &c).await,
        Err(IdentityError::PasskeyRejected(r)) if r.contains("counter")
    ));
    // The password alone does not step up an MFA account.
    assert!(id.step_up(&claims, None, Some(PW), &c).await.is_err());

    // Removal: unknown or foreign ids are 404; removing the last factor drops MFA and the codes.
    let list = id.passkeys(u.id).await?;
    assert_eq!(list.len(), 1);
    assert!(list[0].last_used_at.is_some());
    assert!(matches!(id.passkey_remove(u.id, uuid::Uuid::now_v7(), &c).await, Err(IdentityError::NotFound)));
    let other = id.create_user("hal@acme.test", "Hal", PW, &c).await?;
    assert!(matches!(id.passkey_remove(other.id, list[0].id, &c).await, Err(IdentityError::NotFound)));
    id.passkey_remove(u.id, list[0].id, &c).await?;
    assert!(matches!(id.login("gwen@acme.test", PW, &c).await?, LoginOutcome::Authenticated(_)));
    let report = procera_audit::verify(&t.db, WsCtx { workspace_id: procera_db::SYSTEM_WORKSPACE }).await?;
    assert_eq!(report.first_broken, None);
    Ok(())
}
