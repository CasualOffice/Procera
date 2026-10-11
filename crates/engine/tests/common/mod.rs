//! Shared fixtures: a throwaway database, a manual clock, and compiled test processes.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_db::test_support::{self, TestDb};
use procera_db::{Db, WsCtx};
use procera_engine::{Engine, Executable, ProgramRef, Scheduler, StaticPrograms};
use procera_ir::{CompileContext, compile};
use procera_kernel::UuidV7Gen;
use procera_kernel::clock::ManualClock;
use procera_runtime::Program;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;
use uuid::Uuid;

/// Friday 16 Oct 2026, 09:30 UTC (15:00 in +05:30).
pub const T0: OffsetDateTime = datetime!(2026-10-16 09:30 UTC);

pub struct World {
    pub t: TestDb,
    pub clock: Arc<ManualClock>,
    pub engine: Engine,
    pub ctx: WsCtx,
    pub refs: BTreeMap<String, ProgramRef>,
}

pub fn program(doc: Value) -> Executable {
    let out = compile(&doc, &CompileContext::default());
    let compiled = out.compiled.unwrap_or_else(|| panic!("does not compile: {:?}", out.diagnostics));
    Executable::new(Program::from_compiled(&compiled).expect("program"))
}

pub async fn world(docs: Vec<Value>) -> Option<World> {
    let t = test_support::fresh().await?.expect("test database");
    let clock = Arc::new(ManualClock::new(T0));
    let mut programs = StaticPrograms::default();
    let mut refs = BTreeMap::new();
    for d in docs {
        let r = programs.insert(program(d));
        refs.insert(r.process_key.clone(), r);
    }
    let engine = Engine::new(t.db.clone(), clock.clone(), Arc::new(UuidV7Gen), Arc::new(programs));
    Some(World { t, clock, engine, ctx: WsCtx { workspace_id: Uuid::now_v7() }, refs })
}

impl World {
    pub fn db(&self) -> &Db {
        &self.t.db
    }

    pub fn scheduler(&self, owner: &str) -> Scheduler {
        Scheduler::new(self.engine.clone(), owner)
    }

    pub fn program(&self, key: &str) -> &ProgramRef {
        &self.refs[key]
    }

    pub async fn scalar_i64(&self, sql: &str, id: Uuid) -> i64 {
        let mut tx = self.db().begin_ws(self.ctx).await.expect("tx");
        let v: i64 = sqlx::query_scalar(sql).bind(id).fetch_one(tx.conn()).await.expect(sql);
        tx.rollback().await.expect("rollback");
        v
    }

    pub async fn strings(&self, sql: &str, id: Uuid) -> Vec<String> {
        let mut tx = self.db().begin_ws(self.ctx).await.expect("tx");
        let v: Vec<String> = sqlx::query_scalar(sql).bind(id).fetch_all(tx.conn()).await.expect(sql);
        tx.rollback().await.expect("rollback");
        v
    }
}

fn base(key: &str, nodes: Value, edges: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "process": { "key": key, "version": 1, "name": { "en": key } },
        "variables": [],
        "nodes": nodes,
        "edges": edges
    })
}

/// start → wait (timer `after`) → done.
pub fn timer_process(key: &str, after: &str, business: bool) -> Value {
    let mut wait = json!({ "id": "wait", "kind": "wait.timer", "after": after });
    if business {
        wait["business"] = json!(true);
    }
    let mut d = base(
        key,
        json!([{ "id": "start", "kind": "start" }, wait, { "id": "done", "kind": "end" }]),
        json!([
            { "id": "e1", "from": "start", "to": "wait" },
            { "id": "e2", "from": "wait", "to": "done", "on": "fired" }
        ]),
    );
    if business {
        d["refs"] = json!({ "calendars": { "default": "calendar:in-blr@2026.2" } });
    }
    d
}

/// start → hold (timer PT1H) → review (human task) → done, with an instance SLA of `target`
/// paused while `hold` waits.
pub fn sla_process(key: &str, target: &str, pause: bool) -> Value {
    let mut d = base(
        key,
        json!([
            { "id": "start", "kind": "start" },
            { "id": "hold", "kind": "wait.timer", "after": "PT1H" },
            { "id": "review", "kind": "human.task",
              "assignment": { "candidates": [ { "type": "role", "ref": "process-owner" } ] },
              "outcomes": ["done"] },
            { "id": "done", "kind": "end" }
        ]),
        json!([
            { "id": "e1", "from": "start", "to": "hold" },
            { "id": "e2", "from": "hold", "to": "review", "on": "fired" },
            { "id": "e3", "from": "review", "to": "done", "on": "done" }
        ]),
    );
    let mut sla = json!({ "id": "total", "scope": "instance", "target": target,
                          "breach": [ { "action": "notify", "to": "role:process-owner" } ] });
    if pause {
        sla["pauseWhile"] = json!(["wait.timer:hold"]);
    }
    d["sla"] = json!([sla]);
    d
}
