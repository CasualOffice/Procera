//! Employee onboarding (BPMS-103) against real PostgreSQL: three tasks run at the same time (IT,
//! Facilities, the new member's manager), the request lists all three as where it is, the join
//! waits for every one, then the new member acknowledges and the request finishes "Onboarded".
//! An acknowledgement is personal: never handed to a delegate, `expired` is never a human choice.
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Who, World, world};
use procera_requests::{RequestError, SetOutOfOffice, WorkView};
use serde_json::{Map, Value, json};
use time::macros::date;
use uuid::Uuid;

fn input(new_hire: Who) -> Value {
    json!({
        "new_hire": new_hire.membership.to_string(),
        "start_date": "2026-11-02",
        "role_title": "Product designer",
        "department": "Design",
        "equipment": "standard_laptop",
        "notes": "Prefers a standing desk",
    })
}

/// The open item of `who` on request `id`.
async fn item(w: &World, who: Who, id: Uuid) -> Option<WorkView> {
    w.open(who).await.into_iter().find(|i| i.request.id == id)
}

async fn done(w: &World, who: Who, id: Uuid, fields: Value) -> String {
    let it = item(w, who, id).await.expect("an open task");
    assert_eq!(it.kind, "task");
    assert_eq!(it.outcomes, ["done"]);
    let mut tx = w.tx().await;
    let d = w
        .req
        .decide(&mut tx, &who.actor(), it.id, it.revision, "done", None, fields.as_object().unwrap())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    d.request.status
}

struct Team {
    hr: Who,
    manager: Who,
    nia: Who,
    ian: Who,
    fay: Who,
}

async fn team(w: &mut World) -> Team {
    let hr = w.member("Hana HR", None).await;
    let manager = w.member("Mo Manager", None).await;
    let nia = w.member("Nia New", Some(manager)).await;
    let ian = w.member("Ian IT", None).await;
    let fay = w.member("Fay Facilities", None).await;
    w.group("IT", &[ian]).await;
    w.group("Facilities", &[fay]).await;
    Team { hr, manager, nia, ian, fay }
}

#[tokio::test]
async fn three_tasks_run_at_once_the_join_waits_then_the_new_member_acknowledges() {
    let Some(mut w) = world().await else { return };
    let t = team(&mut w).await;
    assert!(w.offered(t.hr).await.iter().any(|k| k == "hr.onboarding"), "offered by default");

    let id = w.start(t.hr, "hr.onboarding", input(t.nia)).await.unwrap();
    let d = w.detail(id).await;
    assert!(d.summary.display_id.starts_with("ON-2026-"), "{}", d.summary.display_id);
    assert_eq!(d.summary.title, "Onboarding: Product designer, starting 2026-11-02");
    // All three at the same time, each with its own people.
    let open: Vec<(&str, &str, Vec<String>)> =
        d.steps.iter().map(|s| (s.node.as_str(), s.state.as_str(), s.assignees.clone())).collect();
    assert_eq!(
        open,
        [
            ("it_setup", "offered", vec!["Ian IT".to_owned()]),
            ("workspace", "offered", vec!["Fay Facilities".to_owned()]),
            ("first_week", "offered", vec!["Mo Manager".to_owned()]),
        ]
    );
    assert_eq!(
        d.summary.open_steps,
        ["Set up accounts and equipment", "Prepare workspace and access badge", "Plan the first week"]
    );
    assert_eq!(d.summary.assignees, ["Fay Facilities", "Ian IT", "Mo Manager"]);
    assert!(d.steps.iter().all(|s| s.due_at.is_some()), "due in business days");
    assert!(d.steps.iter().all(|s| s.parallel.as_deref() == Some("prepare")), "one parallel block");
    // The list summary carries the same.
    let mut tx = w.tx().await;
    let list = w.req.list(&mut tx, procera_requests::Scope::All, Some("in_progress")).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(list[0].open_steps.len(), 3);
    // Why: the manager is the new member's manager; IT and Facilities are their groups.
    assert_eq!(item(&w, t.manager, id).await.unwrap().why_you.as_deref(), Some("You're Nia New's manager"));
    assert_eq!(item(&w, t.ian, id).await.unwrap().why_you.as_deref(), Some("You're in the IT group"));
    assert!(item(&w, t.nia, id).await.is_none(), "nothing for the new member yet");

    // Any order; the join waits for all three.
    assert_eq!(done(&w, t.fay, id, json!({ "desk": "3F-12, badge 0042" })).await, "in_progress");
    assert_eq!(
        done(&w, t.manager, id, json!({ "first_week_plan": "Pairing with the design lead" })).await,
        "in_progress"
    );
    let d = w.detail(id).await;
    assert_eq!(d.summary.open_steps, ["Set up accounts and equipment"]);
    assert!(item(&w, t.nia, id).await.is_none(), "the join still waits for IT");
    assert_eq!(done(&w, t.ian, id, json!({ "asset_tag": "LT-0193" })).await, "in_progress");

    // Then the acknowledgement, offered to the new member only.
    let ack = item(&w, t.nia, id).await.expect("the acknowledgement");
    assert_eq!(ack.kind, "acknowledge");
    assert_eq!(ack.outcomes, ["acknowledged"], "expired is never a human choice");
    assert!(ack.statement.as_deref().unwrap().contains("employee handbook"));
    assert_eq!(ack.why_you.as_deref(), Some("You're the new hire on this request"));
    assert!(item(&w, t.hr, id).await.is_none());
    let mut tx = w.tx().await;
    let refused =
        w.req.decide(&mut tx, &t.nia.actor(), ack.id, ack.revision, "expired", None, &Map::new()).await;
    assert!(matches!(refused, Err(RequestError::Invalid(_))), "{refused:?}");
    tx.rollback().await.unwrap();
    assert_eq!(w.decide_item(t.nia, ack.id, ack.revision, "acknowledged").await.unwrap(), "completed");

    let d = w.detail(id).await;
    assert_eq!(d.summary.status, "completed");
    assert_eq!(d.result.as_deref(), Some("Onboarded"));
    assert!(d.summary.open_steps.is_empty() && d.summary.current_step.is_none());
    let last = d.steps.last().unwrap();
    assert_eq!((last.node.as_str(), last.outcome.as_deref()), ("welcome", Some("acknowledged")));
    let order: Vec<&str> = d.steps.iter().map(|s| s.node.as_str()).collect();
    assert_eq!(order.last(), Some(&"welcome"), "the acknowledgement comes last");
    assert_eq!(last.parallel, None, "after the join");
    let texts: Vec<&str> = d.timeline.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"Set up accounts and equipment: done by Ian IT"), "{texts:?}");
    assert!(texts.contains(&"Confirm equipment and handbook: acknowledged by Nia New"), "{texts:?}");
    assert_eq!(texts.last(), Some(&"Finished: Onboarded"));
    assert_eq!(d.input["new_hire"]["name"], "Nia New");
    // The task forms kept what each team recorded.
    let vars: Value = {
        let mut tx = w.tx().await;
        let v = sqlx::query_scalar("SELECT state->'variables' FROM process_instances WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        v
    };
    assert_eq!(vars["asset_tag"], "LT-0193");
    // The initiator and the new member hear it is done.
    let mut tx = w.tx().await;
    let told: Vec<Uuid> = sqlx::query_scalar(
        "SELECT membership_id FROM notifications WHERE kind = 'request_notice' ORDER BY membership_id",
    )
    .fetch_all(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let mut want = vec![t.hr.membership, t.nia.membership];
    want.sort();
    assert_eq!(told, want);
}

#[tokio::test]
async fn missing_groups_fall_back_and_the_acknowledgement_is_never_delegated() {
    let Some(mut w) = world().await else { return };
    let owner = w.member("Olivia Owner", None).await;
    w.grant("process.owner", owner).await;
    let hr = w.member("Hana HR", None).await;
    let nia = w.member("Nia New", None).await;
    let dana = w.member("Dana Delegate", None).await;

    // No IT or Facilities group and no manager: every task goes to the process owner.
    let id = w.start(hr, "hr.onboarding", input(nia)).await.unwrap();
    let d = w.detail(id).await;
    assert!(d.steps.iter().all(|s| s.assignees == ["Olivia Owner"]), "{:?}", d.steps);
    for _ in 0..3 {
        done(&w, owner, id, json!({})).await;
    }

    // The new member is away with a delegate: the acknowledgement still waits for them.
    let id2 = w.start(hr, "hr.onboarding", input(nia)).await.unwrap();
    let mut tx = w.tx().await;
    let req = SetOutOfOffice {
        starts_on: date!(2026 - 10 - 12),
        ends_on: date!(2026 - 10 - 16),
        delegate: dana.membership,
        note: None,
    };
    w.req.set_out_of_office(&mut tx, &nia.actor(), nia.membership, &req).await.unwrap();
    tx.commit().await.unwrap();
    for _ in 0..3 {
        done(&w, owner, id2, json!({})).await;
    }
    let d = w.detail(id2).await;
    assert_eq!(d.steps.last().unwrap().assignees, ["Nia New"], "not substituted");
    assert!(item(&w, dana, id2).await.is_none(), "the delegate never sees it");
    let ack = item(&w, nia, id).await.unwrap();
    let mut tx = w.tx().await;
    let r =
        w.req.decide(&mut tx, &dana.actor(), ack.id, ack.revision, "acknowledged", None, &Map::new()).await;
    assert!(matches!(r, Err(RequestError::NotFound | RequestError::Forbidden(_))), "{r:?}");
    tx.rollback().await.unwrap();
    assert_eq!(w.decide_item(nia, ack.id, ack.revision, "acknowledged").await.unwrap(), "completed");
}

#[tokio::test]
async fn the_preview_says_the_three_teams_work_at_the_same_time() {
    let Some(mut w) = world().await else { return };
    let t = team(&mut w).await;
    let mut tx = w.tx().await;
    let p = w
        .req
        .preview(&mut tx, t.hr.membership, "hr.onboarding", input(t.nia).as_object().unwrap())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        p.summary,
        "Ian IT (IT), Fay Facilities (Facilities) and Mo Manager (Nia New's manager) at the same time, then Nia New"
    );
    assert!(p.steps.iter().filter(|s| s.first).count() == 3);
}
