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
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).ok()?);
    let id = Identity::new(
        t.db.clone(),
        keys,
        Arc::new(SystemClock),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )
    .ok()?;
    let p = Platform::new(t.db.clone(), id, Arc::new(SystemClock), Arc::new(UuidV7Gen));
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
    let (_t, p) = setup_or_skip!();
    let token = p.issue_bootstrap_token().await?.expect("token before bootstrap");
    assert!(matches!(
        p.bootstrap("wrong-token", "root@acme.test", "Root", PW, &c()).await,
        Err(PlatformError::BootstrapInvalid)
    ));
    let owner = p.bootstrap(&token, "root@acme.test", "Root", PW, &c()).await?;
    assert_eq!(p.system_roles(owner.id).await?, vec!["system.owner"]);
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
