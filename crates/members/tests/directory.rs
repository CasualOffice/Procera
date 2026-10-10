//! BPMS-023 directory: department tree, manager cycles, deterministic manager resolution, groups and
//! CSV import against a real database.

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{ClientInfo, Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_members::directory::{DepartmentPatch, MemberPatch};
use procera_members::{Actor, Members, MembersError};
use uuid::Uuid;

async fn member(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection, email: &str) -> anyhow::Result<Uuid> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $2, $3)")
        .bind(user)
        .bind(email)
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

fn mgr(m: Uuid) -> MemberPatch {
    MemberPatch { manager_membership_id: Some(Some(m)), ..MemberPatch::default() }
}

#[tokio::test]
async fn tree_cycles_resolution_groups_and_csv_import() -> anyhow::Result<()> {
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
    let members = Members::new(
        t.db.clone(),
        identity,
        procera_access::Access::new(),
        keys,
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
    );
    let c = ClientInfo::default();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let a = member(&mut tx, &mut admin, "ann@acme.test").await?;
    let b = member(&mut tx, &mut admin, "ben@acme.test").await?;
    let cc = member(&mut tx, &mut admin, "cat@acme.test").await?;
    let d = member(&mut tx, &mut admin, "dev@acme.test").await?;
    let actor = Actor { membership: a, subject_ref: Uuid::new_v4() };

    // Department tree: Ops > Support. Names unique among siblings; no moving under a descendant.
    let ops = members.create_department(&mut tx, actor, "Operations", None, &c).await?;
    let support = members.create_department(&mut tx, actor, "Support", Some(ops.id), &c).await?;
    assert_eq!(support.path, vec![ops.id, support.id]);
    assert!(matches!(
        members.create_department(&mut tx, actor, "support", Some(ops.id), &c).await,
        Err(MembersError::Conflict(_))
    ));
    let mv = DepartmentPatch { parent_id: Some(Some(support.id)), ..DepartmentPatch::default() };
    assert!(matches!(
        members.update_department(&mut tx, actor, ops.id, &mv, &c).await,
        Err(MembersError::Cycle(_))
    ));
    // Moving Support to the root rewrites its path.
    members
        .update_department(
            &mut tx,
            actor,
            support.id,
            &DepartmentPatch { parent_id: Some(None), ..DepartmentPatch::default() },
            &c,
        )
        .await?;
    members
        .update_department(
            &mut tx,
            actor,
            support.id,
            &DepartmentPatch { parent_id: Some(Some(ops.id)), ..DepartmentPatch::default() },
            &c,
        )
        .await?;
    let tree = members.list_departments(&mut tx).await?;
    assert_eq!(
        tree.iter().find(|x| x.id == support.id).map(|x| x.path.clone()),
        Some(vec![ops.id, support.id])
    );

    // Manager cycles are rejected.
    members.update_member_directory(&mut tx, actor, b, &mgr(a), &c).await?;
    members.update_member_directory(&mut tx, actor, cc, &mgr(b), &c).await?;
    assert!(matches!(
        members.update_member_directory(&mut tx, actor, a, &mgr(cc), &c).await,
        Err(MembersError::Cycle(_))
    ));
    assert!(matches!(
        members.update_member_directory(&mut tx, actor, a, &mgr(a), &c).await,
        Err(MembersError::Cycle(_))
    ));

    // Resolution: explicit manager first; else nearest department head that is not the member.
    let r = members.resolve_manager(&mut tx, cc).await?;
    assert_eq!((r.membership_id, r.via), (Some(b), "explicit"));
    members
        .update_member_directory(
            &mut tx,
            actor,
            d,
            &MemberPatch {
                department_id: Some(Some(support.id)),
                title: Some(Some("Agent".into())),
                ..MemberPatch::default()
            },
            &c,
        )
        .await?;
    members
        .update_department(
            &mut tx,
            actor,
            support.id,
            &DepartmentPatch { head_membership_id: Some(Some(d)), ..DepartmentPatch::default() },
            &c,
        )
        .await?;
    members
        .update_department(
            &mut tx,
            actor,
            ops.id,
            &DepartmentPatch { head_membership_id: Some(Some(a)), ..DepartmentPatch::default() },
            &c,
        )
        .await?;
    let r = members.resolve_manager(&mut tx, d).await?;
    assert_eq!(
        (r.membership_id, r.via, r.department_id),
        (Some(a), "department_head", Some(ops.id)),
        "d heads Support, so Ops' head applies"
    );
    assert!(matches!(
        members.delete_department(&mut tx, actor, support.id, &c).await,
        Err(MembersError::Conflict(_))
    ));

    // Groups.
    let g = members.create_group(&mut tx, actor, "Fire wardens", Some("Floor safety"), &c).await?;
    members.add_group_member(&mut tx, actor, g.id, d, &c).await?;
    assert!(matches!(
        members.add_group_member(&mut tx, actor, g.id, d, &c).await,
        Err(MembersError::Conflict(_))
    ));
    assert_eq!(members.group_members(&mut tx, g.id).await?, vec![d]);
    members.remove_group_member(&mut tx, actor, g.id, d, &c).await?;

    // CSV: creates departments by path, managers resolved after all rows, per-row errors.
    let csv = "email,department,title,manager_email\n\
               ben@acme.test,Finance/Payables,Clerk,cat@acme.test\n\
               cat@acme.test,Finance,Controller,\n\
               nobody@acme.test,Finance,Ghost,\n\
               ann@acme.test,,,ben@acme.test\n";
    let report = members.import_csv(&mut tx, actor, csv, &c).await?;
    assert_eq!(report.created_departments, vec!["Finance", "Finance/Payables"]);
    let row = |e: &str| report.rows.iter().find(|r| r.email == e).expect("row");
    assert_eq!(
        row("ben@acme.test").outcome,
        "error",
        "cat -> ben -> ... ben -> cat would be a cycle: {:?}",
        row("ben@acme.test")
    );
    assert_eq!(row("nobody@acme.test").error.as_deref(), Some("not an active member; invite them first"));
    assert_eq!(row("cat@acme.test").changes, vec!["title", "department"]);
    assert_eq!(row("ann@acme.test").outcome, "error", "ann -> ben while ben -> ann exists");
    let dir = members.member_directory(&mut tx, cc).await?;
    assert_eq!(dir["field_sources"]["department"], "csv");
    assert_eq!(dir["title"], "Controller");
    tx.rollback().await?;
    Ok(())
}
