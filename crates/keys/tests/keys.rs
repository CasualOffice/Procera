//! BPMS-030: workspace KEKs: envelopes open under their version, rotation rewraps without
//! re-encrypting, retired keys are destroyed once unused, and shredding makes data unreadable.

use std::sync::Arc;

use procera_crypto::{LocalKey, envelope_version};
use procera_db::{WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_keys::{KeyError, Keys};
use uuid::Uuid;

#[tokio::test]
async fn rotate_rewrap_destroy_and_shred() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let root = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let keys = Keys::new(t.db.clone(), root, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let ws = WsCtx { workspace_id: Uuid::now_v7() };

    // A value sealed under v1 (created on first use) and stored like an email body.
    let mut tx = t.db.begin_ws(ws).await?;
    let id = Uuid::now_v7();
    let sealed = keys.seal(&mut tx, &format!("email_messages:{id}"), b"secret body").await?;
    assert_eq!(envelope_version(&sealed), Some(1));
    sqlx::query("INSERT INTO email_messages (workspace_id, id, kind, to_email, subject, body_sealed, dedupe_key, created_at) VALUES ($1, $2, 'invitation', 'a@acme.test', 's', $3, 'k1', now())")
        .bind(ws.workspace_id).bind(id).bind(&sealed).execute(tx.conn()).await?;
    assert_eq!(keys.rotate(&mut tx, None, Some("drill"), (None, None)).await?, 2);
    let st = keys.status(&mut tx).await?;
    assert_eq!(
        st.iter().map(|k| (k.version, k.state.as_str(), k.values)).collect::<Vec<_>>(),
        vec![(2, "active", 0), (1, "retired", 1)]
    );
    // The old envelope still opens with v1 before it is rewrapped.
    assert_eq!(
        keys.open(&mut tx, &format!("email_messages:{id}"), &sealed).await?.as_slice(),
        b"secret body"
    );
    tx.commit().await?;

    assert_eq!(keys.rewrap_due(100).await?, 1);
    let mut tx = t.db.begin_ws(ws).await?;
    let now: Vec<u8> = sqlx::query_scalar("SELECT body_sealed FROM email_messages WHERE id = $1")
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
    assert_eq!(envelope_version(&now), Some(2));
    assert_eq!(&now[65..], &sealed[65..], "the ciphertext is unchanged; only the DEK was rewrapped");
    assert_eq!(keys.open(&mut tx, &format!("email_messages:{id}"), &now).await?.as_slice(), b"secret body");
    let st = keys.status(&mut tx).await?;
    assert_eq!(
        st.iter().map(|k| (k.version, k.state.as_str())).collect::<Vec<_>>(),
        vec![(2, "active"), (1, "destroyed")],
        "an unused retired key is destroyed"
    );

    // Crypto-shredding: nothing of the workspace can be opened or sealed any more.
    keys.shred(&mut tx, None, "workspace deleted").await?;
    assert!(matches!(
        keys.open(&mut tx, &format!("email_messages:{id}"), &now).await,
        Err(KeyError::Destroyed)
    ));
    assert!(matches!(keys.seal(&mut tx, "x", b"y").await, Err(KeyError::Destroyed)));
    tx.commit().await?;
    Ok(())
}
