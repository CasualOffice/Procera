//! BPMS-024 property test: nobody can create, edit, grant or revoke beyond what they hold (docs/12 §5).
//!
//! For a random actor permission set P and random requests, against a real database:
//! - `create_role(Q)` succeeds iff Q ⊆ P and Q has no owner-only permission;
//! - granting a built-in template succeeds iff its permissions ⊆ P;
//! - every permission a grantee gains through the actor is in P;
//! - Cedar allows a workspace action for the actor iff it is in P.

use std::collections::BTreeSet;

use procera_access::catalogue::{OWNER_ONLY, PERMISSIONS, TEMPLATES};
use procera_access::{Access, AccessError, Resource, Scope};
use procera_db::{WsCtx, WsTx, test_support};
use proptest::prelude::*;
use proptest::sample::subsequence;
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

fn perms(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[derive(Debug, Clone)]
struct Case {
    held: Vec<&'static str>,
    requested: Vec<&'static str>,
    template: usize,
    probe: &'static str,
}

fn cases() -> impl Strategy<Value = Case> {
    let grantable: Vec<&'static str> =
        PERMISSIONS.iter().copied().filter(|p| !OWNER_ONLY.contains(p)).collect();
    let n = grantable.len();
    (
        subsequence(grantable, 0..n.min(40)),
        subsequence(PERMISSIONS.to_vec(), 1..8),
        0..TEMPLATES.len(),
        proptest::sample::select(PERMISSIONS.to_vec()),
    )
        .prop_map(|(held, requested, template, probe)| Case { held, requested, template, probe })
}

async fn run(t: &test_support::TestDb, a: &Access, c: &Case) -> anyhow::Result<()> {
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    a.seed_workspace(&mut tx).await?;
    let actor = member(&mut tx, &mut admin).await?;
    let grantee = member(&mut tx, &mut admin).await?;
    let held: BTreeSet<String> = c.held.iter().map(|s| (*s).to_owned()).collect();
    if !held.is_empty() {
        a.create_role(&mut tx, None, "actor.role", "Actor", &perms(&c.held), None).await?;
        a.grant(&mut tx, None, actor, "actor.role", Scope::Workspace, None, None).await?;
    }
    assert_eq!(a.effective_workspace_permissions(&mut tx, actor).await?, held);

    // Cedar agrees with the held set.
    let allowed = a.check(&mut tx, actor, c.probe, &Resource::Workspace).await?.allowed;
    assert_eq!(allowed, held.contains(c.probe), "probe {}", c.probe);

    // create_role: allowed iff requested ⊆ held and no owner-only permission.
    let req = perms(&c.requested);
    let ok_expected = c.requested.iter().all(|p| held.contains(*p) && !OWNER_ONLY.contains(p));
    match a.create_role(&mut tx, Some(actor), "probe.role", "Probe", &req, None).await {
        Ok(_) => {
            assert!(ok_expected, "created a role beyond held permissions: {req:?}");
            a.grant(&mut tx, Some(actor), grantee, "probe.role", Scope::Workspace, None, None).await?;
        }
        Err(AccessError::Escalation(missing)) => {
            assert!(!ok_expected, "rejected a role within held permissions: {req:?}");
            assert!(missing.iter().all(|p| !held.contains(p) || OWNER_ONLY.contains(&p.as_str())));
        }
        Err(e) => return Err(e.into()),
    }

    // Granting a built-in template: allowed iff its permissions ⊆ held.
    let tpl = &TEMPLATES[c.template];
    let tpl_ok = tpl.permissions().iter().all(|p| held.contains(p));
    match a.grant(&mut tx, Some(actor), grantee, tpl.key, Scope::Workspace, None, None).await {
        Ok(g) => {
            assert!(tpl_ok, "granted {} beyond held permissions", tpl.key);
            // ... and the same actor may revoke what it could grant.
            a.revoke_grant(&mut tx, Some(actor), g).await?;
        }
        Err(AccessError::Escalation(_)) => assert!(!tpl_ok, "rejected {} within held permissions", tpl.key),
        Err(e) => return Err(e.into()),
    }

    // Whatever the grantee gained through the actor is within the actor's permissions.
    let gained = a.effective_workspace_permissions(&mut tx, grantee).await?;
    assert!(gained.is_subset(&held), "grantee gained {:?}", gained.difference(&held).collect::<Vec<_>>());
    tx.rollback().await?;
    Ok(())
}

#[test]
fn nobody_grants_beyond_what_they_hold() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let Some(t) = rt.block_on(test_support::fresh()) else { return Ok(()) };
    let t = t?;
    let a = Access::new();
    let cfg = ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() };
    let ran = std::sync::atomic::AtomicU32::new(0);
    proptest!(cfg, |(c in cases())| {
        rt.block_on(run(&t, &a, &c)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        ran.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    assert_eq!(ran.into_inner(), 64, "every generated case ran against the database");
    Ok(())
}

#[tokio::test]
async fn revocation_is_bounded_and_keeps_the_last_owner() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let a = Access::new();
    let mut admin_conn = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    a.seed_workspace(&mut tx).await?;
    let owner = member(&mut tx, &mut admin_conn).await?;
    let admin = member(&mut tx, &mut admin_conn).await?;
    let og = a.grant(&mut tx, None, owner, "workspace.owner", Scope::Workspace, None, None).await?;
    a.grant(&mut tx, None, admin, "workspace.admin", Scope::Workspace, None, None).await?;
    assert!(
        matches!(a.revoke_grant(&mut tx, Some(admin), og).await, Err(AccessError::Escalation(_))),
        "an admin cannot strip the owner role"
    );
    assert!(
        matches!(a.revoke_grant(&mut tx, Some(owner), og).await, Err(AccessError::LastOwner)),
        "the last owner keeps the owner role"
    );
    let co = member(&mut tx, &mut admin_conn).await?;
    a.grant(&mut tx, Some(owner), co, "workspace.owner", Scope::Workspace, None, None).await?;
    a.revoke_grant(&mut tx, Some(owner), og).await?;

    // Role preview reports the diff and the affected members; editing needs both old and new sets.
    let role =
        a.create_role(&mut tx, Some(co), "hr.viewer", "HR viewer", &perms(&["member:view"]), None).await?;
    let viewer = member(&mut tx, &mut admin_conn).await?;
    a.grant(&mut tx, Some(co), viewer, "hr.viewer", Scope::Workspace, None, None).await?;
    let p = a.preview_role_change(&mut tx, role.id, &perms(&["member:view", "role:view"])).await?;
    assert_eq!((p.added, p.removed, p.affected_members), (perms(&["role:view"]), vec![], vec![viewer]));
    let limited = member(&mut tx, &mut admin_conn).await?;
    a.grant(&mut tx, None, limited, "member", Scope::Workspace, None, None).await?;
    assert!(matches!(
        a.update_role(&mut tx, Some(limited), role.id, &perms(&["member:view", "role:view"])).await,
        Err(AccessError::Escalation(_))
    ));
    tx.rollback().await?;
    Ok(())
}
