//! BPMS-065 (CFG-006): platform email settings — round trip, the password is sealed and never
//! readable, the environment wins, hot reload swaps the transport, every change is audited.

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::test_support;
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_notify::smtp_config::{EmailConfigInput, TlsMode};
use procera_notify::{Mailers, Notify, NotifyError, Source};
use serde_json::{Value, json};
use uuid::Uuid;

fn input(from: &str, password: Option<&str>) -> EmailConfigInput {
    EmailConfigInput {
        host: "localhost".into(),
        port: 1,
        tls_mode: TlsMode::None,
        username: Some("mailer".into()),
        password: password.map(str::to_owned),
        from_address: from.into(),
        from_name: Some("Acme".into()),
        reply_to: Some("help@acme.test".into()),
        reason: "set up email".into(),
    }
}

#[tokio::test]
async fn email_settings_round_trip_hot_reload_and_audit() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let n = Notify::new(t.db.clone(), keys, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let user = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'sys@acme.test', 'Sys', $2)",
    )
    .bind(user)
    .bind(Uuid::new_v4())
    .execute(&mut admin)
    .await?;
    let client = (Some("127.0.0.1".to_owned()), Some("test".to_owned()));

    let mailers = Mailers::new(n.clone(), None)?;
    assert_eq!(mailers.source().await?, Source::None);
    assert!(mailers.current().await?.is_none(), "not set up: emails stay queued");
    assert!(n.email_config().await?.is_none());

    // Create: If-Match 0 when nothing exists; anything else is stale.
    assert!(matches!(
        n.set_email_config(user, 3, &input("a@acme.test", Some("hunter22")), client.clone()).await,
        Err(NotifyError::Stale { expected: 3, current: 0 })
    ));
    let c = n.set_email_config(user, 0, &input("a@acme.test", Some("hunter22")), client.clone()).await?;
    assert_eq!((c.revision, c.password_set, c.host.as_str()), (1, true, "localhost"));
    let api = serde_json::to_string(&n.email_config().await?)?;
    assert!(api.contains("\"password_set\":true") && !api.contains("hunter22"), "never returned: {api}");
    let raw: Vec<u8> =
        sqlx::query_scalar("SELECT password_sealed FROM platform_email_config").fetch_one(&mut admin).await?;
    assert!(!String::from_utf8_lossy(&raw).contains("hunter22"), "sealed at rest");
    assert!(procera_crypto::is_envelope(&raw));

    // Hot reload: the transport follows the stored generation without a restart.
    assert_eq!(mailers.source().await?, Source::Database);
    let first = mailers.current().await?.expect("configured");
    assert_eq!(first.from_mailbox().to_string(), "Acme <a@acme.test>");
    // A blank password keeps the stored one.
    let c2 = n.set_email_config(user, 1, &input("b@acme.test", Some("")), client.clone()).await?;
    assert_eq!((c2.revision, c2.password_set), (2, true));
    let kept: Vec<u8> =
        sqlx::query_scalar("SELECT password_sealed FROM platform_email_config").fetch_one(&mut admin).await?;
    assert_eq!(kept, raw, "kept as is");
    let second = mailers.current().await?.expect("configured");
    assert_eq!(second.from_mailbox().to_string(), "Acme <b@acme.test>", "rebuilt after the change");
    // Clearing the username removes the password.
    let mut no_auth = input("b@acme.test", None);
    no_auth.username = None;
    assert!(!n.set_email_config(user, 2, &no_auth, client.clone()).await?.password_set);

    // The test send reports a safe, classified error (nothing listens on port 1).
    let probe = mailers.for_test().await?.expect("configured");
    let failure = probe.send_test("zoe@acme.test").await.expect_err("unreachable");
    assert_eq!(failure.kind, "connection_refused");
    n.record_email_test(user, "zoe@acme.test", &json!({"sent": false, "kind": failure.kind}), client.clone())
        .await?;

    // The environment wins over the stored settings.
    let env = Mailers::new(n.clone(), Some(("smtp://relay.test:2525", "Ops <ops@acme.test>")))?;
    assert_eq!(env.source().await?, Source::Environment);
    assert_eq!(env.current().await?.expect("env").from_mailbox().to_string(), "Ops <ops@acme.test>");

    // Audit: three changes and one test on the system chain, never the password.
    let rows: Vec<(String, Option<Value>, Option<String>)> = sqlx::query_as(
        "SELECT action, diff, reason FROM audit_events WHERE workspace_id = '00000000-0000-0000-0000-000000000000'
         AND action LIKE 'platform.email_%' ORDER BY seq",
    )
    .fetch_all(&mut admin)
    .await?;
    let actions: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        actions,
        [
            "platform.email_changed",
            "platform.email_changed",
            "platform.email_changed",
            "platform.email_tested"
        ]
    );
    let diffs: Vec<String> = rows.iter().map(|r| r.1.clone().unwrap_or_default().to_string()).collect();
    assert!(diffs.iter().all(|d| !d.contains("hunter22")));
    assert!(diffs[0].contains("\"password\":\"set\"") && diffs[1].contains("\"password\":\"kept\""));
    assert!(diffs[2].contains("\"password\":\"removed\""));
    assert!(diffs[3].contains("\"recipient_domain\":\"acme.test\"") && !diffs[3].contains("zoe@"));
    assert_eq!(rows[0].2.as_deref(), Some("set up email"));
    Ok(())
}
