//! Requests and approvals (Procera 0.2; BPMS-085 work items, BPMS-101 forms subset, BPMS-103
//! request API; docs/04 §2.6, §4.6, docs/08 "Requests"/"Work", docs/12 §10).
//!
//! - Request types are built in ([`catalogue`]): compiled at boot, pinned by checksum.
//! - Start and step forms are validated server-side ([`forms`]).

pub mod catalogue;
pub mod forms;
pub mod handler;
pub mod resolve;

pub use catalogue::{Catalogue, Field, Form, RequestType};
pub use forms::FieldError;

/// The calendar every built-in names (`refs.calendars.default`), seeded per workspace on first use.
pub const DEFAULT_CALENDAR: &str = "calendar:default@1";
