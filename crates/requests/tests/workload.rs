//! FR-A03 workload against real PostgreSQL: a manager's team is their whole reporting line, each
//! person's open steps are counted, an away member holding steps is a coverage gap, and the steps
//! one person holds can be listed. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{leave, world};
use procera_requests::{Reach, SetOutOfOffice, WorkState};
use time::macros::date;

#[tokio::test]
async fn a_team_is_the_reporting_line_with_open_steps_and_coverage_gaps() {
    let Some(mut w) = world().await else { return };
    let director = w.member("Dee Director", None).await;
    let boss = w.member("Bo Boss", Some(director)).await;
    let ann = w.member("Ann", Some(boss)).await;
    let bob = w.member("Bob", Some(boss)).await;
    let outsider = w.member("Oz Outsider", None).await;
    let dana = w.member("Dana", None).await;
    w.start(ann, "hr.leave-request", leave()).await.unwrap();
    w.start(bob, "hr.leave-request", leave()).await.unwrap();

    let mut tx = w.tx().await;
    let team = w.req.workload(&mut tx, Reach::Team(director.membership, vec![])).await.unwrap();
    let names: Vec<_> = team.people.iter().map(|p| (p.name.as_str(), p.open)).collect();
    assert_eq!(names, [("Bo Boss", 2), ("Ann", 0), ("Bob", 0)], "busiest first; indirect reports included");
    assert_eq!((team.totals.people, team.totals.open, team.totals.coverage_gaps), (3, 2, 0));
    assert!(team.people[0].oldest.is_some());
    let mine = w.req.workload(&mut tx, Reach::Team(boss.membership, vec![])).await.unwrap();
    assert_eq!(mine.people.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Ann", "Bob"]);
    let all = w.req.workload(&mut tx, Reach::All).await.unwrap();
    assert!(all.people.iter().any(|p| p.name == "Oz Outsider"));
    assert!(w.req.reports_to(&mut tx, ann.membership, director.membership, &[]).await.unwrap());
    assert!(!w.req.reports_to(&mut tx, outsider.membership, director.membership, &[]).await.unwrap());
    assert!(!w.req.reports_to(&mut tx, director.membership, ann.membership, &[]).await.unwrap());
    let held = w.req.work(&mut tx, director.membership, WorkState::HeldBy(boss.membership)).await.unwrap();
    assert_eq!(held.len(), 2);
    // Within the team: the director may move Boss's steps to Ann or take them, not hand them out.
    let item = held[0].id;
    assert!(
        w.req
            .may_reassign_within_team(&mut tx, director.membership, &[], item, ann.membership)
            .await
            .unwrap()
    );
    assert!(
        w.req
            .may_reassign_within_team(&mut tx, director.membership, &[], item, director.membership)
            .await
            .unwrap()
    );
    assert!(
        !w.req
            .may_reassign_within_team(&mut tx, director.membership, &[], item, outsider.membership)
            .await
            .unwrap()
    );
    assert!(
        !w.req.may_reassign_within_team(&mut tx, ann.membership, &[], item, bob.membership).await.unwrap(),
        "Boss isn't in Ann's line"
    );
    tx.rollback().await.unwrap();

    // Boss goes away: the steps already with them are a coverage gap.
    let mut tx = w.tx().await;
    let req = SetOutOfOffice {
        starts_on: date!(2026 - 10 - 12),
        ends_on: date!(2026 - 10 - 16),
        delegate: dana.membership,
        note: None,
    };
    w.req.set_out_of_office(&mut tx, &director.actor(), boss.membership, &req).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = w.tx().await;
    let team = w.req.workload(&mut tx, Reach::Team(director.membership, vec![])).await.unwrap();
    let away = team.people[0].away.as_ref().expect("away");
    assert_eq!((away.until.as_str(), away.delegate.as_str()), ("2026-10-16", "Dana"));
    assert_eq!(team.totals.coverage_gaps, 1);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn a_department_team_manager_sees_the_department_and_its_sub_teams() {
    let Some(mut w) = world().await else { return };
    let tm = w.member("Tess Teamlead", None).await;
    let sam = w.member("Sam Support", None).await;
    let nia = w.member("Nia Night", None).await;
    let oz = w.member("Oz Outsider", None).await;
    let (ops, night) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    let mut tx = w.tx().await;
    for (id, parent, name, path) in
        [(ops, None, "Operations", vec![ops]), (night, Some(ops), "Night shift", vec![ops, night])]
    {
        sqlx::query("INSERT INTO departments (workspace_id, id, parent_id, name, path) VALUES (current_setting('app.workspace_id')::uuid, $1, $2, $3, $4)")
            .bind(id)
            .bind(parent)
            .bind(name)
            .bind(path)
            .execute(tx.conn())
            .await
            .unwrap();
    }
    for (who, dept) in [(sam, ops), (nia, night), (tm, ops)] {
        sqlx::query("UPDATE memberships SET department_id = $2 WHERE id = $1")
            .bind(who.membership)
            .bind(dept)
            .execute(tx.conn())
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();

    let mut tx = w.tx().await;
    let team = w.req.workload(&mut tx, Reach::Team(tm.membership, vec![ops])).await.unwrap();
    let names: Vec<_> = team.people.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["Nia Night", "Sam Support"],
        "the department and its sub-team, not the manager, not outsiders"
    );
    assert!(w.req.reports_to(&mut tx, nia.membership, tm.membership, &[ops]).await.unwrap());
    assert!(!w.req.reports_to(&mut tx, oz.membership, tm.membership, &[ops]).await.unwrap());
    assert!(w.req.manages(&mut tx, tm.membership, &[ops]).await.unwrap());
    assert!(
        !w.req.manages(&mut tx, tm.membership, &[]).await.unwrap(),
        "no reports and no department: no team"
    );
    // Without the department grant, the same person has no team.
    assert!(w.req.workload(&mut tx, Reach::Team(tm.membership, vec![])).await.unwrap().people.is_empty());
    tx.rollback().await.unwrap();
}
