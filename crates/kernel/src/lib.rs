//! Procera kernel: the smallest shared vocabulary for every other crate.
//!
//! Deliberately dependency-light. Domain crates depend only on this crate
//! (docs/04-LLD.md §1, docs/24 §3.1).

pub mod clock;
pub mod id;
pub mod problem;

pub use clock::{Clock, SystemClock};
pub use id::{IdGen, UuidV7Gen};
pub use problem::Problem;
