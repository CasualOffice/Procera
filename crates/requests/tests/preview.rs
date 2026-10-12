//! "Who approves this" (BPMS-103) against real PostgreSQL: the first step resolved with the real
//! resolver (manager, delegate, fallback, unassigned), later steps in words with conditions checked
//! against the answers so far, and no writes. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Who, World, world};
use procera_requests::{Preview, RequestError, SetOutOfOffice};
use serde_json::{Value, json};
use time::macros::date;

async fn preview(w: &World, who: Who, key: &str, input: Value) -> Result<Preview, RequestError> {
    let mut tx = w.tx().await;
    let p = w.req.preview(&mut tx, who.membership, key, input.as_object().unwrap()).await;
    tx.rollback().await.unwrap();
    p
}

#[tokio::test]
async fn names_the_manager_and_reads_the_finance_threshold() {
    let Some(mut w) = world().await else { return };
    let olivia = w.member("Olivia Owner", None).await;
    let ann = w.member("Ann", Some(olivia)).await;
    let fin = w.member("Fiona", None).await;
    w.group("Finance", &[fin]).await;

    let p = preview(&w, ann, "hr.expense-claim", json!({})).await.unwrap();
    assert_eq!(p.summary, "Olivia Owner (your manager), then Finance if the amount is over 500 EUR");
    assert!(!p.unassigned);
    assert_eq!(p.steps[0].people[0].reason.as_deref(), Some("your manager"));
    assert_eq!(p.steps[1].applies, None);

    // Crossing the threshold: finance applies; under it, finance drops out of the sentence.
    let big = json!({"amount": {"amount": "900", "currency": "EUR"}});
    let p = preview(&w, ann, "hr.expense-claim", big).await.unwrap();
    assert_eq!(p.summary, "Olivia Owner (your manager), then Finance (the amount is over 500 EUR)");
    let small = json!({"amount": {"amount": "120"}, "description": 5});
    let p = preview(&w, ann, "hr.expense-claim", small).await.unwrap();
    assert_eq!(p.summary, "Olivia Owner (your manager)");
    assert_eq!(p.steps[1].applies, Some(false));

    // Purchases: two thresholds in the workspace currency.
    w.set("requests.currency", json!("USD")).await;
    let p =
        preview(&w, ann, "fin.purchase-requisition", json!({"amount": {"amount": "60000"}})).await.unwrap();
    assert_eq!(
        p.summary,
        "Olivia Owner (your manager), then Finance (the total amount is over 5,000 USD), then CFO (the total amount is over 50,000 USD)"
    );

    // Nothing was written.
    let mut tx = w.tx().await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM work_items").fetch_one(tx.conn()).await.unwrap();
    assert_eq!(n, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn a_delegate_stands_in_and_nobody_reads_as_unassigned() {
    let Some(mut w) = world().await else { return };
    let olivia = w.member("Olivia Owner", None).await;
    let dan = w.member("Dan Deputy", None).await;
    let ann = w.member("Ann", Some(olivia)).await;
    let mut tx = w.tx().await;
    let ooo = SetOutOfOffice {
        starts_on: date!(2026 - 10 - 12),
        ends_on: date!(2026 - 10 - 16),
        delegate: dan.membership,
        note: None,
    };
    w.req.set_out_of_office(&mut tx, &olivia.actor(), olivia.membership, &ooo).await.unwrap();
    tx.commit().await.unwrap();
    let p = preview(&w, ann, "hr.leave-request", json!({})).await.unwrap();
    assert_eq!(p.summary, "Dan Deputy (standing in for Olivia Owner, who is away)");

    // No manager and no process owner: an admin assigns it.
    let orphan = w.member("Otto", None).await;
    let p = preview(&w, orphan, "hr.leave-request", json!({})).await.unwrap();
    assert!(p.unassigned);
    assert_eq!(p.summary, procera_requests::preview::NOBODY);

    // A process owner is the fallback, and says so.
    let pat = w.member("Pat", None).await;
    w.grant("process.owner", pat).await;
    let p = preview(&w, orphan, "hr.leave-request", json!({})).await.unwrap();
    assert!(p.summary.starts_with("Pat ("), "{}", p.summary);
    assert!(p.summary.contains("as nobody else could be found"), "{}", p.summary);

    // A type the workspace does not offer cannot be previewed either.
    w.set("requests.types.enabled", json!(["hr.expense-claim"])).await;
    assert!(matches!(preview(&w, ann, "hr.leave-request", json!({})).await, Err(RequestError::Forbidden(_))));
}
