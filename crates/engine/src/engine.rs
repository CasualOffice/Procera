//! Instance persistence around the pure interpreter (docs/04 §4.2).
//!
//! `advance(instance, stimulus)` runs in **one transaction**: lock the instance row, check the
//! revision, run the interpreter, save the state, append the stimulus to `instance_events`, turn
//! the commands into durable effects (timers, jobs, outbox events, pending effects), move the SLA
//! clocks, and append the audit record. Nothing leaves the database before commit.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::{Db, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_runtime::{
    Budget, CelEvaluator, Command, Event, EventError, InstanceState, InstanceStatus, Interpreter, Program,
    Step, Stimulus,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{EngineError, effects, sla};

/// Which published program an instance runs. Until release bundles exist (BPMS-084) the key and
/// version name it, and the checksum pins the compiled IR.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProgramRef {
    pub process_key: String,
    pub version: u32,
    pub checksum: Option<String>,
}

/// A program ready to run: the compiled IR, its evaluator (rule stand-ins, time-zone offset) and
/// the step budget.
#[derive(Debug)]
pub struct Executable {
    pub program: Program,
    pub evaluator: CelEvaluator,
    pub budget: Budget,
}

impl Executable {
    pub fn new(program: Program) -> Self {
        Self { program, evaluator: CelEvaluator::default(), budget: Budget::default() }
    }

    /// The calendar reference for a business duration: a `refs.calendars` key, a reference, or
    /// the process default.
    pub fn calendar_ref(&self, name: Option<&str>) -> Option<String> {
        match name {
            Some(n) => Some(self.program.ir().refs.calendars.get(n).cloned().unwrap_or_else(|| n.to_owned())),
            None => self.program.default_calendar().map(str::to_owned),
        }
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where programs come from. BPMS-084 implements it over release bundles; [`StaticPrograms`]
/// serves a fixed set.
pub trait ProgramSource: Send + Sync + Debug {
    fn load<'a>(
        &'a self,
        ctx: WsCtx,
        program: &'a ProgramRef,
    ) -> BoxFuture<'a, Result<Arc<Executable>, String>>;
}

/// A fixed set of programs keyed by process key and version (tests, and until BPMS-084).
#[derive(Debug, Default)]
pub struct StaticPrograms(pub BTreeMap<(String, u32), Arc<Executable>>);

impl StaticPrograms {
    pub fn insert(&mut self, exe: Executable) -> ProgramRef {
        let ir = exe.program.ir();
        let r =
            ProgramRef { process_key: ir.process.key.clone(), version: ir.process.version, checksum: None };
        self.0.insert((r.process_key.clone(), r.version), Arc::new(exe));
        r
    }
}

impl StaticPrograms {
    /// Like [`StaticPrograms::insert`], but the returned reference pins the compiled checksum, so an
    /// instance started from it refuses to run on a different build of the same key and version.
    pub fn insert_pinned(&mut self, exe: Executable) -> ProgramRef {
        let checksum = exe.program.checksum().map(str::to_owned);
        ProgramRef { checksum, ..self.insert(exe) }
    }
}

impl ProgramSource for StaticPrograms {
    fn load<'a>(&'a self, _ctx: WsCtx, p: &'a ProgramRef) -> BoxFuture<'a, Result<Arc<Executable>, String>> {
        let found = self.0.get(&(p.process_key.clone(), p.version)).cloned();
        Box::pin(async move {
            let exe = found.ok_or_else(|| format!("program {}@{} not found", p.process_key, p.version))?;
            match (&p.checksum, exe.program.checksum()) {
                (Some(want), Some(have)) if want != have => Err(format!(
                    "program {}@{} checksum {have} does not match the pinned {want}",
                    p.process_key, p.version
                )),
                _ => Ok(exe),
            }
        })
    }
}

/// Who caused a stimulus, for the runtime audit record (A-28 (4)): a user's audit subject. Timer
/// and job driven advances have none and are recorded as the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    pub subject_ref: Uuid,
}

/// What an [`EffectHandler`] sees after the interpreter accepted a stimulus.
#[derive(Debug, Clone, Copy)]
pub struct EffectContext<'a> {
    pub program: &'a ProgramRef,
    pub exe: &'a Executable,
    /// The state before the stimulus; `None` when the instance just started.
    pub before: Option<&'a InstanceState>,
    pub state: &'a InstanceState,
    pub commands: &'a [Command],
    pub stimulus: &'a Stimulus,
    pub actor: Option<Actor>,
}

/// The port that consumes the commands the engine leaves `pending` (work items, notifications;
/// BPMS-085). It runs inside the advance transaction, after the engine's own effects, so a work
/// item exists exactly when the state that waits for it commits.
pub trait EffectHandler: Send + Sync + Debug {
    fn apply<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        cx: EffectContext<'a>,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// The result of an accepted stimulus.
#[derive(Debug, Clone, PartialEq)]
pub struct Advanced {
    pub instance_id: Uuid,
    pub revision: u64,
    pub status: InstanceStatus,
    pub commands: Vec<Command>,
}

/// A stored instance.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceRecord {
    pub program: ProgramRef,
    pub status: String,
    pub revision: i64,
    pub state: InstanceState,
}

/// The stored status column (docs/04 §2.5) for a runtime status. `terminated` is `cancelled`
/// (A-23 (9)); `waiting` is `running` with the derived `waiting` flag.
pub fn stored_status(s: InstanceStatus) -> &'static str {
    match s {
        InstanceStatus::Running | InstanceStatus::Waiting => "running",
        InstanceStatus::Completed => "completed",
        InstanceStatus::Terminated => "cancelled",
        InstanceStatus::Failed => "failed",
    }
}

fn event_type(e: &Event) -> &'static str {
    match e {
        Event::Start { .. } => "start",
        Event::WorkItemCompleted { .. } => "work_item_completed",
        Event::TimerFired { .. } => "timer_fired",
        Event::SignalReceived { .. } => "signal_received",
        Event::ServiceCompleted { .. } => "service_completed",
        Event::SubprocessFinished { .. } => "subprocess_finished",
        Event::CaseClosed { .. } => "case_closed",
        Event::Cancel { .. } => "cancel",
        Event::Continue => "continue",
    }
}

#[derive(Clone)]
pub struct Engine {
    pub(crate) db: Db,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdGen>,
    pub(crate) programs: Arc<dyn ProgramSource>,
    pub(crate) effects: Option<Arc<dyn EffectHandler>>,
}

impl Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    pub fn new(db: Db, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>, programs: Arc<dyn ProgramSource>) -> Self {
        Self { db, clock, ids, programs, effects: None }
    }

    /// Installs the consumer of pending commands (work items, notifications).
    pub fn with_effects(mut self, handler: Arc<dyn EffectHandler>) -> Self {
        self.effects = Some(handler);
        self
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn ids(&self) -> &Arc<dyn IdGen> {
        &self.ids
    }

    /// Stamps an event with the engine clock.
    pub fn stamp(&self, event: Event) -> Stimulus {
        Stimulus::stamp(self.clock.as_ref(), event)
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    async fn executable(&self, ctx: WsCtx, program: &ProgramRef) -> Result<Arc<Executable>, EngineError> {
        self.programs.load(ctx, program).await.map_err(EngineError::Program)
    }

    /// Starts an instance. The id comes from the injected `IdGen`, the time from the `Clock`.
    pub async fn start(
        &self,
        ctx: WsCtx,
        program: &ProgramRef,
        initiator: Value,
        variables: BTreeMap<String, Value>,
    ) -> Result<Advanced, EngineError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let out = self.start_in(&mut tx, program, initiator, variables, None).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// [`Engine::start`] inside the caller's transaction, recording `actor` as the user who started it.
    pub async fn start_in(
        &self,
        tx: &mut WsTx<'_>,
        program: &ProgramRef,
        initiator: Value,
        variables: BTreeMap<String, Value>,
        actor: Option<Actor>,
    ) -> Result<Advanced, EngineError> {
        let ctx = tx.ctx;
        let exe = self.executable(ctx, program).await?;
        // Restricted values need envelope encryption at rest, which is not built yet (A-28).
        if let Some(v) = exe.program.ir().variables.iter().find(|v| v.is_restricted()) {
            return Err(EngineError::Restricted(v.name.clone()));
        }
        let stimulus = Stimulus::start(self.clock.as_ref(), self.ids.as_ref(), initiator, variables);
        let step = Interpreter::new(&exe.program, &exe.evaluator)
            .with_budget(exe.budget)
            .start(&stimulus)
            .map_err(EngineError::Rejected)?;
        let id = step.state.instance_id;
        let now = self.clock.now();
        sqlx::query(
            "INSERT INTO process_instances (workspace_id, id, display_id, process_key, process_version, program_checksum,
                                            status, waiting, revision, state, started_at, updated_at, ended_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(ctx.workspace_id)
        .bind(id)
        .bind(id.to_string())
        .bind(&program.process_key)
        .bind(i32::try_from(program.version).map_err(|_| EngineError::Program("version out of range".into()))?)
        .bind(&program.checksum)
        .bind(stored_status(step.state.status))
        .bind(step.state.status == InstanceStatus::Waiting)
        .bind(rev(step.state.revision)?)
        .bind(serde_json::to_value(&step.state)?)
        .bind(step.state.started_at)
        .bind(now)
        .bind(step.state.ended_at)
        .execute(tx.conn())
        .await?;
        self.persist(tx, program, &exe, None, step, &stimulus, actor).await
    }

    /// Applies an event to an instance, stamped now. `expected_revision` is the optimistic check
    /// (`If-Match`); a mismatch is [`EngineError::Conflict`].
    pub async fn advance(
        &self,
        ctx: WsCtx,
        instance_id: Uuid,
        event: Event,
        expected_revision: Option<u64>,
    ) -> Result<Advanced, EngineError> {
        let stimulus = Stimulus::stamp(self.clock.as_ref(), event);
        let mut tx = self.db.begin_ws(ctx).await?;
        let out = self.apply_in(&mut tx, instance_id, &stimulus, expected_revision).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// [`Engine::advance`] inside the caller's transaction (the job runner commits it together
    /// with the fenced job completion).
    pub async fn apply_in(
        &self,
        tx: &mut WsTx<'_>,
        instance_id: Uuid,
        stimulus: &Stimulus,
        expected_revision: Option<u64>,
    ) -> Result<Advanced, EngineError> {
        self.apply_in_as(tx, instance_id, stimulus, expected_revision, None).await
    }

    /// [`Engine::apply_in`] recording `actor` as the user who caused the stimulus (A-28 (4)).
    pub async fn apply_in_as(
        &self,
        tx: &mut WsTx<'_>,
        instance_id: Uuid,
        stimulus: &Stimulus,
        expected_revision: Option<u64>,
        actor: Option<Actor>,
    ) -> Result<Advanced, EngineError> {
        let rec = load(tx, instance_id, true).await?;
        let current = u64::try_from(rec.revision).unwrap_or_default();
        if let Some(expected) = expected_revision
            && expected != current
        {
            return Err(EngineError::Conflict { expected, actual: current });
        }
        let exe = self.executable(tx.ctx, &rec.program).await?;
        let step = Interpreter::new(&exe.program, &exe.evaluator)
            .with_budget(exe.budget)
            .handle(&rec.state, stimulus)
            .map_err(EngineError::Rejected)?;
        let updated = sqlx::query(
            "UPDATE process_instances SET status = $3, waiting = $4, revision = $5, state = $6, updated_at = $7, ended_at = $8
             WHERE id = $1 AND revision = $2",
        )
        .bind(instance_id)
        .bind(rec.revision)
        .bind(stored_status(step.state.status))
        .bind(step.state.status == InstanceStatus::Waiting)
        .bind(rev(step.state.revision)?)
        .bind(serde_json::to_value(&step.state)?)
        .bind(self.clock.now())
        .bind(step.state.ended_at)
        .execute(tx.conn())
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(EngineError::Conflict { expected: current, actual: current + 1 });
        }
        self.persist(tx, &rec.program, &exe, Some(&rec.state), step, stimulus, actor).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist(
        &self,
        tx: &mut WsTx<'_>,
        program: &ProgramRef,
        exe: &Executable,
        before: Option<&InstanceState>,
        step: Step,
        stimulus: &Stimulus,
        actor: Option<Actor>,
    ) -> Result<Advanced, EngineError> {
        let st = &step.state;
        let kind = event_type(&stimulus.event);
        sqlx::query(
            "INSERT INTO instance_events (workspace_id, instance_id, seq, event_type, payload, happened_at, recorded_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(st.instance_id)
        .bind(rev(st.revision)?)
        .bind(kind)
        .bind(serde_json::to_value(stimulus)?)
        .bind(stimulus.at)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        effects::apply(self, tx, exe, st, &step.commands).await?;
        if let Some(h) = &self.effects {
            let cx =
                EffectContext { program, exe, before, state: st, commands: &step.commands, stimulus, actor };
            h.apply(tx, cx).await.map_err(EngineError::Effects)?;
        }
        sla::sync(self, tx, exe, before, st, stimulus.at).await?;
        let action = match (before, st.status) {
            (None, _) => "instance.started",
            (Some(_), InstanceStatus::Completed) => "instance.completed",
            (Some(_), InstanceStatus::Terminated) => "instance.cancelled",
            (Some(_), InstanceStatus::Failed) => "instance.failed",
            (Some(_), _) => "instance.advanced",
        };
        let (who, kind) = match actor {
            Some(a) => (Some(a.subject_ref), ActorKind::User),
            None => (None, ActorKind::System),
        };
        let mut ev = AuditEvent::new(who, kind, Category::Runtime, action, Outcome::Allowed)
            .object("process_instance", st.instance_id);
        ev.diff = Some(json!({ "event": kind, "revision": st.revision, "status": stored_status(st.status) }));
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(Advanced {
            instance_id: st.instance_id,
            revision: st.revision,
            status: st.status,
            commands: step.commands,
        })
    }

    /// The stored instance.
    pub async fn instance(&self, ctx: WsCtx, instance_id: Uuid) -> Result<InstanceRecord, EngineError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let rec = load(&mut tx, instance_id, false).await?;
        tx.rollback().await?;
        Ok(rec)
    }

    /// Replays the stimulus log from the start and returns the replayed state. It equals the
    /// stored state for every instance (the replay invariant, docs/11 §15).
    pub async fn replay(&self, ctx: WsCtx, instance_id: Uuid) -> Result<InstanceState, EngineError> {
        let mut tx = self.db.begin_ws(ctx).await?;
        let rec = load(&mut tx, instance_id, false).await?;
        let rows: Vec<(Value,)> =
            sqlx::query_as("SELECT payload FROM instance_events WHERE instance_id = $1 ORDER BY seq")
                .bind(instance_id)
                .fetch_all(tx.conn())
                .await?;
        tx.rollback().await?;
        let stimuli =
            rows.into_iter().map(|(v,)| serde_json::from_value(v)).collect::<Result<Vec<Stimulus>, _>>()?;
        let exe = self.executable(ctx, &rec.program).await?;
        let step = Interpreter::new(&exe.program, &exe.evaluator)
            .with_budget(exe.budget)
            .replay(&stimuli)
            .map_err(|e| EngineError::Replay(e.to_string()))?;
        Ok(step.state)
    }
}

fn rev(r: u64) -> Result<i64, EngineError> {
    i64::try_from(r).map_err(|_| EngineError::State("revision out of range".into()))
}

type InstanceRow = (String, i32, Option<String>, String, i64, Value);

async fn load(tx: &mut WsTx<'_>, id: Uuid, lock: bool) -> Result<InstanceRecord, EngineError> {
    let sql = if lock {
        "SELECT process_key, process_version, program_checksum, status, revision, state FROM process_instances WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT process_key, process_version, program_checksum, status, revision, state FROM process_instances WHERE id = $1"
    };
    let row: Option<InstanceRow> = sqlx::query_as(sql).bind(id).fetch_optional(tx.conn()).await?;
    let (process_key, version, checksum, status, revision, state) = row.ok_or(EngineError::NotFound(id))?;
    Ok(InstanceRecord {
        program: ProgramRef { process_key, version: u32::try_from(version).unwrap_or_default(), checksum },
        status,
        revision,
        state: serde_json::from_value(state)?,
    })
}

impl EngineError {
    /// A stale or duplicate stimulus: the caller treats it as a no-op (docs/04 §4.2).
    pub fn is_rejection(&self) -> bool {
        matches!(self, Self::Rejected(_))
    }

    pub fn rejection(&self) -> Option<&EventError> {
        match self {
            Self::Rejected(e) => Some(e),
            _ => None,
        }
    }
}
