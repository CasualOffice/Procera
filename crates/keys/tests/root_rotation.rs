//! BPMS-030: `procera rotate-master-key`. Every root-sealed value (workspace KEKs, TOTP secrets,
//! invitation tokens, pre-envelope email bodies) opens after rotation with the new root key and no
//! longer with the old one; envelopes under the KEKs keep opening; a wrong current key or a repeat
//! of the same key changes nothing.

use std::sync::Arc;

use procera_crypto::{LocalKey, open, seal};
use procera_db::{SYSTEM_WORKSPACE, WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_keys::{KeyError, Keys, RootRotation, rotate_root};
use uuid::Uuid;

fn root() -> anyhow::Result<Arc<LocalKey>> {
    Ok(Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?))
}

#[tokio::test]
async fn rotation_reseals_every_root_sealed_value() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let (old, new) = (root()?, root()?);
    let keys_with = |k: Arc<LocalKey>| Keys::new(t.db.clone(), k, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let ws = WsCtx { workspace_id: Uuid::now_v7() };

    // Under the old root: a workspace envelope, a pre-envelope body, an invitation token, the
    // system plane's KEK and a user's TOTP secret.
    let (envelope_id, legacy_id, invitation_id, cred_id) =
        (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let mut tx = t.db.begin_ws(ws).await?;
    let envelope = keys_with(old.clone())
        .seal(&mut tx, &format!("email_messages:{envelope_id}"), b"enveloped body")
        .await?;
    let legacy = seal(old.as_ref(), "secrets", &format!("email_messages:{legacy_id}"), b"legacy body")?;
    for (id, body, key) in [(envelope_id, &envelope, "k1"), (legacy_id, &legacy, "k2")] {
        sqlx::query("INSERT INTO email_messages (workspace_id, id, kind, to_email, subject, body_sealed, dedupe_key, created_at) VALUES ($1, $2, 'invitation', 'a@acme.test', 's', $3, $4, now())")
            .bind(ws.workspace_id).bind(id).bind(body).bind(key).execute(tx.conn()).await?;
    }
    let token = seal(old.as_ref(), "secrets", &format!("invitations:{invitation_id}"), b"invite-token")?;
    sqlx::query("INSERT INTO invitations (workspace_id, id, email, role_keys, token_hash, token_sealed, expires_at) VALUES ($1, $2, 'b@acme.test', '{member}', $3, $4, now() + interval '7 days')")
        .bind(ws.workspace_id).bind(invitation_id).bind(invitation_id.as_bytes().to_vec()).bind(&token).execute(tx.conn()).await?;
    let user = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'c@acme.test', 'C', $2)",
    )
    .bind(user)
    .bind(Uuid::now_v7())
    .execute(tx.conn())
    .await?;
    let totp =
        seal(old.as_ref(), "secrets", &format!("user_credentials:{cred_id}:totp"), b"JBSWY3DPEHPK3PXP")?;
    sqlx::query("INSERT INTO user_credentials (id, user_id, kind, secret) VALUES ($1, $2, 'totp', $3)")
        .bind(cred_id)
        .bind(user)
        .bind(&totp)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    let mut sys = t.db.begin_system().await?;
    keys_with(old.clone()).active(&mut sys).await?;
    sys.commit().await?;

    // Same key twice: refused.
    assert!(matches!(
        rotate_root(&t.db, old.as_ref(), old.as_ref(), &SystemClock, &UuidV7Gen).await,
        Err(KeyError::SameRootKey)
    ));

    let done = rotate_root(&t.db, old.as_ref(), new.as_ref(), &SystemClock, &UuidV7Gen).await?;
    assert_eq!(
        done,
        RootRotation {
            workspaces: 2,
            workspace_keys: 2,
            totp_secrets: 1,
            invitation_tokens: 1,
            legacy_email_bodies: 1
        }
    );
    assert_eq!(done.total(), 5);

    // The workspace envelope opens with the new root (through its re-sealed KEK), not the old one.
    let mut tx = t.db.begin_ws(ws).await?;
    let ctx = format!("email_messages:{envelope_id}");
    assert_eq!(keys_with(new.clone()).open(&mut tx, &ctx, &envelope).await?.as_slice(), b"enveloped body");
    assert!(
        keys_with(old.clone()).open(&mut tx, &ctx, &envelope).await.is_err(),
        "the old root no longer opens the KEK"
    );
    let stored = |sql: &'static str, id: Uuid| sqlx::query_scalar::<_, Vec<u8>>(sql).bind(id);
    let legacy_now = stored("SELECT body_sealed FROM email_messages WHERE id = $1", legacy_id)
        .fetch_one(tx.conn())
        .await?;
    let token_now = stored("SELECT token_sealed FROM invitations WHERE id = $1", invitation_id)
        .fetch_one(tx.conn())
        .await?;
    let envelope_now = stored("SELECT body_sealed FROM email_messages WHERE id = $1", envelope_id)
        .fetch_one(tx.conn())
        .await?;
    tx.rollback().await?;
    assert_eq!(envelope_now, envelope, "envelopes are untouched");
    let totp_now =
        stored("SELECT secret FROM user_credentials WHERE id = $1", cred_id).fetch_one(t.db.pool()).await?;
    for (value, aad, plain) in [
        (&legacy_now, format!("email_messages:{legacy_id}"), &b"legacy body"[..]),
        (&token_now, format!("invitations:{invitation_id}"), &b"invite-token"[..]),
        (&totp_now, format!("user_credentials:{cred_id}:totp"), &b"JBSWY3DPEHPK3PXP"[..]),
    ] {
        assert_eq!(
            open(new.as_ref(), "secrets", &aad, value)?.as_slice(),
            plain,
            "{aad} opens with the new root"
        );
        assert!(
            open(old.as_ref(), "secrets", &aad, value).is_err(),
            "{aad} no longer opens with the old root"
        );
    }
    let mut sys = t.db.begin_system().await?;
    keys_with(new.clone()).active(&mut sys).await?;
    assert!(keys_with(old.clone()).active(&mut sys).await.is_err(), "the system KEK moved too");
    sys.rollback().await?;

    // Running it again with the old key as "current" refuses and changes nothing.
    let other = root()?;
    let err = rotate_root(&t.db, old.as_ref(), other.as_ref(), &SystemClock, &UuidV7Gen).await;
    assert!(matches!(err, Err(KeyError::RootMismatch { .. })), "{err:?}");
    let totp_after =
        stored("SELECT secret FROM user_credentials WHERE id = $1", cred_id).fetch_one(t.db.pool()).await?;
    assert_eq!(totp_after, totp_now);

    // The rotation is on the system audit chain, which still verifies.
    let mut sys = t.db.begin_system().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'key.root_rotated'")
        .fetch_one(sys.conn())
        .await?;
    assert_eq!(n, 1);
    sys.rollback().await?;
    let report = procera_audit::verify(&t.db, WsCtx { workspace_id: SYSTEM_WORKSPACE }).await?;
    assert_eq!(report.first_broken, None);
    Ok(())
}
