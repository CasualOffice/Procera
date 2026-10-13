//! BPMS-084 against real PostgreSQL: publishing a process compiles it against the workspace and
//! writes an immutable release bundle pinning what it references; a process that references an
//! unpublished SOP is refused with the compiler's message; the engine then runs the published
//! program from its bundle. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_db::{WsCtx, WsTx, test_support};
use procera_engine::{Engine, ProgramRef};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_library::{Actor, Change, Library, LibraryError, ReviewPolicy};
use procera_processes::{DbPrograms, PublishProcesses};
use serde_json::{Value, json};
use uuid::Uuid;

async fn actor(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection) -> Actor {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await
        .unwrap();
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .execute(tx.conn())
        .await
        .unwrap();
    Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() }
}

/// A one-step task process that cites the `onboarding` SOP at `sop_version`.
fn process(sop_version: i32) -> Value {
    json!({
        "schemaVersion": 1,
        "process": { "key": "ops.welcome", "version": 1, "name": { "en": "Welcome pack" } },
        "variables": [],
        "nodes": [
            { "id": "start", "kind": "start", "name": { "en": "Start" }, "trigger": "manual" },
            { "id": "pack", "kind": "human.task", "name": { "en": "Prepare the welcome pack" },
              "assignment": { "candidates": [{ "type": "user", "ref": "expr:cel: initiator" }] } },
            { "id": "done", "kind": "end", "name": { "en": "Done" }, "result": "completed" }
        ],
        "edges": [
            { "id": "e1", "from": "start", "to": "pack" },
            { "id": "e2", "from": "pack", "to": "done", "on": "submitted" }
        ],
        "refs": { "sop": format!("sop:onboarding@{sop_version}") }
    })
}

#[tokio::test]
async fn publishing_a_process_compiles_it_pins_its_references_and_the_engine_runs_it() {
    let Some(t) = test_support::fresh().await else { return };
    let t = t.unwrap();
    let clock = Arc::new(SystemClock);
    let ids = Arc::new(UuidV7Gen);
    let lib = Library::new(t.db.clone(), clock.clone(), ids.clone())
        .with_publish_hook(Arc::new(PublishProcesses::new(clock.clone(), ids.clone())));
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await.unwrap();
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await.unwrap();
    let author = actor(&mut tx, &mut admin).await;
    let none =
        ReviewPolicy { required_for: "none".into(), min_reviewers: 0, reason_for_change_required: false };

    // The process cites an SOP that is still a draft: publishing it is refused, and nothing is kept.
    let sop = json!({"blocks": [{"type": "paragraph", "text": "Greet them at the door."}]});
    let (_, sop_v1) =
        lib.create(&mut tx, &author, "sop", "onboarding", "Onboarding", "standard", &sop).await.unwrap();
    let (_, p1) = lib
        .create(&mut tx, &author, "process", "ops.welcome", "Welcome pack", "standard", &process(1))
        .await
        .unwrap();
    lib.submit(&mut tx, &author, p1.id, &none).await.unwrap();
    let refused = lib.publish(&mut tx, &author, p1.id, None, None, Change::Major, &none).await;
    match refused {
        Err(LibraryError::Invalid(m)) => assert!(
            m.contains("IR-E023") && m.contains("sop:onboarding@1") && m.contains("is not published"),
            "{m}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    tx.rollback().await.unwrap();

    // Publish the SOP, then the process: a bundle pins sop:onboarding@1.
    let mut tx = t.db.begin_ws(ws).await.unwrap();
    let author = actor(&mut tx, &mut admin).await;
    let (_, sop_v1b) =
        lib.create(&mut tx, &author, "sop", "onboarding", "Onboarding", "standard", &sop).await.unwrap();
    let _ = sop_v1;
    lib.submit(&mut tx, &author, sop_v1b.id, &none).await.unwrap();
    lib.publish(&mut tx, &author, sop_v1b.id, None, None, Change::Major, &none).await.unwrap();
    let (proc_a, p1) = lib
        .create(&mut tx, &author, "process", "ops.welcome", "Welcome pack", "standard", &process(1))
        .await
        .unwrap();
    lib.submit(&mut tx, &author, p1.id, &none).await.unwrap();
    lib.publish(&mut tx, &author, p1.id, None, None, Change::Major, &none).await.unwrap();
    let (manifest, checksum, hash_len): (Value, String, i32) = sqlx::query_as(
        "SELECT manifest, program_checksum, length(manifest_hash) FROM release_bundles WHERE process_version_id = $1",
    )
    .bind(p1.id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    assert_eq!(manifest["sop:onboarding@1"]["version_id"], json!(sop_v1b.id));
    assert_eq!(hash_len, 32);
    let _ = proc_a;
    // Bundles never change.
    let mut sp = <sqlx::PgConnection as sqlx::Connection>::begin(tx.conn()).await.unwrap();
    assert!(sqlx::query("UPDATE release_bundles SET manifest = '{}'").execute(&mut *sp).await.is_err());
    sp.rollback().await.unwrap();
    tx.commit().await.unwrap();

    // The engine starts and runs the published program from its bundle, pinned by checksum.
    let engine = Engine::new(t.db.clone(), clock, ids, Arc::new(DbPrograms::new(t.db.clone())));
    let program =
        ProgramRef { process_key: "ops.welcome".into(), version: 1, checksum: Some(checksum.clone()) };
    let started = engine.start(ws, &program, json!({}), BTreeMap::new()).await.unwrap();
    let rec = engine.instance(ws, started.instance_id).await.unwrap();
    assert_eq!(rec.status.as_str(), "running");
    let wrong = ProgramRef { process_key: "ops.welcome".into(), version: 1, checksum: Some("nope".into()) };
    assert!(
        engine.start(ws, &wrong, json!({}), BTreeMap::new()).await.is_err(),
        "an unknown build is refused"
    );
}
