//! Procera native interpreter (BPMS-082, docs/04 §4, docs/11 §15).
//!
//! This slice holds the expression port, the executable program and stimuli. The instance
//! state, the command vocabulary and the advance loop follow.

mod event;
mod port;
mod program;

pub use event::{Event, Stimulus};
pub use port::{EvalError, Evaluator, Scope};
pub use program::{Program, ProgramError};
