//! BPMS-046: launch snapshot + notifications, own-only and single acknowledgement with the exact
//! version hash, reminder ladder and manager escalation on the application clock, dynamic new
//! joiners, evidence export, ending and evidence immutability.

use std::sync::Arc;

use procera_campaigns::{Actor, CampaignError, Campaigns, Defaults, NewCampaign};
use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::clock::ManualClock;
use procera_kernel::{Clock, UuidV7Gen};
use procera_library::{Change, Library, ReviewPolicy};
use procera_members::Members;
use procera_members::audience::Audience;
use procera_notify::Notify;
use serde_json::json;
use uuid::Uuid;

const DAY: i64 = 86_400;

async fn member(
    tx: &mut WsTx<'_>,
    admin: &mut sqlx::PgConnection,
    name: &str,
    dept: Option<Uuid>,
) -> anyhow::Result<Actor> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
        .bind(user)
        .bind(format!("{name}-{user}@acme.test"))
        .bind(name)
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status, department_id) VALUES ($1, $2, $3, 'active', $4)")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .bind(dept)
        .execute(tx.conn())
        .await?;
    Ok(Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() })
}

fn defaults(snapshot: &str) -> Defaults {
    Defaults {
        snapshot: snapshot.into(),
        cadence: vec![7 * DAY, 3 * DAY],
        escalation_after: Some(14 * DAY),
        due_in: 14 * DAY,
        ack_text: "I have read and understood this policy.".into(),
    }
}

async fn count(tx: &mut WsTx<'_>, sql: &str, m: Uuid) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(sql).bind(m).fetch_one(tx.conn()).await?)
}

const NOTES: &str = "SELECT count(*) FROM notifications WHERE membership_id = $1 AND kind = $2";

async fn notes(tx: &mut WsTx<'_>, m: Uuid, kind: &str) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(NOTES).bind(m).bind(kind).fetch_one(tx.conn()).await?)
}

#[tokio::test]
async fn launch_acknowledge_remind_escalate_enrol_and_export() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let clock = Arc::new(ManualClock::new(time::OffsetDateTime::now_utc()));
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
    let cmp = Campaigns::new(
        t.db.clone(),
        members,
        notify,
        clock.clone(),
        Arc::new(UuidV7Gen),
        "https://app.test/",
    );
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;

    let ops = Uuid::now_v7();
    sqlx::query("INSERT INTO departments (workspace_id, id, name, path) VALUES ($1, $2, 'Ops', ARRAY[$2])")
        .bind(ws.workspace_id)
        .bind(ops)
        .execute(tx.conn())
        .await?;
    let owner = member(&mut tx, &mut admin, "Olivia", None).await?;
    let boss = member(&mut tx, &mut admin, "Boss", None).await?;
    let ann = member(&mut tx, &mut admin, "Ann", Some(ops)).await?;
    let ben = member(&mut tx, &mut admin, "Ben", Some(ops)).await?;
    sqlx::query("UPDATE departments SET head_membership_id = $2 WHERE id = $1")
        .bind(ops)
        .bind(boss.membership)
        .execute(tx.conn())
        .await?;

    // A published policy, and a draft one.
    let none =
        ReviewPolicy { required_for: "none".into(), min_reviewers: 0, reason_for_change_required: false };
    let body = json!({"blocks": [{"type": "paragraph", "text": "Lock your screen."}]});
    let lib_actor = procera_library::Actor {
        membership: owner.membership,
        subject_ref: owner.subject_ref,
        ..Default::default()
    };
    let (_, v1) =
        lib.create(&mut tx, &lib_actor, "policy", "sec.screens", "Screen lock", "standard", &body).await?;
    lib.submit(&mut tx, &lib_actor, v1.id, &none).await?;
    let v1 = lib.publish(&mut tx, &lib_actor, v1.id, None, None, Change::Major, &none).await?;
    let hash = v1.content_hash.clone().expect("hash");
    let (_, draft) = lib.create(&mut tx, &lib_actor, "sop", "ops.draft", "Draft", "standard", &body).await?;

    let aud = Audience { departments: vec![ops], ..Audience::default() };
    let new = |version_id| NewCampaign {
        title: None,
        version_id,
        audience: aud.clone(),
        due_at: None,
        snapshot: None,
        ack_text: None,
    };
    assert!(matches!(
        cmp.create(&mut tx, &owner, &new(draft.id), &defaults("dynamic")).await,
        Err(CampaignError::Conflict(_))
    ));
    assert!(matches!(
        cmp.create(
            &mut tx,
            &owner,
            &NewCampaign { audience: Audience::default(), ..new(v1.id) },
            &defaults("dynamic")
        )
        .await,
        Err(CampaignError::Invalid(_))
    ));

    // Launch: one assignment per member in Ops, each notified in-app and by email.
    let c = cmp.create(&mut tx, &owner, &new(v1.id), &defaults("dynamic")).await?;
    assert_eq!(
        (c.state.as_str(), c.title.as_str(), c.reminder_schedule.clone()),
        ("draft", "Screen lock", vec![3 * DAY, 7 * DAY])
    );
    assert_eq!(c.subject.content_hash, hash);
    assert_eq!(cmp.audience_size(&mut tx, &aud).await?, 2);
    let c = cmp.launch(&mut tx, &owner, c.id).await?;
    assert_eq!((c.state.as_str(), c.progress.assigned, c.progress.pending), ("active", 2, 2));
    assert!(matches!(cmp.launch(&mut tx, &owner, c.id).await, Err(CampaignError::Conflict(_))));
    assert_eq!(notes(&mut tx, ann.membership, "campaign_assigned").await?, 1);
    assert_eq!(count(&mut tx, "SELECT count(*) FROM email_messages WHERE kind = 'campaign_assigned' AND to_email LIKE (SELECT u.email FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1)", ben.membership).await?, 1);
    assert_eq!(notes(&mut tx, owner.membership, "campaign_assigned").await?, 0, "not in the audience");

    // Acknowledge: own assignment only, exact hash, once.
    let mine = cmp.my_assignments(&mut tx, ann.membership, Some("pending")).await?;
    assert_eq!(mine.len(), 1);
    let a = &mine[0];
    assert_eq!(
        (a.subject.version_id, a.subject.content_hash.as_str(), a.subject.kind.as_str()),
        (v1.id, hash.as_str(), "policy")
    );
    assert!(matches!(
        cmp.acknowledge(&mut tx, &ben, a.id, None, json!({})).await,
        Err(CampaignError::NotFound)
    ));
    assert!(matches!(cmp.assignment_kind(&mut tx, a.id, ben.membership).await, Err(CampaignError::NotFound)));
    assert!(matches!(
        cmp.acknowledge(&mut tx, &ann, a.id, Some(&"0".repeat(64)), json!({})).await,
        Err(CampaignError::ContentChanged { .. })
    ));
    let ann_ua = Actor { ip: Some("203.0.113.7".into()), user_agent: Some("UA/1".into()), ..ann.clone() };
    let done = cmp
        .acknowledge(&mut tx, &ann_ua, a.id, Some(&hash.to_uppercase()), json!({"amr": ["pwd", "otp"]}))
        .await?;
    assert_eq!(
        (done.state.as_str(), done.acknowledged_version_id, done.acknowledged_content_hash.as_deref()),
        ("completed", Some(v1.id), Some(hash.as_str()))
    );
    assert!(matches!(
        cmp.acknowledge(&mut tx, &ann_ua, a.id, None, json!({})).await,
        Err(CampaignError::AlreadyAcknowledged)
    ));
    let ip: Option<String> = sqlx::query_scalar("SELECT host(ack_ip) FROM campaign_recipients WHERE id = $1")
        .bind(a.id)
        .fetch_one(tx.conn())
        .await?;
    assert_eq!(ip.as_deref(), Some("203.0.113.7"));
    // Recorded acknowledgements are frozen, even for direct SQL.
    let mut sp = <sqlx::PgConnection as sqlx::Connection>::begin(tx.conn()).await?;
    assert!(
        sqlx::query("UPDATE campaign_recipients SET ack_text = 'x' WHERE id = $1")
            .bind(a.id)
            .execute(&mut *sp)
            .await
            .is_err()
    );
    sp.rollback().await?;

    // An at-launch campaign on the same version, for the joiner check below.
    let fixed = cmp
        .create(
            &mut tx,
            &owner,
            &NewCampaign { snapshot: Some("at_launch".into()), title: Some("Fixed".into()), ..new(v1.id) },
            &defaults("dynamic"),
        )
        .await?;
    let fixed = cmp.launch(&mut tx, &owner, fixed.id).await?;
    assert_eq!(fixed.audience_snapshot, "at_launch");
    tx.commit().await?;

    // Reminders on the application clock: step 1 at +3 d, step 2 at +7 d, escalation at +14 d.
    clock.advance(time::Duration::days(4));
    let p = cmp.run_due().await?;
    assert_eq!((p.reminders, p.escalations), (3, 0), "Ben in both, Ann in the fixed one");
    assert_eq!(cmp.run_due().await?.reminders, 0, "each step is sent once");
    clock.advance(time::Duration::days(11));
    let p = cmp.run_due().await?;
    assert_eq!((p.reminders, p.escalations), (3, 3), "one reminder for the missed ladder, then escalation");
    let mut tx = t.db.begin_ws(ws).await?;
    assert_eq!(notes(&mut tx, ben.membership, "campaign_reminder").await?, 4, "two steps in each campaign");
    assert_eq!(notes(&mut tx, boss.membership, "campaign_escalated").await?, 3);
    let c = cmp.get(&mut tx, c.id).await?;
    assert_eq!((c.progress.acknowledged, c.progress.overdue), (1, 1));
    let overdue = cmp.recipients(&mut tx, c.id, Some("overdue")).await?;
    assert_eq!(overdue.len(), 1);
    assert_eq!((overdue[0].membership_id, overdue[0].escalated_to), (ben.membership, Some(boss.membership)));

    // A new member in Ops joins the dynamic campaign only (once), with the campaign's 14-day window.
    let cy = member(&mut tx, &mut admin, "Cy", Some(ops)).await?;
    assert_eq!(cmp.enroll_joiner(&mut tx, cy.membership).await?, 1);
    assert_eq!(cmp.enroll_joiner(&mut tx, cy.membership).await?, 0);
    let cy_open = cmp.my_assignments(&mut tx, cy.membership, Some("pending")).await?;
    assert_eq!(cy_open.len(), 1);
    assert_eq!(cy_open[0].campaign_id, c.id);
    assert_eq!(cy_open[0].due_at - cy_open[0].assigned_at, time::Duration::days(14));
    assert!(!cy_open[0].overdue);
    let outsider = member(&mut tx, &mut admin, "Out", None).await?;
    assert_eq!(cmp.enroll_joiner(&mut tx, outsider.membership).await?, 0);

    // Evidence export.
    let csv = cmp.evidence_csv(&mut tx, &owner, c.id).await?;
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "member,email,department,state,assigned_at,due_at,acknowledged_at,version,version_id,content_hash,auth_method"
    );
    assert_eq!(lines.len(), 4);
    let ann_line = lines.iter().find(|l| l.starts_with("Ann,")).expect("ann row");
    assert!(
        ann_line.contains(&hash) && ann_line.contains(",Ops,completed,") && ann_line.ends_with("pwd+otp")
    );

    // Ending: close freezes; cancel needs a reason; no acknowledgements after the end.
    assert!(matches!(
        cmp.end(&mut tx, &owner, fixed.id, "cancelled", None).await,
        Err(CampaignError::Invalid(_))
    ));
    assert_eq!(
        cmp.end(&mut tx, &owner, fixed.id, "cancelled", Some("wrong audience")).await?.state,
        "cancelled"
    );
    assert_eq!(cmp.remind(&mut tx, &owner, c.id).await?, 2, "Ben and Cy");
    let closed = cmp.end(&mut tx, &owner, c.id, "closed", None).await?;
    assert_eq!(closed.state, "closed");
    assert!(matches!(cmp.end(&mut tx, &owner, c.id, "closed", None).await, Err(CampaignError::Conflict(_))));
    assert!(matches!(
        cmp.acknowledge(&mut tx, &cy, cy_open[0].id, None, json!({})).await,
        Err(CampaignError::Conflict(_))
    ));
    assert!(cmp.my_assignments(&mut tx, cy.membership, Some("pending")).await?.is_empty());

    // BPMS-045: a re-acknowledgement campaign starts once per version, as the system when the
    // scheduler published it, and launches at once with the requested due date.
    let due = clock.now() + time::Duration::days(14);
    let reack = NewCampaign {
        title: Some("Re-acknowledge Screen lock v1.0".into()),
        due_at: Some(due),
        ..new(v1.id)
    };
    let r = cmp
        .start_reack(&mut tx, None, owner.membership, &reack, &defaults("dynamic"))
        .await?
        .expect("started");
    assert_eq!((r.origin.as_str(), r.state.as_str(), r.due_at), ("reack", "active", Some(due)));
    assert_eq!((r.created_by, r.progress.assigned), (owner.membership, 3), "Ann, Ben and Cy");
    assert!(
        cmp.start_reack(&mut tx, Some(&owner), owner.membership, &reack, &defaults("dynamic"))
            .await?
            .is_none()
    );
    let nobody = NewCampaign {
        audience: Audience { groups: vec![Uuid::now_v7()], ..Audience::default() },
        ..new(v1.id)
    };
    assert!(cmp.start_reack(&mut tx, None, owner.membership, &nobody, &defaults("dynamic")).await?.is_none());
    let system: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action IN ('campaign.created', 'campaign.activated') AND object_id = $1 AND actor_kind = 'system'")
        .bind(r.id).fetch_one(tx.conn()).await?;
    assert_eq!(system, 2, "audited as the system");
    tx.commit().await?;

    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!(report.first_broken, None);
    Ok(())
}

#[tokio::test]
async fn campaigns_are_isolated_per_workspace() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM procera_rls_violations").fetch_one(tx.conn()).await?;
    assert_eq!(n, 0, "campaign tables have forced RLS");
    let seen: i64 = sqlx::query_scalar("SELECT count(*) FROM campaigns").fetch_one(tx.conn()).await?;
    assert_eq!(seen, 0);
    tx.rollback().await?;
    Ok(())
}
