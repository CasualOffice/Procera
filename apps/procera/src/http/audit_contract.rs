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
    emits(
        "POST",
        "/api/v1/auth/login",
        &[&["login.succeeded"], &["login.password_ok_mfa_required"], &["login.password_change_required"]],
    ),
    // Account recovery (OQ-15, docs/06 §3.6.1). A forgot request is audited whether or not the
    // account exists (the answer is the same); the reset revokes every session in the same event.
    emits("POST", "/api/v1/auth/password/forgot", &[&["password.reset_requested"]]),
    emits("POST", "/api/v1/auth/password/reset", &[&["password.reset"]]),
    emits(
        "POST",
        "/api/v1/auth/password/expired",
        &[&["login.password_ok_mfa_required", "password.changed"], &["login.succeeded", "password.changed"]],
    ),
    emits("POST", "/api/v1/me/password", &[&["password.changed"]]),
    // Admin account controls: the account change on the system chain, the record on the workspace chain.
    emits(
        "POST",
        "/api/v1/members/{id}/security/reset-link",
        &[&["member.password_reset_link_sent", "password.reset_requested"]],
    ),
    emits(
        "POST",
        "/api/v1/members/{id}/security/require-password-change",
        &[&["member.password_change_required", "password.change_required"]],
    ),
    emits(
        "POST",
        "/api/v1/members/{id}/security/sign-out-everywhere",
        &[&["member.signed_out_everywhere", "sessions.revoked_all"]],
    ),
    emits("POST", "/api/v1/members/{id}/security/reset-mfa", &[&["member.mfa_reset", "mfa.reset"]]),
    emits("POST", "/api/v1/auth/mfa", &[&["login.succeeded"]]),
    exempt(
        "POST",
        "/api/v1/auth/mfa/webauthn/options",
        "issues a single-use passkey challenge only; the answer is audited by /auth/mfa (login.succeeded or mfa.failed)",
    ),
    exempt(
        "POST",
        "/api/v1/auth/refresh",
        "rotation on every session renewal; reuse is audited as security.token_reuse and sign-in as login.succeeded",
    ),
    emits("POST", "/api/v1/auth/logout", &[&["logout"]]),
    emits("POST", "/api/v1/me/sign-out-everywhere", &[&["sessions.revoked_all"]]),
    emits("POST", "/api/v1/me/mfa/totp", &[&["mfa.totp_enrollment_started"]]),
    emits("POST", "/api/v1/me/mfa/totp/confirm", &[&["mfa.totp_enabled"]]),
    exempt(
        "POST",
        "/api/v1/me/mfa/webauthn/options",
        "issues a single-use passkey challenge only; the registration is audited as mfa.webauthn_added",
    ),
    emits("POST", "/api/v1/me/mfa/webauthn", &[&["mfa.webauthn_added"]]),
    emits("DELETE", "/api/v1/me/mfa/webauthn/{id}", &[&["mfa.webauthn_removed"]]),
    // System plane.
    emits("POST", "/sysapi/v1/bootstrap", &[&["platform.bootstrapped", "user.created"]]),
    emits("POST", "/sysapi/v1/workspaces", &[&["member.invited", "workspace.created"]]),
    emits("PATCH", "/sysapi/v1/workspaces/{id}", &[&["workspace.renamed"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/suspend", &[&["workspace.suspended"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/restore", &[&["workspace.restored"]]),
    emits("POST", "/sysapi/v1/workspaces/{id}/schedule-deletion", &[&["workspace.deletion_scheduled"]]),
    emits("PUT", "/sysapi/v1/workspaces/{id}/quotas", &[&["workspace.quotas_changed"]]),
    // Who copied it on the system chain; the owner invitation's new token on the workspace chain.
    emits(
        "POST",
        "/sysapi/v1/workspaces/{id}/owner-invitation/link",
        &[&["member.invitation_link_copied", "workspace.owner_invitation_link_copied"]],
    ),
    emits("PUT", "/sysapi/v1/settings/{key}", &[&["platform.floor_changed"]]),
    emits("PUT", "/sysapi/v1/quota-defaults/{profile}", &[&["platform.quota_defaults_changed"]]),
    emits("PUT", "/sysapi/v1/email", &[&["platform.email_changed"]]),
    // Audited whether the message went out or not; the outcome is in the diff.
    emits("POST", "/sysapi/v1/email/test", &[&["platform.email_tested"]]),
    // Workspace plane.
    emits("POST", "/api/v1/session/workspace", &[&["session.workspace_selected"]]),
    emits("POST", "/api/v1/session/step-up", &[&["session.stepped_up"]]),
    exempt(
        "POST",
        "/api/v1/session/step-up/webauthn/options",
        "issues a single-use passkey challenge only; the answer is audited by /session/step-up",
    ),
    emits("POST", "/api/v1/invitations/accept", &[&["member.joined"]]),
    emits("POST", "/api/v1/invitations/register", &[&["member.joined", "user.created"]]),
    emits("POST", "/api/v1/members/invitations", &[&["member.invited"]]),
    emits("DELETE", "/api/v1/members/invitations/{id}", &[&["member.invitation_revoked"]]),
    emits("POST", "/api/v1/members/invitations/{id}/link", &[&["member.invitation_link_copied"]]),
    emits("POST", "/api/v1/members/{id}/deactivate", &[&["member.deactivated"]]),
    emits("POST", "/api/v1/members/{id}/reactivate", &[&["member.reactivated"]]),
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
    emits("PUT", "/api/v1/settings/departments/{id}/{key}", &[&["setting.override_set"]]),
    emits("DELETE", "/api/v1/settings/departments/{id}/{key}", &[&["setting.override_cleared"]]),
    emits("POST", "/api/v1/settings/profile/apply", &[&["settings.profile_switched"]]),
    // Library (approval and publication can also append version.approved / version.published).
    emits("POST", "/api/v1/artifacts", &[&["artifact.created"]]),
    emits("POST", "/api/v1/artifacts/{id}/versions", &[&["version.created"]]),
    emits("PATCH", "/api/v1/versions/{id}", &[&["version.edited"]]),
    emits(
        "POST",
        "/api/v1/versions/{id}/submit",
        &[&["version.submitted"], &["version.approved", "version.submitted"]],
    ),
    emits("POST", "/api/v1/versions/{id}/withdraw", &[&["version.withdrawn"]]),
    emits(
        "POST",
        "/api/v1/versions/{id}/reviews",
        &[&["version.reviewed"], &["version.approved", "version.reviewed"]],
    ),
    // A version that requires re-acknowledgement starts its campaign in the same call (BPMS-045).
    emits(
        "POST",
        "/api/v1/versions/{id}/publish",
        &[
            &["version.published"],
            &["version.scheduled"],
            &["campaign.activated", "campaign.created", "version.published"],
        ],
    ),
    emits("POST", "/api/v1/versions/{id}/retire", &[&["version.retired"]]),
    // Library governance (BPMS-044).
    emits("PUT", "/api/v1/artifacts/{id}/review-cadence", &[&["artifact.review_cadence_set"]]),
    emits("POST", "/api/v1/artifacts/{id}/recertify", &[&["artifact.recertified"]]),
    emits("POST", "/api/v1/artifacts/{id}/verification", &[&["artifact.verified"]]),
    emits("DELETE", "/api/v1/artifacts/{id}/verification", &[&["artifact.verification_removed"]]),
    emits("POST", "/api/v1/artifacts/{id}/change-requests", &[&["change_request.opened"]]),
    emits("POST", "/api/v1/change-requests/{id}/resolve", &[&["change_request.resolved"]]),
    emits("PUT", "/api/v1/me/notification-preferences", &[&["notification_preferences.changed"]]),
    // Out of office (BPMS-086): your own, or someone's as an admin or their manager.
    emits("PUT", "/api/v1/me/out-of-office", &[&["member.ooo_set"]]),
    emits("DELETE", "/api/v1/me/out-of-office", &[&["member.ooo_cleared"]]),
    emits("PUT", "/api/v1/members/{id}/out-of-office", &[&["member.ooo_set"]]),
    emits("DELETE", "/api/v1/members/{id}/out-of-office", &[&["member.ooo_cleared"]]),
    // Collections, owners, RACI (BPMS-041).
    emits("POST", "/api/v1/collections", &[&["collection.created"]]),
    emits("PATCH", "/api/v1/collections/{id}", &[&["collection.updated"]]),
    emits("DELETE", "/api/v1/collections/{id}", &[&["collection.deleted"]]),
    emits("PUT", "/api/v1/collections/{id}/visibility", &[&["collection.visibility_set"]]),
    emits("PUT", "/api/v1/artifacts/{id}/collection", &[&["artifact.collection_set"]]),
    emits("PUT", "/api/v1/artifacts/{id}/owner", &[&["artifact.owner_set"]]),
    emits("PUT", "/api/v1/artifacts/{id}/raci", &[&["artifact.raci_set"]]),
    emits("POST", "/api/v1/security/encryption/rotate", &[&["key.rotated"]]),
    // Saved views (personal, everyone or one role; the audience is in the diff) and grid exports
    // (CSV recorded, XLSX built and recorded: one `data.exported` either way) (BPMS-028).
    emits("POST", "/api/v1/views", &[&["view.created"]]),
    emits("PATCH", "/api/v1/views/{id}", &[&["view.updated"]]),
    emits("DELETE", "/api/v1/views/{id}", &[&["view.deleted"]]),
    emits("POST", "/api/v1/exports", &[&["data.exported"]]),
    // E-signing (BPMS-107): drafts, send, void; signing (the last signature also completes the envelope
    // and stores the signed copy as a file).
    emits("POST", "/api/v1/envelopes", &[&["envelope.created", "file.linked"]]),
    emits("PUT", "/api/v1/envelopes/{id}/recipients", &[&["envelope.recipients_set"]]),
    emits("PUT", "/api/v1/envelopes/{id}/fields", &[&["envelope.fields_set"]]),
    emits("POST", "/api/v1/envelopes/{id}/send", &[&["envelope.sent"]]),
    emits("POST", "/api/v1/envelopes/{id}/void", &[&["envelope.voided"]]),
    emits(
        "POST",
        "/api/v1/signing/{recipient}/sign",
        &[&["envelope.signed"], &["envelope.completed", "envelope.signed", "file.linked", "file.uploaded"]],
    ),
    emits("POST", "/api/v1/signing/{recipient}/decline", &[&["envelope.declined"]]),
    emits("POST", "/api/v1/envelopes/{id}/send-bulk", &[&["envelope.bulk_sent"]]),
    emits("POST", "/api/v1/envelopes/{id}/remind", &[&["envelope.reminded"], &[]]),
    emits("POST", "/api/v1/envelope-batches/{id}/remind", &[&["envelope.batch_reminded"], &[]]),
    emits("POST", "/api/v1/envelope-batches/{id}/void", &[&["envelope.batch_voided"], &[]]),
    // Email-actionable approvals (BPMS-145): the decision's own records plus the email marker.
    emits(
        "POST",
        "/api/v1/email-actions/{token}",
        &[
            &["instance.advanced", "work.decided_by_email", "work_item.decided"],
            &["instance.completed", "work.decided_by_email", "work_item.decided"],
        ],
    ),
    // Outgoing webhooks (BPMS-143).
    emits("POST", "/api/v1/webhooks", &[&["webhook.created"]]),
    emits("PATCH", "/api/v1/webhooks/{id}", &[&["webhook.updated"]]),
    emits("DELETE", "/api/v1/webhooks/{id}", &[&["webhook.deleted"]]),
    emits("POST", "/api/v1/webhooks/{id}/rotate-secret", &[&["webhook.secret_rotated"]]),
    emits("POST", "/api/v1/webhooks/{id}/test", &[&["webhook.tested"]]),
    emits("POST", "/api/v1/webhooks/deliveries/{id}/replay", &[&["webhook.replayed"]]),
    // Personal API tokens (BPMS-144): created, revoked by their owner or an admin (again: no-op).
    emits("POST", "/api/v1/me/api-tokens", &[&["api_token.created"]]),
    emits("DELETE", "/api/v1/me/api-tokens/{id}", &[&["api_token.revoked"], &[]]),
    emits("DELETE", "/api/v1/api-tokens/{id}", &[&["api_token.revoked"], &[]]),
    // Files (BPMS-060/062).
    exempt(
        "POST",
        "/api/v1/files",
        "reserves an upload; file.uploaded/quarantined/rejected is appended when it completes",
    ),
    exempt(
        "PUT",
        "/api/v1/files/{id}/content",
        "chunk transfer of the caller's own upload; audited at completion",
    ),
    emits(
        "POST",
        "/api/v1/files/{id}/complete",
        &[&["file.uploaded"], &["file.quarantined"], &["file.rejected"], &["file.scan_failed"]],
    ),
    emits("DELETE", "/api/v1/files/{id}", &[&["file.deleted"]]),
    // Recorded on the system chain (by the platform service) and on the workspace chain.
    emits("PATCH", "/api/v1/workspace", &[&["workspace.renamed", "workspace.renamed"]]),
    emits("PATCH", "/api/v1/me/profile", &[&["user.profile_updated"]]),
    emits("POST", "/api/v1/files/retention", &[&["file.retention_changed"]]),
    emits("POST", "/api/v1/files/{id}/links", &[&["file.linked"]]),
    emits("DELETE", "/api/v1/files/{id}/links/{object_type}/{object_id}", &[&["file.unlinked"]]),
    emits("POST", "/api/v1/files/orphans/purge", &[&["file.orphans_purged"]]),
    emits("PUT", "/api/v1/artifacts/{id}/applicability", &[&["artifact.applicability_set"]]),
    // Campaigns (BPMS-046). Reminder steps, escalations and joiner enrolment are scheduler/consumer
    // work, audited as the system (campaign.reminders_sent, campaign.escalated, campaign.recipient_enrolled).
    emits("POST", "/api/v1/campaigns", &[&["campaign.created"]]),
    emits("POST", "/api/v1/campaigns/{id}/activate", &[&["campaign.activated"]]),
    emits("POST", "/api/v1/campaigns/{id}/close", &[&["campaign.closed"]]),
    emits("POST", "/api/v1/campaigns/{id}/cancel", &[&["campaign.cancelled"]]),
    emits("POST", "/api/v1/campaigns/{id}/remind", &[&["campaign.reminded"]]),
    emits("POST", "/api/v1/acknowledgements/{id}/complete", &[&["acknowledgement.completed"]]),
    // Announcements (approval and publication can also append announcement.published / .scheduled).
    // FR-AN04: one that requires acknowledgement and goes live now also starts its campaign in the
    // same call (campaign.created + campaign.activated); archiving closes it (campaign.closed).
    emits("POST", "/api/v1/announcements", &[&["announcement.created"]]),
    emits("PATCH", "/api/v1/announcements/{id}", &[&["announcement.edited"]]),
    emits("POST", "/api/v1/announcements/{id}/submit", &[&["announcement.submitted"]]),
    emits(
        "POST",
        "/api/v1/announcements/{id}/approve",
        &[
            &["announcement.approved", "announcement.published"],
            &["announcement.approved", "announcement.scheduled"],
            &["announcement.approved", "announcement.published", "campaign.activated", "campaign.created"],
        ],
    ),
    emits("POST", "/api/v1/announcements/{id}/reject", &[&["announcement.rejected"]]),
    emits(
        "POST",
        "/api/v1/announcements/{id}/publish",
        &[
            &["announcement.published"],
            &["announcement.scheduled"],
            &["announcement.published", "campaign.activated", "campaign.created"],
        ],
    ),
    emits(
        "POST",
        "/api/v1/announcements/{id}/archive",
        &[&["announcement.archived"], &["announcement.archived", "campaign.closed"]],
    ),
    // Receipts are evidence (FR-AN04): the first read and the acknowledgement are audited; repeats change nothing.
    // The acknowledgement also completes the campaign assignment; its one record cites it (no second event).
    emits("POST", "/api/v1/announcements/{id}/reads", &[&["announcement.read"], &[]]),
    emits("POST", "/api/v1/announcements/{id}/acks", &[&["announcement.acknowledged"], &[]]),
    // Template packs (BPMS-106, A-38): one template.installed plus one artifact.created per document
    // created (0 when every chosen document already exists and is skipped; at most 5 per pack).
    emits(
        "POST",
        "/api/v1/templates/{key}/install",
        &[
            &["template.installed"],
            &["artifact.created", "template.installed"],
            &["artifact.created", "artifact.created", "template.installed"],
            &["artifact.created", "artifact.created", "artifact.created", "template.installed"],
            &[
                "artifact.created",
                "artifact.created",
                "artifact.created",
                "artifact.created",
                "template.installed",
            ],
            &[
                "artifact.created",
                "artifact.created",
                "artifact.created",
                "artifact.created",
                "artifact.created",
                "template.installed",
            ],
        ],
    ),
    // Notifications: personal read state only.
    exempt(
        "POST",
        "/api/v1/notifications/read-all",
        "personal read state of the caller's own inbox; no security or business effect",
    ),
    exempt(
        "POST",
        "/api/v1/notifications/{id}/read",
        "personal read state of the caller's own inbox; no security or business effect",
    ),
    // Directory.
    emits("POST", "/api/v1/departments", &[&["department.created"]]),
    emits("PATCH", "/api/v1/departments/{id}", &[&["department.updated"]]),
    emits("DELETE", "/api/v1/departments/{id}", &[&["department.deleted"]]),
    emits("POST", "/api/v1/groups", &[&["group.created"]]),
    emits("DELETE", "/api/v1/groups/{id}", &[&["group.deleted"]]),
    emits("POST", "/api/v1/groups/{id}/members", &[&["group.member_added"]]),
    emits("DELETE", "/api/v1/groups/{id}/members/{membership_id}", &[&["group.member_removed"]]),
    emits("POST", "/api/v1/positions", &[&["position.created"]]),
    emits("DELETE", "/api/v1/positions/{id}", &[&["position.deleted"]]),
    // A patch that changes nothing appends nothing.
    emits("PATCH", "/api/v1/members/{id}", &[&["member.directory_updated"], &[]]),
    // mode=plan rolls back (nothing appended); mode=apply records one event listing created departments.
    emits("POST", "/api/v1/members/import", &[&["member.directory_imported"], &[]]),
    // Audit explorer (reads are not logged per query; exports always are, FR-T01).
    emits("POST", "/api/v1/audit/saved-queries", &[&["audit.query_saved"]]),
    emits("DELETE", "/api/v1/audit/saved-queries/{id}", &[&["audit.query_deleted"]]),
    emits("POST", "/api/v1/audit/exports", &[&["audit.exported"], &["audit.export_requested"]]),
    exempt("POST", "/api/v1/audit/query", "read-only query; POST only carries the structured filter body"),
    // Requests and approvals (BPMS-103/085): the user's record plus the runtime record of the same
    // stimulus, both with the user as actor (A-28 (4)).
    emits("POST", "/api/v1/requests", &[&["instance.started", "request.started"]]),
    exempt(
        "POST",
        "/api/v1/requests/preview",
        "read-only dry run of the routing (who approves); POST only carries the draft answers, nothing is written",
    ),
    emits("POST", "/api/v1/requests/{id}/cancel", &[&["instance.cancelled", "request.cancelled"]]),
    emits(
        "POST",
        "/api/v1/work/{id}/decisions",
        &[&["instance.advanced", "work_item.decided"], &["instance.completed", "work_item.decided"]],
    ),
    emits("POST", "/api/v1/work/{id}/reassign", &[&["work_item.reassigned"]]),
    emits("POST", "/api/v1/processes", &[&["artifact.created"]]),
    emits(
        "PUT",
        "/api/v1/processes/{id}/draft",
        &[&["version.edited"], &["version.created", "version.edited"]],
    ),
    exempt(
        "POST",
        "/api/v1/processes/validate",
        "a read: compiles a draft against the workspace and returns its problems",
    ),
    exempt(
        "POST",
        "/api/v1/processes/test-run",
        "a read: simulates a draft in memory; nothing is stored or sent",
    ),
    exempt("POST", "/api/v1/processes/who", "a read: counts who a step's candidates reach in the directory"),
    emits(
        "POST",
        "/api/v1/processes/{id}/publish",
        &[&["version.submitted"], &["version.approved", "version.published", "version.submitted"]],
    ),
    emits("POST", "/api/v1/work/{id}/delegate", &[&["work_item.delegated"]]),
    emits("POST", "/api/v1/requests/{id}/comments", &[&["request.commented"]]),
    emits("PUT", "/api/v1/requests/{id}/watch", &[&["request.watched"]]),
    emits("DELETE", "/api/v1/requests/{id}/watch", &[&["request.unwatched"]]),
    emits("POST", "/api/v1/operations/incidents/{source}/{id}/retry", &[&["incident.retried"]]),
    emits("POST", "/api/v1/operations/incidents/{source}/{id}/dismiss", &[&["incident.dismissed"]]),
];

#[cfg(test)]
mod tests {
    use super::CONTRACT;
    use std::collections::BTreeSet;

    /// Mutating routes declared in the router sources, as (METHOD, full path).
    fn declared_routes() -> BTreeSet<(String, String)> {
        let sources = [
            ("/api/v1", include_str!("auth.rs")),
            ("/api/v1", include_str!("recovery.rs")),
            ("/api/v1", include_str!("workspace.rs")),
            ("/api/v1", include_str!("settings.rs")),
            ("/api/v1", include_str!("directory.rs")),
            ("/api/v1", include_str!("notifications.rs")),
            ("/api/v1", include_str!("library.rs")),
            ("/api/v1", include_str!("campaigns.rs")),
            ("/api/v1", include_str!("templates.rs")),
            ("/api/v1", include_str!("announcements.rs")),
            ("/api/v1", include_str!("search.rs")),
            ("/api/v1", include_str!("audit.rs")),
            ("/api/v1", include_str!("encryption.rs")),
            ("/api/v1", include_str!("files.rs")),
            ("/api/v1", include_str!("views.rs")),
            ("/api/v1", include_str!("profile.rs")),
            ("/api/v1", include_str!("delegation.rs")),
            ("/api/v1", include_str!("requests.rs")),
            ("/api/v1", include_str!("work.rs")),
            ("/api/v1", include_str!("processes.rs")),
            ("/api/v1", include_str!("tokens.rs")),
            ("/api/v1", include_str!("signing.rs")),
            ("/api/v1", include_str!("webhooks.rs")),
            ("/api/v1", include_str!("email_actions.rs")),
            ("/sysapi/v1", include_str!("system.rs")),
            ("/sysapi/v1", include_str!("system_console.rs")),
            ("/sysapi/v1", include_str!("system_email.rs")),
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
