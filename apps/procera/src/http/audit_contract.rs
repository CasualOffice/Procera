//! Audit coverage contract (FR-T01, BPMS-026): every mutating route declares the audit actions a
//! successful call appends, or why it appends none. The unit test below fails when a mutating route
//! exists without an entry (or an entry without a route); the E2E pack `r1/audit.api.spec.ts`
//! calls each route and checks the events actually appended against this table.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Audit {
    /// A successful call appends exactly one of these action multisets (sorted), across the
    /// system chain and the workspace chain.
    Emits { alternatives: &'static [&'static [&'static str]] },
    /// No event, with the reason (reviewed like any other contract change).
    Exempt { reason: &'static str },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Entry {
    pub method: &'static str,
    pub path: &'static str,
    pub audit: Audit,
}

const fn emits(
    method: &'static str,
    path: &'static str,
    alternatives: &'static [&'static [&'static str]],
) -> Entry {
    Entry { method, path, audit: Audit::Emits { alternatives } }
}

const fn exempt(method: &'static str, path: &'static str, reason: &'static str) -> Entry {
    Entry { method, path, audit: Audit::Exempt { reason } }
}

pub const CONTRACT: &[Entry] = &[
    // Authentication (system chain).
    emits("POST", "/api/v1/auth/login", &[&["login.succeeded"], &["login.password_ok_mfa_required"]]),
    emits("POST", "/api/v1/auth/mfa", &[&["login.succeeded"]]),
    exempt(
        "POST",
        "/api/v1/auth/refresh",
        "rotation on every session renewal; reuse is audited as security.token_reuse and sign-in as login.succeeded",
    ),
    emits("POST", "/api/v1/auth/logout", &[&["logout"]]),
    emits("POST", "/api/v1/me/sign-out-everywhere", &[&["sessions.revoked_all"]]),
    emits("POST", "/api/v1/me/mfa/totp", &[&["mfa.totp_enrollment_started"]]),
    emits("POST", "/api/v1/me/mfa/totp/confirm", &[&["mfa.totp_enabled"]]),
    // System plane.
    emits("POST", "/sysapi/v1/bootstrap", &[&["platform.bootstrapped", "user.created"]]),
    emits("POST", "/sysapi/v1/workspaces", &[&["member.invited", "workspace.created"]]),
    emits("PATCH", "/sysapi/v1/workspaces/{id}", &[&["workspace.renamed"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/suspend", &[&["workspace.suspended"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/restore", &[&["workspace.restored"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/schedule-deletion", &[&["workspace.deletion_scheduled"]]),
    emits("PUT", "/sysapi/v1/workspaces/{id}/quotas", &[&["workspace.quotas_changed"]]),
    emits("PUT", "/sysapi/v1/settings/{key}", &[&["platform.floor_changed"]]),
    // Workspace plane.
    emits("POST", "/api/v1/session/workspace", &[&["session.workspace_selected"]]),
    emits("POST", "/api/v1/invitations/accept", &[&["member.joined"]]),
    emits("POST", "/api/v1/invitations/register", &[&["member.joined", "user.created"]]),
    emits("POST", "/api/v1/members/invitations", &[&["member.invited"]]),
    emits("DELETE", "/api/v1/members/invitations/{id}", &[&["member.invitation_revoked"]]),
    emits("POST", "/api/v1/members/{id}/deactivate", &[&["member.deactivated"]]),
    emits("POST", "/api/v1/roles", &[&["role.created"]]),
    emits("PATCH", "/api/v1/roles/{id}", &[&["role.updated"]]),
    exempt(
        "POST",
        "/api/v1/roles/{id}/preview",
        "read-only impact preview; POST only because it carries a body",
    ),
    emits("POST", "/api/v1/role-grants", &[&["role.granted"]]),
    emits("DELETE", "/api/v1/role-grants/{id}", &[&["role.revoked"]]),
    emits("PUT", "/api/v1/settings/{key}", &[&["setting.changed"]]),
];

#[cfg(test)]
mod tests {
    use super::CONTRACT;
    use std::collections::BTreeSet;

    /// Mutating routes declared in the router sources, as (METHOD, full path).
    fn declared_routes() -> BTreeSet<(String, String)> {
        let sources = [
            ("/api/v1", include_str!("auth.rs")),
            ("/api/v1", include_str!("workspace.rs")),
            ("/api/v1", include_str!("settings.rs")),
            ("/sysapi/v1", include_str!("system.rs")),
            ("", include_str!("../http.rs")),
        ];
        let mut out = BTreeSet::new();
        for (prefix, src) in sources {
            for line in src.lines().map(str::trim).filter(|l| l.starts_with(".route(\"")) {
                let rest = &line[".route(\"".len()..];
                let Some(end) = rest.find('"') else { continue };
                let path = format!("{prefix}{}", &rest[..end]);
                let handlers = &rest[end..];
                for m in ["post", "put", "patch", "delete"] {
                    if handlers.contains(&format!("({m}("))
                        || handlers.contains(&format!(".{m}("))
                        || handlers.contains(&format!(" {m}("))
                    {
                        out.insert((m.to_uppercase(), path.clone()));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn every_mutating_route_has_an_audit_contract_and_vice_versa() {
        let routes = declared_routes();
        let contract: BTreeSet<(String, String)> =
            CONTRACT.iter().map(|e| (e.method.to_owned(), e.path.to_owned())).collect();
        assert_eq!(contract.len(), CONTRACT.len(), "duplicate contract entries");
        let missing: Vec<_> = routes.difference(&contract).collect();
        let stale: Vec<_> = contract.difference(&routes).collect();
        assert!(missing.is_empty(), "mutating routes without an audit contract: {missing:?}");
        assert!(stale.is_empty(), "contract entries without a route: {stale:?}");
        assert!(
            routes.len() >= 25,
            "parser found too few routes ({}) — did the router style change?",
            routes.len()
        );
    }

    #[test]
    fn alternatives_are_sorted_and_exemptions_explained() {
        for e in CONTRACT {
            match e.audit {
                super::Audit::Emits { alternatives } => {
                    assert!(!alternatives.is_empty());
                    for a in alternatives {
                        assert!(a.windows(2).all(|w| w[0] <= w[1]), "{} {}: sort {a:?}", e.method, e.path);
                    }
                }
                super::Audit::Exempt { reason } => {
                    assert!(reason.len() > 20, "{} {}: explain the exemption", e.method, e.path)
                }
            }
        }
    }
}
