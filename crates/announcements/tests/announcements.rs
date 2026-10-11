//! BPMS-047: compose, targeting snapshot, approval with SoD, scheduling, expiry, receipts, analytics.

use std::sync::Arc;

use procera_announcements::{AnnouncementError, Announcements, Compose, Defaults, ListScope};
use procera_db::{WsCtx, WsTx, test_support};
use procera_kernel::UuidV7Gen;
use procera_kernel::clock::ManualClock;
use procera_members::audience::Audience;
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

async fn user(admin: &mut sqlx::PgConnection) -> anyhow::Result<Uuid> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await?;
    Ok(user)
}

async fn member(
    tx: &mut WsTx<'_>,
    admin: &mut sqlx::PgConnection,
    department: Option<Uuid>,
) -> anyhow::Result<procera_announcements::Actor> {
    let u = user(admin).await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status, department_id) VALUES ($1, $2, $3, 'active', $4)")
        .bind(tx.ctx.workspace_id).bind(m).bind(u).bind(department).execute(tx.conn()).await?;
    Ok(procera_announcements::Actor {
        membership: m,
        subject_ref: Uuid::new_v4(),
        ip: Some("192.0.2.7".into()),
        user_agent: Some("tests".into()),
    })
}

async fn department(
    tx: &mut WsTx<'_>,
    name: &str,
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
    .bind(name)
    .bind(&path)
    .execute(tx.conn())
    .await?;
    Ok((id, path))
}

fn compose(title: &str, audience: Audience) -> Compose {
    Compose {
        title: title.into(),
        content: json!({"blocks": [{"type": "paragraph", "text": title}]}),
        audience,
        priority: "normal".into(),
        pinned: false,
        require_ack: None,
        publish_at: None,
        expires_at: None,
    }
}

const CHANNELS: &[&str] = &["in_app"];

#[tokio::test]
async fn compose_target_approve_schedule_expire_and_receipts() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let start = OffsetDateTime::now_utc();
    let clock = Arc::new(ManualClock::new(start));
    let ann = Announcements::new(t.db.clone(), clock.clone(), Arc::new(UuidV7Gen));
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let creator = user(&mut admin).await?;
    sqlx::query("INSERT INTO workspaces (id, slug, name, profile, state, created_by) VALUES ($1, $2, 'Acme', 'standard', 'active', $3)")
        .bind(ws.workspace_id).bind(format!("acme-{}", &ws.workspace_id.simple().to_string()[..8])).bind(creator)
        .execute(&mut admin).await?;
    let channels: Vec<String> = CHANNELS.iter().map(|c| (*c).to_owned()).collect();
    let defaults = Defaults { require_ack: true, expiry: Duration::days(30) };

    let mut tx = t.db.begin_ws(ws).await?;
    let ops = department(&mut tx, "Operations", None).await?;
    let support = department(&mut tx, "Support", Some(ops.clone())).await?;
    let author = member(&mut tx, &mut admin, None).await?;
    let approver = member(&mut tx, &mut admin, None).await?;
    let alice = member(&mut tx, &mut admin, Some(ops.0)).await?;
    let bob = member(&mut tx, &mut admin, Some(support.0)).await?;
    let carol = member(&mut tx, &mut admin, None).await?;

    // Validation: empty audience, unknown department, expiry before publication.
    assert!(matches!(
        ann.create(&mut tx, &author, &compose("Empty", Audience::default()), &defaults).await,
        Err(AnnouncementError::Invalid(_))
    ));
    let unknown = Audience { departments: vec![Uuid::now_v7()], ..Audience::default() };
    assert!(matches!(
        ann.create(&mut tx, &author, &compose("Unknown", unknown), &defaults).await,
        Err(AnnouncementError::Invalid(_))
    ));
    let mut bad = compose("Bad expiry", Audience { all: true, ..Audience::default() });
    bad.expires_at = Some(start - Duration::minutes(1));
    assert!(matches!(
        ann.create(&mut tx, &author, &bad, &defaults).await,
        Err(AnnouncementError::Invalid(_))
    ));

    // Department-targeted (with subtree), defaults applied, edited with optimistic concurrency.
    let to_ops = Audience { departments: vec![ops.0], ..Audience::default() };
    let a = ann.create(&mut tx, &author, &compose("Office move", to_ops.clone()), &defaults).await?;
    assert_eq!((a.state.as_str(), a.require_ack), ("draft", true));
    assert_eq!(a.expires_at, start + Duration::days(30));
    let mut edit = compose("Office move on Monday", to_ops.clone());
    edit.pinned = true;
    let a = ann.edit(&mut tx, &author, a.id, &edit, &defaults, Some(a.revision)).await?;
    assert!(matches!(
        ann.edit(&mut tx, &author, a.id, &edit, &defaults, Some(a.revision - 1)).await,
        Err(AnnouncementError::StaleRevision { .. })
    ));

    // Not workspace-wide: no approval under for_workspace_wide, published at once to the snapshot.
    let p = ann.publish(&mut tx, &author, a.id, "for_workspace_wide", &channels).await?;
    assert_eq!((p.state.as_str(), p.recipient_count), ("published", Some(2)));
    assert_eq!(p.content_hash.as_ref().map(Vec::len), Some(32));
    assert!(matches!(
        ann.edit(&mut tx, &author, a.id, &edit, &defaults, None).await,
        Err(AnnouncementError::Conflict(_))
    ));
    let mut sp = <sqlx::PgConnection as sqlx::Connection>::begin(tx.conn()).await?;
    let err = sqlx::query("UPDATE announcements SET title = 'sneaky' WHERE id = $1")
        .bind(a.id)
        .execute(&mut *sp)
        .await;
    assert!(err.is_err(), "the trigger freezes published content");
    sp.rollback().await?;
    assert_eq!(ann.feed(&mut tx, alice.membership, false).await?.len(), 1);
    assert_eq!(ann.feed(&mut tx, bob.membership, false).await?.len(), 1, "sub-departments are included");
    assert!(ann.feed(&mut tx, carol.membership, false).await?.is_empty(), "only the audience sees it");
    assert!(matches!(ann.mark_read(&mut tx, &carol, a.id).await, Err(AnnouncementError::NotFound)));

    // Receipts and analytics.
    let r1 = ann.mark_read(&mut tx, &alice, a.id).await?;
    let r2 = ann.mark_read(&mut tx, &alice, a.id).await?;
    assert_eq!(r1.read_at, r2.read_at, "first read wins");
    let ack = ann.acknowledge(&mut tx, &alice, a.id).await?;
    assert!(ack.acked_at.is_some());
    assert!(ann.feed(&mut tx, alice.membership, true).await?.is_empty(), "read items leave the unread feed");
    let stats = ann.analytics(&mut tx, a.id).await?;
    assert_eq!((stats.recipients, stats.read, stats.acknowledged, stats.pending_ack), (2, 1, 1, 1));
    assert_eq!(stats.by_department.len(), 2);
    let d = ann.delivery(&mut tx, a.id).await?.ok_or_else(|| anyhow::anyhow!("delivery"))?;
    assert_eq!((d.recipients.len(), d.workspace.as_str(), d.channels.clone()), (2, "Acme", channels.clone()));

    // Workspace-wide: approval applies; authors cannot approve their own; rejection needs a comment.
    let everyone = Audience { all: true, ..Audience::default() };
    let mut c = compose("Holiday schedule", everyone);
    c.require_ack = Some(false);
    c.priority = "critical".into();
    let w = ann.create(&mut tx, &author, &c, &defaults).await?;
    assert!(matches!(
        ann.publish(&mut tx, &author, w.id, "for_workspace_wide", &channels).await,
        Err(AnnouncementError::Conflict(_))
    ));
    let s = ann.submit(&mut tx, &author, w.id, "for_workspace_wide").await?;
    assert_eq!((s.state.as_str(), s.approval_required), ("pending_approval", true));
    assert!(matches!(
        ann.approve(&mut tx, &author, w.id, &channels).await,
        Err(AnnouncementError::SeparationOfDuties(_))
    ));
    assert!(matches!(ann.reject(&mut tx, &approver, w.id, " ").await, Err(AnnouncementError::Invalid(_))));
    assert_eq!(ann.reject(&mut tx, &approver, w.id, "Add dates").await?.state, "draft");
    ann.submit(&mut tx, &author, w.id, "for_workspace_wide").await?;
    let w = ann.approve(&mut tx, &approver, w.id, &channels).await?;
    assert_eq!(
        (w.state.as_str(), w.recipient_count, w.decided_by),
        ("published", Some(5), Some(approver.membership))
    );
    assert!(matches!(ann.acknowledge(&mut tx, &bob, w.id).await, Err(AnnouncementError::Conflict(_))));
    let feed = ann.feed(&mut tx, alice.membership, false).await?;
    assert_eq!(feed.first().map(|f| f.id), Some(a.id), "pinned first");
    assert_eq!(feed.len(), 2);

    // Scheduled publication and expiry, driven by the clock.
    let mut later = compose("Fire drill", Audience { departments: vec![support.0], ..Audience::default() });
    later.publish_at = Some(start + Duration::hours(1));
    later.expires_at = Some(start + Duration::hours(3));
    let sch = ann.create(&mut tx, &author, &later, &defaults).await?;
    assert_eq!(ann.publish(&mut tx, &author, sch.id, "off", &channels).await?.state, "scheduled");
    assert_eq!(ann.feed(&mut tx, bob.membership, false).await?.len(), 2);
    let mut short =
        compose("Lunch today", Audience { groups: vec![], departments: vec![ops.0], ..Audience::default() });
    short.expires_at = Some(start + Duration::minutes(30));
    let lunch = ann.create(&mut tx, &author, &short, &defaults).await?;
    ann.publish(&mut tx, &author, lunch.id, "off", &channels).await?;
    tx.commit().await?;
    assert_eq!(ann.publish_due().await?, 0, "not due yet");
    clock.advance(Duration::hours(2));
    assert_eq!(ann.publish_due().await?, 1);
    let mut tx = t.db.begin_ws(ws).await?;
    let bob_feed: Vec<Uuid> =
        ann.feed(&mut tx, bob.membership, false).await?.into_iter().map(|f| f.id).collect();
    assert!(
        bob_feed.contains(&sch.id) && !bob_feed.contains(&lunch.id),
        "scheduled is live, expired is hidden"
    );
    assert!(ann.get(&mut tx, lunch.id).await?.expired);
    assert!(matches!(ann.mark_read(&mut tx, &bob, lunch.id).await, Err(AnnouncementError::NotFound)));

    // Archive (withdraw) with a reason; it leaves the feed; drafts cannot be archived.
    assert!(matches!(ann.archive(&mut tx, &author, sch.id, " ").await, Err(AnnouncementError::Invalid(_))));
    assert_eq!(ann.archive(&mut tx, &author, sch.id, "Drill cancelled").await?.state, "archived");
    assert!(!ann.feed(&mut tx, bob.membership, false).await?.iter().any(|f| f.id == sch.id));
    let draft = ann.create(&mut tx, &author, &compose("Draft", to_ops), &defaults).await?;
    assert!(matches!(
        ann.archive(&mut tx, &author, draft.id, "x").await,
        Err(AnnouncementError::Conflict(_))
    ));

    // Management lists: the author sees their own; a publisher sees everyone's submitted ones.
    let mine = ann
        .list(&mut tx, ListScope { all: false, others_submitted: false, me: author.membership }, None)
        .await?;
    assert_eq!(mine.len(), 5);
    let theirs = ann
        .list(&mut tx, ListScope { all: false, others_submitted: true, me: approver.membership }, None)
        .await?;
    assert_eq!(theirs.len(), 4, "everything except the draft");
    tx.commit().await?;

    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!(report.first_broken, None);
    assert!(report.checked >= 18, "every transition is audited ({})", report.checked);
    Ok(())
}
