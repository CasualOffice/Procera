//! Procera native interpreter (BPMS-082, docs/04 §4, docs/11 §15).
//!
//! The interpreter is a pure function over a compiled IR:
//!
//! ```text
//! (Program, InstanceState, Stimulus) ──► (InstanceState', [Command])
//! ```

mod command;
mod event;
mod interp;
mod port;
mod program;
mod state;

pub use command::{Command, InstanceResult, TimerDue, TimerPurpose, WorkKind};
pub use event::{Event, Stimulus};
pub use interp::{Budget, EventError, Interpreter, ReplayError, Step};
pub use port::{EvalError, Evaluator, Scope};
pub use program::{Program, ProgramError};
pub use state::{
    Frame, Incident, IncidentCode, InstanceState, InstanceStatus, Split, SplitId, Token, TokenId, TokenState,
    TraceEntry, TraceEvent, Wait, WaitKind,
};
