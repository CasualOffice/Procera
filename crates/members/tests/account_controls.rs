//! OQ-15 / A-39 (docs/06 §3.6.1): the rules behind admin account controls against a real database.
//! `member:security.manage` is held by owners and admins only, and a workspace admin may change an
//! account's password state, sessions or second factors only when this is its only active workspace
//! and it has no system role.

use std::sync::Arc;

use procera_access::{Access, Resource, catalogue};
use procera_crypto::LocalKey;
use procera_db::{WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_members::{Members, MembersError};
use uuid::Uuid;

async fn workspace(admin: &mut sqlx::PgConnection, slug: &str, by: Uuid) -> anyhow::Result<WsCtx> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, slug, name, profile, state, created_by) VALUES ($1, $2, $2, 'standard', 'active', $3)")
        .bind(id)
        .bind(slug)
        .bind(by)
        .execute(&mut *admin)
        .await?;
    Ok(WsCtx { workspace_id: id })
}

#[tokio::test]
async fn security_permission_and_the_only_active_workspace_rule() -> anyhow::Result<()> {
    for t in catalogue::TEMPLATES {
        let has = t.permissions().iter().any(|p| p == "member:security.manage");
        assert_eq!(has, matches!(t.key, "workspace.owner" | "workspace.admin"), "{}", t.key);
    }
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
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;

    let user = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'zoe@acme.test', 'Zoe', $2)",
    )
    .bind(user)
    .bind(Uuid::new_v4())
    .execute(&mut admin)
    .await?;
    let boss = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'boss@acme.test', 'Boss', $2)",
    )
    .bind(boss)
    .bind(Uuid::new_v4())
    .execute(&mut admin)
    .await?;
    let acme = workspace(&mut admin, "acme", boss).await?;
    let globex = workspace(&mut admin, "globex", boss).await?;

    let mut tx = t.db.begin_ws(acme).await?;
    members.access().seed_workspace(&mut tx).await?;
    let (zoe, owner) = (Uuid::now_v7(), Uuid::now_v7());
    for (m, u) in [(zoe, user), (owner, boss)] {
        sqlx::query(
            "INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')",
        )
        .bind(acme.workspace_id)
        .bind(m)
        .bind(u)
        .execute(tx.conn())
        .await?;
    }
    members
        .access()
        .grant(&mut tx, None, owner, "workspace.owner", procera_access::Scope::Workspace, None, None)
        .await?;
    members
        .access()
        .grant(&mut tx, None, zoe, "member", procera_access::Scope::Workspace, None, None)
        .await?;
    let a = members.access();
    assert!(a.check(&mut tx, owner, "member:security.manage", &Resource::Member(zoe)).await?.allowed);
    assert!(!a.check(&mut tx, zoe, "member:security.manage", &Resource::Member(owner)).await?.allowed);
    let (uid, status, name) = members.member_account(&mut tx, zoe).await?;
    assert_eq!((uid, status.as_str(), name.as_str()), (user, "active", "Zoe"));
    assert!(matches!(members.member_account(&mut tx, Uuid::now_v7()).await, Err(MembersError::NotFound)));
    tx.commit().await?;

    // Only this workspace: the workspace admin may act.
    assert!(!members.account_used_elsewhere(user, acme.workspace_id).await?);
    // An active membership elsewhere: a system administrator must do it.
    let mut tx = t.db.begin_ws(globex).await?;
    let other = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(globex.workspace_id)
        .bind(other)
        .bind(user)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    assert!(members.account_used_elsewhere(user, acme.workspace_id).await?);
    assert!(members.account_used_elsewhere(user, globex.workspace_id).await?);
    // A deactivated membership elsewhere does not count.
    let mut tx = t.db.begin_ws(globex).await?;
    sqlx::query("UPDATE memberships SET status = 'deactivated' WHERE id = $1")
        .bind(other)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    assert!(!members.account_used_elsewhere(user, acme.workspace_id).await?);
    // A system role always counts.
    sqlx::query("INSERT INTO system_role_grants (user_id, role) VALUES ($1, 'system.operator')")
        .bind(user)
        .execute(&mut admin)
        .await?;
    assert!(members.account_used_elsewhere(user, acme.workspace_id).await?);
    Ok(())
}
