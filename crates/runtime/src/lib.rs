//! Procera native interpreter (BPMS-082, docs/04 §4, docs/11 §15).
//!
//! The interpreter is a pure function over a compiled IR:
//!
//! ```text
//! (Program, InstanceState, Stimulus) ──► (InstanceState', [Command])
//! ```
//!
//! - **Tokens** move through the graph until every live token waits or ends (the advance loop).
//! - **Waits** (`human.*`, timers, signals, connector calls, subprocesses, cases) park a token and
//!   emit a [`Command`]; the completion [`Event`] names the command's key and resumes the token.
//! - **Effects are data.** Work items, timers, connector calls, notifications and domain events
//!   are returned as commands with deterministic idempotency keys. Nothing here does I/O.
//! - **Deterministic.** Time comes from the stimulus (stamped once by the injected
//!   [`procera_kernel::Clock`]); the instance id from the injected [`procera_kernel::IdGen`] at
//!   start; every other id from the state's own counter. Replaying recorded stimuli reproduces
//!   the state and trace exactly.
//! - **Budgeted.** Each advance runs at most [`Budget::max_steps`] node executions and yields with
//!   [`Command::ContinueAdvance`]; a node entered too often raises incident `IR-R004`.
//!
//! Expressions are evaluated through the [`Evaluator`] port. [`CelEvaluator`] is the production
//! implementation (procera-expr); it evaluates the compiler's checked forms without parsing.
//!
//! ```
//! use procera_ir::{CompileContext, compile};
//! use procera_runtime::{CelEvaluator, Event, Interpreter, Program, Stimulus};
//! let source = serde_json::json!({
//!     "schemaVersion": 1,
//!     "process": { "key": "hello", "version": 1, "name": { "en": "Hello" } },
//!     "variables": [],
//!     "nodes": [ { "id": "start", "kind": "start" }, { "id": "done", "kind": "end" } ],
//!     "edges": [ { "id": "e1", "from": "start", "to": "done" } ]
//! });
//! let compiled = compile(&source, &CompileContext::default()).compiled.unwrap();
//! let program = Program::from_compiled(&compiled).unwrap();
//! let eval = CelEvaluator::default();
//! let start = Stimulus {
//!     at: time::macros::datetime!(2026-10-11 09:00 UTC),
//!     event: Event::Start { instance_id: uuid::Uuid::nil(), initiator: serde_json::json!({}), variables: Default::default() },
//! };
//! let step = Interpreter::new(&program, &eval).start(&start).unwrap();
//! assert_eq!(step.state.status, procera_runtime::InstanceStatus::Completed);
//! ```

mod cel;
mod command;
mod event;
mod interp;
mod port;
mod program;
mod state;
pub mod testkit;

pub use cel::CelEvaluator;
pub use command::{Command, InstanceResult, TimerDue, TimerPurpose, WorkKind};
pub use event::{Event, Stimulus};
pub use interp::{Budget, EventError, Interpreter, ReplayError, Step};
pub use port::{EvalError, Evaluator, Scope};
pub use procera_expr::Budget as ExprBudget;
pub use program::{Program, ProgramError};
pub use state::{
    Frame, Incident, IncidentCode, InstanceState, InstanceStatus, Split, SplitId, Token, TokenId, TokenState,
    TraceEntry, TraceEvent, Wait, WaitKind,
};
