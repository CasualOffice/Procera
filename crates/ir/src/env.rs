//! The compiler's view of the workspace (docs/11 §11 checks 8-10).
//!
//! Whether a reference is published, a connector enabled or a group exists depends on the
//! workspace, which the compiler must not read itself (no I/O, docs/24 §3.6). The caller passes an
//! [`Environment`] answered from a snapshot it loaded beforehand: the publish integration
//! (BPMS-084) backs it with the library, connector registry and directory. Without one, those
//! checks are skipped and the compile reports `IR-I001` so nobody mistakes a draft check for a
//! publish check.

use crate::model::CandidateType;

/// Outcome of resolving an artifact reference such as `form:purchase-request@4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// A published version the author may use.
    Published,
    /// No such artifact or version.
    NotFound,
    /// The version exists but is not published (draft, in review, retired).
    NotPublished,
    /// Published, but the author may not use it.
    NotPermitted,
}

/// Read-only workspace snapshot for the environment-dependent checks.
pub trait Environment {
    /// Resolves an artifact reference (`kind:key@version`) from `refs`.
    fn resolve(&self, reference: &str) -> Resolution;
    /// Whether the connector is enabled for the workspace and its scopes are granted.
    fn connector_usable(&self, reference: &str) -> bool;
    /// Whether a named user, group, role or position exists. `directory_rule` is never asked.
    fn principal_exists(&self, ty: CandidateType, name: &str) -> bool;
}
