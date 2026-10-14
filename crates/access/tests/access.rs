//! BPMS-024: roles, grants and Cedar decisions against a real database.

use procera_access::{Access, AccessError, Resource, Scope};
use procera_db::{WsCtx, WsTx, test_support};
use uuid::Uuid;

macro_rules! db_or_skip {
    () => {
        match test_support::fresh().await {
            Some(r) => r?,
            None => return Ok(()),
        }
    };
}

async fn member(tx: &mut WsTx<'_>, admin_url: &str, status: &str) -> anyhow::Result<Uuid> {
    // users is a global table; create the user with the admin connection, membership in the ws tx.
    let user = Uuid::now_v7();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(admin_url).await?;
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut admin)
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

#[tokio::test]
async fn grants_scopes_escalation_invariants_and_cache() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let a = Access::new();
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    a.seed_workspace(&mut tx).await?;
    assert_eq!(a.list_roles(&mut tx).await?.iter().filter(|r| r.is_builtin).count(), 13, "12 templates plus HR Administrator (BPMS-223)");

    let admin = member(&mut tx, &t.admin_url, "active").await?;
    let plain = member(&mut tx, &t.admin_url, "active").await?;
    let designer = member(&mut tx, &t.admin_url, "active").await?;
    let gone = member(&mut tx, &t.admin_url, "deactivated").await?;
    a.grant(&mut tx, None, admin, "workspace.admin", Scope::Workspace, None, Some("bootstrap")).await?;
    a.grant(&mut tx, None, plain, "member", Scope::Workspace, None, None).await?;
    a.grant(&mut tx, None, gone, "workspace.admin", Scope::Workspace, None, None).await?;
    let hr = Uuid::now_v7();
    let fin = Uuid::now_v7();
    a.grant(&mut tx, None, designer, "process.designer", Scope::Collection(hr), None, None).await?;

    let d = a.check(&mut tx, admin, "member:invite", &Resource::Workspace).await?;
    assert!(d.allowed);
    assert_eq!(d.reasons, vec!["grant:workspace.admin@workspace"]);
    assert!(!a.check(&mut tx, plain, "member:invite", &Resource::Workspace).await?.allowed);
    assert!(a.check(&mut tx, plain, "request:start", &Resource::Workspace).await?.allowed);

    // Scope: designer can edit drafts inside the HR collection subtree only.
    let in_hr =
        Resource::Scoped { scope: Scope::Process(Uuid::now_v7()), ancestors: vec![Scope::Collection(hr)] };
    let in_fin =
        Resource::Scoped { scope: Scope::Process(Uuid::now_v7()), ancestors: vec![Scope::Collection(fin)] };
    assert!(a.check(&mut tx, designer, "process:edit_draft", &in_hr).await?.allowed);
    assert!(!a.check(&mut tx, designer, "process:edit_draft", &in_fin).await?.allowed);
    assert!(!a.check(&mut tx, designer, "process:publish", &in_hr).await?.allowed, "designer cannot publish");

    // Invariant: deactivated members are denied even with an admin grant.
    let d = a.check(&mut tx, gone, "member:invite", &Resource::Workspace).await?;
    assert!(!d.allowed);
    assert_eq!(d.reasons, vec!["invariant:active-members-only"]);

    // No escalation: a plain member cannot mint a role with permissions they lack; admins can't create owners.
    assert!(matches!(
        a.create_role(&mut tx, Some(plain), "sneaky", "Sneaky", &["process:publish".into()], None).await,
        Err(AccessError::Escalation(_))
    ));
    assert!(matches!(
        a.grant(&mut tx, Some(admin), plain, "workspace.owner", Scope::Workspace, None, None).await,
        Err(AccessError::Escalation(_))
    ));
    let hr_designer = a
        .create_role(
            &mut tx,
            Some(admin),
            "hr.designer",
            "HR Designer",
            &["process:view".into(), "process:edit_draft".into()],
            Some("process.designer"),
        )
        .await?;
    assert_eq!(hr_designer.builtin_template.as_deref(), Some("process.designer"));
    assert!(matches!(
        a.create_role(&mut tx, Some(admin), "hr.designer", "Dup", &[], None).await,
        Err(AccessError::RoleExists)
    ));
    let builtin = a
        .list_roles(&mut tx)
        .await?
        .into_iter()
        .find(|r| r.key == "member")
        .map(|r| r.id)
        .unwrap_or_default();
    assert!(matches!(
        a.update_role(&mut tx, Some(admin), builtin, &[]).await,
        Err(AccessError::BuiltinImmutable)
    ));

    // Expired grants don't count; revocation takes effect immediately (cache keyed by revision).
    let temp = member(&mut tx, &t.admin_url, "active").await?;
    a.grant(
        &mut tx,
        Some(admin),
        temp,
        "analyst",
        Scope::Workspace,
        Some(time::OffsetDateTime::now_utc() - time::Duration::hours(1)),
        Some("expired auditor"),
    )
    .await?;
    assert!(!a.check(&mut tx, temp, "report:view", &Resource::Workspace).await?.allowed);
    let g = a.grant(&mut tx, Some(admin), temp, "analyst", Scope::Workspace, None, None).await?;
    assert!(a.check(&mut tx, temp, "report:view", &Resource::Workspace).await?.allowed);
    a.revoke_grant(&mut tx, None, g).await?;
    assert!(!a.check(&mut tx, temp, "report:view", &Resource::Workspace).await?.allowed);
    tx.commit().await?;

    // Isolation: the same member id checked from another workspace finds nothing and is denied.
    let mut other = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    assert!(!a.check(&mut other, admin, "member:invite", &Resource::Workspace).await?.allowed);
    Ok(())
}
