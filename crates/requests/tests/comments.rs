//! BPMS-214 request comments, @mentions and watchers against real PostgreSQL: the requester and
//! the approver talk; an internal note is hidden from the requester (who can't post one or be
//! mentioned in one); mentions and followers are notified once each and never the author; watchers
//! hear new comments and the outcome. Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{World, leave, world};
use procera_requests::{NewComment, RequestError};
use uuid::Uuid;

async fn kinds(w: &World, to: Uuid) -> Vec<String> {
    let mut tx = w.tx().await;
    let k = sqlx::query_scalar("SELECT kind FROM notifications WHERE membership_id = $1 AND kind LIKE 'request_%' ORDER BY kind")
        .bind(to)
        .fetch_all(tx.conn())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    k
}

fn say(body: &str) -> NewComment {
    NewComment { body: body.into(), ..NewComment::default() }
}

#[tokio::test]
async fn requester_and_approver_talk_internal_notes_stay_internal_and_followers_hear_once() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let hal = w.member("Hal HR", None).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();

    let mut tx = w.tx().await;
    // Ann asks; the boss (on the open step) hears it.
    w.req.comment(&mut tx, &ann.actor(), id, &say("  Is the 23rd ok?  ")).await.unwrap();
    // The boss writes an internal note mentioning Hal: Hal hears a mention, Ann hears nothing.
    let note = NewComment {
        body: "Team is thin that week".into(),
        internal: true,
        mentions: vec![hal.membership, hal.membership, boss.membership],
    };
    let c = w.req.comment(&mut tx, &boss.actor(), id, &note).await.unwrap();
    assert_eq!(
        c.mentions.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Hal HR"],
        "deduplicated, never yourself"
    );
    // Hal watches; the boss replies publicly: Ann and Hal hear it.
    assert!(w.req.watch(&mut tx, &hal.actor(), id, true).await.unwrap());
    w.req.watch(&mut tx, &hal.actor(), id, true).await.unwrap(); // idempotent
    w.req.comment(&mut tx, &boss.actor(), id, &say("Yes, approved shortly")).await.unwrap();

    // What each person sees.
    let for_ann = w.req.conversation(&mut tx, id, ann.membership).await.unwrap();
    assert_eq!(
        for_ann.comments.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(),
        ["Is the 23rd ok?", "Yes, approved shortly"]
    );
    assert!(!for_ann.watching);
    let for_boss = w.req.conversation(&mut tx, id, boss.membership).await.unwrap();
    assert_eq!(for_boss.comments.len(), 3);
    assert!(for_boss.comments[1].internal);
    let for_hal = w.req.conversation(&mut tx, id, hal.membership).await.unwrap();
    assert!(for_hal.watching);
    assert_eq!(for_hal.watchers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Hal HR"]);

    // The rules.
    let internal = NewComment { body: "psst".into(), internal: true, ..NewComment::default() };
    assert!(
        matches!(w.req.comment(&mut tx, &ann.actor(), id, &internal).await, Err(RequestError::Invalid(_))),
        "requester can't post internal"
    );
    let mention_ann = NewComment { body: "x".into(), internal: true, mentions: vec![ann.membership] };
    assert!(matches!(
        w.req.comment(&mut tx, &boss.actor(), id, &mention_ann).await,
        Err(RequestError::Invalid(_))
    ));
    assert!(matches!(
        w.req.comment(&mut tx, &boss.actor(), id, &say("   ")).await,
        Err(RequestError::Invalid(_))
    ));
    assert!(matches!(
        w.req.comment(&mut tx, &boss.actor(), id, &say(&"a".repeat(4001))).await,
        Err(RequestError::Invalid(_))
    ));
    tx.commit().await.unwrap();

    assert_eq!(
        kinds(&w, boss.membership).await,
        ["request_comment"],
        "Ann's question; never his own comments"
    );
    assert_eq!(kinds(&w, ann.membership).await, ["request_comment"], "only the public reply");
    assert_eq!(kinds(&w, hal.membership).await, ["request_comment", "request_mention"]);

    // The outcome reaches the watcher too; unwatching stops it.
    w.decide(boss, "approved").await.unwrap();
    assert!(kinds(&w, hal.membership).await.contains(&"request_completed".to_owned()));
    let mut tx = w.tx().await;
    assert!(!w.req.watch(&mut tx, &hal.actor(), id, false).await.unwrap());
    assert!(w.req.conversation(&mut tx, id, hal.membership).await.unwrap().watchers.is_empty());
    let audited: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events WHERE action LIKE 'request.%ed' AND action <> 'request.started' ORDER BY seq")
        .fetch_all(tx.conn())
        .await
        .unwrap();
    assert_eq!(
        audited,
        [
            "request.commented",
            "request.commented",
            "request.watched",
            "request.commented",
            "request.unwatched"
        ]
    );
    tx.commit().await.unwrap();
}
