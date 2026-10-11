//! BPMS-048: email queue (sealed, idempotent, retried with backoff) and the in-app inbox.

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_notify::{Notify, NotifyError, SmtpMailer};
use serde_json::json;
use uuid::Uuid;

async fn member(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection) -> anyhow::Result<Uuid> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .execute(tx.conn())
        .await?;
    Ok(m)
}

#[tokio::test]
async fn email_queue_inbox_and_retry() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let n = Notify::new(t.db.clone(), keys, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;

    let vars = json!({"inviter": "Ann", "workspace": "Acme", "link": "https://app.test/invite?token=secret-token", "expires": "18 Oct 2026"});
    let mut tx = t.db.begin_ws(ws).await?;
    assert!(n.queue_email(&mut tx, "invitation", "Zoe@Acme.test", &vars, "msg-1").await?);
    assert!(!n.queue_email(&mut tx, "invitation", "zoe@acme.test", &vars, "msg-1").await?, "deduplicated");
    // The token never sits in the database in plaintext.
    let raw: Vec<u8> =
        sqlx::query_scalar("SELECT body_sealed FROM email_messages").fetch_one(tx.conn()).await?;
    assert!(!String::from_utf8_lossy(&raw).contains("secret-token"));

    let m = member(&mut tx, &mut admin).await?;
    let other = member(&mut tx, &mut admin).await?;
    assert!(
        n.notify_member(
            &mut tx,
            m,
            "member_joined",
            &json!({"name": "Ben", "workspace": "Acme"}),
            Some("/members"),
            None,
            "msg-2"
        )
        .await?
    );
    assert!(
        !n.notify_member(
            &mut tx,
            m,
            "member_joined",
            &json!({"name": "Ben", "workspace": "Acme"}),
            None,
            None,
            "msg-2"
        )
        .await?
    );
    let (items, unread) = n.inbox(&mut tx, m, false, 50).await?;
    assert_eq!((items.len(), unread, items[0].title.as_str()), (1, 1, "Ben joined Acme"));
    assert!(
        matches!(n.mark_read(&mut tx, other, Some(items[0].id)).await, Err(NotifyError::NotFound)),
        "not your notification"
    );
    assert_eq!(n.mark_read(&mut tx, m, Some(items[0].id)).await?, 1);
    assert_eq!(n.inbox(&mut tx, m, true, 50).await?.1, 0);
    tx.commit().await?;

    let mail = n.emails_to(ws, "zoe@acme.test").await?;
    assert_eq!(mail.len(), 1);
    assert!(mail[0].text.contains("secret-token") && mail[0].subject.contains("Acme"));

    // An unreachable relay: the email stays queued with a backoff and the error recorded.
    let mailer = SmtpMailer::from_url("smtp://127.0.0.1:1", "Procera <no-reply@acme.test>")?;
    assert_eq!(n.send_due(&mailer).await?, (0, 1));
    let mut tx = t.db.begin_ws(ws).await?;
    let (state, attempts, err, due): (String, i32, Option<String>, bool) =
        sqlx::query_as("SELECT state, attempts, last_error, next_attempt_at > now() FROM email_messages")
            .fetch_one(tx.conn())
            .await?;
    tx.rollback().await?;
    assert_eq!((state.as_str(), attempts, due), ("queued", 1, true));
    assert!(err.is_some());
    assert_eq!(n.send_due(&mailer).await?, (0, 0), "backing off: not retried immediately");
    Ok(())
}
