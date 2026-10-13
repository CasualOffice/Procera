//! BPMS-103 workspace request settings against real PostgreSQL: a disabled type is neither listed
//! nor startable; a type's audience limits who may start it; money is priced in the workspace
//! currency and approval thresholds read in it. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{expense, leave, purchase, world};
use procera_requests::RequestError;
use serde_json::json;

#[tokio::test]
async fn a_disabled_type_is_not_offered_and_cannot_be_started() {
    let Some(mut w) = world().await else { return };
    let ann = w.member("Ann", None).await;
    assert_eq!(w.offered(ann).await.len(), 5, "every built-in by default");
    w.set("requests.types.enabled", json!(["hr.expense-claim", "it.access-request"])).await;
    assert_eq!(w.offered(ann).await, ["hr.expense-claim", "it.access-request"]);
    match w.start(ann, "hr.leave-request", leave()).await {
        Err(RequestError::Forbidden(m)) => assert!(m.contains("not offered"), "{m}"),
        other => panic!("{other:?}"),
    }
    w.set("requests.types.enabled", json!(["*"])).await;
    assert!(w.start(ann, "hr.leave-request", leave()).await.is_ok());
}

#[tokio::test]
async fn a_type_audience_limits_who_may_start_it() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let fin = w.member("Fiona", Some(boss)).await;
    let ann = w.member("Ann", Some(boss)).await;
    let ivan = w.member("Ivan", Some(boss)).await;
    let finance = w.group("Finance team", &[fin]).await;
    w.grant("buyer", ivan).await;
    w.set(
        "requests.types.audience",
        json!({"fin.purchase-requisition": {"groups": [finance], "roles": ["buyer"]}, "hr.leave-request": {"all": true}}),
    )
    .await;
    assert!(w.offered(fin).await.iter().any(|k| k == "fin.purchase-requisition"), "in the group");
    assert!(w.offered(ivan).await.iter().any(|k| k == "fin.purchase-requisition"), "holds the role");
    assert!(!w.offered(ann).await.iter().any(|k| k == "fin.purchase-requisition"));
    assert_eq!(w.offered(ann).await.len(), 4, "the other types stay open to everyone");
    match w.start(ann, "fin.purchase-requisition", purchase("100.00", "EUR")).await {
        Err(RequestError::Forbidden(m)) => assert!(m.contains("limited"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(w.start(fin, "fin.purchase-requisition", purchase("100.00", "EUR")).await.is_ok());
}

#[tokio::test]
async fn money_uses_the_workspace_currency_and_thresholds_read_in_it() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let fin = w.member("Fiona", None).await;
    w.grant("finance.approver", fin).await;

    // EUR by default: another currency is refused, a missing one is filled in.
    match w.start(ann, "hr.expense-claim", expense("100.00", "USD")).await {
        Err(RequestError::Invalid(e)) => {
            assert_eq!((e[0].pointer.as_str(), e[0].code.as_str()), ("/input/amount", "currency"))
        }
        other => panic!("{other:?}"),
    }
    w.set("requests.currency", json!("USD")).await;
    assert!(matches!(
        w.start(ann, "hr.expense-claim", expense("100.00", "EUR")).await,
        Err(RequestError::Invalid(_))
    ));

    // 900 USD is over the 500 threshold, read in USD: finance approves too.
    let big = w.start(ann, "hr.expense-claim", expense("900.00", "USD")).await.unwrap();
    let d = w.detail(big).await;
    assert_eq!(d.input["amount"], json!({"amount": "900.00", "currency": "USD"}));
    assert_eq!(d.fields[0].currency.as_deref(), Some("USD"));
    assert_eq!(w.decide(boss, "approved").await.unwrap(), "in_progress");
    assert_eq!(w.detail(big).await.summary.current_step.as_deref(), Some("Finance approval"));
    assert_eq!(w.decide(fin, "approved").await.unwrap(), "approved");

    // A request keeps its currency when the setting changes later; its step forms follow it.
    w.set("requests.currency", json!("GBP")).await;
    let small = w.start(ann, "hr.expense-claim", expense("400.00", "GBP")).await.unwrap();
    assert_eq!(w.decide(boss, "approved").await.unwrap(), "approved", "under the threshold");
    assert_eq!(w.detail(small).await.steps.len(), 1);
    assert_eq!(w.detail(big).await.fields[0].currency.as_deref(), Some("USD"));

    // Purchases: finance above 5,000 and the CFO above 50,000, in the workspace currency.
    let pr = w.start(ann, "fin.purchase-requisition", purchase("6000.00", "GBP")).await.unwrap();
    assert_eq!(w.decide(boss, "approved").await.unwrap(), "in_progress");
    assert_eq!(w.detail(pr).await.summary.current_step.as_deref(), Some("Finance approval"));
}
