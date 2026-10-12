//! PR-08 against real PostgreSQL: every decision node an advance passes through is recorded in
//! `instance_decisions` with the branch taken, the conditions checked and the values they read.
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{World, program, world};
use procera_engine::{Engine, StaticPrograms};
use procera_kernel::UuidV7Gen;
use serde_json::{Value, json};
use uuid::Uuid;

/// start → by_amount (finance when amount > 500) → by_count (many when n > 5) → bump (n := n + 1)
/// → end; small amounts end at once.
fn routed() -> Value {
    json!({
        "schemaVersion": 1,
        "process": { "key": "p.route", "version": 1, "name": { "en": "Route" } },
        "variables": [
            { "name": "amount", "type": { "money": { "currency": "EUR" } }, "required": true },
            { "name": "n", "type": "integer", "required": true }
        ],
        "nodes": [
            { "id": "start", "kind": "start" },
            { "id": "by_amount", "kind": "decision",
              "branches": [ { "name": "finance", "when": "expr:cel: amount.amount > 500.0" } ] },
            { "id": "by_count", "kind": "decision",
              "branches": [ { "name": "many", "when": "expr:cel: n > 5" } ] },
            { "id": "bump", "kind": "set", "assign": { "n": "expr:cel: n + 1" } },
            { "id": "end", "kind": "end" },
            { "id": "small", "kind": "end" }
        ],
        "edges": [
            { "id": "e1", "from": "start", "to": "by_amount" },
            { "id": "e2", "from": "by_amount", "to": "by_count", "on": "finance" },
            { "id": "e3", "from": "by_amount", "to": "small", "on": "default" },
            { "id": "e4", "from": "by_count", "to": "bump", "on": "many" },
            { "id": "e5", "from": "by_count", "to": "end", "on": "default" },
            { "id": "e6", "from": "bump", "to": "end" }
        ]
    })
}

type DecisionRow = (i64, String, String, Value, Option<String>);

async fn decision_rows(w: &World, id: Uuid) -> Vec<DecisionRow> {
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    let rows = sqlx::query_as(
        "SELECT seq, node_id, branch, facts, program_checksum FROM instance_decisions WHERE instance_id = $1 ORDER BY seq, ord",
    )
    .bind(id)
    .fetch_all(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    rows
}

#[tokio::test]
async fn decisions_record_the_branch_the_conditions_and_the_values_read() {
    let Some(w) = world(vec![]).await else { return };
    let mut programs = StaticPrograms::default();
    let r = programs.insert_pinned(program(routed()));
    let engine = Engine::new(w.db().clone(), w.clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs));
    let vars = |amount: &str, n: i64| {
        BTreeMap::from([
            ("amount".to_owned(), json!({ "amount": amount, "currency": "EUR" })),
            ("n".to_owned(), json!(n)),
        ])
    };
    let big = engine.start(w.ctx, &r, json!({}), vars("640.00", 9)).await.unwrap().instance_id;
    let rows = decision_rows(&w, big).await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (seq, node, branch, facts, checksum) = &rows[0];
    assert_eq!((*seq, node.as_str(), branch.as_str()), (1, "by_amount", "finance"));
    assert_eq!(checksum, &r.checksum);
    assert_eq!(
        facts["checks"],
        json!([{ "branch": "finance", "expression": "amount.amount > 500.0", "result": true }])
    );
    assert_eq!(facts["inputs"]["amount"]["value"], json!({ "amount": "640.00", "currency": "EUR" }));
    assert_eq!(facts["inputs"]["amount"]["classification"], json!("internal"));
    // `n` is set again after the decision in the same advance: no value is claimed for it.
    let (_, node, branch, facts, _) = &rows[1];
    assert_eq!((node.as_str(), branch.as_str()), ("by_count", "many"));
    assert_eq!(facts["inputs"]["n"]["changed_after"], json!(true));
    assert!(facts["inputs"]["n"].get("value").is_none());

    // Below the threshold: the default branch, with the condition that failed.
    let small = engine.start(w.ctx, &r, json!({}), vars("120.00", 1)).await.unwrap().instance_id;
    let rows = decision_rows(&w, small).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "default");
    assert_eq!(rows[0].3["checks"][0]["result"], json!(false));
    assert_eq!(rows[0].3["inputs"]["amount"]["value"]["amount"], json!("120.00"));

    // The trace is append-only for the app role.
    let mut tx = w.db().begin_ws(w.ctx).await.unwrap();
    assert!(sqlx::query("DELETE FROM instance_decisions").execute(tx.conn()).await.is_err());
    tx.rollback().await.unwrap();
}
