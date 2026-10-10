//! Fixed system-plane permissions (docs/12 §4.1). One table, one place.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAction {
    WorkspaceRead,
    WorkspaceWrite,
    SystemAuditRead,
}

/// `true` if any of `roles` permits `action`.
pub fn can(roles: &[String], action: SystemAction) -> bool {
    roles.iter().any(|r| {
        matches!(
            (r.as_str(), action),
            ("system.owner" | "system.admin", _)
                | ("system.operator", SystemAction::WorkspaceRead)
                | ("system.auditor", SystemAction::WorkspaceRead | SystemAction::SystemAuditRead)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: &str) -> Vec<String> {
        vec![x.to_owned()]
    }

    #[test]
    fn matrix() {
        assert!(can(&r("system.owner"), SystemAction::WorkspaceWrite));
        assert!(can(&r("system.admin"), SystemAction::WorkspaceWrite));
        assert!(can(&r("system.operator"), SystemAction::WorkspaceRead));
        assert!(!can(&r("system.operator"), SystemAction::WorkspaceWrite));
        assert!(can(&r("system.auditor"), SystemAction::SystemAuditRead));
        assert!(!can(&r("system.auditor"), SystemAction::WorkspaceWrite));
        assert!(!can(&[], SystemAction::WorkspaceRead));
    }
}
