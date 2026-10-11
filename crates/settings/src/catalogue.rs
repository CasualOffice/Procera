//! The settings catalogue (docs/20 §2). A key that is not listed here does not exist. This is the
//! P0 subset that current features read; later keys arrive with the feature that uses them.
//!
//! Units: durations are integer **seconds**, sizes integer **bytes**.

use serde::Serialize;
use serde_json::{Value, json};

/// Value type and validation bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    /// Seconds.
    Duration {
        min: i64,
        max: i64,
    },
    /// Bytes.
    Size {
        min: i64,
        max: i64,
    },
    Enum {
        values: &'static [&'static str],
    },
    /// A set drawn from `values`; `"*"` means "all" when `allow_all`.
    Set {
        values: &'static [&'static str],
        allow_all: bool,
    },
    /// Free list of strings (e.g. email domains), each at most `max_len`.
    List {
        max_items: usize,
        max_len: usize,
    },
    Text {
        max_len: usize,
    },
    /// `#RRGGBB`.
    Colour,
    /// Ascending list of distinct durations in seconds (e.g. a reminder ladder).
    DurationList {
        min: i64,
        max: i64,
        max_items: usize,
    },
}

/// How "stricter" is decided for a floor-bound key (docs/20 §1 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparator {
    /// Smaller number is stricter (timeouts, sizes, TTLs).
    Smaller,
    /// Larger number is stricter (minimum lengths).
    Larger,
    /// A superset is stricter (roles that must use MFA). `"*"` is the largest set.
    Superset,
    /// A subset is stricter (allowed methods).
    Subset,
    /// `true` is stricter.
    True,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Def {
    pub key: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    /// Permission needed to change it in a workspace; `None` = not editable by workspaces (floor-locked or computed).
    pub perm: Option<&'static str>,
    /// Comparator when platform floors apply.
    pub floor: Option<Comparator>,
    /// ⚡ applies immediately everywhere (docs/20 §1 rule 3); a reason is required to change it.
    pub immediate: bool,
    /// Requires a fresh MFA-verified session to change.
    pub step_up: bool,
    /// A department may override it (docs/20 §2 column Ovr, department scope). Never a ⚡ key: a
    /// department-scoped admin can never change workspace security (docs/20 §0).
    pub department: bool,
    #[serde(skip)]
    pub default: fn() -> Value,
}

const SETTINGS: &str = "workspace:settings.manage";
const BRANDING: &str = "workspace:branding.manage";
const DIRECTORY: &str = "directory:manage";
const RETENTION: &str = "file:manage_retention";

const MFA_ROLES: &[&str] = &[
    "workspace.owner",
    "workspace.admin",
    "security.admin",
    "process.owner",
    "process.designer",
    "reviewer",
    "compliance.officer",
    "case.manager",
    "investigator",
    "team.manager",
    "analyst",
    "member",
];

const fn def(
    key: &'static str,
    group: &'static str,
    kind: Kind,
    perm: Option<&'static str>,
    default: fn() -> Value,
) -> Def {
    Def { key, group, kind, perm, floor: None, immediate: false, step_up: false, department: false, default }
}

const fn sec(
    key: &'static str,
    kind: Kind,
    perm: Option<&'static str>,
    floor: Comparator,
    step_up: bool,
    default: fn() -> Value,
) -> Def {
    Def {
        key,
        group: "security",
        kind,
        perm,
        floor: Some(floor),
        immediate: true,
        step_up,
        department: false,
        default,
    }
}

/// Marks a key as department-overridable (Ovr ✓ or ✓ (dept) in docs/20 §2; "(collection)" only is not).
const fn dept(d: Def) -> Def {
    Def { department: true, ..d }
}

const MIN: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;
const MB: i64 = 1_048_576;

pub const CATALOGUE: &[Def] = &[
    // 2.1 Branding
    def("branding.accent", "branding", Kind::Colour, Some(BRANDING), || json!("#EE4136")),
    def(
        "branding.theme_default",
        "branding",
        Kind::Enum { values: &["system", "light", "dark"] },
        Some(BRANDING),
        || json!("system"),
    ),
    def("branding.email_sender_name", "branding", Kind::Text { max_len: 80 }, Some(BRANDING), || json!("")),
    // 2.2 Localisation
    dept(def("locale.default", "locale", Kind::Text { max_len: 35 }, Some(SETTINGS), || json!("en"))),
    dept(def("locale.timezone", "locale", Kind::Text { max_len: 64 }, Some(SETTINGS), || json!("UTC"))),
    dept(def(
        "locale.week_start",
        "locale",
        Kind::Enum { values: &["mon", "tue", "wed", "thu", "fri", "sat", "sun"] },
        Some(SETTINGS),
        || json!("mon"),
    )),
    // 2.3 Identity and security (⚡)
    sec(
        "auth.mfa.required_roles",
        Kind::Set { values: MFA_ROLES, allow_all: true },
        Some(SETTINGS),
        Comparator::Superset,
        true,
        || json!(["security.admin", "workspace.admin", "workspace.owner"]),
    ),
    sec(
        "auth.mfa.allowed_methods",
        Kind::Set { values: &["totp", "webauthn"], allow_all: false },
        Some(SETTINGS),
        Comparator::Subset,
        false,
        || json!(["totp", "webauthn"]),
    ),
    sec(
        "auth.password.min_length",
        Kind::Int { min: 12, max: 128 },
        Some(SETTINGS),
        Comparator::Larger,
        false,
        || json!(12),
    ),
    sec("auth.password.breach_check", Kind::Bool, None, Comparator::True, false, || json!(true)),
    sec(
        "auth.session.idle_timeout",
        Kind::Duration { min: 5 * MIN, max: 30 * DAY },
        Some(SETTINGS),
        Comparator::Smaller,
        false,
        || json!(30 * MIN),
    ),
    sec(
        "auth.session.absolute_timeout",
        Kind::Duration { min: HOUR, max: 90 * DAY },
        Some(SETTINGS),
        Comparator::Smaller,
        false,
        || json!(12 * HOUR),
    ),
    sec(
        "auth.stepup.window",
        Kind::Duration { min: MIN, max: HOUR },
        Some(SETTINGS),
        Comparator::Smaller,
        false,
        || json!(5 * MIN),
    ),
    def(
        "auth.allowed_email_domains",
        "security",
        Kind::List { max_items: 50, max_len: 253 },
        Some(SETTINGS),
        || json!([]),
    ),
    def("auth.invite_only", "security", Kind::Bool, Some(SETTINGS), || json!(true)),
    sec(
        "api.tokens.max_ttl",
        Kind::Duration { min: DAY, max: 730 * DAY },
        Some(SETTINGS),
        Comparator::Smaller,
        false,
        || json!(90 * DAY),
    ),
    // 2.4 Access
    def("access.delegation.max_depth", "access", Kind::Int { min: 1, max: 3 }, Some(SETTINGS), || json!(1)),
    def("access.delegation.self_service_ooo", "access", Kind::Bool, Some(SETTINGS), || json!(true)),
    sec("access.admin_content_access_log", Kind::Bool, None, Comparator::True, false, || json!(true)),
    // 2.5 Directory
    def("directory.positions.enabled", "directory", Kind::Bool, Some(DIRECTORY), || json!(false)),
    def(
        "directory.deactivation.reassign_strategy",
        "directory",
        Kind::Enum { values: &["fallback", "manager", "process_owner"] },
        Some(DIRECTORY),
        || json!("fallback"),
    ),
    // 2.6 Governance
    def(
        "governance.review.required_for",
        "governance",
        Kind::Enum { values: &["none", "controlled", "all"] },
        Some(SETTINGS),
        || json!("controlled"),
    ),
    dept(def(
        "governance.review.min_reviewers",
        "governance",
        Kind::Int { min: 0, max: 5 },
        Some(SETTINGS),
        || json!(1),
    )),
    dept(def(
        "governance.reack.policy",
        "governance",
        Kind::Enum { values: &["major_only", "every_publish", "none"] },
        Some(SETTINGS),
        || json!("major_only"),
    )),
    dept(def(
        "governance.reack.window",
        "governance",
        Kind::Duration { min: DAY, max: 365 * DAY },
        Some(SETTINGS),
        || json!(14 * DAY),
    )),
    dept(def("governance.reason_for_change.required", "governance", Kind::Bool, Some(SETTINGS), || {
        json!(true)
    })),
    // BPMS-044: how long a published artifact stays current before it must be recertified, and how
    // long a verification badge lasts.
    dept(def(
        "governance.review_cadence.default",
        "governance",
        Kind::Duration { min: 7 * DAY, max: 3 * 365 * DAY },
        Some(SETTINGS),
        || json!(365 * DAY),
    )),
    dept(def(
        "governance.review_cadence.controlled",
        "governance",
        Kind::Duration { min: 7 * DAY, max: 3 * 365 * DAY },
        Some(SETTINGS),
        || json!(180 * DAY),
    )),
    dept(def(
        "governance.verification.validity",
        "governance",
        Kind::Duration { min: DAY, max: 365 * DAY },
        Some(SETTINGS),
        || json!(90 * DAY),
    )),
    // 2.7 Execution
    def("exec.request_id_pattern", "execution", Kind::Text { max_len: 40 }, Some(SETTINGS), || {
        json!("REQ-{yyyy}-{seq:6}")
    }),
    def("exec.loop.max_iterations", "execution", Kind::Int { min: 1, max: 200 }, Some(SETTINGS), || {
        json!(50)
    }),
    def("exec.bulk_approve.allowed", "execution", Kind::Bool, Some(SETTINGS), || json!(true)),
    def("exec.cancel.allowed_for_initiator", "execution", Kind::Bool, Some(SETTINGS), || json!(true)),
    // 2.9 Notifications
    def(
        "notify.channels.enabled",
        "notifications",
        Kind::Set { values: &["in_app", "email", "push", "webhook", "chat"], allow_all: false },
        Some(SETTINGS),
        || json!(["email", "in_app"]),
    ),
    // FR-N04: workspace defaults; each member may override them (notification preferences).
    def(
        "notify.digest.schedule",
        "notifications",
        Kind::Enum { values: &["off", "daily", "weekly"] },
        Some(SETTINGS),
        || json!("daily"),
    ),
    def("notify.digest.hour", "notifications", Kind::Int { min: 0, max: 23 }, Some(SETTINGS), || json!(9)),
    def("notify.quiet_hours.enabled", "notifications", Kind::Bool, Some(SETTINGS), || json!(true)),
    def("notify.quiet_hours.start", "notifications", Kind::Int { min: 0, max: 23 }, Some(SETTINGS), || {
        json!(20)
    }),
    def("notify.quiet_hours.end", "notifications", Kind::Int { min: 0, max: 23 }, Some(SETTINGS), || {
        json!(8)
    }),
    def("notify.critical_override_quiet", "notifications", Kind::Bool, Some(SETTINGS), || json!(true)),
    // Retention classes (BPMS-066, docs/14 §4). Shortening a period needs a reason and a recent sign-in.
    Def {
        key: "retention.draft.inactive_after",
        group: "retention",
        kind: Kind::Duration { min: 30 * DAY, max: 3 * 365 * DAY },
        perm: Some(SETTINGS),
        floor: None,
        immediate: true,
        step_up: true,
        department: false,
        default: || json!(180 * DAY),
    },
    Def {
        key: "retention.draft.notice_before",
        group: "retention",
        kind: Kind::Duration { min: DAY, max: 60 * DAY },
        perm: Some(SETTINGS),
        floor: None,
        immediate: false,
        step_up: false,
        department: false,
        default: || json!(14 * DAY),
    },
    Def {
        key: "retention.operational.keep_for",
        group: "retention",
        kind: Kind::Duration { min: 30 * DAY, max: 10 * 365 * DAY },
        perm: Some(SETTINGS),
        floor: None,
        immediate: true,
        step_up: true,
        department: false,
        default: || json!(400 * DAY),
    },
    Def {
        key: "retention.governance_record.keep_for",
        group: "retention",
        kind: Kind::Duration { min: 365 * DAY, max: 50 * 365 * DAY },
        perm: Some(SETTINGS),
        floor: Some(Comparator::Larger),
        immediate: true,
        step_up: true,
        department: false,
        default: || json!(3650 * DAY),
    },
    // 2.10 Files
    Def {
        key: "files.max_size",
        group: "files",
        kind: Kind::Size { min: MB, max: 5 * 1024 * MB },
        perm: Some(RETENTION),
        floor: Some(Comparator::Smaller),
        immediate: false,
        step_up: false,
        department: false,
        default: || json!(100 * MB),
    },
    Def {
        key: "files.scan.required",
        group: "files",
        kind: Kind::Bool,
        perm: Some(SETTINGS),
        floor: Some(Comparator::True),
        immediate: true,
        step_up: false,
        department: false,
        default: || json!(true),
    },
    def("files.exif_strip", "files", Kind::Bool, Some(SETTINGS), || json!(true)),
    def(
        "files.orphan_purge_after",
        "files",
        Kind::Duration { min: DAY, max: 90 * DAY },
        Some(RETENTION),
        || json!(7 * DAY),
    ),
    // 2.13 Monitoring
    def(
        "monitoring.stuck_instance_threshold",
        "monitoring",
        Kind::Duration { min: HOUR, max: 365 * DAY },
        Some(SETTINGS),
        || json!(7 * DAY),
    ),
    // 2.14 Acknowledgement campaigns (copied onto each campaign when it is created)
    dept(def(
        "campaign.audience.snapshot",
        "campaigns",
        Kind::Enum { values: &["at_launch", "dynamic"] },
        Some(SETTINGS),
        || json!("dynamic"),
    )),
    dept(def(
        "campaign.reminders.cadence",
        "campaigns",
        Kind::DurationList { min: HOUR, max: 365 * DAY, max_items: 10 },
        Some(SETTINGS),
        || json!([3 * DAY, 7 * DAY]),
    )),
    // 0 = off.
    dept(def(
        "campaign.escalation.to_manager_after",
        "campaigns",
        Kind::Duration { min: 0, max: 365 * DAY },
        Some(SETTINGS),
        || json!(14 * DAY),
    )),
    dept(def(
        "campaign.default_due",
        "campaigns",
        Kind::Duration { min: DAY, max: 365 * DAY },
        Some(SETTINGS),
        || json!(14 * DAY),
    )),
    dept(def("campaign.ack_text", "campaigns", Kind::Text { max_len: 500 }, Some(SETTINGS), || {
        json!("I have read and understood this document and will follow it.")
    })),
    // 2.15 Policies
    dept(def("policies.attest_on_join", "policies", Kind::Bool, Some(SETTINGS), || json!(true))),
    def("policies.numbering_pattern", "policies", Kind::Text { max_len: 40 }, Some(SETTINGS), || {
        json!("POL-{seq:4}")
    }),
    // 2.17 Announcements
    def("announcements.enabled", "announcements", Kind::Bool, Some(SETTINGS), || json!(true)),
    dept(def(
        "announcements.approval_required",
        "announcements",
        Kind::Enum { values: &["off", "for_workspace_wide", "always"] },
        Some(SETTINGS),
        || json!("for_workspace_wide"),
    )),
    dept(def("announcements.require_ack.default", "announcements", Kind::Bool, Some(SETTINGS), || {
        json!(false)
    })),
    dept(def(
        "announcements.expiry.default",
        "announcements",
        Kind::Duration { min: HOUR, max: 730 * DAY },
        Some(SETTINGS),
        || json!(30 * DAY),
    )),
    def(
        "announcements.channels",
        "announcements",
        Kind::Set { values: &["in_app", "email"], allow_all: false },
        Some(SETTINGS),
        || json!(["email", "in_app"]),
    ),
];

pub fn get(key: &str) -> Option<&'static Def> {
    CATALOGUE.iter().find(|d| d.key == key)
}

/// Configuration profiles (docs/20 §0): values written once at workspace creation.
pub fn profile(name: &str) -> Option<Vec<(&'static str, Value)>> {
    let v = match name {
        "starter" => vec![
            ("auth.mfa.required_roles", json!(["workspace.admin", "workspace.owner"])),
            ("auth.session.idle_timeout", json!(8 * HOUR)),
            ("auth.session.absolute_timeout", json!(30 * DAY)),
            ("auth.invite_only", json!(false)),
            ("governance.review.required_for", json!("none")),
            ("governance.review.min_reviewers", json!(0)),
            ("governance.reack.policy", json!("none")),
        ],
        "standard" => vec![
            (
                "auth.mfa.required_roles",
                json!(["process.owner", "security.admin", "workspace.admin", "workspace.owner"]),
            ),
            ("auth.session.idle_timeout", json!(30 * MIN)),
            ("auth.session.absolute_timeout", json!(12 * HOUR)),
            ("governance.review.min_reviewers", json!(1)),
        ],
        "enterprise" => vec![
            ("auth.mfa.required_roles", json!(["*"])),
            ("auth.session.idle_timeout", json!(30 * MIN)),
            ("auth.session.absolute_timeout", json!(12 * HOUR)),
            ("governance.review.required_for", json!("controlled")),
            ("governance.review.min_reviewers", json!(1)),
        ],
        "regulated" => vec![
            ("auth.mfa.required_roles", json!(["*"])),
            ("auth.mfa.allowed_methods", json!(["totp", "webauthn"])),
            ("auth.session.idle_timeout", json!(15 * MIN)),
            ("auth.session.absolute_timeout", json!(8 * HOUR)),
            ("governance.review.required_for", json!("all")),
            ("governance.review.min_reviewers", json!(2)),
            ("governance.reack.policy", json!("every_publish")),
            ("exec.bulk_approve.allowed", json!(false)),
        ],
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_consistent() {
        let mut keys = std::collections::BTreeSet::new();
        for d in CATALOGUE {
            assert!(keys.insert(d.key), "duplicate key {}", d.key);
            crate::validate(d, &(d.default)()).unwrap_or_else(|e| panic!("{}: default invalid: {e}", d.key));
            if d.step_up {
                assert!(d.immediate, "{}: step-up keys are security keys", d.key);
            }
            if d.department {
                assert!(!d.immediate && !d.step_up, "{}: ⚡ keys are never department-scopable", d.key);
                assert!(d.perm.is_some(), "{}: a department override needs an editable key", d.key);
            }
        }
        for p in ["starter", "standard", "enterprise", "regulated"] {
            for (k, v) in profile(p).expect("profile") {
                let d = get(k).unwrap_or_else(|| panic!("profile {p}: unknown key {k}"));
                crate::validate(d, &v).unwrap_or_else(|e| panic!("profile {p} {k}: {e}"));
            }
        }
        assert!(profile("nope").is_none());
        assert!(CATALOGUE.iter().any(|d| d.department), "some keys are department-scopable");
        assert!(!get("governance.review.required_for").is_some_and(|d| d.department), "collection-only Ovr");
    }

    #[test]
    fn duration_lists_are_sorted_distinct_and_bounded() {
        let d = get("campaign.reminders.cadence").expect("key");
        assert_eq!(
            crate::validate(d, &json!([7 * DAY, 3 * DAY, 3 * DAY])).ok(),
            Some(json!([3 * DAY, 7 * DAY]))
        );
        assert!(crate::validate(d, &json!([60])).is_err(), "below the minimum");
        assert!(crate::validate(d, &json!(["3d"])).is_err());
        assert!(crate::validate(d, &json!(3 * DAY)).is_err());
    }
}
