//! Proof tests for the 0.2 evidence gates (docs/23 E2E-R2-SYS-01, -03), against real PostgreSQL:
//!
//! - **Restart durability** (V-06): a request, its open step and the step's reminder and
//!   escalation timers live only in the database. After the request service, its engine and the
//!   scheduler are dropped and rebuilt, nothing is lost and a reminder is sent exactly once.
//! - **Concurrent approvals** (V-05): twenty eligible approvers decide the same step at once;
//!   exactly one decision commits, the rest are refused, and the database holds one decision and
//!   one instance advance.
//!
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

/// Monday 12 Oct 2026, 09:30 UTC: a leave request's manager step is due Wed 14 Oct 09:30 (P2D
/// business), with reminders at Tue 09:30 and Wed 09:30 and the escalation on Thu 09:30.
const T0: OffsetDateTime = datetime!(2026-10-12 09:30 UTC);

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
    root_key: String,
    ctx: WsCtx,
    admin: sqlx::PgConnection,
}

/// The request service and its engine, built from nothing but the database, the clock and the
/// configured root key (what a process start does).
fn boot(t: &test_support::TestDb, clock: Arc<ManualClock>, root_key: &str) -> Requests {
    let keys = Arc::new(LocalKey::from_base64(root_key).unwrap());
    let ids = Arc::new(UuidV7Gen);
    let cfg = IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() };
    let identity = Identity::new(t.db.clone(), keys.clone(), clock.clone(), ids.clone(), cfg).unwrap();
    let access = procera_access::Access::new();
    let members = Members::new(t.db.clone(), identity, access, keys.clone(), clock.clone(), ids.clone());
    let notify = Notify::new(t.db.clone(), keys, clock.clone(), ids.clone());
    Requests::new(t.db.clone(), members, notify, clock, ids, "https://app.test").unwrap()
}

async fn world() -> Option<World> {
    let t = test_support::fresh().await?.expect("test database");
    let clock = Arc::new(ManualClock::new(T0));
    let root_key = LocalKey::generate_base64();
    let req = boot(&t, clock.clone(), &root_key);
    let admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await.unwrap();
    Some(World { t, clock, req, root_key, ctx: WsCtx { workspace_id: Uuid::now_v7() }, admin })
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

    async fn grant(&self, role: &str, to: Who) {
        let mut tx = self.tx().await;
        let ws = self.ctx.workspace_id;
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO roles (workspace_id, id, key, name, permissions) VALUES ($1, $2, $3, $3, '{}')
             ON CONFLICT (workspace_id, key) DO UPDATE SET name = excluded.name RETURNING id",
        )
        .bind(ws)
        .bind(Uuid::now_v7())
        .bind(role)
        .fetch_one(tx.conn())
        .await
        .unwrap();
        sqlx::query("INSERT INTO role_grants (workspace_id, id, role_id, membership_id, scope_kind) VALUES ($1, $2, $3, $4, 'workspace')")
            .bind(ws)
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(to.membership)
            .execute(tx.conn())
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    async fn start(&self, who: Who, key: &str, input: Value) -> Uuid {
        let mut tx = self.tx().await;
        let s = self.req.start(&mut tx, &who.actor(), key, input.as_object().unwrap()).await.unwrap();
        tx.commit().await.unwrap();
        s.id
    }

    async fn open(&self, who: Who) -> Vec<WorkView> {
        let mut tx = self.tx().await;
        let w = self.req.work(&mut tx, who.membership, WorkState::Open).await.unwrap();
        tx.rollback().await.unwrap();
        w
    }

    async fn decide(&self, who: Who, outcome: &str) -> Result<String, RequestError> {
        let item = self.open(who).await.into_iter().next().expect("an open work item");
        let mut tx = self.tx().await;
        let d = self
            .req
            .decide(&mut tx, &who.actor(), item.id, item.revision, outcome, None, &Map::new())
            .await?;
        tx.commit().await.unwrap();
        Ok(d.request.status)
    }

    async fn n(&self, sql: &str, id: Uuid) -> i64 {
        let mut tx = self.tx().await;
        let n = sqlx::query_scalar(sql).bind(id).fetch_one(tx.conn()).await.unwrap();
        tx.rollback().await.unwrap();
        n
    }

    async fn strings(&self, sql: &str, id: Uuid) -> Vec<String> {
        let mut tx = self.tx().await;
        let v = sqlx::query_scalar(sql).bind(id).fetch_all(tx.conn()).await.unwrap();
        tx.rollback().await.unwrap();
        v
    }

    async fn reminders(&self, who: Who) -> i64 {
        self.n(
            "SELECT count(*) FROM notifications WHERE membership_id = $1 AND kind = 'work_reminder'",
            who.membership,
        )
        .await
    }
}

const TIMERS: &str =
    "SELECT purpose || '@' || to_char(due_at AT TIME ZONE 'UTC', 'Dy HH24:MI') || ':' || state FROM timers
      WHERE instance_id = $1 AND purpose IN ('reminder', 'escalation') ORDER BY due_at, timer_key";

#[tokio::test]
async fn sys01_restart_keeps_the_request_its_open_step_and_timers_and_a_reminder_fires_once() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let input = json!({"leave_type": "annual", "start_date": "2026-10-20", "end_date": "2026-10-22"});
    let id = w.start(ann, "hr.leave-request", input).await;
    let timers = w.strings(TIMERS, id).await;
    assert_eq!(
        timers,
        ["reminder@Tue 09:30:scheduled", "reminder@Wed 09:30:scheduled", "escalation@Thu 09:30:scheduled"]
    );
    let step = w.open(boss).await;
    assert_eq!(step.len(), 1);

    // Restart: the service, its engine and every in-memory registry are rebuilt from scratch.
    w.req = boot(&w.t, w.clock.clone(), &w.root_key);
    let (s1, s2) =
        (Scheduler::new(w.req.engine().clone(), "node-1"), Scheduler::new(w.req.engine().clone(), "node-2"));
    assert_eq!(w.strings(TIMERS, id).await, timers, "armed timers survive");
    let again = w.open(boss).await;
    assert_eq!((again.len(), again[0].id, again[0].revision), (1, step[0].id, step[0].revision));
    assert_eq!(w.n("SELECT revision FROM process_instances WHERE id = $1", id).await, 1);

    // Tuesday 09:31: two scheduler nodes race; the first reminder goes out once.
    w.clock.advance(datetime!(2026-10-13 09:31 UTC) - w.clock.now());
    let (a, b) = tokio::join!(s1.drain(), s2.drain());
    assert_eq!(a.unwrap().timers_fired + b.unwrap().timers_fired, 1);
    assert_eq!(w.reminders(boss).await, 1);
    s1.drain().await.unwrap();
    assert_eq!(w.reminders(boss).await, 1, "draining again sends nothing");

    // The step is still decidable after the restart; deciding withdraws the remaining timers.
    assert_eq!(w.decide(boss, "approved").await.unwrap(), "approved");
    assert_eq!(
        w.strings(TIMERS, id).await,
        ["reminder@Tue 09:30:fired", "reminder@Wed 09:30:cancelled", "escalation@Thu 09:30:cancelled"]
    );
    w.clock.advance(time::Duration::days(5));
    assert!(!s2.drain().await.unwrap().progressed());
    assert_eq!(w.reminders(boss).await, 1);
}

#[tokio::test]
async fn sys03_twenty_concurrent_decisions_on_one_step_have_exactly_one_winner() {
    const N: usize = 20;
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let mut approvers = Vec::new();
    for i in 0..N {
        let f = w.member(&format!("Finance {i}"), None).await;
        w.grant("finance.approver", f).await;
        approvers.push(f);
    }
    let input = json!({"amount": {"amount": "800.00", "currency": "EUR"}, "category": "travel",
                       "description": "Client visit", "expense_date": "2026-10-01"});
    let id = w.start(ann, "hr.expense-claim", input).await;
    w.decide(boss, "approved").await.unwrap();
    let item = w.open(approvers[0]).await.remove(0);
    assert_eq!(item.assignees.len(), N, "every approver is a candidate");
    let revision_before = w.n("SELECT revision FROM process_instances WHERE id = $1", id).await;

    let mut tasks = Vec::new();
    for (i, who) in approvers.iter().copied().enumerate() {
        let (req, db, ctx) = (w.req.clone(), w.t.db.clone(), w.ctx);
        let (item_id, rev) = (item.id, item.revision);
        tasks.push(tokio::spawn(async move {
            let mut tx = db.begin_ws(ctx).await.unwrap();
            let (outcome, comment) =
                if i % 2 == 0 { ("approved", None) } else { ("rejected", Some("Over budget")) };
            let r = req.decide(&mut tx, &who.actor(), item_id, rev, outcome, comment, &Map::new()).await;
            if r.is_ok() {
                tx.commit().await.unwrap();
            } else {
                tx.rollback().await.unwrap();
            }
            r.map(|d| d.outcome)
        }));
    }
    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.unwrap());
    }
    let winners: Vec<&String> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    for r in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(r, RequestError::AlreadyDecided | RequestError::Stale { .. }), "{r:?}");
    }

    // One decision row, one instance advance, one audit record, one outcome notice.
    assert_eq!(w.n("SELECT count(*) FROM work_items WHERE id = $1 AND state = 'completed' AND completed_by IS NOT NULL", item.id).await, 1);
    assert_eq!(w.n("SELECT revision FROM process_instances WHERE id = $1", id).await, revision_before + 1);
    let key: String = w.strings("SELECT work_key FROM work_items WHERE id = $1", item.id).await.remove(0);
    let mut tx = w.tx().await;
    let advances: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM instance_events WHERE instance_id = $1 AND event_type = 'work_item_completed' AND payload->'event'->>'key' = $2",
    )
    .bind(id)
    .bind(&key)
    .fetch_one(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(advances, 1);
    assert_eq!(
        w.n(
            "SELECT count(*) FROM audit_events WHERE action = 'work_item.decided' AND object_id = $1",
            item.id
        )
        .await,
        1
    );
    assert_eq!(
        w.n(
            "SELECT count(*) FROM notifications WHERE membership_id = $1 AND kind = 'request_completed'",
            ann.membership
        )
        .await,
        1
    );
    let status = w.strings("SELECT status FROM requests WHERE instance_id = $1", id).await.remove(0);
    let expected = if winners[0] == "approved" { "approved" } else { "rejected" };
    assert_eq!(status, expected);
}
