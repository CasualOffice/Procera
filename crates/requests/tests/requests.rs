//! BPMS-085/101/103 against real PostgreSQL: start → work item → decision → completion with
//! notifications; a second approval by role above the threshold; SoD (the initiator and earlier
//! deciders are never candidates, and are refused at decision time); the send-back loop; cancel;
//! a decision race with exactly one winner; fallback and unassigned work items.
//! Skips when PROCERA_TEST_DATABASE_URL is unset.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::UuidV7Gen;
use procera_kernel::clock::ManualClock;
use procera_members::Members;
use procera_notify::Notify;
use procera_requests::{Actor, RequestError, Requests, Scope, WorkState};
use serde_json::{Map, Value, json};
use time::macros::datetime;
use uuid::Uuid;

struct World {
    t: test_support::TestDb,
    req: Requests,
    ctx: WsCtx,
    admin: sqlx::PgConnection,
}

async fn world() -> Option<World> {
    let t = test_support::fresh().await?.expect("test database");
    // Monday 12 Oct 2026, 09:30 UTC: inside the default calendar's working hours.
    let clock = Arc::new(ManualClock::new(datetime!(2026-10-12 09:30 UTC)));
    let keys = Arc::new(LocalKey::from_base64(&LocalKey::generate_base64()).unwrap());
    let identity = Identity::new(
        t.db.clone(),
        keys.clone(),
        clock.clone(),
        Arc::new(UuidV7Gen),
        IdentityConfig { cost: HashCost::FAST_FOR_TESTS, ..IdentityConfig::default() },
    )
    .unwrap();
    let members = Members::new(
        t.db.clone(),
        identity,
        procera_access::Access::new(),
        keys.clone(),
        clock.clone(),
        Arc::new(UuidV7Gen),
    );
    let notify = Notify::new(t.db.clone(), keys, clock.clone(), Arc::new(UuidV7Gen));
    let req =
        Requests::new(t.db.clone(), members, notify, clock, Arc::new(UuidV7Gen), "https://app.test").unwrap();
    let admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await.unwrap();
    Some(World { t, req, ctx: WsCtx { workspace_id: Uuid::now_v7() }, admin })
}

impl World {
    async fn tx(&self) -> WsTx<'static> {
        self.t.db.begin_ws(self.ctx).await.unwrap()
    }

    async fn member(&mut self, name: &str, manager: Option<Actor>) -> Actor {
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
        Actor { membership: m, subject_ref: Uuid::new_v4() }
    }

    async fn grant(&self, role: &str, to: Actor) {
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

    async fn start(&self, who: Actor, key: &str, input: Value) -> Result<Uuid, RequestError> {
        let mut tx = self.tx().await;
        let s = self.req.start(&mut tx, who, key, input.as_object().unwrap()).await?;
        tx.commit().await.unwrap();
        Ok(s.id)
    }

    async fn open(&self, who: Actor) -> Vec<procera_requests::WorkView> {
        let mut tx = self.tx().await;
        let w = self.req.work(&mut tx, who.membership, WorkState::Open).await.unwrap();
        tx.rollback().await.unwrap();
        w
    }

    async fn decide(
        &self,
        who: Actor,
        outcome: &str,
        comment: Option<&str>,
        fields: Value,
    ) -> Result<String, RequestError> {
        let item = self.open(who).await.into_iter().next().expect("an open work item");
        self.decide_item(who, item.id, item.revision, outcome, comment, fields).await
    }

    async fn decide_item(
        &self,
        who: Actor,
        id: Uuid,
        rev: i64,
        outcome: &str,
        comment: Option<&str>,
        fields: Value,
    ) -> Result<String, RequestError> {
        let mut tx = self.tx().await;
        let fields: Map<String, Value> = fields.as_object().cloned().unwrap_or_default();
        let d = self.req.decide(&mut tx, who, id, rev, outcome, comment, &fields).await?;
        tx.commit().await.unwrap();
        Ok(d.request.status)
    }

    async fn detail(&self, id: Uuid) -> procera_requests::RequestDetail {
        let mut tx = self.tx().await;
        let d = self.req.detail(&mut tx, id).await.unwrap();
        tx.rollback().await.unwrap();
        d
    }

    async fn notes(&self, who: Actor, kind: &str) -> i64 {
        let mut tx = self.tx().await;
        let n =
            sqlx::query_scalar("SELECT count(*) FROM notifications WHERE membership_id = $1 AND kind = $2")
                .bind(who.membership)
                .bind(kind)
                .fetch_one(tx.conn())
                .await
                .unwrap();
        tx.rollback().await.unwrap();
        n
    }
}

fn leave() -> Value {
    json!({"leave_type": "annual", "start_date": "2026-10-20", "end_date": "2026-10-22", "reason": "Family visit"})
}

fn expense(amount: &str) -> Value {
    json!({"amount": {"amount": amount, "currency": "EUR"}, "category": "travel", "description": "Client visit", "expense_date": "2026-10-01"})
}

#[tokio::test]
async fn leave_request_is_approved_by_the_manager_and_the_requester_is_told() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();

    let d = w.detail(id).await;
    assert!(d.summary.display_id.starts_with("LR-2026-"), "{}", d.summary.display_id);
    assert_eq!(d.summary.title, "Annual leave, 2026-10-20 to 2026-10-22");
    assert_eq!(
        (d.summary.status.as_str(), d.summary.current_step.as_deref()),
        ("in_progress", Some("Manager approval"))
    );
    assert_eq!(d.summary.assignees, ["Boss"]);
    // Due in two business days on the default calendar (Mon 09:30 → Wed 09:30).
    assert_eq!(d.steps[0].due_at, Some(datetime!(2026-10-14 09:30 UTC)));
    assert!(d.sla.is_some());
    assert!(w.open(ann).await.is_empty(), "the requester is never offered her own request");
    assert_eq!(w.notes(boss, "work_assigned").await, 1);

    assert_eq!(w.decide(boss, "approved", None, json!({})).await.unwrap(), "approved");
    let d = w.detail(id).await;
    assert_eq!(d.summary.status, "approved");
    assert_eq!(d.steps[0].decided_by.as_deref(), Some("Boss"));
    assert!(d.timeline.iter().any(|t| t.kind == "completed"));
    assert_eq!(w.notes(ann, "request_completed").await, 1);
    assert_eq!(
        w.req.engine().replay(w.ctx, id).await.unwrap(),
        w.req.engine().instance(w.ctx, id).await.unwrap().state
    );

    let mut tx = w.tx().await;
    let audited: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, actor_kind FROM audit_events WHERE object_id = $1 OR object_id IN (SELECT id FROM work_items WHERE instance_id = $1) ORDER BY seq",
    )
    .bind(id)
    .fetch_all(tx.conn())
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    let audited: Vec<(&str, &str)> = audited.iter().map(|(a, k)| (a.as_str(), k.as_str())).collect();
    assert_eq!(
        audited,
        [
            ("instance.started", "user"),
            ("request.started", "user"),
            ("instance.completed", "user"),
            ("work_item.decided", "user")
        ]
    );
    let mut tx = w.tx().await;
    let mine = w.req.list(&mut tx, Scope::Mine(ann.membership), None).await.unwrap();
    assert_eq!(mine.len(), 1);
    assert!(w.req.list(&mut tx, Scope::Mine(boss.membership), None).await.unwrap().is_empty());
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn invalid_input_is_refused_with_every_field_listed() {
    let Some(mut w) = world().await else { return };
    let ann = w.member("Ann", None).await;
    let bad = json!({"leave_type": "annual", "start_date": "2026-10-22", "end_date": "2026-10-20", "colour": "red"});
    match w.start(ann, "hr.leave-request", bad).await {
        Err(RequestError::Invalid(e)) => assert_eq!(e[0].pointer, "/input/colour"),
        other => panic!("{other:?}"),
    }
    let dates = json!({"leave_type": "annual", "start_date": "2026-10-22", "end_date": "2026-10-20"});
    match w.start(ann, "hr.leave-request", dates).await {
        Err(RequestError::Invalid(e)) => {
            assert_eq!((e[0].pointer.as_str(), e[0].code.as_str()), ("/input/end_date", "before_start"))
        }
        other => panic!("{other:?}"),
    }
    let stranger = json!({"system": "ERP", "access_role": "viewer", "sensitivity": "standard",
                          "beneficiary": Uuid::now_v7().to_string(), "justification": "Reports"});
    match w.start(ann, "it.access-request", stranger).await {
        Err(RequestError::Invalid(e)) => assert_eq!(e[0].code, "unknown_member"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(w.start(ann, "hr.nope", json!({})).await, Err(RequestError::UnknownType(_))));
}

#[tokio::test]
async fn expenses_above_the_threshold_need_finance_and_sod_holds() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let fin = w.member("Fiona", None).await;
    // Boss and Ann also hold the finance role: SoD keeps them off the finance step.
    for m in [boss, ann, fin] {
        w.grant("finance.approver", m).await;
    }
    let small = w.start(ann, "hr.expense-claim", expense("120.00")).await.unwrap();
    assert_eq!(w.decide(boss, "approved", None, json!({})).await.unwrap(), "approved");
    assert_eq!(w.detail(small).await.steps.len(), 1);

    let big = w.start(ann, "hr.expense-claim", expense("900.00")).await.unwrap();
    assert_eq!(w.decide(boss, "approved", None, json!({})).await.unwrap(), "in_progress");
    let d = w.detail(big).await;
    assert_eq!(d.summary.current_step.as_deref(), Some("Finance approval"));
    assert_eq!(d.summary.assignees, ["Fiona"], "initiator and the manager who approved are excluded");
    assert_eq!(w.notes(ann, "request_decided").await, 1, "told about the intermediate approval");
    assert!(w.open(boss).await.is_empty() && w.open(ann).await.is_empty());
    assert_eq!(w.decide(fin, "approved", None, json!({})).await.unwrap(), "approved");

    // Not a candidate: an uninvolved member sees nothing; the requester is refused.
    let third = w.start(ann, "hr.expense-claim", expense("700.00")).await.unwrap();
    let item = w.open(boss).await.remove(0);
    let outsider = w.member("Olaf", None).await;
    assert!(matches!(
        w.decide_item(outsider, item.id, item.revision, "approved", None, json!({})).await,
        Err(RequestError::NotFound)
    ));
    assert!(matches!(
        w.decide_item(ann, item.id, item.revision, "approved", None, json!({})).await,
        Err(RequestError::Forbidden(_))
    ));
    assert!(matches!(
        w.decide_item(boss, item.id, item.revision + 1, "approved", None, json!({})).await,
        Err(RequestError::Stale { .. })
    ));
    assert_eq!(w.detail(third).await.summary.status, "in_progress");
}

#[tokio::test]
async fn sent_back_requests_return_to_the_requester_and_then_the_manager() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    assert!(matches!(
        w.decide(boss, "changes_requested", None, json!({})).await,
        Err(RequestError::Invalid(_))
    ));
    assert_eq!(
        w.decide(boss, "changes_requested", Some("Pick other dates"), json!({})).await.unwrap(),
        "changes_requested"
    );
    assert_eq!(w.notes(ann, "request_decided").await, 1);
    let revise = w.open(ann).await;
    assert_eq!((revise.len(), revise[0].step.as_str()), (1, "Revise request"));
    assert_eq!(revise[0].values["end_date"], "2026-10-22", "the form is prefilled");
    assert!(matches!(
        w.decide(ann, "submitted", None, json!({"end_date": "2026-10-23", "bogus": 1})).await,
        Err(RequestError::Invalid(_))
    ));
    assert_eq!(
        w.decide(ann, "submitted", None, json!({"end_date": "2026-10-23"})).await.unwrap(),
        "in_progress"
    );
    let d = w.detail(id).await;
    assert_eq!(d.input["end_date"], "2026-10-23");
    assert_eq!(d.summary.current_step.as_deref(), Some("Manager approval"));
    assert_eq!(d.steps.len(), 3);
    assert_eq!(w.open(boss).await.len(), 1);
}

#[tokio::test]
async fn cancelling_closes_the_open_steps() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let id = w.start(ann, "hr.leave-request", leave()).await.unwrap();
    let mut tx = w.tx().await;
    assert_eq!(w.req.cancel(&mut tx, ann, id, "Plans changed").await.unwrap().status, "cancelled");
    tx.commit().await.unwrap();
    assert!(w.open(boss).await.is_empty());
    let d = w.detail(id).await;
    assert_eq!(d.steps[0].state, "cancelled");
    assert!(d.timeline.iter().any(|t| t.kind == "cancelled"));
    let mut tx = w.tx().await;
    assert!(matches!(w.req.cancel(&mut tx, ann, id, "again").await, Err(RequestError::Conflict(_))));
}

#[tokio::test]
async fn concurrent_decisions_have_exactly_one_winner() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let ann = w.member("Ann", Some(boss)).await;
    let f1 = w.member("Fiona", None).await;
    let f2 = w.member("Frank", None).await;
    w.grant("finance.approver", f1).await;
    w.grant("finance.approver", f2).await;
    let id = w.start(ann, "hr.expense-claim", expense("800")).await.unwrap();
    w.decide(boss, "approved", None, json!({})).await.unwrap();
    let item = w.open(f1).await.remove(0);
    let (a, b) = tokio::join!(
        w.decide_item(f1, item.id, item.revision, "approved", None, json!({})),
        w.decide_item(f2, item.id, item.revision, "rejected", Some("No"), json!({}))
    );
    let wins = [&a, &b].iter().filter(|r| r.is_ok()).count();
    assert_eq!(wins, 1, "{a:?} {b:?}");
    assert!([&a, &b].iter().any(|r| matches!(r, Err(RequestError::AlreadyDecided))));
    let d = w.detail(id).await;
    assert!(matches!(d.summary.status.as_str(), "approved" | "rejected"));
    assert_eq!(d.steps.iter().filter(|s| s.state == "completed").count(), 2);
}

#[tokio::test]
async fn nobody_to_ask_falls_back_then_stays_visible_unassigned() {
    let Some(mut w) = world().await else { return };
    let carl = w.member("Carl", None).await;
    let id = w.start(carl, "hr.leave-request", leave()).await.unwrap();
    let d = w.detail(id).await;
    assert!(d.steps[0].unassigned && d.steps[0].assignees.is_empty());
    let mut tx = w.tx().await;
    let unassigned = w.req.work(&mut tx, carl.membership, WorkState::Unassigned).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(unassigned.len(), 1);

    let olivia = w.member("Olivia", None).await;
    w.grant("process.owner", olivia).await;
    w.grant("process.owner", carl).await;
    let id = w.start(carl, "hr.leave-request", leave()).await.unwrap();
    let d = w.detail(id).await;
    assert_eq!(d.steps[0].assignees, ["Olivia"], "the fallback, without the initiator");
    assert!(!d.steps[0].unassigned);
}

#[tokio::test]
async fn access_requests_route_by_sensitivity_to_groups_and_notify_the_beneficiary() {
    let Some(mut w) = world().await else { return };
    let boss = w.member("Boss", None).await;
    let bea = w.member("Bea", Some(boss)).await;
    let ann = w.member("Ann", None).await;
    let it = w.member("Ivan", None).await;
    let mut tx = w.tx().await;
    let g = Uuid::now_v7();
    sqlx::query("INSERT INTO groups (workspace_id, id, name) VALUES ($1, $2, 'IT')")
        .bind(w.ctx.workspace_id)
        .bind(g)
        .execute(tx.conn())
        .await
        .unwrap();
    sqlx::query("INSERT INTO group_members (workspace_id, group_id, membership_id) VALUES ($1, $2, $3)")
        .bind(w.ctx.workspace_id)
        .bind(g)
        .bind(it.membership)
        .execute(tx.conn())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let input = json!({"system": "ERP", "access_role": "Viewer", "sensitivity": "standard",
                       "beneficiary": bea.membership.to_string(), "justification": "Monthly reports"});
    let id = w.start(ann, "it.access-request", input).await.unwrap();
    assert_eq!(w.detail(id).await.summary.assignees, ["Boss"], "the beneficiary's manager");
    assert_eq!(w.detail(id).await.input["beneficiary"]["name"], "Bea");
    w.decide(boss, "approved", None, json!({})).await.unwrap();
    assert_eq!(w.decide(it, "submitted", None, json!({"ticket_ref": "IT-42"})).await.unwrap(), "approved");
    assert_eq!(w.notes(bea, "request_notice").await, 1);
    assert_eq!(w.notes(ann, "request_completed").await, 1);
}
