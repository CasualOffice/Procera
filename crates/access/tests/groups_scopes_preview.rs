//! BPMS-024: grants through groups, department-subtree scopes in Cedar, and the property that a
//! role-change preview names exactly the members whose effective permissions change (FR-W03).

use std::collections::{BTreeMap, BTreeSet};

use procera_access::catalogue::{OWNER_ONLY, PERMISSIONS};
use procera_access::{Access, AccessError, Resource, Scope};
use procera_db::{WsCtx, WsTx, test_support};
use proptest::prelude::*;
use uuid::Uuid;

async fn member(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection, status: &str) -> anyhow::Result<Uuid> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, $4)")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .bind(status)
        .execute(tx.conn())
        .await?;
    Ok(m)
}

async fn group(tx: &mut WsTx<'_>, members: &[Uuid]) -> anyhow::Result<Uuid> {
    let g = Uuid::now_v7();
    sqlx::query("INSERT INTO groups (workspace_id, id, name) VALUES ($1, $2, $3)")
        .bind(tx.ctx.workspace_id)
        .bind(g)
        .bind(g.to_string())
        .execute(tx.conn())
        .await?;
    for m in members {
        sqlx::query("INSERT INTO group_members (workspace_id, group_id, membership_id) VALUES ($1, $2, $3)")
            .bind(tx.ctx.workspace_id)
            .bind(g)
            .bind(m)
            .execute(tx.conn())
            .await?;
    }
    Ok(g)
}

async fn department(
    tx: &mut WsTx<'_>,
    parent: Option<(Uuid, Vec<Uuid>)>,
) -> anyhow::Result<(Uuid, Vec<Uuid>)> {
    let id = Uuid::now_v7();
    let mut path = parent.as_ref().map(|p| p.1.clone()).unwrap_or_default();
    path.push(id);
    sqlx::query(
        "INSERT INTO departments (workspace_id, id, parent_id, name, path) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(tx.ctx.workspace_id)
    .bind(id)
    .bind(parent.map(|p| p.0))
    .bind(id.to_string())
    .bind(&path)
    .execute(tx.conn())
    .await?;
    Ok((id, path))
}

fn perms(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[tokio::test]
async fn group_grants_and_department_scopes() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let a = Access::new();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    a.seed_workspace(&mut tx).await?;
    let ann = member(&mut tx, &mut admin, "active").await?;
    let ben = member(&mut tx, &mut admin, "active").await?;
    let gone = member(&mut tx, &mut admin, "deactivated").await?;

    // A group grant applies to current group members only (and never to deactivated ones).
    let g = group(&mut tx, &[ann, gone]).await?;
    a.grant_to_group(&mut tx, None, g, "analyst", Scope::Workspace, None, None).await?;
    assert!(a.check(&mut tx, ann, "report:view", &Resource::Workspace).await?.allowed);
    assert!(!a.check(&mut tx, ben, "report:view", &Resource::Workspace).await?.allowed);
    assert!(!a.check(&mut tx, gone, "report:view", &Resource::Workspace).await?.allowed);
    assert!(a.effective_workspace_permissions(&mut tx, ann).await?.contains("report:view"));
    assert!(matches!(
        a.grant_to_group(&mut tx, None, g, "workspace.owner", Scope::Workspace, None, None).await,
        Err(AccessError::Invalid(_))
    ));
    sqlx::query("DELETE FROM group_members WHERE membership_id = $1").bind(ann).execute(tx.conn()).await?;
    a.invalidate(&mut tx).await?;
    assert!(
        !a.check(&mut tx, ann, "report:view", &Resource::Workspace).await?.allowed,
        "leaving the group removes the access"
    );

    // A department-scoped admin manages members anywhere in that subtree, and nowhere else.
    let emea = department(&mut tx, None).await?;
    let uk = department(&mut tx, Some(emea.clone())).await?;
    let apac = department(&mut tx, None).await?;
    let in_uk = member(&mut tx, &mut admin, "active").await?;
    let in_apac = member(&mut tx, &mut admin, "active").await?;
    sqlx::query("UPDATE memberships SET department_id = $2 WHERE id = $1")
        .bind(in_uk)
        .bind(uk.0)
        .execute(tx.conn())
        .await?;
    sqlx::query("UPDATE memberships SET department_id = $2 WHERE id = $1")
        .bind(in_apac)
        .bind(apac.0)
        .execute(tx.conn())
        .await?;
    a.grant(&mut tx, None, ben, "workspace.admin", Scope::Department(emea.0), None, None).await?;
    let d = a.check(&mut tx, ben, "member:deactivate", &Resource::Member(in_uk)).await?;
    assert!(d.allowed, "EMEA admin reaches UK (child department)");
    assert_eq!(d.reasons, vec![format!("grant:workspace.admin@department:{}", emea.0)]);
    assert!(!a.check(&mut tx, ben, "member:deactivate", &Resource::Member(in_apac)).await?.allowed);
    assert!(
        !a.check(&mut tx, ben, "member:deactivate", &Resource::Member(ann)).await?.allowed,
        "no department = workspace level"
    );
    tx.rollback().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Case {
    old: Vec<&'static str>,
    new: Vec<&'static str>,
    /// Per member: (active, direct grant, via group, expired grant).
    members: Vec<(bool, bool, bool, bool)>,
}

fn cases() -> impl Strategy<Value = Case> {
    let pool: Vec<&'static str> =
        PERMISSIONS.iter().copied().filter(|p| !OWNER_ONLY.contains(p)).take(12).collect();
    (
        proptest::sample::subsequence(pool.clone(), 0..6),
        proptest::sample::subsequence(pool, 0..6),
        proptest::collection::vec((any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()), 1..6),
    )
        .prop_map(|(old, new, members)| Case { old, new, members })
}

async fn run(t: &test_support::TestDb, a: &Access, c: &Case) -> anyhow::Result<()> {
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    a.seed_workspace(&mut tx).await?;
    let role = a.create_role(&mut tx, None, "probe", "Probe", &perms(&c.old), None).await?;
    let mut ids = vec![];
    for (active, direct, via_group, expired) in &c.members {
        let m = member(&mut tx, &mut admin, if *active { "active" } else { "deactivated" }).await?;
        let exp = expired.then(|| time::OffsetDateTime::now_utc() - time::Duration::days(1));
        if *direct {
            a.grant(&mut tx, None, m, "probe", Scope::Workspace, exp, None).await?;
        }
        if *via_group {
            let g = group(&mut tx, &[m]).await?;
            a.grant_to_group(&mut tx, None, g, "probe", Scope::Workspace, None, None).await?;
        }
        ids.push(m);
    }
    let mut before = BTreeMap::new();
    for m in &ids {
        before.insert(*m, a.effective_workspace_permissions(&mut tx, *m).await?);
    }
    let preview = a.preview_role_change(&mut tx, role.id, &perms(&c.new)).await?;
    a.update_role(&mut tx, None, role.id, &perms(&c.new)).await?;
    let affected: BTreeSet<Uuid> = preview.affected_members.iter().copied().collect();
    for m in &ids {
        let after = a.effective_workspace_permissions(&mut tx, *m).await?;
        if after != before[m] {
            assert!(affected.contains(m), "member {m} changed but was not previewed");
        }
        if affected.contains(m) && !preview.added.is_empty() {
            assert!(
                preview.added.iter().all(|p| after.contains(p)),
                "previewed member {m} lacks an added permission"
            );
        }
    }
    // Everyone previewed actually holds the role (active, live grant, direct or via a group).
    for m in &affected {
        let idx = ids.iter().position(|x| x == m).ok_or_else(|| anyhow::anyhow!("unknown member {m}"))?;
        let (active, direct, via_group, expired) = c.members[idx];
        assert!(active && ((direct && !expired) || via_group), "{m} previewed without a live grant");
    }
    tx.rollback().await?;
    Ok(())
}

#[test]
fn role_preview_matches_the_effect() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let Some(t) = rt.block_on(test_support::fresh()) else { return Ok(()) };
    let t = t?;
    let a = Access::new();
    let ran = std::sync::atomic::AtomicU32::new(0);
    proptest!(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() }, |(c in cases())| {
        rt.block_on(run(&t, &a, &c)).map_err(|e| TestCaseError::fail(e.to_string()))?;
        ran.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    assert_eq!(ran.into_inner(), 48);
    Ok(())
}
