//! BPMS-106 (P0 subset): installing a template pack creates drafts, never duplicates silently, and
//! appends one `template.installed` plus the normal `artifact.created` records.

use std::sync::Arc;

use procera_db::{WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_library::templates::{Install, OnConflict, pack};
use procera_library::{Actor, Library, LibraryError};
use uuid::Uuid;

#[tokio::test]
async fn install_creates_drafts_skips_or_copies_existing_keys_and_audits_once() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let lib = Library::new(t.db.clone(), Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut admin)
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(ws.workspace_id)
        .bind(m)
        .bind(user)
        .execute(tx.conn())
        .await?;
    let actor = Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() };
    let p = pack("policy-ack").ok_or_else(|| anyhow::anyhow!("policy-ack pack"))?;

    // First install: everything, with the campaign plan.
    let all = Install { plan_campaign: true, ..Install::default() };
    let first = lib.install_pack(&mut tx, &actor, p, &all).await?;
    assert_eq!(first.documents.len(), p.documents.len());
    assert!(
        first
            .documents
            .iter()
            .all(|d| d.outcome == "created" && d.state == "draft" && d.version_id.is_some())
    );
    let handbook = first.documents.iter().find(|d| d.key == "employee-handbook").map(|d| d.artifact_id);
    let plan = first.campaign.as_ref().ok_or_else(|| anyhow::anyhow!("campaign plan"))?;
    assert_eq!(
        (Some(plan.artifact_id), plan.status, plan.due_in_days, plan.audience),
        (handbook, "publish_first", 14, "everyone")
    );
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_membership_id FROM artifacts WHERE key = 'code-of-conduct'")
            .fetch_one(tx.conn())
            .await?;
    assert_eq!(owner, Some(m), "the installer owns the drafts");

    // Second install: nothing is duplicated; every document is reported as skipped.
    let again = lib.install_pack(&mut tx, &actor, p, &all).await?;
    assert!(again.documents.iter().all(|d| d.outcome == "skipped" && d.version_id.is_none()));
    assert_eq!(
        again.campaign.as_ref().map(|c| c.artifact_id),
        handbook,
        "the plan follows the existing handbook"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM artifacts").fetch_one(tx.conn()).await?;
    assert_eq!(count, p.documents.len() as i64);

    // Asking for a copy of one document takes the next free key and numbers the title.
    let copy = Install {
        documents: Some(vec!["code-of-conduct".into()]),
        plan_campaign: true,
        on_conflict: OnConflict::Copy,
    };
    let third = lib.install_pack(&mut tx, &actor, p, &copy).await?;
    let d = &third.documents[0];
    assert_eq!(
        (d.outcome, d.key.as_str(), d.title.as_str()),
        ("copied", "code-of-conduct-2", "Code of conduct (2)")
    );
    assert!(third.campaign.is_none(), "no plan without the handbook in the install");

    // Unknown documents change nothing.
    let bad = Install { documents: Some(vec!["nope".into()]), ..Install::default() };
    assert!(matches!(lib.install_pack(&mut tx, &actor, p, &bad).await, Err(LibraryError::Invalid(_))));

    let audit: Vec<(String, i64)> = sqlx::query_as(
        "SELECT action, count(*) FROM audit_events WHERE action IN ('template.installed', 'artifact.created') GROUP BY action ORDER BY action",
    )
    .fetch_all(tx.conn())
    .await?;
    let created = p.documents.len() as i64 + 1;
    assert_eq!(audit, [("artifact.created".to_owned(), created), ("template.installed".to_owned(), 3)]);
    let diff: serde_json::Value = sqlx::query_scalar(
        "SELECT diff FROM audit_events WHERE action = 'template.installed' ORDER BY seq LIMIT 1",
    )
    .fetch_one(tx.conn())
    .await?;
    assert_eq!((diff["template"].as_str(), diff["version"].as_str()), (Some("policy-ack"), Some("1.0.0")));
    assert_eq!(diff["campaign_planned"], true);
    tx.rollback().await?;
    Ok(())
}
