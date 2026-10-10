//! BPMS-020 integration tests.

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{ClientInfo, Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_platform::{NewWorkspace, Platform, PlatformError, Quotas, WorkspaceState};

const PW: &str = "correct horse battery staple";

async fn setup() -> Option<anyhow::Result<(test_support::TestDb, Platform)>> {
    let t = match test_support::fresh().await? {
        Ok(t) => t,
        Err(e) => return Some(Err(e.into())),
    };
    let keys: Arc<LocalKey> = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).ok()?);
    let id = Identity::new(
        t.db.clone(),
        keys.clone(),
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )
    .ok()?;
    let members = procera_members::Members::new(
        t.db.clone(),
        id.clone(),
        procera_access::Access::new(),
        keys,
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
    );
    let p = Platform::new(t.db.clone(), id, members, Arc::new(SystemClock), Arc::new(UuidV7Gen));
    Some(Ok((t, p)))
}

macro_rules! setup_or_skip {
    () => {
        match setup().await {
            Some(r) => r?,
            None => return Ok(()),
        }
    };
}

fn c() -> ClientInfo {
    ClientInfo::default()
}

fn new_ws(slug: &str, profile: &str) -> NewWorkspace {
    NewWorkspace {
        name: format!("Acme {slug}"),
        slug: slug.into(),
        profile: profile.into(),
        owner_email: "owner@acme.test".into(),
        region: None,
    }
}

#[tokio::test]
async fn bootstrap_is_single_use_and_creates_the_owner() -> anyhow::Result<()> {
    let (t, p) = setup_or_skip!();
    let token = p.issue_bootstrap_token().await?.expect("token before bootstrap");
    assert!(matches!(
        p.bootstrap("wrong-token", "root@acme.test", "Root", PW, &c()).await,
        Err(PlatformError::BootstrapInvalid)
    ));
    let owner = p.bootstrap(&token, "root@acme.test", "Root", PW, &c()).await?;
    assert_eq!(p.system_roles(owner.id).await?, vec!["system.owner"]);
    // Audit contract for POST /sysapi/v1/bootstrap (apps/procera/src/http/audit_contract.rs).
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: procera_db::SYSTEM_WORKSPACE }).await?;
    let mut actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE action <> 'platform.bootstrap_token_issued'",
    )
    .fetch_all(tx.conn())
    .await?;
    tx.rollback().await?;
    actions.sort();
    assert_eq!(actions, vec!["platform.bootstrapped", "user.created"]);
    assert!(matches!(
        p.bootstrap(&token, "x@acme.test", "X", PW, &c()).await,
        Err(PlatformError::AlreadyBootstrapped)
    ));
    assert_eq!(p.issue_bootstrap_token().await?, None, "no more tokens once bootstrapped");
    Ok(())
}

#[tokio::test]
async fn workspace_lifecycle_quotas_and_audit() -> anyhow::Result<()> {
    let (t, p) = setup_or_skip!();
    let token = p.issue_bootstrap_token().await?.expect("token");
    let owner = p.bootstrap(&token, "root@acme.test", "Root", PW, &c()).await?;

    let a = p.create_workspace(owner.id, &new_ws("acme-hr", "starter"), &c()).await?;
    assert_eq!(a.state, WorkspaceState::Provisioning);
    assert_eq!(a.quotas.max_members, Some(100));
    assert!(matches!(
        p.create_workspace(owner.id, &new_ws("acme-hr", "standard"), &c()).await,
        Err(PlatformError::SlugTaken)
    ));
    assert!(matches!(
        p.create_workspace(owner.id, &new_ws("Bad Slug!", "starter"), &c()).await,
        Err(PlatformError::Invalid(_))
    ));
    let b = p.create_workspace(owner.id, &new_ws("acme-fin", "enterprise"), &c()).await?;
    assert_eq!(b.quotas.max_members, None, "enterprise is unlimited by default");
    assert_eq!(p.list_workspaces().await?.len(), 2);

    let s = p.suspend(owner.id, a.id, Some(a.revision), false, "unpaid invoice", &c()).await?;
    assert_eq!(
        (s.state, s.suspended_reason.as_deref()),
        (WorkspaceState::SuspendedRo, Some("unpaid invoice"))
    );
    assert!(
        matches!(
            p.restore(owner.id, a.id, Some(a.revision), &c()).await,
            Err(PlatformError::StaleRevision { .. })
        ),
        "stale If-Match"
    );
    let r = p.restore(owner.id, a.id, Some(s.revision), &c()).await?;
    assert_eq!(r.state, WorkspaceState::Active);
    assert!(matches!(p.restore(owner.id, a.id, None, &c()).await, Err(PlatformError::BadTransition(_))));

    let d = p.schedule_deletion(owner.id, b.id, None, 30, &c()).await?;
    assert_eq!(d.state, WorkspaceState::PendingDeletion);
    assert!(d.deletion_due_at.is_some());
    assert!(matches!(
        p.schedule_deletion(owner.id, a.id, None, 1, &c()).await,
        Err(PlatformError::Invalid(_))
    ));
    let q = p
        .set_quotas(
            owner.id,
            a.id,
            &Quotas { max_members: Some(250), ..Quotas::for_profile("starter") },
            &c(),
        )
        .await?;
    assert_eq!(q.quotas.max_members, Some(250));
    let renamed = p.rename(owner.id, a.id, Some(q.revision), "Acme People", &c()).await?;
    assert_eq!(renamed.name, "Acme People");

    let report = procera_audit::verify(&t.db, WsCtx { workspace_id: procera_db::SYSTEM_WORKSPACE }).await?;
    assert_eq!(report.first_broken, None);
    assert!(report.checked >= 8, "every lifecycle change is audited ({} events)", report.checked);
    Ok(())
}

#[tokio::test]
async fn onboarding_owner_invite_activates_workspace_then_members_and_guards() -> anyhow::Result<()> {
    let Some(r) = test_support::fresh().await else { return Ok(()) };
    let t = r?;
    let keys: Arc<LocalKey> = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let identity = Identity::new(
        t.db.clone(),
        keys.clone(),
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )?;
    let access = procera_access::Access::new();
    let members = procera_members::Members::new(
        t.db.clone(),
        identity.clone(),
        access.clone(),
        keys,
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
    );
    let p = Platform::new(
        t.db.clone(),
        identity.clone(),
        members.clone(),
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
    );
    let token = p.issue_bootstrap_token().await?.expect("token");
    let root = p.bootstrap(&token, "root@acme.test", "Root", PW, &c()).await?;

    let ws = p
        .create_workspace(
            root.id,
            &NewWorkspace {
                name: "Acme".into(),
                slug: "acme".into(),
                profile: "standard".into(),
                owner_email: "olivia@acme.test".into(),
                region: None,
            },
            &c(),
        )
        .await?;
    assert_eq!(ws.state, WorkspaceState::Provisioning);
    let ctx = WsCtx { workspace_id: ws.id };
    let owner_token = members.pending_invitation_token(ctx, "olivia@acme.test").await?.expect("owner invite");

    // A different user cannot use Olivia's invitation.
    let mallory = identity.create_user("mallory@acme.test", "Mallory", PW, &c()).await?;
    assert!(matches!(
        members.accept(&owner_token, mallory.id, &c()).await,
        Err(procera_members::MembersError::EmailMismatch)
    ));
    // Olivia registers through the invitation; the workspace becomes active and she holds owner permissions.
    let (olivia, ws_id, olivia_m) = members.register(&owner_token, "Olivia", PW, &c()).await?;
    assert_eq!(ws_id, ws.id);
    assert_eq!(p.get_workspace(ws.id).await?.state, WorkspaceState::Active);
    assert!(
        matches!(
            members.accept(&owner_token, olivia.id, &c()).await,
            Err(procera_members::MembersError::InvitationInvalid)
        ),
        "single use"
    );
    let mut tx = t.db.begin_ws(ctx).await?;
    assert!(
        access
            .check(&mut tx, olivia_m, "workspace:delete.request", &procera_access::Resource::Workspace)
            .await?
            .allowed
    );
    let olivia_actor = procera_members::Actor { membership: olivia_m, subject_ref: olivia.subject_ref };
    let (_inv, admin_token) = members
        .invite(&mut tx, Some(olivia_actor), "adam@acme.test", &["workspace.admin".into()], &c())
        .await?;
    tx.commit().await?;
    let adam = identity.create_user("adam@acme.test", "Adam", PW, &c()).await?;
    let (_, adam_m) = members.accept(&admin_token, adam.id, &c()).await?;

    let mut tx = t.db.begin_ws(ctx).await?;
    let adam_actor = procera_members::Actor { membership: adam_m, subject_ref: adam.subject_ref };
    // Admins cannot invite owners (escalation) and nobody can remove the last owner or themselves.
    assert!(matches!(
        members.invite(&mut tx, Some(adam_actor), "eve@acme.test", &["workspace.owner".into()], &c()).await,
        Err(procera_members::MembersError::Access(_))
    ));
    assert!(matches!(
        members.deactivate(&mut tx, adam_actor, olivia_m, &c()).await,
        Err(procera_members::MembersError::LastOwner)
    ));
    assert!(matches!(
        members.deactivate(&mut tx, adam_actor, adam_m, &c()).await,
        Err(procera_members::MembersError::SelfDeactivation)
    ));
    let list = members.list(&mut tx).await?;
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|m| m.email == "olivia@acme.test" && m.roles == vec!["workspace.owner"]));
    // Olivia deactivates Adam: his access is denied immediately.
    members.deactivate(&mut tx, olivia_actor, adam_m, &c()).await?;
    assert!(
        !access.check(&mut tx, adam_m, "member:view", &procera_access::Resource::Workspace).await?.allowed
    );
    tx.commit().await?;

    assert_eq!(
        members.user_workspaces(olivia.id).await?.iter().map(|w| w.slug.as_str()).collect::<Vec<_>>(),
        vec!["acme"]
    );
    assert!(members.user_workspaces(adam.id).await?.is_empty(), "deactivated memberships are not listed");
    let report = procera_audit::verify(&t.db, ctx).await?;
    assert_eq!(report.first_broken, None);
    assert!(report.checked >= 5, "workspace chain has invites, joins, deactivation ({})", report.checked);
    Ok(())
}
