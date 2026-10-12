//! BPMS-085/088 against real PostgreSQL: work-item reminders fire once at the right business time,
//! an overdue manager approval is reassigned to the next manager up (audited as the system), a
//! decision cancels the remaining timers, and an admin reassignment re-runs separation of duties.
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_engine::Scheduler;
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::clock::ManualClock;
use procera_kernel::{Clock, UuidV7Gen};
use procera_members::Members;
use procera_notify::Notify;
use procera_requests::{Actor, RequestError, Requests, WorkState, WorkView};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::macros::datetime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
struct Who {
    membership: Uuid,
    subject_ref: Uuid,
}

impl Who {
    fn actor(self) -> Actor {
        Actor { membership: self.membership, subject_ref: self.subject_ref, ip: None, user_agent: None }
    }
}

struct World {
    t: test_support::TestDb,
    clock: Arc<ManualClock>,
    req: Requests,
    sched: Scheduler,
    ctx: WsCtx,
    admin: sqlx::PgConnection,
}

/// Monday 12 Oct 2026, 09:30 UTC: a leave request's manager step is due Wed 14 Oct 09:30 (P2D
/// business); reminders at Tue 09:30 and Wed 09:30, the escalation at Thu 15 Oct 09:30.
const T0: OffsetDateTime = datetime!(2026-10-12 09:30 UTC);

async fn world() -> Option<World> {
    let t = test_support::fresh().await?.expect("test database");
    let clock = Arc::new(ManualClock::new(T0));
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).unwrap());
    let ids = Arc::new(UuidV7Gen);
    let cfg = IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() };
    let identity = Identity::new(t.db.clone(), keys.clone(), clock.clone(), ids.clone(), cfg).unwrap();
    let access = procera_access::Access::new();
    let members = Members::new(t.db.clone(), identity, access, keys.clone(), clock.clone(), ids.clone());
    let notify = Notify::new(t.db.clone(), keys, clock.clone(), ids.clone());
    let req = Requests::new(t.db.clone(), members, notify, clock.clone(), ids, "https://app.test").unwrap();
    let sched = Scheduler::new(req.engine().clone(), "escalations-test");
    let admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await.unwrap();
    Some(World { t, clock, req, sched, ctx: WsCtx { workspace_id: Uuid::now_v7() }, admin })
}

impl World {
    async fn tx(&self) -> WsTx<'static> {
        self.t.db.begin_ws(self.ctx).await.unwrap()
    }

    async fn member(&mut self, name: &str, manager: Option<Who>) -> Who {
        let user = Uuid::now_v7();
        sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
            .bind(user)
            .bind(format!("{name}-{user}@acme.test"))
            .bind(name)
            .bind(Uuid::new_v4())
            .execute(&mut self.admin)
            .await
            .unwrap();
        let m = Uuid::now_v7();
        let mut tx = self.tx().await;
        sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status, manager_membership_id) VALUES ($1, $2, $3, 'active', $4)")
            .bind(self.ctx.workspace_id)
            .bind(m)
            .bind(user)
            .bind(manager.map(|a| a.membership))
            .execute(tx.conn())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        Who { membership: m, subject_ref: Uuid::new_v4() }
    }

    async fn leave(&self, who: Who) -> Uuid {
        let input = json!({"leave_type": "annual", "start_date": "2026-10-20", "end_date": "2026-10-22"});
        let mut tx = self.tx().await;
        let s = self
            .req
            .start(&mut tx, &who.actor(), "hr.leave-request", input.as_object().unwrap())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        s.id
    }

    /// Moves the clock to `at` and runs the scheduler until it is idle.
    async fn at(&self, at: OffsetDateTime) {
        self.clock.advance(at - self.clock.now());
        self.sched.drain().await.unwrap();
    }

    async fn work(&self, who: Who, state: WorkState) -> Vec<WorkView> {
        let mut tx = self.tx().await;
        let w = self.req.work(&mut tx, who.membership, state).await.unwrap();
        tx.rollback().await.unwrap();
        w
    }

    async fn notes(&self, who: Who, kind: &str) -> i64 {
        self.count(
            "SELECT count(*) FROM notifications WHERE membership_id = $1 AND kind = $2",
            who.membership,
            kind,
        )
        .await
    }

    async fn count(&self, sql: &str, id: Uuid, s: &str) -> i64 {
        let mut tx = self.tx().await;
        let n = sqlx::query_scalar(sql).bind(id).bind(s).fetch_one(tx.conn()).await.unwrap();
        tx.rollback().await.unwrap();
        n
    }

    async fn timeline(&self, id: Uuid) -> Vec<(String, String)> {
        let mut tx = self.tx().await;
        let d = self.req.detail(&mut tx, id).await.unwrap();
        tx.rollback().await.unwrap();
        d.timeline.into_iter().map(|t| (t.kind, t.text)).collect()
    }
}

#[tokio::test]
async fn reminders_fire_once_each_at_their_business_time() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let id = w.leave(ann).await;

    w.at(datetime!(2026-10-13 09:29 UTC)).await;
    assert_eq!(w.notes(boss, "work_reminder").await, 0, "not before one business day");
    w.at(datetime!(2026-10-13 09:31 UTC)).await;
    assert_eq!(w.notes(boss, "work_reminder").await, 1);
    w.sched.run_once().await.unwrap();
    assert_eq!(w.notes(boss, "work_reminder").await, 1, "fires once");
    let open = w.work(boss, WorkState::Open).await;
    assert!(!open[0].overdue);

    // At the due time: the second reminder, and the item now counts as overdue.
    w.at(datetime!(2026-10-14 09:31 UTC)).await;
    assert_eq!(w.notes(boss, "work_reminder").await, 2);
    assert!(w.work(boss, WorkState::Open).await[0].overdue);
    let overdue = w.work(boss, WorkState::Overdue).await;
    assert_eq!((overdue.len(), overdue[0].assignees.clone()), (1, vec!["Boss".to_owned()]));
    let tl = w.timeline(id).await;
    assert_eq!(tl.iter().filter(|(k, _)| k == "reminded").count(), 2, "{tl:?}");
    assert!(tl.iter().any(|(_, t)| t == "Reminded: Manager approval (Boss)"), "{tl:?}");
    assert_eq!(w.notes(ann, "work_reminder").await, 0, "the requester is not reminded");
}

#[tokio::test]
async fn an_overdue_manager_step_is_reassigned_to_the_next_manager_and_audited() {
    let Some(mut w) = world().await else { return };
    let director = w.member("Dana", None).await;
    let boss = w.member("Boss", Some(director)).await;
    let ann = w.member("Ann", Some(boss)).await;
    let id = w.leave(ann).await;

    w.at(datetime!(2026-10-15 09:31 UTC)).await;
    assert!(w.work(boss, WorkState::Open).await.is_empty(), "taken off the manager");
    let item = w.work(director, WorkState::Open).await.pop().expect("with the director");
    assert_eq!(w.notes(director, "work_assigned").await, 1);
    assert_eq!(w.notes(boss, "work_reassigned").await, 1);
    let audit: Vec<(String, Option<Uuid>, Value)> = {
        let mut tx = w.tx().await;
        let rows = sqlx::query_as(
            "SELECT actor_kind, actor_subject_ref, diff FROM audit_events WHERE action = 'work_item.reassigned' AND object_id = $1",
        )
        .bind(item.id)
        .fetch_all(tx.conn())
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        rows
    };
    assert_eq!(audit.len(), 1);
    assert_eq!((audit[0].0.as_str(), audit[0].1), ("system", None));
    assert_eq!(audit[0].2["reason"], "escalated after due");
    let tl = w.timeline(id).await;
    assert!(
        tl.contains(&(
            "escalated".into(),
            "Escalated: Manager approval reassigned to Dana (escalated after due)".into()
        )),
        "{tl:?}"
    );

    // The director decides; the request completes.
    let mut tx = w.tx().await;
    let d =
        w.req.decide(&mut tx, &director.actor(), item.id, item.revision, "approved", None, &Map::new()).await;
    tx.commit().await.unwrap();
    assert_eq!(d.unwrap().request.status, "approved");
}

#[tokio::test]
async fn with_no_manager_above_the_step_stays_and_a_decision_cancels_the_timers() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let first = w.leave(ann).await;
    w.at(datetime!(2026-10-15 09:31 UTC)).await;
    assert_eq!(w.work(boss, WorkState::Open).await.len(), 1, "nobody above Boss: it stays");
    assert!(w.timeline(first).await.iter().any(|(_, t)| t.contains("nobody to reassign it to")));

    // A second request decided at once: its reminders and escalation are cancelled.
    let second = w.leave(ann).await;
    let item = w.work(boss, WorkState::Open).await.into_iter().find(|i| i.request.id == second).unwrap();
    let mut tx = w.tx().await;
    w.req
        .decide(&mut tx, &boss.actor(), item.id, item.revision, "approved", None, &Map::new())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let scheduled = w
        .count(
            "SELECT count(*) FROM timers WHERE instance_id = $1 AND purpose IN ('reminder', 'escalation') AND state = $2",
            second,
            "scheduled",
        )
        .await;
    assert_eq!(scheduled, 0);
    let before = w.notes(boss, "work_reminder").await;
    w.at(datetime!(2026-10-23 09:31 UTC)).await;
    assert_eq!(w.notes(boss, "work_reminder").await, before);
}

#[tokio::test]
async fn an_admin_reassignment_reruns_separation_of_duties() {
    let Some(mut w) = world().await else { return };
    let admin = w.member("Ada", None).await;
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let cleo = w.member("Cleo", None).await;
    let id = w.leave(ann).await;
    let item = w.work(boss, WorkState::Open).await.pop().unwrap();

    let reassign = |to: Who, reason: &'static str| {
        let w = &w;
        async move {
            let mut tx = w.tx().await;
            let r = w.req.reassign(&mut tx, &admin.actor(), item.id, to.membership, reason).await;
            tx.commit().await.unwrap();
            r
        }
    };
    match reassign(ann, "she is away").await {
        Err(RequestError::Invalid(e)) => {
            assert_eq!((e[0].pointer.as_str(), e[0].code.as_str()), ("/membership_id", "sod_excluded"));
            assert!(e[0].message.contains("started this request"), "{}", e[0].message);
        }
        other => panic!("expected a SoD refusal, got {other:?}"),
    }
    assert!(matches!(reassign(cleo, "  ").await, Err(RequestError::Invalid(_))), "a reason is required");

    let r = reassign(cleo, "Boss is on leave").await.unwrap();
    assert_eq!(r.assignees, ["Cleo"]);
    assert_eq!(r.revision, item.revision + 1);
    assert!(w.work(boss, WorkState::Open).await.is_empty());
    assert_eq!(w.work(cleo, WorkState::Open).await.len(), 1);
    assert_eq!(w.notes(cleo, "work_assigned").await, 1);
    assert_eq!(w.notes(boss, "work_reassigned").await, 1);
    assert_eq!(
        w.count(
            "SELECT count(*) FROM audit_events WHERE object_id = $1 AND action = $2 AND actor_kind = 'user'",
            item.id,
            "work_item.reassigned"
        )
        .await,
        1
    );
    let tl = w.timeline(id).await;
    assert!(
        tl.contains(&(
            "reassigned".into(),
            "Reassigned: Manager approval to Cleo by Ada (Boss is on leave)".into()
        )),
        "{tl:?}"
    );

    // Cleo decides; a decided step can't be reassigned.
    let mut tx = w.tx().await;
    w.req.decide(&mut tx, &cleo.actor(), item.id, r.revision, "approved", None, &Map::new()).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(reassign(boss, "late").await, Err(RequestError::AlreadyDecided)));
}
