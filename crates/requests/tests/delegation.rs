//! BPMS-086 out of office against real PostgreSQL: new work goes to the delegate and the
//! substitution is recorded; the delegate decides existing items on behalf of the away member;
//! separation of duties still applies to the delegate; the chain depth is 1 unless the workspace
//! raises it; an expired out-of-office no longer counts. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Who, World, expense, leave, world};
use procera_requests::{RequestError, SetOutOfOffice};
use serde_json::{Value, json};
use time::macros::date;

async fn away(w: &World, by: Who, member: Who, delegate: Who) {
    let mut tx = w.tx().await;
    let req = SetOutOfOffice {
        starts_on: date!(2026 - 10 - 12),
        ends_on: date!(2026 - 10 - 16),
        delegate: delegate.membership,
        note: Some("Back on Monday".into()),
    };
    w.req.set_out_of_office(&mut tx, &by.actor(), member.membership, &req).await.unwrap();
    tx.commit().await.unwrap();
}

async fn snapshot(w: &World, request: uuid::Uuid) -> Value {
    let mut tx = w.tx().await;
    let v = sqlx::query_scalar(
        "SELECT resolution FROM work_items WHERE instance_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(request)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    v
}

#[tokio::test]
async fn new_work_goes_to_the_delegate_who_decides_on_behalf() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let dana = w.member("Dana", None).await;
    away(&w, boss, boss, dana).await;

    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    let d = w.detail(id).await;
    assert_eq!(d.summary.assignees, ["Dana"]);
    assert!(
        d.timeline.iter().any(|t| t.kind == "delegated" && t.text.contains("Dana decides on behalf of Boss"))
    );
    let snap = snapshot(&w, id).await;
    assert_eq!(snap["substitutions"][0]["for"], json!(boss.membership));
    assert_eq!(snap["substitutions"][0]["until"], json!("2026-10-16"));
    assert!(w.open(boss).await.is_empty(), "new work does not wait for the away member");
    let mine = w.open(dana).await;
    let behalf = mine[0].on_behalf_of.as_ref().expect("on behalf");
    assert_eq!((behalf.name.as_str(), behalf.until), ("Boss", Some(date!(2026 - 10 - 16))));

    assert_eq!(w.decide(dana, "approved").await.unwrap(), "approved");
    let d = w.detail(id).await;
    assert_eq!(
        (d.steps[0].decided_by.as_deref(), d.steps[0].on_behalf_of.as_deref()),
        (Some("Dana"), Some("Boss"))
    );
    assert!(d.timeline.iter().any(|t| t.text == "Manager approval: approved by Dana on behalf of Boss"));
}

#[tokio::test]
async fn the_delegate_decides_existing_items_while_the_member_is_away() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let dana = w.member("Dana", None).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert!(w.open(dana).await.is_empty());
    {
        let mut tx = w.tx().await;
        assert!(!w.req.involved(&mut tx, id, dana.membership).await.unwrap());
        tx.rollback().await.unwrap();
    }

    away(&w, boss, boss, dana).await;
    let step = &w.detail(id).await.steps[0];
    assert_eq!(step.assignees, ["Boss"], "existing items stay");
    assert_eq!(step.away[0].name, "Boss");
    assert_eq!(step.away[0].delegate.as_ref().map(|p| p.name.as_str()), Some("Dana"));
    assert_eq!(w.open(boss).await.len(), 1);
    let item = w.open(dana).await.remove(0);
    assert_eq!(item.on_behalf_of.as_ref().map(|a| a.name.as_str()), Some("Boss"));
    {
        let mut tx = w.tx().await;
        assert!(w.req.involved(&mut tx, id, dana.membership).await.unwrap(), "the delegate may open it");
        tx.rollback().await.unwrap();
    }
    assert_eq!(w.decide_item(dana, item.id, item.revision, "approved").await.unwrap(), "approved");
    let mut tx = w.tx().await;
    let (by, behalf): (uuid::Uuid, Option<uuid::Uuid>) =
        sqlx::query_as("SELECT completed_by, completed_on_behalf_of FROM work_items WHERE id = $1")
            .bind(item.id)
            .fetch_one(tx.conn())
            .await
            .unwrap();
    let diff: Value = sqlx::query_scalar(
        "SELECT diff FROM audit_events WHERE action = 'work_item.decided' AND object_id = $1",
    )
    .bind(item.id)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!((by, behalf), (dana.membership, Some(boss.membership)));
    assert_eq!(diff["on_behalf_of"], json!(boss.membership));
}

#[tokio::test]
async fn separation_of_duties_holds_for_the_delegate() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let fin = w.member("Fiona", None).await;
    w.grant("finance.approver", fin).await;

    // The approver's delegate is the requester: no substitution, and she cannot decide for him.
    away(&w, boss, boss, ann).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert_eq!(w.detail(id).await.summary.assignees, ["Boss"]);
    assert_eq!(snapshot(&w, id).await["substitutions"][0]["skipped"], json!("separation_of_duties"));
    let item = w.open(boss).await.remove(0);
    assert!(w.open(ann).await.is_empty(), "never offered her own request");
    assert!(matches!(
        w.decide_item(ann, item.id, item.revision, "approved").await,
        Err(RequestError::SoD(_))
    ));
    assert_eq!(w.decide_item(boss, item.id, item.revision, "approved").await.unwrap(), "approved");

    // Finance is away and delegates to the manager who approved the first level: SoD keeps Fiona.
    let mut tx = w.tx().await;
    w.req.clear_out_of_office(&mut tx, &boss.actor(), boss.membership).await.unwrap();
    tx.commit().await.unwrap();
    away(&w, fin, fin, boss).await;
    let big = w.start(ann, "hr.expense-claim", expense("900.00", "EUR")).await.unwrap();
    assert_eq!(w.decide(boss, "approved").await.unwrap(), "in_progress");
    assert_eq!(w.detail(big).await.summary.assignees, ["Fiona"]);
    let item = w.open(fin).await.remove(0);
    assert!(matches!(
        w.decide_item(boss, item.id, item.revision, "approved").await,
        Err(RequestError::SoD(_))
    ));
}

#[tokio::test]
async fn delegation_depth_is_one_unless_raised_and_cycles_stop() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let dana = w.member("Dana", None).await;
    let eve = w.member("Eve", None).await;
    away(&w, boss, boss, dana).await;
    away(&w, dana, dana, eve).await;

    let one = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert_eq!(w.detail(one).await.summary.assignees, ["Dana"], "no chains by default");
    assert!(w.open(eve).await.is_empty(), "Dana holds it for Boss: Eve may not pass it on a second hop");

    w.set("access.delegation.max_depth", json!(2)).await;
    let two = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert_eq!(w.detail(two).await.summary.assignees, ["Eve"]);

    // Eve hands back to Boss: even with room for a third hop, the chain stops before it loops.
    away(&w, eve, eve, boss).await;
    w.set("access.delegation.max_depth", json!(3)).await;
    let three = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert_eq!(w.detail(three).await.summary.assignees, ["Eve"]);
    assert_eq!(snapshot(&w, three).await["substitutions"][0]["skipped"], json!("cycle"));
}

#[tokio::test]
async fn an_expired_out_of_office_no_longer_counts_and_bad_input_is_refused() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let dana = w.member("Dana", None).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    away(&w, boss, boss, dana).await;
    let item = w.open(dana).await.remove(0);
    // A week later it has ended: Dana no longer sees or decides Boss's work.
    w.clock.advance(time::Duration::days(7));
    assert!(w.open(dana).await.is_empty());
    assert!(matches!(
        w.decide_item(dana, item.id, item.revision, "approved").await,
        Err(RequestError::NotFound)
    ));
    let again = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert_eq!(w.detail(again).await.summary.assignees, ["Boss"]);
    assert_eq!(w.detail(id).await.steps[0].away.len(), 0);

    let mut tx = w.tx().await;
    let bad = |delegate: Who, starts: time::Date, ends: time::Date| SetOutOfOffice {
        starts_on: starts,
        ends_on: ends,
        delegate: delegate.membership,
        note: None,
    };
    let today = date!(2026 - 10 - 19);
    let cases = [
        (bad(boss, today, today), "/delegate"),
        (bad(dana, today, date!(2026 - 10 - 18)), "/ends_on"),
        (bad(dana, date!(2026 - 10 - 01), date!(2026 - 10 - 02)), "/ends_on"),
        (bad(dana, today, date!(2028 - 01 - 01)), "/ends_on"),
    ];
    for (req, pointer) in cases {
        match w.req.set_out_of_office(&mut tx, &boss.actor(), boss.membership, &req).await {
            Err(RequestError::Invalid(e)) => assert_eq!(e[0].pointer, pointer),
            other => panic!("{other:?}"),
        }
    }
    let ok = w
        .req
        .set_out_of_office(&mut tx, &ann.actor(), boss.membership, &bad(dana, today, today))
        .await
        .unwrap();
    assert!(ok.active);
    assert_eq!(ok.set_by.name, "Ann");
    w.req.clear_out_of_office(&mut tx, &boss.actor(), boss.membership).await.unwrap();
    assert!(matches!(
        w.req.clear_out_of_office(&mut tx, &boss.actor(), boss.membership).await,
        Err(RequestError::NotFound)
    ));
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE action LIKE 'member.ooo_%' AND object_id = $1 ORDER BY seq",
    )
    .bind(boss.membership)
    .fetch_all(tx.conn())
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(actions, ["member.ooo_set", "member.ooo_set", "member.ooo_cleared"]);
}

#[tokio::test]
async fn the_approver_delegates_one_step_to_a_colleague_and_sod_still_applies() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let cleo = w.member("Cleo", None).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    let item = w.open(boss).await.pop().expect("with the boss").id;
    let mut tx = w.tx().await;
    // Only someone the step waits on may delegate it; not to themselves; never to the requester.
    assert!(matches!(
        w.req.delegate(&mut tx, &cleo.actor(), item, cleo.membership, None).await,
        Err(RequestError::Forbidden(_))
    ));
    assert!(matches!(
        w.req.delegate(&mut tx, &boss.actor(), item, boss.membership, None).await,
        Err(RequestError::Invalid(_))
    ));
    assert!(matches!(
        w.req.delegate(&mut tx, &boss.actor(), item, ann.membership, None).await,
        Err(RequestError::Invalid(_))
    ));
    let done = w
        .req
        .delegate(&mut tx, &boss.actor(), item, cleo.membership, Some("I'm at the offsite"))
        .await
        .unwrap();
    assert_eq!(done.assignees, ["Cleo"]);
    tx.commit().await.unwrap();
    assert!(w.open(boss).await.is_empty(), "off the boss's list");
    assert_eq!(w.open(cleo).await.len(), 1);
    let d = w.detail(id).await;
    assert!(
        d.timeline.iter().any(|t| t.text.contains("delegated by Boss: I'm at the offsite")),
        "{:?}",
        d.timeline
    );
    let mut tx = w.tx().await;
    let audited: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_events WHERE action = 'work_item.delegated'")
            .fetch_all(tx.conn())
            .await
            .unwrap();
    assert_eq!(audited.len(), 1);
    tx.rollback().await.unwrap();
    // Cleo decides it.
    assert!(w.decide(cleo, "approved").await.is_ok());
}
