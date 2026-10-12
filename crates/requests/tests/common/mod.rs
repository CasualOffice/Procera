//! A workspace with members, roles, groups and settings for the request tests that need more than
//! `requests.rs` sets up (workspace request settings, out of office). Skips when
//! PROCERA_TEST_DATABASE_URL is unset.
#![allow(dead_code)]

use std::sync::Arc;

use procera_crypto::LocalKey;
use procera_db::{WsCtx, WsTx, test_support};
use procera_identity::password::HashCost;
use procera_identity::{Identity, IdentityConfig};
use procera_kernel::UuidV7Gen;
use procera_kernel::clock::ManualClock;
use procera_members::Members;
use procera_notify::Notify;
use procera_requests::{Actor, RequestError, Requests, WorkState};
use serde_json::{Map, Value, json};
use time::macros::datetime;
use uuid::Uuid;

/// A member in these tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Who {
    pub membership: Uuid,
    pub subject_ref: Uuid,
}

impl Who {
    pub fn actor(self) -> Actor {
        Actor {
            membership: self.membership,
            subject_ref: self.subject_ref,
            ip: None,
            user_agent: Some("requests-test".into()),
        }
    }
}

pub struct World {
    pub t: test_support::TestDb,
    pub req: Requests,
    pub ctx: WsCtx,
    pub admin: sqlx::PgConnection,
    pub clock: Arc<ManualClock>,
}

pub async fn world() -> Option<World> {
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
        Requests::new(t.db.clone(), members, notify, clock.clone(), Arc::new(UuidV7Gen), "https://app.test")
            .unwrap();
    let admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await.unwrap();
    Some(World { t, req, ctx: WsCtx { workspace_id: Uuid::now_v7() }, admin, clock })
}

impl World {
    pub async fn tx(&self) -> WsTx<'static> {
        self.t.db.begin_ws(self.ctx).await.unwrap()
    }

    pub async fn member(&mut self, name: &str, manager: Option<Who>) -> Who {
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

    pub async fn grant(&self, role: &str, to: Who) {
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

    /// A static group with these members; returns its id.
    pub async fn group(&self, name: &str, members: &[Who]) -> Uuid {
        let mut tx = self.tx().await;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO groups (workspace_id, id, name) VALUES ($1, $2, $3)")
            .bind(self.ctx.workspace_id)
            .bind(id)
            .bind(name)
            .execute(tx.conn())
            .await
            .unwrap();
        for m in members {
            sqlx::query(
                "INSERT INTO group_members (workspace_id, group_id, membership_id) VALUES ($1, $2, $3)",
            )
            .bind(self.ctx.workspace_id)
            .bind(id)
            .bind(m.membership)
            .execute(tx.conn())
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
        id
    }

    /// Stores a workspace setting as an admin would (validated by the catalogue).
    pub async fn set(&self, key: &str, value: Value) {
        let d = procera_settings::def(key).unwrap();
        let v = procera_settings::validate(d, &value).unwrap();
        let mut tx = self.tx().await;
        sqlx::query(
            "INSERT INTO workspace_settings (workspace_id, key, value, updated_at) VALUES ($1, $2, $3, now())
             ON CONFLICT (workspace_id, key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(self.ctx.workspace_id)
        .bind(key)
        .bind(v)
        .execute(tx.conn())
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }

    pub async fn start(&self, who: Who, key: &str, input: Value) -> Result<Uuid, RequestError> {
        let mut tx = self.tx().await;
        let s = self.req.start(&mut tx, &who.actor(), key, input.as_object().unwrap()).await?;
        tx.commit().await.unwrap();
        Ok(s.id)
    }

    pub async fn offered(&self, who: Who) -> Vec<&'static str> {
        let mut tx = self.tx().await;
        let (_, types) = self.req.offered(&mut tx, who.membership).await.unwrap();
        tx.rollback().await.unwrap();
        types.iter().map(|t| t.key).collect()
    }

    pub async fn open(&self, who: Who) -> Vec<procera_requests::WorkView> {
        let mut tx = self.tx().await;
        let w = self.req.work(&mut tx, who.membership, WorkState::Open).await.unwrap();
        tx.rollback().await.unwrap();
        w
    }

    pub async fn decide(&self, who: Who, outcome: &str) -> Result<String, RequestError> {
        let item = self.open(who).await.into_iter().next().expect("an open work item");
        self.decide_item(who, item.id, item.revision, outcome).await
    }

    pub async fn decide_item(
        &self,
        who: Who,
        id: Uuid,
        rev: i64,
        outcome: &str,
    ) -> Result<String, RequestError> {
        let mut tx = self.tx().await;
        let comment = (outcome != "approved").then_some("Not this time");
        let d = self.req.decide(&mut tx, &who.actor(), id, rev, outcome, comment, &Map::new()).await?;
        tx.commit().await.unwrap();
        Ok(d.request.status)
    }

    pub async fn detail(&self, id: Uuid) -> procera_requests::RequestDetail {
        let mut tx = self.tx().await;
        let d = self.req.detail(&mut tx, id).await.unwrap();
        tx.rollback().await.unwrap();
        d
    }
}

pub fn leave() -> Value {
    json!({"leave_type": "annual", "start_date": "2026-10-20", "end_date": "2026-10-22", "reason": "Family visit"})
}

pub fn money(amount: &str, currency: &str) -> Value {
    json!({"amount": amount, "currency": currency})
}

pub fn expense(amount: &str, currency: &str) -> Value {
    json!({"amount": money(amount, currency), "category": "travel", "description": "Client visit", "expense_date": "2026-10-01"})
}

pub fn purchase(amount: &str, currency: &str) -> Value {
    json!({"amount": money(amount, currency), "vendor": "Acme", "cost_center": "CC-1", "justification": "Laptops"})
}
