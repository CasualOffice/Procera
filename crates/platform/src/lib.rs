//! System plane (BPMS-020, BPMS-065). Authorization here is a fixed role → action table ([`authz`]):
//! the system plane has four built-in roles and no custom roles. Workspace-plane authorization uses
//! Cedar (ADR-0011).

pub mod authz;
mod console;
mod service;

pub use console::{ConsoleWorkspace, Migrations, PROFILES, Queues, QuotaDefaults, Usage};
pub use service::{
    NewWorkspace, Platform, PlatformError, Quotas, Workspace, WorkspaceFilter, WorkspaceState,
};
