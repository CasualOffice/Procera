//! Copy invite link (BPMS-023): when email isn't set up, an admin re-issues a pending invitation's
//! token and hands the link over. The earlier link stops working, the new one works, the change is
//! audited, and settled invitations cannot be re-issued.

use std::sync::Arc;

use procera_access::{Access, Scope};
use procera_crypto::LocalKey;
use procera_db::{WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{ClientInfo, Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_members::{Actor, Members, MembersError};
use uuid::Uuid;

#[tokio::test]
async fn reissued_link_replaces_the_old_one_and_is_audited() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let identity = Identity::new(
        t.db.clone(),
        keys.clone(),
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )?;
    let members =
        Members::new(t.db.clone(), identity, Access::new(), keys, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let c = ClientInfo { ip: Some("203.0.113.9".into()), user_agent: Some("link-test".into()) };
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;

    let (user, subject) = (Uuid::now_v7(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'boss@acme.test', 'Boss', $2)",
    )
    .bind(user)
    .bind(subject)
    .execute(&mut admin)
    .await?;
    let ws = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, slug, name, profile, state, created_by) VALUES ($1, 'acme', 'Acme', 'standard', 'provisioning', $2)")
        .bind(ws)
        .bind(user)
        .execute(&mut admin)
        .await?;
    let ctx = WsCtx { workspace_id: ws };
    let mut tx = t.db.begin_ws(ctx).await?;
    members.access().seed_workspace(&mut tx).await?;
    // The system plane's owner invitation (no inviter), then a member invited by the owner.
    let (owner_inv, _) =
        members.invite(&mut tx, None, "owner@acme.test", &["workspace.owner".into()], &c).await?;
    assert_eq!(members.pending_owner_invitation(&mut tx).await?, Some(owner_inv.id));
    let boss = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(ws)
        .bind(boss)
        .bind(user)
        .execute(tx.conn())
        .await?;
    members.access().grant(&mut tx, None, boss, "workspace.owner", Scope::Workspace, None, None).await?;
    let actor = Actor { membership: boss, subject_ref: subject };
    let (inv, old) = members.invite(&mut tx, Some(actor), "ann@acme.test", &["member".into()], &c).await?;
    tx.commit().await?;

    let mut tx = t.db.begin_ws(ctx).await?;
    let (again, new) = members.reissue_invitation_link(&mut tx, Some(actor), inv.id, &c).await?;
    tx.commit().await?;
    assert_eq!((again.id, again.email.as_str()), (inv.id, "ann@acme.test"));
    assert_ne!(old, new);
    assert!(matches!(members.invitation_preview(&old).await, Err(MembersError::InvitationInvalid)));
    assert_eq!(members.invitation_preview(&new).await?.email, "ann@acme.test");

    let (action, actor_ref, ip, ua): (String, Option<Uuid>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT action, actor_subject_ref, ip::text, user_agent FROM audit_events WHERE object_id = $1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(inv.id)
    .fetch_one(&mut admin)
    .await?;
    assert_eq!(action, "member.invitation_link_copied");
    assert_eq!(actor_ref, Some(subject));
    assert!(ip.is_some_and(|i| i.starts_with("203.0.113.9")));
    assert_eq!(ua.as_deref(), Some("link-test"));

    // A revoked invitation has no link to copy.
    let mut tx = t.db.begin_ws(ctx).await?;
    members.revoke_invitation(&mut tx, actor, inv.id, &c).await?;
    assert!(matches!(
        members.reissue_invitation_link(&mut tx, Some(actor), inv.id, &c).await,
        Err(MembersError::NotFound)
    ));
    tx.rollback().await?;
    Ok(())
}
