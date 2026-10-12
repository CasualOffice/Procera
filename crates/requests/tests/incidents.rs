//! PR-09 technical incidents against real PostgreSQL: a dead-lettered job and a failed reminder are
//! listed with their request; retry requeues the job, dismiss cancels the timer; a reason is
//! required, each action is audited, and handling one twice is a conflict. Skips when
//! PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{leave, world};
use procera_requests::RequestError;
use uuid::Uuid;

#[tokio::test]
async fn incidents_are_listed_retried_and_dismissed_with_a_reason() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let request = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    let (job, timer) = (Uuid::now_v7(), Uuid::now_v7());
    let mut tx = w.tx().await;
    sqlx::query(
        "INSERT INTO jobs (workspace_id, id, kind, instance_id, payload, run_at, attempts, max_attempts, state, idempotency_key, created_at, finished_at, last_error)
         VALUES (current_setting('app.workspace_id')::uuid, $1, 'advance', $2, '{}', now(), 10, 10, 'dead', 'k-' || $1::text, now(), now(), '{\"message\": \"boom\"}')",
    )
    .bind(job)
    .bind(request)
    .execute(tx.conn())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO timers (workspace_id, id, timer_key, instance_id, purpose, due_at, state, created_at, attempts, last_error)
         SELECT workspace_id, $1, work_key || '/esc/9', instance_id, 'reminder', now(), 'failed', now(), 6, 'smtp down'
           FROM work_items WHERE instance_id = $2 LIMIT 1",
    )
    .bind(timer)
    .bind(request)
    .execute(tx.conn())
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = w.tx().await;
    let list = w.req.incidents(&mut tx).await.unwrap();
    let whats: Vec<_> =
        list.iter().map(|i| (i.source.as_str(), i.what.as_str(), i.error.as_deref())).collect();
    assert!(
        whats.contains(&("job", "The request couldn’t move to its next step", Some("boom"))),
        "{whats:?}"
    );
    assert!(
        whats.contains(&("timer", "Reminder for “Manager approval” couldn’t be sent", Some("smtp down"))),
        "{whats:?}"
    );
    assert!(list.iter().all(|i| i.request.as_ref().is_some_and(|r| r.id == request)));
    let owner = boss.actor();
    assert!(matches!(
        w.req.retry_incident(&mut tx, &owner, "job", job, "  ").await,
        Err(RequestError::Invalid(_))
    ));
    w.req.retry_incident(&mut tx, &owner, "job", job, "mail server back").await.unwrap();
    w.req.dismiss_incident(&mut tx, &owner, "timer", timer, "sent by hand").await.unwrap();
    assert!(matches!(
        w.req.dismiss_incident(&mut tx, &owner, "timer", timer, "again").await,
        Err(RequestError::Conflict(_))
    ));
    assert!(matches!(
        w.req.retry_incident(&mut tx, &owner, "job", Uuid::now_v7(), "x").await,
        Err(RequestError::NotFound)
    ));
    assert!(w.req.incidents(&mut tx).await.unwrap().is_empty());
    let (state, attempts): (String, i32) = sqlx::query_as("SELECT state, attempts FROM jobs WHERE id = $1")
        .bind(job)
        .fetch_one(tx.conn())
        .await
        .unwrap();
    assert_eq!((state.as_str(), attempts), ("queued", 0));
    let actions: Vec<(String,)> =
        sqlx::query_as("SELECT action FROM audit_events WHERE action LIKE 'incident.%' ORDER BY seq")
            .fetch_all(tx.conn())
            .await
            .unwrap();
    assert_eq!(
        actions.into_iter().map(|a| a.0).collect::<Vec<_>>(),
        ["incident.retried", "incident.dismissed"]
    );
    tx.rollback().await.unwrap();
}
