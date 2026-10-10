//! System plane (BPMS-020). Authorization here is a fixed role → action table ([`authz`]): the system
//! plane has four built-in roles and no custom roles. Workspace-plane authorization uses Cedar (ADR-0011).

pub mod authz;
mod service;

pub use service::{NewWorkspace, Platform, PlatformError, Quotas, Workspace, WorkspaceState};
