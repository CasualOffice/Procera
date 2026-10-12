//! BPMS-046/023: when a member leaves (deactivated), their open assignments close as `left`, never
//! as acknowledged; they drop out of the progress counts, stay frozen as evidence, and each closure
//! is audited. Completed assignments and active members are untouched.

use std::sync::Arc;

use procera_campaigns::{Actor, Campaigns, Defaults, NewCampaign};
use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_library::{Change, Library, ReviewPolicy};
use procera_members::Members;
use procera_members::audience::Audience;
use procera_notify::Notify;
use serde_json::json;
use uuid::Uuid;

async fn member(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection, name: &str) -> anyhow::Result<Actor> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
        .bind(user)
        .bind(format!("{name}-{user}@acme.test"))
        .bind(name)
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
    Ok(Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() })
}

#[tokio::test]
async fn leavers_assignments_close_as_left_and_leave_the_counts() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let clock = Arc::new(SystemClock);
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64())?);
    let identity = Identity::new(
        t.db.clone(),
        keys.clone(),
        clock.clone(),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )?;
    let members = Members::new(
        t.db.clone(),
        identity,
        procera_access::Access::new(),
        keys.clone(),
        clock.clone(),
        Arc::new(UuidV7Gen),
    );
    let notify = Notify::new(t.db.clone(), keys, clock.clone(), Arc::new(UuidV7Gen));
    let lib = Library::new(t.db.clone(), clock.clone(), Arc::new(UuidV7Gen));
    let cmp = Campaigns::new(t.db.clone(), members, notify, clock, Arc::new(UuidV7Gen), "https://app.test/");
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;

    let owner = member(&mut tx, &mut admin, "Olivia").await?;
    let ben = member(&mut tx, &mut admin, "Ben").await?;
    let none =
        ReviewPolicy { required_for: "none".into(), min_reviewers: 0, reason_for_change_required: false };
    let body = json!({"blocks": [{"type": "paragraph", "text": "Lock your screen."}]});
    let la = procera_library::Actor {
        membership: owner.membership,
        subject_ref: owner.subject_ref,
        ..Default::default()
    };
    let (_, v1) = lib.create(&mut tx, &la, "policy", "sec.screens", "Screen lock", "standard", &body).await?;
    lib.submit(&mut tx, &la, v1.id, &none).await?;
    lib.publish(&mut tx, &la, v1.id, None, None, Change::Major, &none).await?;
    let new = NewCampaign {
        title: None,
        version_id: v1.id,
        audience: Audience { all: true, ..Audience::default() },
        due_at: None,
        snapshot: None,
        ack_text: None,
    };
    let defaults = Defaults {
        snapshot: "at_launch".into(),
        cadence: vec![],
        escalation_after: None,
        due_in: 14 * 86_400,
        ack_text: "I have read it.".into(),
    };
    let c = cmp.create(&mut tx, &owner, &new, &defaults).await?;
    let c = cmp.launch(&mut tx, &owner, c.id).await?;
    assert_eq!(c.progress.assigned, 2);

    // Still active: nothing to close.
    assert_eq!(cmp.close_for_leaver(&mut tx, ben.membership).await?, 0);
    sqlx::query("UPDATE memberships SET status = 'deactivated', deactivated_at = now() WHERE id = $1")
        .bind(ben.membership)
        .execute(tx.conn())
        .await?;
    assert_eq!(cmp.close_for_leaver(&mut tx, ben.membership).await?, 1);
    assert_eq!(cmp.close_for_leaver(&mut tx, ben.membership).await?, 0, "idempotent");

    let c = cmp.get(&mut tx, c.id).await?;
    assert_eq!((c.progress.assigned, c.progress.pending, c.progress.left), (1, 1, 1));
    assert!(cmp.my_assignments(&mut tx, ben.membership, Some("pending")).await?.is_empty());
    let (state, left_at): (String, Option<time::OffsetDateTime>) =
        sqlx::query_as("SELECT state, left_at FROM campaign_recipients WHERE membership_id = $1")
            .bind(ben.membership)
            .fetch_one(tx.conn())
            .await?;
    assert_eq!(state, "left");
    assert!(left_at.is_some());
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'campaign.recipient_left' AND diff->>'membership_id' = $1",
    )
    .bind(ben.membership.to_string())
    .fetch_one(tx.conn())
    .await?;
    assert_eq!(audited, 1);
    tx.commit().await?;

    // A closed assignment is evidence: it can't be reopened.
    let mut tx = t.db.begin_ws(ws).await?;
    let reopen = sqlx::query("UPDATE campaign_recipients SET state = 'pending' WHERE membership_id = $1")
        .bind(ben.membership)
        .execute(tx.conn())
        .await;
    assert!(reopen.is_err());
    Ok(())
}
