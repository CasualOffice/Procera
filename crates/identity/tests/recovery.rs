//! OQ-15 / A-39 (docs/06 §3.6.1): account recovery and admin account controls against a throwaway
//! Postgres (PROCERA_TEST_DATABASE_URL). Skips without one.

use std::sync::Arc;

use procera_crypto::{LocalKey, hash_token};
use procera_db::{SYSTEM_WORKSPACE, WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{
    ClientInfo, Identity, IdentityConfig, IdentityError, LoginOutcome, RESETS_PER_HOUR, totp,
};
use procera_kernel::clock::ManualClock;
use procera_kernel::{Clock, UuidV7Gen};
use uuid::Uuid;

const PW: &str = "correct horse battery staple";
const NEW: &str = "a brand new passphrase here";

async fn setup() -> Option<anyhow::Result<(test_support::TestDb, Identity, Arc<ManualClock>)>> {
    let t = match test_support::fresh().await? {
        Ok(t) => t,
        Err(e) => return Some(Err(e.into())),
    };
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).ok()?);
    let clock = Arc::new(ManualClock::new(time::OffsetDateTime::now_utc()));
    let cfg = IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() };
    let id = Identity::new(t.db.clone(), keys, clock.clone(), Arc::new(UuidV7Gen), cfg).ok()?;
    Some(Ok((t, id, clock)))
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

fn tokens(o: LoginOutcome) -> procera_identity::Tokens {
    match o {
        LoginOutcome::Authenticated(t) => *t,
        _ => panic!("expected a session"),
    }
}

/// Audit actions on the system chain, oldest first.
async fn system_actions(t: &test_support::TestDb) -> anyhow::Result<Vec<String>> {
    let mut tx = t.db.begin_system().await?;
    let v = sqlx::query_scalar("SELECT action FROM audit_events ORDER BY seq").fetch_all(tx.conn()).await?;
    tx.rollback().await?;
    Ok(v)
}

#[tokio::test]
async fn reset_link_is_hashed_single_use_and_revokes_every_session() -> anyhow::Result<()> {
    let (t, id, _clock) = setup_or_skip!();
    let u = id.create_user("ann@acme.test", "Ann", PW, &client("10.0.0.1")).await?;
    let s1 = tokens(id.login("ann@acme.test", PW, &client("10.0.0.1")).await?);
    let s2 = tokens(id.login("ann@acme.test", PW, &client("10.0.0.2")).await?);

    // Unknown accounts get the same answer and nothing to send.
    assert!(id.request_password_reset("nobody@acme.test", &client("10.0.0.9")).await?.is_none());
    let link = id.request_password_reset(" ANN@acme.test ", &client("10.0.0.9")).await?.expect("a link");
    assert_eq!(link.user_id, u.id);
    assert_eq!(link.email, "ann@acme.test");
    // Only the SHA-256 is stored; 32 random bytes (43 base64url characters).
    assert_eq!(link.token.len(), 43);
    let mut tx = t.db.begin_system().await?;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM password_resets WHERE id = $1")
        .bind(link.reset_id)
        .fetch_one(tx.conn())
        .await?;
    tx.rollback().await?;
    assert_eq!(stored, hash_token(&link.token));
    assert!(id.check_reset_token(&link.token).await?);
    assert!(!id.check_reset_token("not-a-token").await?);

    // The policy applies and a rejected password leaves the link usable.
    assert!(matches!(
        id.reset_password(&link.token, "short", &client("10.0.0.9")).await,
        Err(IdentityError::Policy(_))
    ));
    assert!(id.check_reset_token(&link.token).await?);
    let n = id.reset_password(&link.token, NEW, &client("10.0.0.9")).await?;
    assert_eq!(n.email, "ann@acme.test");
    // Single use.
    assert!(!id.check_reset_token(&link.token).await?);
    assert!(matches!(
        id.reset_password(&link.token, NEW, &client("10.0.0.9")).await,
        Err(IdentityError::InvalidToken)
    ));
    // Every session and refresh token is revoked; outstanding access tokens die with the version bump.
    assert!(id.refresh(&s1.refresh_token, &client("10.0.0.1")).await.is_err());
    assert!(id.refresh(&s2.refresh_token, &client("10.0.0.2")).await.is_err());
    let claims = id.verify_access(&s1.access_token).expect("signature still valid");
    assert!(id.token_state(u.id).await?.expect("user").1 > claims.tv);
    // The old password is gone, the new one works.
    assert!(id.login("ann@acme.test", PW, &client("10.0.0.1")).await.is_err());
    tokens(id.login("ann@acme.test", NEW, &client("10.0.0.1")).await?);
    let actions = system_actions(&t).await?;
    assert!(
        actions.contains(&"password.reset_requested".to_owned())
            && actions.contains(&"password.reset".to_owned())
    );
    assert_eq!(
        procera_audit::verify(&t.db, WsCtx { workspace_id: SYSTEM_WORKSPACE }).await?.first_broken,
        None
    );
    Ok(())
}

#[tokio::test]
async fn reset_links_expire_newer_ones_void_older_and_requests_are_rate_limited() -> anyhow::Result<()> {
    let (_t, id, clock) = setup_or_skip!();
    id.create_user("ben@acme.test", "Ben", PW, &client("10.0.0.1")).await?;
    let first = id.request_password_reset("ben@acme.test", &client("10.0.0.1")).await?.expect("link");
    let second = id.request_password_reset("ben@acme.test", &client("10.0.0.1")).await?.expect("link");
    assert!(!id.check_reset_token(&first.token).await?, "a newer request voids older links");
    assert!(id.reset_password(&first.token, NEW, &client("10.0.0.1")).await.is_err());
    assert!(id.check_reset_token(&second.token).await?);
    // 3 per account per hour: the 4th is accepted but sends nothing.
    let third = id.request_password_reset("ben@acme.test", &client("10.0.0.1")).await?;
    assert!(third.is_some() && RESETS_PER_HOUR == 3);
    assert!(id.request_password_reset("ben@acme.test", &client("10.0.0.1")).await?.is_none());
    let third = third.expect("link");
    // 30 minutes.
    clock.advance(time::Duration::minutes(31));
    assert!(!id.check_reset_token(&third.token).await?);
    assert!(matches!(
        id.reset_password(&third.token, NEW, &client("10.0.0.1")).await,
        Err(IdentityError::InvalidToken)
    ));
    // An hour later the budget is back.
    clock.advance(time::Duration::minutes(30));
    assert!(id.request_password_reset("ben@acme.test", &client("10.0.0.1")).await?.is_some());
    Ok(())
}

#[tokio::test]
async fn reset_keeps_two_step_verification_and_admin_reset_removes_it() -> anyhow::Result<()> {
    let (t, id, clock) = setup_or_skip!();
    let u = id.create_user("cat@acme.test", "Cat", PW, &client("10.0.0.1")).await?;
    let e = id.totp_begin(u.id, &client("10.0.0.1")).await?;
    let secret = data_encoding::BASE32_NOPAD.decode(e.secret.as_bytes())?;
    let now = clock.now().unix_timestamp();
    id.totp_confirm(u.id, &totp::code_at(&secret, now / 30), &client("10.0.0.1")).await?;

    let link = id.request_password_reset("cat@acme.test", &client("10.0.0.1")).await?.expect("link");
    id.reset_password(&link.token, NEW, &client("10.0.0.1")).await?;
    // The next sign-in still asks for the second factor (A-39).
    assert!(matches!(
        id.login("cat@acme.test", NEW, &client("10.0.0.1")).await?,
        LoginOutcome::MfaRequired { .. }
    ));

    let admin = Uuid::new_v4();
    id.reset_mfa(u.id, admin, "lost phone", &client("10.0.0.5")).await?;
    let me = id.me(u.id).await?.expect("user");
    assert!(!me.mfa_enabled && me.mfa_methods.is_empty());
    let mut tx = t.db.begin_system().await?;
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_credentials WHERE user_id = $1 AND kind <> 'password'")
            .bind(u.id)
            .fetch_one(tx.conn())
            .await?;
    let (actor, object, reason): (Option<Uuid>, Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT actor_subject_ref, object_id, reason FROM audit_events WHERE action = 'mfa.reset'",
    )
    .fetch_one(tx.conn())
    .await?;
    tx.rollback().await?;
    assert_eq!(left, 0, "TOTP and recovery codes are gone");
    assert_eq!((actor, object), (Some(admin), Some(me.subject_ref)));
    assert!(reason.unwrap_or_default().starts_with("lost phone"));
    // The password is kept: signing in works without a second step now.
    tokens(id.login("cat@acme.test", NEW, &client("10.0.0.1")).await?);
    Ok(())
}

#[tokio::test]
async fn change_password_checks_the_current_one_and_keeps_only_this_session() -> anyhow::Result<()> {
    let (t, id, _clock) = setup_or_skip!();
    let u = id.create_user("dan@acme.test", "Dan", PW, &client("10.0.0.1")).await?;
    let here = tokens(id.login("dan@acme.test", PW, &client("10.0.0.1")).await?);
    let there = tokens(id.login("dan@acme.test", PW, &client("10.0.0.2")).await?);
    let claims = id.verify_access(&here.access_token).expect("token");

    // A wrong current password fails and counts toward the lockout.
    assert!(matches!(
        id.change_password(&claims, "wrong password!!", NEW, &client("10.0.0.1")).await,
        Err(IdentityError::InvalidCredentials)
    ));
    let mut tx = t.db.begin_system().await?;
    let failures: i32 =
        sqlx::query_scalar("SELECT failures FROM auth_throttle WHERE key = 'email:dan@acme.test'")
            .fetch_one(tx.conn())
            .await?;
    tx.rollback().await?;
    assert_eq!(failures, 1);
    assert!(matches!(
        id.change_password(&claims, PW, "short", &client("10.0.0.1")).await,
        Err(IdentityError::Policy(_))
    ));

    let (token, _, notice) = id.change_password(&claims, PW, NEW, &client("10.0.0.1")).await?;
    assert_eq!(notice.email, "dan@acme.test");
    let fresh = id.verify_access(&token).expect("new access token");
    let (_, tv) = id.token_state(u.id).await?.expect("user");
    assert_eq!(fresh.tv, tv, "the current session continues with the new token");
    assert!(claims.tv < tv, "the old access tokens are invalidated");
    assert_eq!(fresh.sid, claims.sid);
    // This session's refresh token still works; the other sign-in is revoked.
    id.refresh(&here.refresh_token, &client("10.0.0.1")).await?;
    assert!(id.refresh(&there.refresh_token, &client("10.0.0.2")).await.is_err());
    assert!(system_actions(&t).await?.contains(&"password.changed".to_owned()));
    Ok(())
}

#[tokio::test]
async fn required_change_blocks_sign_in_until_a_new_password_is_set() -> anyhow::Result<()> {
    let (t, id, _clock) = setup_or_skip!();
    let u = id.create_user("eve@acme.test", "Eve", PW, &client("10.0.0.1")).await?;
    let old = tokens(id.login("eve@acme.test", PW, &client("10.0.0.1")).await?);
    let admin = Uuid::new_v4();
    id.require_password_change(u.id, admin, "shared on a sticky note", &client("10.0.0.5")).await?;

    let change_token = match id.login("eve@acme.test", PW, &client("10.0.0.1")).await? {
        LoginOutcome::PasswordChangeRequired { change_token } => change_token,
        _ => panic!("a password change must be required"),
    };
    // A wrong password still looks like any failed sign-in.
    assert!(matches!(
        id.login("eve@acme.test", "wrong password!!", &client("10.0.0.1")).await,
        Err(IdentityError::InvalidCredentials)
    ));
    // The change token is not an access or MFA token.
    assert!(id.verify_access(&change_token).is_none());
    assert!(id.complete_mfa(&change_token, "000000", &client("10.0.0.1")).await.is_err());
    assert!(matches!(
        id.complete_expired_change(&change_token, "short", &client("10.0.0.1")).await,
        Err(IdentityError::Policy(_))
    ));

    let (outcome, notice) = id.complete_expired_change(&change_token, NEW, &client("10.0.0.1")).await?;
    assert_eq!(notice.user_id, u.id);
    tokens(outcome); // no second factor on this account: straight to a session
    // Single use, and the old sessions are gone.
    assert!(
        id.complete_expired_change(&change_token, "another long passphrase", &client("10.0.0.1"))
            .await
            .is_err()
    );
    assert!(id.refresh(&old.refresh_token, &client("10.0.0.1")).await.is_err());
    tokens(id.login("eve@acme.test", NEW, &client("10.0.0.1")).await?);
    let actions = system_actions(&t).await?;
    for a in ["password.change_required", "login.password_change_required", "password.changed"] {
        assert!(actions.contains(&a.to_owned()), "{a} audited");
    }
    Ok(())
}

#[tokio::test]
async fn admin_sign_out_everywhere_and_reset_link_are_audited_with_the_admin_as_actor() -> anyhow::Result<()>
{
    let (t, id, _clock) = setup_or_skip!();
    let u = id.create_user("fay@acme.test", "Fay", PW, &client("10.0.0.1")).await?;
    let s = tokens(id.login("fay@acme.test", PW, &client("10.0.0.1")).await?);
    let admin = Uuid::new_v4();
    id.revoke_all_sessions(u.id, Some(admin), "laptop stolen", &client("10.0.0.5")).await?;
    assert!(id.refresh(&s.refresh_token, &client("10.0.0.1")).await.is_err());
    // An admin's reset link does not use the self-service budget.
    for _ in 0..RESETS_PER_HOUR {
        id.request_password_reset("fay@acme.test", &client("10.0.0.1")).await?;
    }
    let link = id.admin_reset_link(u.id, admin, "locked out", &client("10.0.0.5")).await?;
    assert!(id.check_reset_token(&link.token).await?);
    let mut tx = t.db.begin_system().await?;
    let rows: Vec<(String, Option<Uuid>, Option<String>)> = sqlx::query_as(
        "SELECT action, actor_subject_ref, reason FROM audit_events WHERE actor_subject_ref = $1 ORDER BY seq",
    )
    .bind(admin)
    .fetch_all(tx.conn())
    .await?;
    let by: Option<Uuid> = sqlx::query_scalar("SELECT requested_by FROM password_resets WHERE id = $1")
        .bind(link.reset_id)
        .fetch_one(tx.conn())
        .await?;
    tx.rollback().await?;
    assert_eq!(by, Some(admin));
    let actions: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(actions, ["sessions.revoked_all", "password.reset_requested"]);
    assert_eq!(rows[0].2.as_deref(), Some("laptop stolen"));
    Ok(())
}
