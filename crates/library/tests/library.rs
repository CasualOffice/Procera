//! BPMS-040: lifecycle, separation of duties, immutability (T07), supersede and scheduling.

use std::sync::Arc;

use procera_db::{WsCtx, WsTx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_library::{Actor, Change, Library, LibraryError, ReviewPolicy, content_hash};
use serde_json::json;
use uuid::Uuid;

async fn actor(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection) -> anyhow::Result<Actor> {
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
    Ok(Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() })
}

fn policy(required_for: &str, min: i64) -> ReviewPolicy {
    ReviewPolicy { required_for: required_for.into(), min_reviewers: min, reason_for_change_required: true }
}

#[tokio::test]
async fn lifecycle_sod_immutability_supersede_and_schedule() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let lib = Library::new(t.db.clone(), Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let (author, r1, r2) = (
        actor(&mut tx, &mut admin).await?,
        actor(&mut tx, &mut admin).await?,
        actor(&mut tx, &mut admin).await?,
    );
    let two = policy("controlled", 2);

    let body = json!({"blocks": [{"type": "paragraph", "text": "Wear a badge."}]});
    let (a, v1) = lib
        .create(&mut tx, &author, "policy", "security.badges", "Badge policy", "controlled", &body)
        .await?;
    assert_eq!((a.state.as_str(), v1.version_no, v1.lifecycle.as_str()), ("draft", 1, "draft"));
    assert!(matches!(
        lib.create(&mut tx, &author, "policy", "security.badges", "Dup", "standard", &body).await,
        Err(LibraryError::KeyTaken)
    ));
    assert!(matches!(
        lib.create(&mut tx, &author, "process", "p", "P", "standard", &body).await,
        Err(LibraryError::Invalid(_))
    ));
    let edited = lib
        .edit(
            &mut tx,
            &author,
            v1.id,
            &json!({"blocks": [{"type": "paragraph", "text": "Always wear a badge."}]}),
            Some(v1.revision),
        )
        .await?;
    assert!(matches!(
        lib.edit(&mut tx, &author, v1.id, &body, Some(v1.revision)).await,
        Err(LibraryError::StaleRevision { .. })
    ));

    // Review: two distinct non-author approvals for a controlled policy.
    let sub = lib.submit(&mut tx, &author, v1.id, &two).await?;
    assert_eq!((sub.lifecycle.as_str(), sub.approvals_required), ("in_review", Some(2)));
    assert!(
        matches!(lib.edit(&mut tx, &author, v1.id, &body, None).await, Err(LibraryError::Conflict(_))),
        "no edits in review"
    );
    assert!(matches!(
        lib.review(&mut tx, &author, v1.id, "approve", None, &two).await,
        Err(LibraryError::SeparationOfDuties(_))
    ));
    let one = lib.review(&mut tx, &r1, v1.id, "approve", None, &two).await?;
    assert_eq!(one.lifecycle, "in_review");
    assert!(
        matches!(
            lib.review(&mut tx, &r1, v1.id, "approve", None, &two).await,
            Err(LibraryError::Conflict(_))
        ),
        "one decision per reviewer"
    );
    let approved = lib.review(&mut tx, &r2, v1.id, "approve", Some("LGTM"), &two).await?;
    assert_eq!(approved.lifecycle, "approved");
    assert_eq!(approved.content_hash, Some(content_hash(&edited.content)?));

    // Approved content is immutable, even with direct SQL (T07).
    let mut sp = <sqlx::PgConnection as sqlx::Connection>::begin(tx.conn()).await?;
    let err = sqlx::query("UPDATE artifact_versions SET content = '{}' WHERE id = $1")
        .bind(v1.id)
        .execute(&mut *sp)
        .await;
    assert!(err.is_err(), "the trigger rejects changing approved content");
    sp.rollback().await?;

    let published = lib.publish(&mut tx, &author, v1.id, None, None, Change::Major, &two).await?;
    assert_eq!(published.lifecycle, "published");
    assert_eq!((published.semver.as_deref(), published.reack_required), (Some("1.0"), false));
    assert_eq!(lib.artifact(&mut tx, a.id).await?.current_published_version_id, Some(v1.id));

    // Version 2: reject sends it back to draft with a required comment; a reason for change is needed to publish.
    let v2 = lib.new_version(&mut tx, &author, a.id).await?;
    assert!(
        matches!(lib.new_version(&mut tx, &author, a.id).await, Err(LibraryError::Conflict(_))),
        "one open draft"
    );
    lib.submit(&mut tx, &author, v2.id, &two).await?;
    assert!(matches!(
        lib.review(&mut tx, &r1, v2.id, "reject", None, &two).await,
        Err(LibraryError::Invalid(_))
    ));
    assert_eq!(
        lib.review(&mut tx, &r1, v2.id, "changes_requested", Some("Add visitors"), &two).await?.lifecycle,
        "draft"
    );
    lib.edit(
        &mut tx,
        &author,
        v2.id,
        &json!({"blocks": [{"type": "paragraph", "text": "Badges for staff and visitors."}]}),
        None,
    )
    .await?;
    lib.submit(&mut tx, &author, v2.id, &two).await?;
    let round2 = lib.review(&mut tx, &r1, v2.id, "approve", None, &two).await?;
    assert_eq!(round2.reviews.len(), 1, "earlier rounds don't count");
    lib.review(&mut tx, &r2, v2.id, "approve", None, &two).await?;
    assert!(matches!(
        lib.publish(&mut tx, &author, v2.id, None, None, Change::Major, &two).await,
        Err(LibraryError::Invalid(_))
    ));

    // Scheduled publication; when due, it supersedes v1.
    let later = time::OffsetDateTime::now_utc() + time::Duration::days(3);
    assert_eq!(
        lib.publish(&mut tx, &author, v2.id, Some(later), Some("Visitors"), Change::Major, &two)
            .await?
            .lifecycle,
        "scheduled"
    );
    assert_eq!(
        lib.version(&mut tx, v1.id, None).await?.0.lifecycle,
        "published",
        "v1 stays live until v2 is effective"
    );
    sqlx::query("UPDATE artifact_versions SET effective_at = now() - interval '1 minute' WHERE id = $1")
        .bind(v2.id)
        .execute(tx.conn())
        .await?;
    tx.commit().await?;
    assert_eq!(lib.publish_due().await?, 1);
    let mut tx = t.db.begin_ws(ws).await?;
    assert_eq!(lib.version(&mut tx, v1.id, None).await?.0.lifecycle, "deprecated");
    assert_eq!(lib.artifact(&mut tx, a.id).await?.current_published_version_id, Some(v2.id));
    // BPMS-045: v2 was scheduled as a major under the default `major_only` policy.
    let live = lib.version(&mut tx, v2.id, None).await?.0;
    assert_eq!((live.semver.as_deref(), live.reack_required), (Some("2.0"), true));
    let plan = lib.publish_plan(&mut tx, v2.id).await?;
    assert_eq!((plan.next_semver_major.as_str(), plan.next_semver_minor.as_str()), ("2.0", "1.1"));
    let subject = lib.reack_subject(&mut tx, v2.id).await?.expect("subject");
    assert_eq!((subject.lifecycle.as_str(), subject.initiator), ("published", Some(author.membership)));
    assert!(matches!(lib.retire(&mut tx, &author, v2.id, " ").await, Err(LibraryError::Invalid(_))));
    assert_eq!(lib.retire(&mut tx, &author, v1.id, "superseded long ago").await?.lifecycle, "retired");

    // A standard SOP under the Starter policy (no review) is approved on submission.
    let (_, s1) = lib.create(&mut tx, &author, "sop", "onboarding", "Onboarding", "standard", &body).await?;
    assert_eq!(lib.submit(&mut tx, &author, s1.id, &policy("none", 0)).await?.lifecycle, "approved");
    tx.commit().await?;

    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!(report.first_broken, None);
    assert!(report.checked >= 15, "every transition is audited ({})", report.checked);
    Ok(())
}

mod diff_props {
    use procera_library::diff::{BlockOp, apply, diff};
    use proptest::prelude::*;
    use serde_json::{Value, json};

    fn block() -> impl Strategy<Value = Value> {
        prop_oneof![
            "[a-c ]{0,12}".prop_map(|t| json!({"type": "paragraph", "text": t})),
            "[a-c]{1,6}".prop_map(|t| json!({"type": "heading", "level": 2, "text": t})),
            proptest::collection::vec("[a-c]{1,4}", 0..4)
                .prop_map(|items| json!({"type": "list", "items": items})),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]
        /// Applying a diff always rebuilds the target exactly, and identical inputs diff to all-same.
        #[test]
        fn diff_rebuilds_the_target(a in proptest::collection::vec(block(), 0..8), b in proptest::collection::vec(block(), 0..8)) {
            let (ca, cb) = (json!({"blocks": a}), json!({"blocks": b.clone()}));
            let d = diff(&ca, &cb);
            prop_assert_eq!(apply(&d), b);
            let same = diff(&ca, &ca).ops.iter().all(|o| matches!(o, BlockOp::Same { .. }));
            prop_assert!(same, "identical inputs must diff to all-same");
        }
    }
}
