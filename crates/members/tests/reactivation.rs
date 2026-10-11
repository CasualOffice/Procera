//! docs/22 A-41: reactivating a deactivated member against a real database. The member comes back
//! with the roles and groups they had; the actor may not restore roles they could not grant; a
//! disabled account or a full member quota refuses; every reactivation is audited with its reason.

use std::sync::Arc;

use procera_access::{Access, AccessError, Resource, Scope};
use procera_crypto::LocalKey;
use procera_db::{WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{ClientInfo, Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_members::{Actor, Members, MembersError};
use uuid::Uuid;

#[tokio::test]
async fn reactivation_restores_roles_and_refuses_escalation_disabled_accounts_and_full_quotas()
-> anyhow::Result<()> {
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
    let c = ClientInfo::default();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;

    // Boss (owner), Ann (workspace admin) and Zoe (a second owner who will leave and come back).
    let mut people = vec![];
    for name in ["boss", "ann", "zoe"] {
        let (user, subject) = (Uuid::now_v7(), Uuid::new_v4());
        sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
            .bind(user)
            .bind(format!("{name}@acme.test"))
            .bind(name)
            .bind(subject)
            .execute(&mut admin)
            .await?;
        people.push((user, subject));
    }
    let ws = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, slug, name, profile, state, created_by) VALUES ($1, 'acme', 'Acme', 'standard', 'active', $2)")
        .bind(ws)
        .bind(people[0].0)
        .execute(&mut admin)
        .await?;
    let ctx = WsCtx { workspace_id: ws };
    let mut tx = t.db.begin_ws(ctx).await?;
    members.access().seed_workspace(&mut tx).await?;
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    for (m, (u, _)) in ids.iter().zip(&people) {
        sqlx::query(
            "INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')",
        )
        .bind(ws)
        .bind(m)
        .bind(u)
        .execute(tx.conn())
        .await?;
    }
    let (boss, ann, zoe) = (ids[0], ids[1], ids[2]);
    for (m, role) in [(boss, "workspace.owner"), (ann, "workspace.admin"), (zoe, "workspace.owner")] {
        members.access().grant(&mut tx, None, m, role, Scope::Workspace, None, None).await?;
    }
    let boss_actor = Actor { membership: boss, subject_ref: people[0].1 };
    let ann_actor = Actor { membership: ann, subject_ref: people[1].1 };
    let group = members.create_group(&mut tx, boss_actor, "Fire wardens", None, &c).await?;
    members.add_group_member(&mut tx, boss_actor, group.id, zoe, &c).await?;
    members.deactivate(&mut tx, boss_actor, zoe, &c).await?;
    let a = members.access();
    assert!(!a.check(&mut tx, zoe, "member:view", &Resource::Workspace).await?.allowed);

    // A reason is required; an admin cannot bring back an owner (owner-only permissions).
    assert!(matches!(
        members.reactivate(&mut tx, boss_actor, zoe, "  ", &c).await,
        Err(MembersError::Invalid(_))
    ));
    assert!(matches!(
        members.reactivate(&mut tx, ann_actor, zoe, "back from leave", &c).await,
        Err(MembersError::Access(AccessError::Escalation(_)))
    ));
    // The owner can: same roles, same groups, access again, one audited event with the reason.
    let roles = members.reactivate(&mut tx, boss_actor, zoe, "back from leave", &c).await?;
    assert_eq!(roles, vec!["workspace.owner".to_owned()]);
    assert!(members.access().check(&mut tx, zoe, "member:view", &Resource::Workspace).await?.allowed);
    let dir = members.member_directory(&mut tx, zoe).await?;
    assert_eq!(dir["group_ids"], serde_json::json!([group.id]));
    let (action, reason): (String, String) = sqlx::query_as(
        "SELECT action, diff->>'reason' FROM audit_events WHERE object_id = $1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(zoe)
    .fetch_one(tx.conn())
    .await?;
    assert_eq!((action.as_str(), reason.as_str()), ("member.reactivated", "back from leave"));
    // Already active.
    assert!(matches!(
        members.reactivate(&mut tx, boss_actor, zoe, "again", &c).await,
        Err(MembersError::Conflict(_))
    ));
    members.deactivate(&mut tx, boss_actor, zoe, &c).await?;
    tx.commit().await?;

    // A disabled account stays out until a system administrator restores it.
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(people[2].0)
        .execute(&mut admin)
        .await?;
    let mut tx = t.db.begin_ws(ctx).await?;
    assert!(matches!(
        members.reactivate(&mut tx, boss_actor, zoe, "back", &c).await,
        Err(MembersError::Conflict(_))
    ));
    tx.rollback().await?;
    sqlx::query("UPDATE users SET status = 'active' WHERE id = $1")
        .bind(people[2].0)
        .execute(&mut admin)
        .await?;

    // The member quota counts the returning member as a seat.
    sqlx::query("INSERT INTO workspace_quotas (target_workspace_id, max_members) VALUES ($1, 2)")
        .bind(ws)
        .execute(&mut admin)
        .await?;
    let mut tx = t.db.begin_ws(ctx).await?;
    assert!(matches!(
        members.reactivate(&mut tx, boss_actor, zoe, "back", &c).await,
        Err(MembersError::QuotaExceeded { limit: 2, used: 2 })
    ));
    tx.rollback().await?;
    Ok(())
}
