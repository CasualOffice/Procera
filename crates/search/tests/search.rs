//! BPMS-049: full-text + trigram search, index freshness, type filtering and workspace isolation.

use std::sync::Arc;

use procera_db::{WsCtx, WsTx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_library::{Actor, Library, ReviewPolicy};
use procera_search::{TYPES, search};
use serde_json::json;
use uuid::Uuid;

async fn member(tx: &mut WsTx<'_>, name: &str, email: &str, title: Option<&str>) -> anyhow::Result<Actor> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
        .bind(user)
        .bind(email)
        .bind(name)
        .bind(Uuid::new_v4())
        .execute(tx.conn())
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status, title) VALUES ($1, $2, $3, 'active', $4)")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .bind(title)
        .execute(tx.conn())
        .await?;
    Ok(Actor { membership: m, subject_ref: Uuid::new_v4(), ..Actor::default() })
}

fn doc(text: &str) -> serde_json::Value {
    json!({"blocks": [{"type": "heading", "text": "Scope"}, {"type": "paragraph", "text": text}]})
}

fn kinds(hits: &[procera_search::Hit]) -> Vec<(String, String)> {
    hits.iter().map(|h| (h.kind.clone(), h.title.clone())).collect()
}

#[tokio::test]
async fn search_ranks_filters_highlights_and_stays_current() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let lib = Library::new(t.db.clone(), Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let none =
        ReviewPolicy { required_for: "none".into(), min_reviewers: 0, reason_for_change_required: false };
    let (ws_a, ws_b) = (WsCtx { workspace_id: Uuid::now_v7() }, WsCtx { workspace_id: Uuid::now_v7() });

    let mut tx = t.db.begin_ws(ws_a).await?;
    let author = member(&mut tx, "Priya Raman", "priya@acme.test", Some("Head of IT")).await?;
    let gone = member(&mut tx, "Bob Gone", "bob@acme.test", None).await?;
    sqlx::query("UPDATE memberships SET status = 'deactivated' WHERE id = $1")
        .bind(gone.membership)
        .execute(tx.conn())
        .await?;
    let (sop, v1) = lib
        .create(
            &mut tx,
            &author,
            "sop",
            "it.onboarding",
            "IT onboarding",
            "standard",
            &doc("Order laptops and create accounts <b>before</b> day one."),
        )
        .await?;
    lib.create(
        &mut tx,
        &author,
        "policy",
        "sec.badges",
        "Badge policy",
        "standard",
        &doc("Wear your badge at all times."),
    )
    .await?;
    sqlx::query(
        "INSERT INTO departments (workspace_id, id, name, path) VALUES ($1, $2, 'Finance', ARRAY[$2])",
    )
    .bind(ws_a.workspace_id)
    .bind(Uuid::now_v7())
    .execute(tx.conn())
    .await?;
    sqlx::query("INSERT INTO groups (workspace_id, id, name, description) VALUES ($1, $2, 'Laptop champions', 'Help with hardware')")
        .bind(ws_a.workspace_id)
        .bind(Uuid::now_v7())
        .execute(tx.conn())
        .await?;
    tx.commit().await?;

    let mut tx = t.db.begin_ws(ws_b).await?;
    let other = member(&mut tx, "Priya Other", "priya@other.test", None).await?;
    lib.create(
        &mut tx,
        &other,
        "sop",
        "it.laptops",
        "Laptop refresh",
        "standard",
        &doc("Laptops every three years."),
    )
    .await?;
    tx.commit().await?;

    let mut tx = t.db.begin_ws(ws_a).await?;
    // Stemmed body match ("laptop" ↔ "laptops") with an escaped, highlighted snippet; the group matches too.
    let hits = search(&mut tx, "laptop", TYPES, 20).await?;
    assert_eq!(
        kinds(&hits),
        vec![("group".into(), "Laptop champions".into()), ("sop".into(), "IT onboarding".into())]
    );
    let snippet = hits[1].snippet.clone().unwrap_or_default();
    assert!(snippet.contains("<mark>laptops</mark>"), "{snippet}");
    assert!(!snippet.contains("<b>"), "markup in content is never passed through: {snippet}");
    // Exact short word kept by the simple configuration; key words are searchable.
    assert_eq!(
        kinds(&search(&mut tx, "IT", &["sop"], 20).await?),
        vec![("sop".into(), "IT onboarding".into())]
    );
    assert_eq!(search(&mut tx, "onboarding", &["sop"], 20).await?[0].id, sop.id);
    // Typos and prefixes go through trigrams.
    assert_eq!(
        kinds(&search(&mut tx, "bdge polcy", TYPES, 20).await?),
        vec![("policy".into(), "Badge policy".into())]
    );
    assert_eq!(
        kinds(&search(&mut tx, "badg", TYPES, 20).await?),
        vec![("policy".into(), "Badge policy".into())]
    );
    assert_eq!(
        kinds(&search(&mut tx, "onbording", &["sop"], 20).await?),
        vec![("sop".into(), "IT onboarding".into())]
    );
    assert_eq!(
        kinds(&search(&mut tx, "Fin", TYPES, 20).await?),
        vec![("department".into(), "Finance".into())]
    );
    // Members by name, email or title; deactivated members are not found; other workspaces never.
    assert_eq!(
        kinds(&search(&mut tx, "priya", TYPES, 20).await?),
        vec![("member".into(), "Priya Raman".into())]
    );
    assert_eq!(search(&mut tx, "head of it", &["member"], 20).await?.len(), 1);
    assert_eq!(search(&mut tx, "priya@acme", &["member"], 20).await?.len(), 1);
    assert!(search(&mut tx, "Bob Gone", TYPES, 20).await?.is_empty());
    assert!(search(&mut tx, "refresh", TYPES, 20).await?.is_empty(), "workspace B's SOP is invisible");
    // Type filtering is exact: no allowed types, no results.
    assert!(search(&mut tx, "laptop", &["policy", "member"], 20).await?.is_empty());
    assert!(search(&mut tx, "laptop", &[], 20).await?.is_empty());
    // LIKE metacharacters are literal.
    assert!(search(&mut tx, "%", TYPES, 20).await?.is_empty());
    assert!(search(&mut tx, "  ", TYPES, 20).await.is_err());

    // Index freshness: draft edits are searchable at once; after publication the published text is
    // indexed even while a newer draft exists.
    lib.edit(&mut tx, &author, v1.id, &doc("Issue a phone and a yubikey."), None).await?;
    assert!(search(&mut tx, "laptops", &["sop"], 20).await?.is_empty());
    assert_eq!(search(&mut tx, "yubikey", &["sop"], 20).await?.len(), 1);
    lib.submit(&mut tx, &author, v1.id, &none).await?;
    lib.publish(&mut tx, &author, v1.id, None, None, &none).await?;
    let v2 = lib.new_version(&mut tx, &author, sop.id).await?;
    lib.edit(&mut tx, &author, v2.id, &doc("Ship a tablet instead."), None).await?;
    assert_eq!(search(&mut tx, "yubikey", &["sop"], 20).await?.len(), 1);
    assert!(search(&mut tx, "tablet", &["sop"], 20).await?.is_empty());
    sqlx::query("UPDATE artifacts SET title = 'Laptop onboarding' WHERE id = $1")
        .bind(sop.id)
        .execute(tx.conn())
        .await?;
    assert_eq!(search(&mut tx, "laptop", &["sop"], 20).await?.len(), 1);
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn limit_is_bounded_and_ranking_prefers_title_prefixes() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let lib = Library::new(t.db.clone(), Arc::new(SystemClock), Arc::new(UuidV7Gen));
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let a = member(&mut tx, "Ann", "ann@acme.test", None).await?;
    for i in 0..60 {
        lib.create(
            &mut tx,
            &a,
            "sop",
            &format!("ops.{i:02}"),
            &format!("Ops step {i:02}"),
            "standard",
            &doc("Expense receipts."),
        )
        .await?;
    }
    lib.create(&mut tx, &a, "sop", "fin.expenses", "Expenses", "standard", &doc("How to claim.")).await?;
    let hits = search(&mut tx, "expenses", &["sop"], 500).await?;
    assert_eq!(hits.len(), 50, "limit is clamped");
    assert_eq!(hits[0].title, "Expenses", "title match ranks above body matches");
    assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
    tx.rollback().await?;
    Ok(())
}
