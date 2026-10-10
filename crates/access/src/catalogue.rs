//! The permission catalogue and built-in role templates (docs/12 §5-6). Fixed per product version.

/// Every permission a role may contain. Roles are subsets of this list.
pub const PERMISSIONS: &[&str] = &[
    "workspace:settings.view",
    "workspace:settings.manage",
    "workspace:branding.manage",
    "workspace:billing.view",
    "workspace:export",
    "workspace:delete.request",
    "member:view",
    "member:invite",
    "member:deactivate",
    "member:manage_roles",
    "role:view",
    "role:manage",
    "directory:view",
    "directory:manage",
    "collection:view",
    "collection:manage",
    "process:view",
    "process:create",
    "process:edit_draft",
    "process:submit_review",
    "process:review",
    "process:approve",
    "process:publish",
    "process:retire",
    "process:manage_owners",
    "process:view_analytics",
    "process:export",
    "sop:view",
    "sop:create",
    "sop:edit_draft",
    "sop:review",
    "sop:approve",
    "sop:publish",
    "sop:acknowledge",
    "sop:export",
    "policy:view",
    "policy:create",
    "policy:edit_draft",
    "policy:review",
    "policy:approve",
    "policy:publish",
    "policy:export",
    "announcement:view",
    "announcement:create",
    "announcement:publish",
    "announcement:manage",
    "campaign:view",
    "campaign:manage",
    "document:send_for_signature",
    "document:view_all",
    "document:void",
    "form:view",
    "form:manage",
    "rule:view",
    "rule:manage",
    "calendar:view",
    "calendar:manage",
    "template:browse",
    "template:install",
    "template:publish_workspace_template",
    "request:start",
    "request:view_own",
    "request:view_all",
    "request:view_detail",
    "request:view_restricted",
    "request:comment",
    "request:cancel",
    "request:suspend_resume",
    "request:retry",
    "request:migrate",
    "work:view_assigned",
    "work:claim",
    "work:complete",
    "work:delegate",
    "work:reassign",
    "work:view_team",
    "case:create",
    "case:view",
    "case:edit",
    "case:add_evidence",
    "case:view_evidence",
    "case:close",
    "case:reopen",
    "control:view",
    "control:map",
    "attestation:attest",
    "attestation:manage_campaign",
    "report:view",
    "report:export",
    "report:manage_dashboards",
    "connector:view",
    "connector:configure",
    "connector:manage_secrets",
    "file:upload",
    "file:view_own",
    "file:view_all",
    "file:download",
    "file:delete",
    "file:manage_retention",
    "file:manage_quarantine",
    "audit:view_own",
    "audit:view",
    "audit:export",
    "audit:investigate",
    "monitoring:view_dashboards",
    "monitoring:manage_dashboards",
    "monitoring:manage_alerts",
    "api_token:manage",
    "ai:use_assistant",
    "ai:manage_settings",
];

/// Permissions that only the Workspace Owner holds (not grantable via Admin or custom roles).
pub const OWNER_ONLY: &[&str] = &["workspace:delete.request", "workspace:billing.view"];

pub fn is_permission(p: &str) -> bool {
    PERMISSIONS.contains(&p)
}

/// A built-in role template (docs/12 §6). Workspaces get a copy; edit by cloning into a custom role.
#[derive(Debug, Clone, Copy)]
pub struct Template {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub permissions: Perms,
}

#[derive(Debug, Clone, Copy)]
pub enum Perms {
    All,
    AllExceptOwnerOnly,
    List(&'static [&'static str]),
}

impl Template {
    pub fn permissions(&self) -> Vec<String> {
        match self.permissions {
            Perms::All => PERMISSIONS.iter().map(|s| (*s).to_owned()).collect(),
            Perms::AllExceptOwnerOnly => {
                PERMISSIONS.iter().filter(|p| !OWNER_ONLY.contains(p)).map(|s| (*s).to_owned()).collect()
            }
            Perms::List(l) => l.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

const MEMBER: &[&str] = &[
    "member:view",
    "directory:view",
    "collection:view",
    "process:view",
    "sop:view",
    "sop:acknowledge",
    "policy:view",
    "announcement:view",
    "template:browse",
    "request:start",
    "request:view_own",
    "request:comment",
    "work:view_assigned",
    "work:claim",
    "work:complete",
    "work:delegate",
    "file:upload",
    "file:view_own",
    "file:download",
    "audit:view_own",
];

pub const TEMPLATES: &[Template] = &[
    Template {
        key: "workspace.owner",
        name: "Workspace Owner",
        description: "Everything in the workspace, including deletion and billing.",
        permissions: Perms::All,
    },
    Template {
        key: "workspace.admin",
        name: "Workspace Admin",
        description: "Manages and sees everything in the workspace except deleting it (docs/12 §6).",
        permissions: Perms::AllExceptOwnerOnly,
    },
    Template {
        key: "security.admin",
        name: "Security Admin",
        description: "Audit, investigations, API tokens and access reviews.",
        permissions: Perms::List(&[
            "member:view",
            "role:view",
            "audit:view",
            "audit:export",
            "audit:investigate",
            "api_token:manage",
            "monitoring:view_dashboards",
            "monitoring:manage_alerts",
            "workspace:settings.view",
        ]),
    },
    Template {
        key: "process.owner",
        name: "Process Owner",
        description: "Accountable for processes in scope: drafts, publication, analytics.",
        permissions: Perms::List(&[
            "process:view",
            "process:create",
            "process:edit_draft",
            "process:submit_review",
            "process:publish",
            "process:retire",
            "process:manage_owners",
            "process:view_analytics",
            "sop:view",
            "sop:create",
            "sop:edit_draft",
            "sop:publish",
            "form:view",
            "form:manage",
            "rule:view",
            "rule:manage",
            "request:view_all",
            "request:view_detail",
            "work:view_team",
            "work:reassign",
            "template:install",
        ]),
    },
    Template {
        key: "process.designer",
        name: "Process Designer",
        description: "Designs drafts, forms and rules; cannot publish.",
        permissions: Perms::List(&[
            "process:view",
            "process:create",
            "process:edit_draft",
            "process:submit_review",
            "sop:view",
            "sop:create",
            "sop:edit_draft",
            "form:view",
            "form:manage",
            "rule:view",
            "rule:manage",
            "calendar:view",
            "template:browse",
        ]),
    },
    Template {
        key: "reviewer",
        name: "Reviewer",
        description: "Reviews and approves drafts; never their own (SoD).",
        permissions: Perms::List(&[
            "process:view",
            "process:review",
            "process:approve",
            "sop:view",
            "sop:review",
            "sop:approve",
            "policy:view",
            "policy:review",
            "policy:approve",
        ]),
    },
    Template {
        key: "compliance.officer",
        name: "Compliance Officer",
        description: "Controls, attestations, evidence and controlled changes.",
        permissions: Perms::List(&[
            "process:view",
            "process:approve",
            "sop:view",
            "sop:approve",
            "policy:view",
            "policy:create",
            "policy:edit_draft",
            "policy:review",
            "policy:approve",
            "policy:publish",
            "control:view",
            "control:map",
            "attestation:manage_campaign",
            "campaign:view",
            "campaign:manage",
            "case:view",
            "case:view_evidence",
            "audit:view",
            "audit:export",
            "report:view",
            "report:export",
        ]),
    },
    Template {
        key: "case.manager",
        name: "Case Manager",
        description: "Creates and runs cases.",
        permissions: Perms::List(&[
            "case:create",
            "case:view",
            "case:edit",
            "case:add_evidence",
            "case:view_evidence",
            "case:close",
            "case:reopen",
            "work:reassign",
        ]),
    },
    Template {
        key: "investigator",
        name: "Investigator",
        description: "Works on cases they are assigned to.",
        permissions: Perms::List(&["case:view", "case:edit", "case:add_evidence", "case:view_evidence"]),
    },
    Template {
        key: "team.manager",
        name: "Team Manager",
        description: "Sees and reassigns their team's work.",
        permissions: Perms::List(&[
            "request:view_all",
            "request:view_detail",
            "work:view_team",
            "work:reassign",
            "report:view",
            "member:view",
        ]),
    },
    Template {
        key: "analyst",
        name: "Report Viewer",
        description: "Dashboards and redacted exports.",
        permissions: Perms::List(&[
            "report:view",
            "report:export",
            "process:view_analytics",
            "monitoring:view_dashboards",
        ]),
    },
    Template {
        key: "member",
        name: "Member",
        description: "Everyone: browse, start requests, do assigned work, acknowledge.",
        permissions: Perms::List(MEMBER),
    },
];

pub fn template(key: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_only_use_catalogue_permissions() {
        for t in TEMPLATES {
            for p in t.permissions() {
                assert!(is_permission(&p), "{} uses unknown permission {p}", t.key);
            }
        }
    }

    #[test]
    fn admin_lacks_owner_only_permissions() {
        let admin = template("workspace.admin").map(Template::permissions).unwrap_or_default();
        assert!(!admin.contains(&"workspace:delete.request".to_owned()));
        assert!(admin.contains(&"request:view_restricted".to_owned()), "admins see everything (docs/12 §6)");
    }

    #[test]
    fn catalogue_has_no_duplicates() {
        let mut v = PERMISSIONS.to_vec();
        v.sort_unstable();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n);
    }
}
