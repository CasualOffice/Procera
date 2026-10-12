//! Files on requests and the linked policy (BPMS-060, BPMS-101, BPMS-103) against real PostgreSQL:
//! only clean files of an allowed type that the requester uploaded are accepted; attached files
//! are linked to the request and listed with the step they were decided with; a decision records
//! the policy version shown. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Who, World, leave, world};
use procera_requests::{DecisionExtras, RequestError};
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// A file row as the upload pipeline leaves it.
async fn file(w: &World, by: Who, name: &str, state: &str, sniffed: &str) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = w.tx().await;
    sqlx::query(
        "INSERT INTO files (workspace_id, id, name, sniffed_type, size, received, state, sha256, wrapped_dek, storage_key, uploaded_by, created_at)
         VALUES ($1, $2, $3, $4, 3, 3, $5, '\\x0a0b'::bytea, '\\x00'::bytea, 'k', $6, now())",
    )
    .bind(w.ctx.workspace_id)
    .bind(id)
    .bind(name)
    .bind(sniffed)
    .bind(state)
    .bind(by.membership)
    .execute(tx.conn())
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

fn with_files(mut input: Value, field: &str, ids: &[Uuid]) -> Value {
    input[field] = json!(ids.iter().map(Uuid::to_string).collect::<Vec<_>>());
    input
}

fn code(r: Result<Uuid, RequestError>) -> (String, String) {
    match r {
        Err(RequestError::Invalid(e)) => (e[0].pointer.clone(), e[0].code.clone()),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn only_clean_allowed_files_the_requester_uploaded_attach() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Olivia Owner", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let ben = w.member("Ben", Some(boss)).await;
    let scanning = file(&w, ann, "scan.pdf", "scanning", "application/pdf").await;
    let bad = file(&w, ann, "evil.pdf", "quarantined", "application/pdf").await;
    let theirs = file(&w, ben, "bens.pdf", "clean", "application/pdf").await;
    let zip = file(&w, ann, "a.zip", "clean", "application/zip").await;
    let foreign = Uuid::now_v7();
    for (id, want) in [
        (scanning, "scanning"),
        (bad, "quarantined"),
        (theirs, "not_yours"),
        (zip, "file_type"),
        (foreign, "unknown_file"),
    ] {
        let got = code(w.start(ann, "hr.leave-request", with_files(leave(), "attachments", &[id])).await);
        assert_eq!(got, ("/input/attachments".to_owned(), want.to_owned()));
    }

    // A clean image and PDF attach, are linked to the request and listed with their field.
    let img = file(&w, ann, "note.png", "clean", "image/png").await;
    let pdf = file(&w, ann, "cert.pdf", "clean", "application/pdf").await;
    let input = with_files(with_files(leave(), "attachments", &[img, pdf]), "supporting_document", &[pdf]);
    let id = w.start(ann, "hr.leave-request", input).await.unwrap();
    let d = w.detail(id).await;
    let listed: Vec<(String, String)> =
        d.attachments.iter().map(|a| (a.name.clone(), a.source.clone())).collect();
    assert_eq!(
        listed,
        [
            ("cert.pdf".to_owned(), "Supporting document".to_owned()),
            ("note.png".to_owned(), "Attachments".to_owned()),
            ("cert.pdf".to_owned(), "Attachments".to_owned())
        ]
    );
    assert_eq!(d.input["attachments"][0]["content_type"], "image/png");
    let mut tx = w.tx().await;
    let links: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_links WHERE object_type = 'request' AND object_id = $1",
    )
    .bind(id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    assert_eq!(links, 2);
    let diff: Value = sqlx::query_scalar(
        "SELECT diff FROM audit_events WHERE action = 'request.started' AND object_id = $1",
    )
    .bind(id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    assert_eq!(diff["files"].as_array().map(Vec::len), Some(2));
    tx.rollback().await.unwrap();
}

async fn policy(w: &World, by: Who, title: &str) -> (Uuid, Uuid) {
    let (a, v) = (Uuid::now_v7(), Uuid::now_v7());
    let mut tx = w.tx().await;
    sqlx::query("INSERT INTO artifacts (workspace_id, id, kind, key, title, created_by, state) VALUES ($1, $2, 'policy', $3, $4, $5, 'published')")
        .bind(w.ctx.workspace_id).bind(a).bind(format!("p-{}", a.simple())).bind(title).bind(by.membership)
        .execute(tx.conn()).await.unwrap();
    sqlx::query("INSERT INTO artifact_versions (workspace_id, id, artifact_id, version_no, lifecycle, content, created_by, published_at, semver) VALUES ($1, $2, $3, 1, 'published', '{}', $4, now(), '1.2')")
        .bind(w.ctx.workspace_id).bind(v).bind(a).bind(by.membership)
        .execute(tx.conn()).await.unwrap();
    sqlx::query("UPDATE artifacts SET current_published_version_id = $2 WHERE id = $1")
        .bind(a)
        .bind(v)
        .execute(tx.conn())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (a, v)
}

#[tokio::test]
async fn a_decision_records_its_files_and_the_policy_version_shown() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Olivia Owner", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let (artifact, version) = policy(&w, boss, "Leave policy").await;
    w.set("requests.types.policy", json!({ "hr.leave-request": artifact })).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    let item = w.open(boss).await.remove(0);
    let shown = item.policy.clone().unwrap();
    assert_eq!(
        (shown.title.as_str(), shown.version.as_str(), shown.version_id),
        ("Leave policy", "1.2", version)
    );
    assert_eq!(w.detail(id).await.policy.map(|p| p.artifact_id), Some(artifact));

    let decide = |files: Vec<Uuid>, policy_version: Option<Uuid>| {
        let w = &w;
        async move {
            let mut tx = w.tx().await;
            let r = w
                .req
                .decide_with(
                    &mut tx,
                    &boss.actor(),
                    item.id,
                    item.revision,
                    "approved",
                    None,
                    &Map::new(),
                    &DecisionExtras { files, policy_version },
                )
                .await;
            if r.is_ok() {
                tx.commit().await.unwrap();
            }
            r
        }
    };
    // Someone else's file, or a version of another document, is refused.
    let anns = file(&w, ann, "x.pdf", "clean", "application/pdf").await;
    assert!(
        matches!(decide(vec![anns], None).await, Err(RequestError::Invalid(e)) if e[0].code == "not_yours")
    );
    assert!(
        matches!(decide(vec![], Some(Uuid::now_v7())).await, Err(RequestError::Invalid(e)) if e[0].code == "unknown_policy_version")
    );

    let quote = file(&w, boss, "signed.pdf", "clean", "application/pdf").await;
    decide(vec![quote], Some(version)).await.unwrap();
    let d = w.detail(id).await;
    assert_eq!(d.attachments[0].source, "Manager approval decision");
    let decided = d.timeline.iter().find(|t| t.kind == "decided").unwrap();
    assert_eq!(decided.files[0].name, "signed.pdf");
    let mut tx = w.tx().await;
    let (decision, diff): (Value, Value) = sqlx::query_as(
        "SELECT w.decision, a.diff FROM work_items w JOIN audit_events a ON a.object_id = w.id AND a.action = 'work_item.decided' WHERE w.id = $1",
    )
    .bind(item.id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    assert_eq!(decision["policy"]["version_id"], json!(version));
    assert_eq!(decision["policy"]["version"], "1.2");
    assert_eq!(diff["policy_version"], json!(version));
    assert_eq!(diff["files"], json!([quote]));
    tx.rollback().await.unwrap();
}
