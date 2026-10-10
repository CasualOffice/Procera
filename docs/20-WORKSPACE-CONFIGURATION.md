# 20 — Workspace and platform configuration model

Status: **Draft for G1 sign-off** · Owner: Product + Platform lead · Implements FR-S05, FR-W01–W12.

Product direction (2026-10-11):
- Configuration is driven by roles.
- Every workspace can be configured independently.
- System Admins manage all workspaces and set platform-wide floors.

This document is the single catalogue of every setting. A setting that isn't listed here doesn't exist.

**Audience:** every company size, from startup to SME to MNC, and every department. Two design rules follow:
- A startup must reach a working workspace with **zero configuration decisions**: pick a profile, invite people, install a template.
- An MNC gets depth: many workspaces, deep department trees, delegated admins scoped to departments and collections, and config-as-code promotion.

## 0. Configuration profiles (presets chosen at workspace creation)

A profile is a **named bundle of defaults** applied once when a workspace is created, either by the System Admin or by self-service signup if the platform allows it.

- It is not a lock. Every setting can still be changed later within platform floors, and the workspace keeps a record of which profile it started from.
- Switching profile later is a **plan/apply diff** (the same mechanism as §4 config-as-code). It shows each key that would change and never touches content.

| Setting area | **Starter** (startups, < 50 people) | **Standard** (SMEs, default) | **Enterprise** (MNCs, many departments) | **Regulated** (pharma, finance, public sector) |
|---|---|---|---|---|
| MFA required for | Owner, Admin | Owner, Admin, Security Admin, Process Owners | All members | All members; WebAuthn preferred |
| Session idle / absolute | 8 h / 30 d (remember device) | 30 min / 12 h | 30 min / 12 h | 15 min / 8 h |
| Invite policy | Open link + allowed domain | Invite-only + allowed domain | Invite-only; SSO-ready | Invite-only; SSO enforced when configured |
| Independent review required for | none (owner publishes) | controlled processes | controlled processes + all policies | everything |
| Min reviewers | 0 | 1 | 1 (2 for controlled) | 2 |
| Re-acknowledgement | none | major versions | major versions | every publish |
| Signature mode | none | simple | simple (Part 11 per collection) | part11 |
| Separation mode | off | off | off (recommended on for HR/Finance workspaces) | **on** |
| Two-person rule for ⚡/roles | off | off | on | on |
| Access reviews | off | off | 180 d | 90 d, auto-revoke unconfirmed |
| Retention defaults | 14 §4 minimums ×0.5 where allowed (audit ≥ 1 y) | 14 §4 defaults | 14 §4 defaults | 14 §4 defaults + WORM export recommended |
| Audit checkpoints / TSA | chain only | chain + checkpoints | chain + checkpoints | chain + checkpoints + TSA |
| Department/collection delegated admins | off (single admin) | optional | on | on |
| Bulk approve | on | on | on | off |
| Templates pre-installed | 3 (leave, expense, purchase) | 5 Wave-1 HR/Finance | none (gallery browse) | GRC + QMS pack suggested |
| AI | off | off | off | off (Annex 22 note) |

Implementation:
- Profiles are versioned JSON documents validated by `workspace-config.schema.json`, so a profile is just a curated config-as-code file.
- System Admins can clone a profile into a custom platform profile, e.g. "Acme Group Standard" for all of an MNC's subsidiaries.
- The key `general.profile` records `{name, version, applied_at}`.

**Delegated administration (MNC depth):** the Workspace Admin role can be granted scoped to `department:<id>` or `collection:<id>` ([12 §5](12-AUTHORIZATION-MODEL.md)). A scoped admin can:
- manage members and roles *within* the scope;
- set department/collection overrides (keys marked Ovr);
- run monitoring for the scope.

A scoped admin can never change workspace-level ⚡ keys.

## 1. Layering and resolution

```
1. Platform defaults        (shipped in code; versioned with the product)
2. Platform policy floors   (System Admin; bind every workspace; can only be tightened below)
3. Workspace settings       (Workspace Admin / Owner)
4. Scope overrides          (department or collection; only for keys marked "overridable")
5. Process-level settings   (Process Owner; in the process definition / release bundle)
```

Resolution rules:
1. **Most specific wins** (5 → 4 → 3 → 1), **except** that floor-bound keys are clamped by layer 2. A stricter value at any layer is always allowed; a looser value is rejected at save time with the floor shown (`422 setting_below_floor`).
2. "Stricter" is defined per key by a comparator in the catalogue, e.g. `session.idle_timeout`: smaller is stricter; `mfa.required_roles`: superset is stricter; `files.max_size`: smaller is stricter.
3. Process-level values are **pinned into the release bundle** at publish (ADR-0012). Changing a workspace default later doesn't alter running instances, except for security keys (marked ⚡), which apply immediately to everything.
4. Lowering a platform floor never loosens existing workspace values automatically.
5. Raising a platform floor clamps non-compliant workspace values at enforcement time and notifies affected Workspace Admins. Their stored value is kept but marked `clamped`.

**Effective settings explainer** (UI + `GET /v1/settings/effective?key=…&scope=…`): for any key and scope it shows the final value and every layer that contributed, e.g. "12 h (workspace) — platform floor 12 h — default 24 h". This is required for supportability.

## 2. Settings catalogue (workspace plane)

Column key:
- **Perm**: the permission required to change the setting ([12 §5](12-AUTHORIZATION-MODEL.md)).
- **Floor**: whether a platform floor applies.
- **Ovr**: whether a department/collection override is allowed.
- **Aud**: every change writes an audit event with before/after. All keys are audited, so this column is omitted.
- **Ph**: the phase the setting ships in.

### 2.1 General and branding

| Key | Type | Default | Perm | Floor | Ovr | Ph |
|---|---|---|---|---|---|---|
| `general.name` | string(80) | set at creation | workspace:settings.manage | – | – | P0 |
| `general.slug` | string, immutable after 7 d | derived | system admin only after creation | – | – | P0 |
| `branding.logo` | file ref (SVG/PNG ≤ 512 KB, sanitized SVG) | product logo | workspace:branding.manage | – | – | P0 |
| `branding.favicon` | file ref | product | workspace:branding.manage | – | – | P0 |
| `branding.accent` | hex colour | `#4F46E5` | workspace:branding.manage | contrast ≥ 4.5:1 enforced (auto-derived text-safe shade) | – | P0 |
| `branding.theme_default` | `system \| light \| dark` | system | workspace:branding.manage | – | – | P0 |
| `branding.email_sender_name` | string | workspace name | workspace:branding.manage | sender address set by platform | – | P0 |
| `branding.login_message` | markdown(500) | empty | workspace:branding.manage | – | – | P1 |

### 2.2 Localisation and terminology

| Key | Type | Default | Perm | Floor | Ovr | Ph |
|---|---|---|---|---|---|---|
| `locale.default` | BCP-47 | `en` | workspace:settings.manage | must be in platform-enabled list | ✓ (dept) | P0 |
| `locale.timezone` | IANA tz | `UTC` | workspace:settings.manage | – | ✓ (dept) | P0 |
| `locale.date_format` / `number_format` | enum (locale-derived or explicit) | locale | workspace:settings.manage | – | – | P0 |
| `locale.week_start` | `mon…sun` | locale | workspace:settings.manage | – | ✓ | P0 |
| `terminology.overrides` | map of term → per-locale label (`request`, `process`, `step`, `sop`, `case`, `work`) | none | workspace:settings.manage | – | – | P0 |

Users may override locale/timezone in their profile. Terminology overrides apply to UI, emails and exports, but **never** to API field names.

### 2.3 Identity and security (⚡ = applies immediately)

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `auth.mfa.required_roles` ⚡ | role set or `all` | Owner, Admin, Security Admin | workspace:settings.manage + step-up | ✓ (platform minimum set) | P0 |
| `auth.mfa.allowed_methods` ⚡ | subset of `totp, webauthn` | both | workspace:settings.manage | ✓ | P0 |
| `auth.password.min_length` ⚡ | int | 12 | workspace:settings.manage | ✓ (≥ floor; floor ≥ 12) | P0 |
| `auth.password.breach_check` ⚡ | bool | true | – (floor-locked true) | ✓ | P0 |
| `auth.session.idle_timeout` ⚡ | duration | 30 min | workspace:settings.manage | ✓ | P0 |
| `auth.session.absolute_timeout` ⚡ | duration | 12 h | workspace:settings.manage | ✓ | P0 |
| `auth.stepup.window` ⚡ | duration | 5 min | workspace:settings.manage | ✓ (≤ floor) | P0 |
| `auth.allowed_email_domains` | domain list | any | workspace:settings.manage | – | P0 |
| `auth.invite_only` | bool | true | workspace:settings.manage | – | P0 |
| `auth.ip_allowlist` ⚡ | CIDR list | none | workspace:settings.manage + step-up | – | P1 |
| `auth.sso.enforced` ⚡ | bool (break-glass local owner kept) | false | workspace:settings.manage + step-up | – | P1 |
| `auth.guest.enabled` | bool | false | workspace:settings.manage | platform must allow | P1 |
| `auth.guest.link_ttl` | duration | 7 d | workspace:settings.manage | ✓ (≤ 30 d) | P1 |
| `api.tokens.max_ttl` | duration | 90 d | workspace:settings.manage | ✓ | P0 |

### 2.4 Roles and access

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `roles.custom` | role definitions (name, permissions, default scope) | none | role:manage + step-up | P0 |
| `roles.builtin_overrides` | rename/hide built-in templates (permissions immutable; clone to change) | none | role:manage | P0 |
| `access.separation_mode` ⚡ | bool | false | Owner only + step-up, 24 h notice ([12 §6.1](12-AUTHORIZATION-MODEL.md)) | P1 |
| `access.delegation.max_depth` | 1–3 | 1 | workspace:settings.manage | P0 |
| `access.delegation.self_service_ooo` | bool | true | workspace:settings.manage | P0 |
| `access.reviews.cadence` | duration or off | off (90 d recommended in regulated) | workspace:settings.manage | P1 |
| `access.reviews.auto_revoke_unconfirmed` | bool | false | workspace:settings.manage | P1 |
| `access.admin_content_access_log` | bool | true (floor-locked) | – | P0 |

### 2.5 Directory

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `directory.manager_source_priority` | ordered list of sources (`scim, hris, chat, csv, manual`) | `[manual]` | directory:manage | P0 (manual/csv), P1+ |
| `directory.field_source_priority` | per-field ordered sources | inherits above | directory:manage | P1 |
| `directory.positions.enabled` | bool | false | directory:manage | P0 |
| `directory.deactivation.reassign_strategy` | `fallback \| manager \| process_owner` | fallback | directory:manage | P0 |

### 2.6 Governance

| Key | Type | Default | Perm | Ovr | Ph |
|---|---|---|---|---|---|
| `governance.review.required_for` | `none \| controlled \| all` (`none` = the owner publishes; Starter) | controlled | workspace:settings.manage | ✓ (collection) | P0 |
| `governance.review.min_reviewers` | int 0–5 (0 only with `required_for = none`, Starter) | 1 | workspace:settings.manage | ✓ | P0 |
| `governance.review.reviewer_roles` | role set | Reviewer, Compliance Officer | workspace:settings.manage | ✓ | P0 |
| `governance.reack.policy` | `major_only \| every_publish \| none` | major_only | workspace:settings.manage | ✓ | P0 |
| `governance.reack.window` | duration | 14 d | workspace:settings.manage | ✓ | P0 |
| `governance.review_cadence.default` | duration | 365 d (controlled 180 d) | workspace:settings.manage | ✓ | P0 |
| `governance.effective_date.min_lead` | duration | 0 (controlled 24 h) | workspace:settings.manage | ✓ | P0 |
| `governance.signature.mode` | `none \| simple \| part11` | simple | Owner + step-up | ✓ (collection) | P1 |
| `governance.signature.meanings` | localised list | authored/reviewed/approved/responsible | workspace:settings.manage | – | P1 |
| `governance.reason_for_change.required` | bool | true for controlled | workspace:settings.manage | ✓ | P0 |

### 2.7 Execution

| Key | Type | Default | Perm | Ovr | Ph |
|---|---|---|---|---|---|
| `exec.request_id_pattern` | pattern (`{prefix}-{yyyy}-{seq:5}`) | `REQ-{yyyy}-{seq:6}` | workspace:settings.manage | per process | P0 |
| `exec.sla.default` | duration + business flag | none | workspace:settings.manage | per process | P0 |
| `exec.escalation.default` | escalation ladder (11-IR §3.2) | remind at 50 %, notify owner at 100 % | workspace:settings.manage | per process | P0 |
| `exec.auto_outcome.allowed` | bool | false (never for controlled, invariant) | workspace:settings.manage | per process | P0 |
| `exec.loop.max_iterations` | int ≤ 200 | 50 | workspace:settings.manage | per process | P0 |
| `exec.variables.max_bytes` | int | 256 KB | – | ✓ (platform cap) | P0 |
| `exec.bulk_approve.allowed` | bool | true (never with restricted fields/part11) | workspace:settings.manage | per process | P0 |
| `exec.cancel.allowed_for_initiator` | bool | true | workspace:settings.manage | per process | P0 |
| `exec.test_run.retention` | duration | 7 d | workspace:settings.manage | – | P0 |

### 2.8 Calendars

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `calendars.list` | versioned calendars (working hours per weekday, holiday sets, timezone) | one "Standard" Mon–Fri 09–18 in workspace tz | calendar:manage | P0 |
| `calendars.default` | calendar ref | Standard | calendar:manage | P0 |
| `calendars.ical_feeds` | URL list (refresh schedule) | none | calendar:manage | P1 |

### 2.9 Notifications

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `notify.channels.enabled` | subset of `in_app, email, push, webhook, chat` | in_app, email | workspace:settings.manage | P0 |
| `notify.digest.schedule` | `off \| daily@hh:mm \| weekly` | daily 09:00 | workspace:settings.manage (user may override) | P0 |
| `notify.quiet_hours` | time range + days | 20:00–08:00 | workspace:settings.manage (user may override) | P0 |
| `notify.critical_override_quiet` | bool | true for SLA breach | workspace:settings.manage | P0 |
| `notify.templates` | per event × locale overrides (sandboxed templating, allowlisted variables) | product templates | workspace:settings.manage | P1 |
| `notify.email_actionable` | bool | false | workspace:settings.manage | P1 |

### 2.10 Files

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `files.max_size` | bytes | 100 MB | file:manage_retention | ✓ | P0 |
| `files.types.policy` | allow-list or deny-list of MIME types | deny executables/scripts/macros | workspace:settings.manage | ✓ (platform deny list always applies) | P0 |
| `files.scan.required` ⚡ | bool | true (floor-locked if platform says so) | – | ✓ | P0 |
| `files.preview.office` | bool | true | workspace:settings.manage | – | P1 |
| `files.exif_strip` | bool | true | workspace:settings.manage | – | P0 |
| `files.download.watermark` | `off \| controlled \| all` | controlled | workspace:settings.manage | – | P1 |
| `files.download.log` | `restricted \| confidential_and_up \| all` | confidential_and_up | workspace:settings.manage | ✓ | P0 |
| `files.orphan_purge_after` | duration | 7 d | file:manage_retention | – | P0 |

### 2.11 Retention and legal hold

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `retention.classes` | per class duration + disposition mode ([14 §4](14-DATA-GOVERNANCE-AND-COMPLIANCE.md)) | 14 §4 defaults | workspace:settings.manage + step-up | ✓ (audit ≥ 1 y) | P1 |
| `retention.disposition.approval` | `auto \| approval_required` | approval_required | workspace:settings.manage | – | P1 |
| `legal_hold.managers` | role set | Compliance Officer, Security Admin | workspace:settings.manage | – | P1 |

### 2.12 Audit

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `audit.retention` | duration | max(linked records, 1 y) | – (computed; can be extended) | ✓ | P0 |
| `audit.checkpoint.interval` | events/minutes | 10 000 / 5 min | workspace:settings.manage | ✓ | P1 |
| `audit.tsa.url` | URL or off | off | workspace:settings.manage | – | P1 |
| `audit.worm_export` | object-lock target ref + retention | off | workspace:settings.manage + step-up | – | P1 |
| `audit.siem.streams` | list of destinations (format, endpoint, secret ref) | none | audit:export + step-up | – | P1 |
| `audit.anomaly.rules` | rule set (mass download N/10 min, off-hours privileged change, repeated denials…) | product defaults enabled | monitoring:manage_alerts | – | P1 |

### 2.13 Monitoring and alerts

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `monitoring.dashboards` | dashboard definitions + sharing | overview built-in | monitoring:manage_dashboards | P1 |
| `monitoring.alerts` | rules (metric, threshold, window, scope, recipients) | SLA breach spike, new admin, file quarantined | monitoring:manage_alerts | P1 |
| `monitoring.scheduled_reports` | report, format, cadence, recipients | none | report:manage_dashboards | P1 |
| `monitoring.stuck_instance_threshold` | duration | 7 d without progress | workspace:settings.manage | P0 |

### 2.14 Acknowledgement & Signature Campaign engine (shared)

One engine powers SOP re-acks, policy attestations, announcement acks and document e-signing. A **campaign** has a subject (a policy/SOP/announcement/document version), an audience, required action(s) (`read`, `ack`, `quiz`, `sign`), order, reminders, escalation, expiry and evidence output. Module settings (§2.15–2.17) supply defaults and the engine enforces them.

| Key | Type | Default | Perm | Floor | Ovr | Ph |
|---|---|---|---|---|---|---|
| `campaign.audience.dimensions` | enabled targeting dimensions (`department, group, role, position, location, employment_type, custom attribute`) | department, group, role | workspace:settings.manage | – | – | P0 |
| `campaign.audience.snapshot` | `at_launch \| dynamic` (dynamic adds new joiners matching the audience) | dynamic | workspace:settings.manage | – | ✓ | P0 |
| `campaign.reminders.cadence` | ladder (e.g. +3 d, +7 d, +14 d) | +3 d, +7 d | workspace:settings.manage | – | ✓ | P0 |
| `campaign.escalation.to_manager_after` | duration or off | 14 d | workspace:settings.manage | – | ✓ | P0 |
| `campaign.default_due` | duration | 14 d | workspace:settings.manage | – | ✓ | P0 |
| `campaign.read_time_min` | `off \| estimated` (require scroll-to-end + minimum dwell before ack) | off | workspace:settings.manage | – | ✓ | P1 |
| `campaign.quiz.pass_mark` | percent | 80 | workspace:settings.manage | – | ✓ | P1 |
| `campaign.quiz.max_attempts` | int | 3 | workspace:settings.manage | – | ✓ | P1 |
| `campaign.evidence.format` | `csv+pdf` (+ signed manifest) | csv+pdf | workspace:settings.manage | – | – | P0 |
| `campaign.ack_text` | localised statement ("I have read and understood…") | product text | workspace:settings.manage | – | ✓ | P0 |
| `campaign.ack.reauth` | `none \| stepup` | none (stepup in Regulated) | workspace:settings.manage | – | ✓ | P1 |
| `campaign.exemptions.allowed` | bool + approver role | true, Compliance Officer | workspace:settings.manage | – | ✓ | P1 |

### 2.15 Policies (policy library)

Policies follow the same governed lifecycle as SOPs (Draft → In review → Approved → Scheduled → Published → Deprecated → Retired) and are attested through campaigns.

| Key | Type | Default | Perm | Ovr | Ph |
|---|---|---|---|---|---|
| `policies.review_cadence` | duration | 365 d | workspace:settings.manage | ✓ (collection) | P0 |
| `policies.review.min_reviewers` | int | 1 (profile-driven) | workspace:settings.manage | ✓ | P0 |
| `policies.reack.policy` | `major_only \| every_publish \| none` | major_only | workspace:settings.manage | ✓ | P0 |
| `policies.attest_on_join` | bool (new members auto-enrolled in campaigns for applicable published policies) | true | workspace:settings.manage | ✓ | P0 |
| `policies.public_read_view` | `off \| workspace \| public_link` (public link = read-only, unauthenticated published version, e.g. a supplier code of conduct) | workspace | workspace:settings.manage + step-up for `public_link` | ✓ | P1 |
| `policies.exceptions.enabled` | bool (links to `grc.policy-exception` template) | true | workspace:settings.manage | – | P0 |
| `policies.numbering_pattern` | pattern | `POL-{seq:4}` | workspace:settings.manage | – | P0 |

### 2.16 Document e-signing

Sign arbitrary documents (contracts, offer letters, NDAs, approvals) with signers in order or in parallel, internal or external. Built on the campaign engine plus signature records (14 §3).

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `esign.enabled` | bool | true | workspace:settings.manage | platform may disable | P1 |
| `esign.signature_types` | subset of `simple` (click + typed name), `advanced` (re-auth + hash-bound, AES-style), `part11` (ESIG-01…05), `qes` (via QTSP connector, P2) | simple, advanced | workspace:settings.manage + step-up | ✓ (Regulated: part11 required for GxP collections) | P1 |
| `esign.external_signers` | bool (email OTP-verified guests) | false (Starter: true) | workspace:settings.manage | platform guest policy | P1 |
| `esign.external_verification` | `email_otp \| email_otp+sms_otp` | email_otp | workspace:settings.manage | – | P1 |
| `esign.order.default` | `sequential \| parallel` | sequential | workspace:settings.manage | – | P1 |
| `esign.reminders.cadence` | ladder | +2 d, +5 d | workspace:settings.manage | – | P1 |
| `esign.expiry.default` | duration | 30 d | workspace:settings.manage | ✓ (≤ 180 d) | P1 |
| `esign.decline.allowed` | bool + reason required | true | workspace:settings.manage | – | P1 |
| `esign.certificate.branding` | logo/colours/footer for the certificate of completion (audit trail page appended to the signed PDF) | workspace branding | workspace:branding.manage | – | P1 |
| `esign.output.pades` | bool (seal final PDF with platform/workspace key, PAdES-B-LT when TSA configured) | true | workspace:settings.manage | – | P1 |
| `esign.retention_class` | retention class | `governance_record` | workspace:settings.manage + step-up | ✓ | P1 |
| `esign.qtsp_connector` | connector ref ([13 INT-015](13-INTEGRATIONS.md)) | none | connector:configure | – | P2 |

### 2.17 Announcements

| Key | Type | Default | Perm | Ovr | Ph |
|---|---|---|---|---|---|
| `announcements.enabled` | bool | true | workspace:settings.manage | – | P1 |
| `announcements.posters` | role set allowed to post (scoped: a department-scoped poster can target only their department) | Workspace Admin, Compliance Officer, Team Manager (own dept) | workspace:settings.manage | ✓ (dept) | P1 |
| `announcements.approval_required` | `off \| for_workspace_wide \| always` | for_workspace_wide | workspace:settings.manage | ✓ | P1 |
| `announcements.audience.dimensions` | inherits `campaign.audience.dimensions` (may restrict) | inherit | workspace:settings.manage | – | P1 |
| `announcements.require_ack.default` | bool | false | workspace:settings.manage | ✓ | P1 |
| `announcements.escalate_unacked_to_manager` | duration or off | off (7 d when ack required) | workspace:settings.manage | ✓ | P1 |
| `announcements.expiry.default` | duration | 30 d | workspace:settings.manage | ✓ | P1 |
| `announcements.comments` | `off \| on \| moderated` | on | workspace:settings.manage | ✓ | P1 |
| `announcements.reactions` | bool | true | workspace:settings.manage | – | P1 |
| `announcements.channels` | subset of enabled notify channels; `pinned_banner` for critical | in_app, email | workspace:settings.manage | – | P1 |

### 2.18 AI

| Key | Type | Default | Perm | Floor | Ph |
|---|---|---|---|---|---|
| `ai.enabled` ⚡ | bool (kill switch) | false | ai:manage_settings | platform must enable a provider | P1 |
| `ai.features` | subset of `draft_process, draft_sop, summarize, translate, form_suggest` | none | ai:manage_settings | – | P1 |
| `ai.provider` | one of the platform-allowed providers | platform default | ai:manage_settings | ✓ | P1 |
| `ai.data_policy` | `no_restricted \| internal_ok` | no_restricted | ai:manage_settings | ✓ | P1 |

People-scoring and behaviour-based routing are **not configurable at all** (AI-01).

### 2.19 Integrations and quotas

| Key | Type | Default | Perm | Ph |
|---|---|---|---|---|
| `integrations.enabled` | connector keys (from platform allowlist) + config refs | none | connector:configure | P1 |
| `integrations.restricted_data_grants` | connector → allowed classifications | none | connector:configure + step-up | P1 |
| `quotas.*` | members, active instances, storage, API rpm, AI tokens/month | platform plan | **read-only** in workspace; system admin sets | P0 |

## 3. System Admin configuration (platform plane)

| Area | Settings | Ph |
|---|---|---|
| **Workspace lifecycle** | create (name, slug, region, isolation tier, first owner, plan/quotas), suspend (`read_only`/`locked` + banner message), schedule deletion (grace 7–90 d, default 30), restore, permanent delete (crypto-shred, certificate) | P0 |
| **Quotas & plans** | named plans (quota bundles), per-workspace overrides, soft-limit %, hard-limit behaviour | P0 |
| **Policy floors** | every key marked Floor in §2: MFA minimums, password, session, step-up window, file deny list, scan required, max file size, audit minimums, token TTLs, allowed locales | P0 |
| **Email** | SMTP host/port/TLS mode (STARTTLS required or implicit TLS), auth secret ref, from-address, DKIM guidance, test send | P0 |
| **Object storage** | endpoint, bucket(s), region, credentials ref, SSE mode, object-lock availability | P0 |
| **Malware scanning** | ClamAV endpoint or alternative adapter, signature freshness alert | P0 |
| **Template gallery** | publish/deprecate templates, featured lists, visibility per plan | P0 |
| **Configuration profiles** | edit/clone Starter/Standard/Enterprise/Regulated presets, custom platform profiles, which profiles self-service signup may pick | P0 |
| **Connectors allowlist** | which connectors workspaces may enable; egress default policy; internal CIDR allowlist | P1 |
| **AI providers** | provider configs (local endpoint / API), per-plan availability, data-processing region | P1 |
| **Identity** | system-plane MFA (mandatory WebAuthn recommended), system admin IP allowlist, break-glass approval rules | P0 |
| **Observability** | OTel exporter endpoints, log level, metrics retention | P0 |
| **Maintenance** | maintenance mode + banner, scheduled maintenance windows, upgrade pre-flight checks | P0 |

System settings follow the same versioning and audit rules as workspace settings (§4). They're recorded in the **system audit log** (FR-S09).

## 4. Versioning, audit and config-as-code

- **Every save creates an immutable `settings_revision`.** It records the author, the reason (required for ⚡ keys and floors) and a JSON diff. The UI history lets an admin pick any past revision and **restore** it, which is itself a new revision.
- **Two-person rule (optional, P1):** a workspace can require that changes to ⚡ keys, roles or retention be approved by a second admin. It reuses the governance review engine.
- **Config-as-code (FR-W12):** `GET /v1/config/export` returns a JSON document validated by `workspace-config.schema.json` (published with each release). It covers settings, custom roles, directory structure (optional), calendars, notification templates, dashboards, alert rules and connector configs **without secrets** (secret refs become `"<secret:name>"` placeholders).
  - `POST /v1/config/import?mode=plan` returns a diff plan. `mode=apply` applies it atomically, with per-key floor validation.
  - Process definitions travel separately as process bundles (FR-P09).
- **Sandbox → production promotion:** create a sandbox workspace (a system admin can clone config only, never content), iterate, export, then import into prod with plan review. The `procera` CLI supports this in CI (13 §5).
- Schema evolution: `configVersion` integer. The import runs migrations forward and rejects files from a newer product version.

## 5. Data model

**As built (BPMS-027, migration 0007):**
- `platform_settings (key, value, is_floor, revision)` holds platform values and floors.
- `workspace_settings (workspace_id, key, value, revision, updated_by)` holds values that differ from the code default; `updated_by IS NULL` marks a value applied from the creation profile.
- `settings_revisions` is the append-only history for every layer (platform changes sit under the system workspace).
- Department/collection overrides (layer 4) arrive with the directory (BPMS-023). Process-level values live in release bundles.
- Step-up keys need an MFA-verified session until the step-up endpoint ships (BPMS-022).

The original sketch, kept for reference:

```sql
CREATE TABLE settings (
  scope_type  text NOT NULL CHECK (scope_type IN ('platform','platform_floor','workspace','department','collection')),
  scope_id    uuid NOT NULL,              -- platform: fixed nil-uuid; workspace-scoped rows also carry workspace_id
  workspace_id uuid,                      -- NULL only for platform rows; RLS uses this
  key         text NOT NULL,              -- e.g. 'auth.session.idle_timeout'
  value       jsonb NOT NULL,             -- validated against catalogue JSON Schema for key
  revision_id uuid NOT NULL,
  updated_by  uuid NOT NULL,
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (scope_type, scope_id, key)
);
CREATE TABLE settings_revisions (
  id uuid PRIMARY KEY, workspace_id uuid, scope_type text NOT NULL, scope_id uuid NOT NULL,
  diff jsonb NOT NULL, reason text, author_id uuid NOT NULL, approved_by uuid,
  created_at timestamptz NOT NULL DEFAULT now()
);
```

- The **catalogue lives in code** (`crates/settings/src/catalogue.rs`; durations in seconds, sizes in bytes): key, JSON Schema, default, comparator, floor-bound, overridable scopes, required permission, ⚡ flag and phase. Generated docs and the admin UI form are both built from it, so they never drift.
- **Effective settings cache:** `(workspace, scope) → resolved map`, in-process (moka) with a TTL. A settings change writes an outbox event `settings.changed.v1{workspace_id, keys}`, and every API/worker node invalidates on receipt. ⚡ keys are additionally checked against the DB on security-sensitive paths (login, step-up) so they never wait on the cache.
- Process-level settings are part of the IR / release bundle, not this table.

## 6. Configuration backlog

| ID | Item | Phase | Size | Depends on |
|---|---|---|---|---|
| CFG-001 | Settings catalogue framework in code (schema, comparator, floor, scopes, perms) + generated docs | P0 | M | — |
| CFG-002 | Settings storage, revisions, RLS, effective-resolution engine + property tests (never looser than floor) | P0 | M | CFG-001 |
| CFG-003 | Effective-settings explainer API + UI | P0 | S | CFG-002 |
| CFG-004 | Cache + outbox invalidation; ⚡ key direct-read paths | P0 | S | CFG-002, outbox |
| CFG-005 | Workspace admin settings UI (auto-generated forms grouped per §2, search across settings) | P0 | L | CFG-001 |
| CFG-006 | System Admin console: workspace lifecycle, plans/quotas, policy floors, email/storage/scan config | P0 | L | CFG-002 |
| CFG-007 | Branding pipeline (logo sanitization, accent contrast derivation, email theming) | P0 | M | CFG-005 |
| CFG-008 | Terminology overrides across UI/email/export (i18n layer integration) | P0 | M | i18n framework |
| CFG-009 | Calendars & holiday sets with versions; iCal import (P1) | P0 | M | CFG-002 |
| CFG-010 | Custom roles editor with permission catalogue picker, scope, preview-of-effect, step-up | P0 | L | AuthZ engine |
| CFG-011 | Settings history + restore UI | P1 | S | CFG-002 |
| CFG-012 | Two-person rule for ⚡/roles/retention changes | P1 | M | Governance review engine |
| CFG-013 | Config-as-code export/import (schema, plan/apply, migrations, secret placeholders) | P1 | M | CFG-002 |
| CFG-014 | Sandbox workspace clone (config only) + promotion via CLI | P1 | M | CFG-013, INT-028 |
| CFG-015 | Floor-raise clamp job + admin notifications | P0 | S | CFG-002 |
| CFG-016 | Separation mode toggle with 24 h notice + permission remap | P1 | M | CFG-010 |
| CFG-017 | Configuration profiles (4 presets as schema-validated JSON), pick-at-creation, profile switch via plan/apply | P0 | M | CFG-013 (plan/apply core pulled into P0) |
| CFG-018 | Custom platform profiles (clone/edit) in System Admin console | P1 | S | CFG-017 |
| CFG-019 | Delegated (department/collection-scoped) admin: scoped override editing + guard against ⚡ keys | P1 | M | CFG-010 |
| CFG-020 | Acknowledgement & Signature Campaign engine: campaign model, audience resolution (snapshot/dynamic), reminders, manager escalation, exemptions, evidence export | P0 | L | Directory, notifications |
| CFG-021 | Campaign settings group (§2.14) + per-module defaults wiring | P0 | S | CFG-020 |
| CFG-022 | Read-time / scroll-to-end and quiz support in campaigns | P1 | M | CFG-020 |
| CFG-023 | Policies module settings (§2.15) incl. attest-on-join and public read view (sanitized, read-only, rate-limited) | P0/P1 | M | CFG-020, governance lifecycle |
| CFG-024 | E-signing settings (§2.16): signature types, external signers (email OTP), order, expiry, certificate branding, PAdES sealing, retention class | P1 | L | CFG-020, signatures (14 §3) |
| CFG-025 | Announcements settings (§2.17): posters by role/scope, approval, audience, ack + manager escalation, expiry, comments moderation | P1 | M | CFG-020 |
