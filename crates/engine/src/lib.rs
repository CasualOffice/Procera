//! Procera runtime engine (BPMS-083, docs/04 §4.2-§4.4): everything around the pure interpreter
//! that needs a database or a clock.
//!
//! - **Calendars** ([`calendar`]): business-duration arithmetic over a calendar version.
//! - **Leases** ([`lease`]): the claim, fence, retry and dead-letter rules for jobs.

pub mod calendar;
pub mod lease;

pub use calendar::{BusinessDuration, Calendar, CalendarError, CalendarSpec};
pub use lease::{Lease, Stale};
