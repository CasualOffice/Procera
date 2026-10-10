# 02 — Product Requirements Document (PRD)

Version 0.2 · 2026-10-11 · Product **Procera** · Supersedes
[v0.1](archive/v0.1/02_PRD.md).

## 1. Vision

One accessible workspace to **discover, document, govern, run, measure and improve** how
an organization works — where **the approved SOP is the running process**.

- *Easy* for the 90%: employees find a process, start a request, approve from anywhere,
  and acknowledge an SOP without training.
- *Powerful* for the 10%: owners, designers and compliance teams get versioned models,
  rules, SLAs, SoD, signatures, evidence and analytics.
- *Trustworthy* by construction: immutable published versions, tamper-evident audit,
  workspace isolation, explicit security protocols.

It is not a task tracker: work is an output of processes, not the primary object.

### 1.1 Who it is for

**Every organization size, every department.**

| Segment | Typical shape | What they need first | Default profile |
|---|---|---|---|
| **Startup** (5–100) | One workspace, founders/ops as admin, no IdP | Zero-config. Install templates (leave, expense, onboarding, policy ack), get going in minutes, local accounts + MFA. | *Starter* |
| **SME** (100–2 000) | 1–3 workspaces, HR/IT/Finance admins, maybe Google/M365 SSO | Policies & announcements with acks, employee e-signing, approvals, light SOP governance, dashboards. | *Standard* |
| **Large enterprise / MNC** (2 000+) | Many workspaces (per entity/region/department), delegated admins, SSO+SCIM, regional data | Governance at scale, SoD, audit/investigations, dedicated DB tier, residency, localisation. | *Enterprise* |
| **Regulated** (pharma, BFSI, public sector) | Any size | Part 11 signatures, Separation mode, retention/legal hold, validation pack. | *Regulated* |

Departments served out of the box (via templates and modules):
- HR (onboarding, leave, policies, offer/NDA signing, acknowledgements)
- IT (access, change, incidents, assets)
- Finance/Procurement
- Compliance/Risk/Legal
- Quality
- Operations/Facilities
- Sales/Customer
- Administration (announcements, notices)

A workspace picks a **configuration profile** at creation (Starter / Standard / Enterprise / Regulated). The profile pre-sets MFA, SoD, review, signature-mode and retention defaults. Every setting remains changeable later ([20-WORKSPACE-CONFIGURATION](20-WORKSPACE-CONFIGURATION.md)).

## 2. Product principles

1. **Standalone first.** Every P0 capability works with zero integrations: local accounts,
   in-app + email notifications, built-in forms, files and directory. Integrations (SSO,
   HRIS, ERP, chat platforms) are additive layers ([13-INTEGRATIONS](13-INTEGRATIONS.md)).
2. **Workspace = tenant.** Multi-tenant from day one; a System Admin plane manages all
   workspaces ([12-AUTHZ](12-AUTHORIZATION-MODEL.md)).
3. **Progressive disclosure.** Business language by default (Request, Step, Owner,
   Decision, Deadline); advanced power (rule tables, boundary events, BPMN view, SoD
   tuning) is one click away, never in the way.
4. **Templates before blank canvas.** An industry-standard process library ships with the
   product ([19-TEMPLATES](19-PROCESS-TEMPLATE-LIBRARY.md)).
5. **Configurable, safe by construction.** Admins compose roles, settings and policies
   from catalogues; invariants (isolation, SoD, signatures, audit) cannot be configured away.
6. **Boundaries are sacred.** Model ≠ published definition ≠ running instance ≠ governed
   SOP ≠ ad hoc case.
7. **Honest claims.** No "BPMN-conformant", "compliant" or "certified" claims without
   evidence (06, 14).

## 3. Personas

| Persona | Plane | Main jobs | Default role |
|---|---|---|---|
| **System Admin** | Platform | Create/suspend workspaces, quotas, platform settings, global template gallery, health | `system.admin` |
| **Platform Operator** | Platform | Run the deployment, backups, incidents (metadata only) | `system.operator` |
| **Workspace Owner/Admin** | Workspace | Members, roles, directory, settings, branding, template install, retention | Workspace Owner / Admin |
| **Process Owner** | Workspace | Accountable for a process: SOP, model, KPIs, publication, review cadence | Process Owner |
| **Process Designer** (analyst) | Workspace | Model, configure forms/rules, test-run, compare versions | Process Designer |
| **Reviewer / Compliance Officer** | Workspace | Review/approve changes, controls, attestations, evidence, audits | Reviewer, Compliance Officer |
| **Team Manager** | Workspace | Approve, watch team workload, reassign, out-of-office cover | Team Manager (+ Member) |
| **Employee / Requester** | Workspace | Find process, submit request, track it, acknowledge SOPs | Member |
| **Participant / Approver** | Workspace | Complete assigned tasks and decisions | Member (+ contextual role) |
| **Case Manager / Investigator** | Workspace | Run flexible cases with evidence | Case Manager / Investigator |
| **External Guest** (P1) | Workspace | Fill one form / act on one item (vendor, candidate) | Guest |

## 4. Scope by plane

```
SYSTEM PLANE      Workspaces · Quotas · Platform settings · Template gallery · Health · System audit
WORKSPACE PLANE   Library & SOPs · Designer · Requests & My Work · Cases · Improve/Reports · Admin
```

## 5. Functional requirements

> The complete inventory, including ~35 additional `FX-` features from the 2026-10-11 parity sweeps (DoA matrix, recall/send-back, master data, request portal, envelope correction, etc.), lives in [21-FEATURE-CATALOGUE](21-FEATURE-CATALOGUE.md). If this section and the catalogue disagree on priority, the catalogue wins.

Priority: **P0** = pilot (M6) · **P1** = GA (M10) · **P2** = expansion. IDs are stable;
new ones are appended. Acceptance criteria (AC) are testable and referenced by the tracker.

### 5.1 System administration (new)

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-S01 | Install bootstrap creates the first `system.owner` (one-time token printed in logs / CLI) with mandatory MFA enrolment. | P0 | Bootstrap token single-use; expires 24 h; no default credentials exist. |
| FR-S02 | Workspace lifecycle: create, rename, suspend (read-only / locked), schedule deletion with grace period, restore within grace, permanent delete (crypto-shred). | P0 | Suspended workspace rejects writes with 423; deletion purges data + keys; audit in system log and export offered. |
| FR-S03 | Assign/replace the first Workspace Owner; invite by email. | P0 | Workspace cannot exist without ≥ 1 owner (except during provisioning). |
| FR-S04 | Quotas per workspace (members, active instances, storage, API rate, AI usage) with usage dashboard and soft/hard limits. | P0 | Hard limit returns 429/507 with clear message; soft limit notifies workspace admins. |
| FR-S05 | Platform settings: SMTP, object storage, AI providers, default locale, platform policy floors (MFA, password, session). | P0 | Workspace settings can tighten but not loosen floors (enforced server-side). |
| FR-S06 | Global template gallery management: publish, version, deprecate templates; see install counts. | P0 | Template publish requires lint-clean IR + fixtures pass. |
| FR-S07 | Break-glass access with workspace approval, time box, read-only default, workspace-visible audit. | P1 | All requests during session tagged; banner shown in workspace; auto-expiry ≤ 4 h. |
| FR-S08 | Platform health: job backlog, timer lateness, outbox lag, error rates, storage, per-workspace usage. | P0 | Metrics also exposed as Prometheus/OTel. |
| FR-S09 | System audit log (workspace lifecycle, admin role changes, break-glass, platform settings). | P0 | Append-only, hash-chained (AUD-01). |
| FR-S10 | Isolation tier per workspace (shared / dedicated DB) set at creation (P1 for dedicated). | P1 | Cross-tier tenant tests pass. |

### 5.2 Workspace administration & configuration (new; detail in [20-WORKSPACE-CONFIGURATION](20-WORKSPACE-CONFIGURATION.md))

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-W01 | Members: invite (email/link with domain restriction), deactivate, remove, bulk CSV import, member profile (title, department, manager, locale, timezone). | P0 | Deactivated member loses access immediately; open work re-routed (SEC-04). |
| FR-W02 | Directory: department/team tree, groups, positions, manager links; CSV import; source attribution per field. | P0 | Manager resolution deterministic; cycles rejected. |
| FR-W03 | **Configurable roles**: built-in role templates, clone → custom role, permission catalogue picker, default scope, assignment to users/groups/directory rules, time-boxed grants, change preview, audit diff. | P0 | No-escalation rule enforced; preview count matches actual effect (property test). |
| FR-W04 | Workspace settings: name, logo, colours (accessible contrast check), terminology overrides (e.g. "Request" → "Ticket"), default locale/timezone, working hours, number formats, request ID pattern (`PR-{yyyy}-{seq}`). | P0 | Terminology applied across UI and emails; contrast ≥ 4.5:1 enforced. |
| FR-W05 | Business calendars & holiday sets (versioned), multiple per workspace; import iCal. | P0 | Timers pin calendar version (B7). |
| FR-W06 | Security settings: MFA requirement by role, session timeouts, allowed email domains, IP allowlist (P1), SSO enforcement (P1). | P0 | Within platform floors. |
| FR-W07 | Governance policy: which criticality needs independent review, # reviewers, re-acknowledgement rules, review cadence defaults. | P0 | Enforced at publish. |
| FR-W08 | Retention & legal hold settings per record class (14 §4). | P1 | Retention jobs honour holds. |
| FR-W09 | Notification settings: channels on/off, digest schedule, email sender name, quiet hours. | P0 | — |
| FR-W10 | Workspace export (full, open formats) and deletion request. | P1 | Export = JSON + BPMN XML (supported subset) + files + audit chain; verifier passes. |
| FR-W11 | Access review campaigns (recertify role grants). | P1 | Unconfirmed grants auto-expire if policy says so. |
| FR-W12 | Config-as-code: export/import workspace configuration (roles, calendars, settings) as versioned JSON for promotion between workspaces (e.g. sandbox → prod). | P1 | Import shows a diff; secrets never exported. |

### 5.3 Process library & governance

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-G01 | Hierarchical landscape (domain › capability › process › subprocess) with collections, owners, RACI, tags, criticality (`standard`/`controlled`), visibility, search. | P0 | Search returns only authorized items; RACI exportable. |
| FR-G02 | Process detail: as-is/to-be, diagram, inputs/outputs, KPIs, dependencies, linked SOPs/controls, versions. | P0 | — |
| FR-G03 | SOP editor (structured blocks: purpose, scope, roles, steps, exceptions, records, references), attachments, linked controls, checklists, acknowledgements (per version, optional quiz), review schedule, retention. | P0 (quiz P1) | Re-ack required on major version per policy; ack evidence exportable. |
| FR-G04 | Canonical artifact lifecycle `Draft → In review → Approved → Scheduled → Published → Deprecated → Retired` with configurable SoD. | P0 | Illegal transitions rejected; author ≠ approver for controlled. |
| FR-G05 | Immutable published versions; semantic diff (steps, rules, forms, roles, SOP text); change requests; effective dates; comments. | P0 | Published version byte-identical on retrieval (checksum); diff covers all bundle artifacts. |
| FR-G06 | Attestation campaigns, evidence collection, exception register, audit exports (CMP-01/03). | P1 | — |
| FR-G07 | Ownership handover, periodic recertification, overdue review alerts. | P1 | Orphaned process (owner deactivated) flagged within 24 h. |
| FR-G08 | **Template gallery**: browse industry templates, preview, install wizard (map role placeholders, calendars, thresholds), installed-copy lineage, upgrade-offer diff. | P0 | Install → test run in < 10 min for Wave-1 templates. |
| FR-G09 | Import: BPMN 2.0 XML (with compatibility report), Excel/CSV landscape, `.docx`/Markdown SOP. Visio/ARIS/Signavio/Bizagi P2. | P1 (BPMN P0) | Unsupported elements listed by id; nothing silently dropped. |
| FR-G10 | Export: SOP/process to PDF (tagged, watermarked if superseded/draft) and DOCX; landscape to XLSX. | P0 (PDF) / P1 | Superseded watermark present. |
| FR-G11 | Electronic signatures on approvals and publication (simple / Part 11 mode). | P1 | ESIG-01…05. |
| FR-G12 | Improvement proposals ("Idea" object) linked to a process; can spawn a draft. | P1 | — |

### 5.4 Design & execution

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-E01 | **Step builder** (default) with node kinds from IR v1; **flow canvas** view; optional BPMN view (P2). Inline validation, undo/redo, autosave, keyboard-only authoring. | P0 | Keyboard-only create/publish (T10); validation errors link to node. |
| FR-E02 | Launch manually, via API, schedule, or authenticated inbound event (event P1). | P0 | — |
| FR-E03 | Form builder: typed fields, layout, conditional visibility, validation, file upload, field classification; JSON Schema output; preview on mobile/desktop. | P0 | Server validates with identical schema. |
| FR-E04 | Assignment: user/group/role/manager/directory rule; claim, round-robin, quorum; delegation, **out-of-office substitution**, reassignment, due dates, escalation. | P0 | SoD re-checked at decision; OOO applied to new work. |
| FR-E05 | Decisions: approve/reject/request changes/custom outcomes; comments, attachments; safe bulk approve (only where permitted and no restricted fields). | P0 | Approval race: exactly one commit (T01). |
| FR-E06 | Durable instances; pause/resume/cancel/retry; idempotent APIs; crash recovery. | P0 | Restart mid-transition loses nothing (T02). |
| FR-E07 | Version pinning via release bundle; governed migration of active instances (P2 tool; P0 = cancel & restart with carried data). | P0/P2 | 30-day dormant instance completes on pinned version (T03). |
| FR-E08 | Subprocesses and linked cases; comments, participants, timeline. | P1 | — |
| FR-E09 | Outbound notifications in P0: in-app + email. Webhooks P1. Connectors (HTTP, HRIS, chat platforms) per [13](13-INTEGRATIONS.md). | P0/P1 | — |
| FR-E10 | Rule tables (UNIQUE/FIRST/COLLECT); DMN import/export P2. | P0 (tables) | Deterministic evaluation; rule version recorded. |
| FR-E11 | **Test run** of a draft with simulated participants; no real notifications or side effects; trace view. | P0 | Test runs never appear in metrics or real inboxes. |
| FR-E12 | SLA definitions with business-calendar clocks, pause states, breach actions. | P0 | SLA attainment recomputable from events. |
| FR-E13 | Ops incidents: failed steps surface to process owner/admin with retry/skip/cancel actions. | P0 | Every failure produces exactly one open incident until resolved. |

### 5.5 Requests, work and notifications

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-R01 | Start page: search + favourites + "most used in my department"; start form with autosave draft. | P0 | ≥ 80% of pilot users start a request untrained. |
| FR-R02 | Request detail: progress stepper, current owner, next step, ETA (SLA), comments, attachments, history, cancel (if allowed). | P0 | — |
| FR-R03 | My Work inbox: due-sorted, filters, saved views, keyboard triage, decision panel without leaving the list. | P0 | Decision in ≤ 3 interactions from inbox. |
| FR-N01 | In-app notification centre (actionable, grouped). | P0 | — |
| FR-N02 | Email notifications with localised templates (workspace terminology). | P0 | — |
| FR-N03 | **Email-actionable approvals** via signed, single-use, short-lived links that land on a confirm screen (never a GET that mutates). | P1 | Link replay rejected; re-auth required for Part 11 processes. |
| FR-N04 | Daily/weekly digests; quiet hours; critical SLA override per workspace policy. | P0 | — |
| FR-N05 | Web push (PWA) for assigned work. | P1 | — |
| FR-N06 | Chat channels (Slack/Teams/others) as integration add-ons. | P2 | Via [13](13-INTEGRATIONS.md). |

### 5.6 Cases, insights, improvement

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-C01 | Case intake, stages, case file, related parties, milestones, discretionary activities, case templates. | P1 | — |
| FR-C02 | Exceptions, investigations, evidence chain-of-custody, closure checklist. | P1 | Closure blocked until mandatory items complete. |
| FR-I01 | Metrics: volume, cycle/wait time, rework, SLA attainment, abandonment, bottlenecks — with sample sizes and exclusions. | P0 (basic) | Numbers reconcile with raw events (±0). |
| FR-I02 | Version comparison (median cycle time v3 vs v4), improvement review. | P1 | — |
| FR-I03 | Event-log export: OCEL 2.0 JSON/SQLite and XES (IEEE 1849-2023). | P2 | Validates against schema. |
| FR-I04 | Statistical simulation of a draft. | P2 | — |
| FR-I05 | Standard reports + CSV/XLSX export; dashboard builder P2; attestation coverage, process health score. | P0/P1 | Exports redacted identically to UI. |

### 5.6a Policies, document signing, announcements and acknowledgements (new)

These modules sit on one shared engine: **Acknowledgement & Signature Campaigns**. The same engine drives SOP and policy re-acknowledgement, attestations (FR-G06), announcement acks and document signing. Employees therefore see one consistent "Acknowledge / Sign" queue in My Work, and admins get one campaign tracker.

**Policies**

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-PL01 | Policy library: policies are governed documents (owner, category, applicability by department/location/role, effective date, review cadence, versions) on the canonical lifecycle (FR-G04). They link to related SOPs, processes and controls. | P0 | A published policy is immutable. A superseded version is watermarked in views and exports. |
| FR-PL02 | Employee policy portal: everyone sees policies applicable to them, searchable, with "what changed" summaries between versions. | P0 | Applicability filters are enforced. Non-applicable restricted policies are hidden. |
| FR-PL03 | Publishing a major version can trigger a re-acknowledgement campaign to the applicable audience (policy setting). | P0 | Ack evidence ties to the exact version hash. |
| FR-PL04 | Policy exception requests via the `grc.policy-exception` template, linked back to the policy. | P1 | — |

**Document signing (e-signature)**

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-DS01 | Create a signing request from an uploaded PDF/DOCX (DOCX converted to PDF in a sandbox) or from a template document with merge fields (e.g. offer letter: `{{employee.name}}`, `{{salary}}`). | P0 (PDF) / P1 (DOCX, merge) | The signed artefact is the exact rendered PDF whose hash was shown to signers. |
| FR-DS02 | Signers: workspace members, groups (bulk send, e.g. "all employees sign Code of Conduct" creates one envelope per person), and external signers by email (P1, guest magic link + OTP). Sequential or parallel order, CC recipients, optional approver before send. | P0 (members, bulk) / P1 (external) | Bulk send of 5 000 envelopes completes asynchronously with progress and per-recipient status. |
| FR-DS03 | Field placement: signature, initials, date-signed, name, text, checkbox, dropdown, attachment. Fields assigned per signer, required/optional, drag-to-place with a keyboard-placement alternative (WCAG 2.5.7). | P0 (sig/date/name/text/checkbox) | Required fields enforced server-side. |
| FR-DS04 | Signing ceremony: mobile-first and accessible. Review document → consent to electronic signing → adopt signature (type / draw / upload) → sign → confirmation. Decline with reason. Re-authentication per signature mode (simple: session; strong: password/passkey/OTP; Part 11: ESIG-03). | P0 | Ceremony completes at 360 px width. A screen-reader user can sign. Consent is recorded. |
| FR-DS05 | Completion: flattened signed PDF with a visible signature manifestation (ESIG-01), an embedded/attached **certificate of completion** (signers, timestamps, IPs, auth method, document hashes, event log), PAdES signature of the platform (P1), and optional RFC 3161 timestamp (P1). | P0 (certificate) / P1 (PAdES, TSA) | Any change to the PDF after completion invalidates verification. The verifier shows the result. |
| FR-DS06 | Tracking: envelope dashboard (sent / viewed / signed / declined / expired) on the shared DataGrid, auto-reminders, expiry, void with reason, resend, audit trail per envelope. | P0 | Every view/sign/decline is in the audit trail. |
| FR-DS07 | Storage: signed documents get retention class `governance_record` or as configured. They are linked to the employee's record and the originating request/process. Signing can be a step in any process (IR node `human.sign`, IR v1.1). | P0 (storage) / P1 (process step) | — |
| FR-DS08 | Legal posture: simple and advanced-style electronic signatures natively. Qualified signatures (eIDAS QES) via a QTSP connector (P2). Legal-validity guidance per jurisdiction is documented, not asserted (14). | P2 (QES) | — |

**Announcements**

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-AN01 | Compose an announcement: rich text, images, attachments, links to policies/SOPs. Categories; urgent flag. | P0 | — |
| FR-AN02 | Audience targeting: everyone, departments (with subtree), groups, roles, locations, custom directory rule. Preview of recipient count. | P0 | Recipients are resolved at publish and recorded (an audience snapshot). |
| FR-AN03 | Schedule publish, expiry, pin to Home, optional comments/reactions (workspace setting). | P0 | Expired announcements are archived, not deleted. |
| FR-AN04 | **Require acknowledgement** with a deadline (and an optional short quiz). Read receipts, reminders, escalation to the recipient's manager after the deadline. | P0 (quiz P1) | Ack records user, version hash and time. Non-ack escalates once per policy. |
| FR-AN05 | Delivery: in-app feed + banner (urgent), email. Web push P1. Chat platforms P2. | P0 | — |
| FR-AN06 | Campaign analytics: reach, read %, ack % by department/location over time, overdue list exportable. | P0 | Numbers reconcile with ack records. |

**Acknowledgement & Signature Campaign engine (shared)**

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-AK01 | Campaign = subject (SOP/policy version, announcement, document, attestation statement) + audience snapshot + required action (`read`, `acknowledge`, `quiz`, `sign`) + deadline + reminder/escalation schedule. | P0 | One engine; all modules use it. |
| FR-AK02 | Employee queue "Acknowledge / Sign" in My Work, with counts on Home and in notifications. | P0 | — |
| FR-AK03 | New joiners automatically receive the campaigns still applicable to them (e.g. all current policies), via a rule on the campaign. | P0 | Joining a department triggers outstanding applicable campaigns. |
| FR-AK04 | Evidence export per campaign (who, what version, when, how authenticated) for audits (CMP-03, QMS-03). | P0 | — |

### 5.7 Files and documents (new)

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-F01 | Upload files to forms, comments, SOPs, cases and evidence. Drag-drop, paste and mobile camera. Chunked/resumable upload for files ≥ 10 MB. Per-workspace size and type policy (allow/deny list, default max 100 MB). | P0 | Interrupted upload resumes. Disallowed type rejected server-side by content sniffing, not the extension. |
| FR-F02 | Security pipeline: SHA-256 hash, magic-byte type detection, **malware scan** (ClamAV adapter by default, pluggable), quarantine state, image EXIF stripping (optional). Files are unusable until `clean`. | P0 | EICAR test file quarantined; quarantined file cannot be downloaded except by `file:manage_quarantine`. |
| FR-F03 | Storage: S3-compatible object store. Keys are bound to the workspace (`ws/{workspace_id}/…`). Per-workspace envelope encryption. Short-lived signed download URLs (≤ 5 min), bound to the user and workspace and re-authorized on every request. | P0 | Signed URL fails after expiry and in another workspace (SEC-10). |
| FR-F04 | In-browser preview: images, PDF, text/Markdown, Office documents (via conversion to PDF in a sandboxed worker; optional OpenDoc engine for `.docx`). No preview runs active content. | P0 (img/pdf) / P1 (office) | Preview runs in a sandboxed iframe with strict CSP. |
| FR-F05 | File versions (SOP attachments, evidence replacement keeps history), immutable once linked to a published version or a signed record. | P0 | A file referenced by a published version can't be deleted or overwritten. |
| FR-F06 | **Workspace Files view** (admin): every file in the workspace, with owner, linked object, size, type, scan status, retention class, legal hold, last accessed. Full data-table features (§5.9). Bulk actions: retention change, hold, delete (where allowed), export. | P0 | Deleting a held or retention-locked file is refused with the reason. |
| FR-F07 | Storage quota usage per workspace, broken down by process, user and type. Orphaned-file report and cleanup. | P0 | Orphans (no referencing object after 24 h) listed and purgeable. |
| FR-F08 | Download/access logging for confidential/restricted files and evidence. Watermarked downloads for controlled documents (user, time). | P1 | Each download appears in the audit trail. |
| FR-F09 | Retention and legal hold for files follow the record class of the owning object (14 §4). Crypto-shred on deletion. | P1 | — |

### 5.8 Admin console, monitoring and dashboards (new)

The Workspace Admin has a full control centre over their workspace. The System Admin has the equivalent for the platform (FR-S08).

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-A01 | **Workspace overview dashboard**: active requests, overdue work, SLA risk, stuck instances/incidents, throughput trend, top processes, pending reviews and attestations, storage and quota, security events (failed logins, MFA gaps, new admins). Time range and department filters. | P0 | Every tile drills down into a pre-filtered table. No empty tiles: show "no data yet" guidance instead. |
| FR-A02 | **All requests** (admin): every instance in the workspace with state, process/version, initiator, current step and assignee, age, SLA status and department. Actions: reassign, suspend/resume, cancel, retry, add comment, open timeline. | P0 | Admin actions require a reason and are audited. Bulk actions show an impact preview. |
| FR-A03 | **All work items** and workload view: per person, team and queue. Overdue and unassigned work, OOO coverage gaps. Bulk reassign. | P0 | — |
| FR-A04 | **Process health**: per process volume, cycle-time percentiles, SLA attainment, rework, abandonment, incident rate, last review date, owner status. | P0 | Reconciles with raw events. |
| FR-A05 | **Operations monitor** (workspace-scoped): failed steps/incidents, connector delivery failures, email bounces, timer lateness. Retry/skip/cancel from the list. | P0 | — |
| FR-A06 | **People & access monitor**: members, last active, roles, MFA status, pending invites, time-boxed grants nearing expiry, privileged-role holders, dormant accounts. | P0 | — |
| FR-A07 | **Alert rules** (workspace): e.g. "SLA breaches > 10/day in Finance", "new Workspace Admin added", "file quarantined", "mass download". Delivered in-app and by email (webhook P1). | P1 | Alerts deduplicate and record acknowledgement. |
| FR-A08 | **Custom dashboards**: tiles from a metric catalogue (counts, percentiles, trends, tables). Shareable to roles. | P1 (fixed P0, builder P1) | Tiles respect the viewer's permissions (the same dashboard shows different data to different roles). |
| FR-A09 | Scheduled reports by email (PDF/XLSX), workspace-branded. | P1 | — |

### 5.9 Data tables and views (cross-cutting UX requirement)

Every list in the product (requests, work, processes, SOPs, files, members, audit events, cases, incidents) uses **one shared data-grid component** with consistent behaviour:

| ID | Requirement | Pri |
|---|---|---|
| FR-V01 | Server-side **sorting** on any sortable column, multi-column sort (shift-click). | P0 |
| FR-V02 | **Filtering**: quick search, per-column filters typed by data type (text contains/equals, number/date ranges, relative dates like "last 7 days", enum multi-select, person picker, "is empty"), AND/OR filter groups (advanced), filter chips. | P0 |
| FR-V03 | Column chooser, reorder, resize, pin. Density toggle. Grouping by a column with counts (P1). | P0 (group P1) |
| FR-V04 | **Saved views**: personal and shared (to roles). Default view per list. The URL encodes the view state, so it can be deep-linked. | P0 |
| FR-V05 | Cursor pagination plus virtualised scrolling. Counts are exact up to 10 k, approximate after (labelled). | P0 |
| FR-V06 | Row selection and bulk actions with an impact preview and per-row result report. | P0 |
| FR-V07 | **Export** CSV/XLSX of the current view (filters + columns), async for large sets, with the same redaction as the UI and an audited download. | P0 |
| FR-V08 | Full keyboard operation (grid pattern, WAI-ARIA), screen-reader announcements for sort/filter changes, responsive card layout ≤ 640 px. | P0 |
| FR-V09 | Filter/sort queries are server-validated against an allowlisted schema per list (no raw SQL, no unindexed sort on huge tables without a warning). | P0 |

### 5.10 Audit trail and investigations (new)

| ID | Requirement | Pri | AC |
|---|---|---|---|
| FR-T01 | **Complete audit trail** of every security-relevant and business-relevant action across the workspace. Covers: auth events (login, failed login, MFA, session revoke); member/role/permission changes; settings; process/SOP lifecycle; every decision and signature; admin interventions; file upload/download/delete; **data access to restricted data** (`audit.data_access`); exports; API token use; break-glass. Each event records who (and on-behalf-of), what, which object, before→after (hashed for restricted values), when, from where (IP, user agent, session), why (reason) and trace id. | P0 | Coverage test: every mutating endpoint emits exactly one audit event (contract test generated from the route table). |
| FR-T02 | Tamper-evidence: per-workspace hash chain (AUD-01), DB-level append-only (AUD-07), signed Merkle checkpoints and optional RFC 3161 timestamps (P1), offline verifier (P1). | P0/P1 | Verifier detects a modified, deleted or reordered event. |
| FR-T03 | **Audit explorer**: data-grid over audit events (§5.9) with filters for actor, action category, object type/id, time range, IP, result (allowed/denied) and severity. Saved queries. | P0 | Querying 10 M events with indexed filters returns in < 2 s at P95. |
| FR-T04 | **Timelines**: per user ("everything Alice did/accessed in March"), per object (the full history of a request, SOP, file or role), per session. Before/after diff view. | P0 | — |
| FR-T05 | **Investigations**: an `Investigation` case type for the audit team. Open an investigation, pin audit events, files and requests as evidence (pinned snapshots with hashes), add notes and findings, assign investigators, apply legal hold to the related records, and close with an outcome. Investigations are themselves fully audited and visible only to `audit:investigate` holders. | P1 | Pinned evidence is immutable. Export includes the verification manifest. |
| FR-T06 | **Evidence export**: selected events or an investigation as a signed bundle (JSON + human-readable PDF + manifest with hashes + chain proofs). | P1 | `procera-verify` validates the bundle offline. |
| FR-T07 | Anomaly signals (rule-based, no people-scoring): mass downloads, off-hours privileged changes, repeated access denials, SoD override attempts, dormant-admin reactivation. Shown in the audit explorer and alertable (FR-A07). | P1 | Rules are configurable and explainable. Each signal links to its events. |
| FR-T08 | SIEM streaming of audit events (syslog/CEF, JSON over HTTPS, OTel logs). | P1 | At-least-once with sequence numbers. |
| FR-T09 | Audit retention ≥ the longest linked record retention and ≥ 1 year. Audit can't be deleted by any workspace role. Workspace deletion keeps a pseudonymised system-plane record of lifecycle events only. | P0 | — |

### 5.11 Platform qualities

| ID | Requirement | Pri |
|---|---|---|
| FR-P01 | Multi-tenant workspaces, isolation tiers (12-AUTHZ §2). | P0 |
| FR-P02 | Local auth + MFA (P0); OIDC/SAML-via-broker, SCIM (P1); API tokens (P0). | P0/P1 |
| FR-P03 | Retention, residency, export, deletion, legal hold (14). | P1 |
| FR-P04 | WCAG 2.2 AA; full keyboard; localisation (ICU MessageFormat, RTL-ready); English first, Hindi + one EU language at GA (OQ-09). | P0 (a11y) |
| FR-P05 | Workspace-scoped, hash-chained audit trail; reporting; observability; backup/restore. | P0 |
| FR-P06 | Self-hosted: Docker Compose (trial) and Helm (prod); air-gapped install (P1). | P0 |
| FR-P07 | Responsive PWA: My Work, request start/detail, decisions, acknowledgements fully usable at 360 px. | P0 |
| FR-P08 | External guests (magic link, single item). | P1 |
| FR-P09 | Promote definitions between workspaces (sandbox → prod) via export/import bundle. | P1 |
| FR-P10 | Public, versioned REST API (OpenAPI 3.1) + signed webhooks; CLI. | P0 (API) / P1 |

## 6. Key user journeys (with acceptance)

| J | Journey | Must hold |
|---|---|---|
| J0 | **Day-zero**: System Admin installs, creates workspace "Acme", invites owner; owner configures branding/roles, installs "Purchase requisition" template, maps roles, test-runs, publishes. | ≤ 30 min from empty workspace to first published process (success metric). |
| J1 | Employee onboarding (template): HR starts; manager approves; IT task; employee sees status. | Approval race safe; survives restart; every decision attributed; other workspaces can't see it. |
| J2 | SOP recertification: reminder → edit → independent review → publish at effective date → re-ack campaign. | Old version retrievable; diff + approval evidence retained; overdue flagged. |
| J3 | Purchase request with thresholds via rule table. | Deterministic; reassignment never orphans; duplicate submit with same idempotency key returns same request. |
| J4 | Incident case with evidence and ad hoc steps. | Per-file/field permissions; complete audit export. |
| J5 | Improvement: bottleneck dashboard, compare versions. | Sample sizes/exclusions shown; timezone-consistent. |
| J6 | **Custom role**: admin clones "Process Designer" → "HR Designer" scoped to HR collection; preview shows effect; audit records diff. | No escalation; scope enforced on every endpoint. |
| J7 | **Out-of-office**: manager sets substitute for 2 weeks; new approvals route; SoD still enforced. | Expiry automatic; "on behalf of" recorded. |
| J8 | **System Admin** suspends a workspace for non-payment, later restores it. | No data loss; members see read-only banner. |
| J9 | **Admin monitoring**: Workspace Admin opens the overview, sees 12 overdue approvals in Finance, drills into the table, filters "overdue > 3 days, department = Finance", bulk-reassigns to the deputy with a reason, and saves the view. | Drill-down preserves filters; bulk action audited per item; view shareable. |
| J10 | **Investigation**: compliance gets a report of leaked salary data; opens an investigation; audit explorer filters `data_access` on field `salary` over the last 30 days; pins the events and the files involved; applies legal hold; exports a signed bundle for HR/legal. | Every access to the restricted field is present; the bundle verifies offline; the investigation itself is audited. |
| J12 | **Startup day-one**: a 20-person startup signs up, picks *Starter*, installs Leave, Expense and Policy-ack templates, uploads its handbook as a policy and publishes it with a required ack. Everyone acknowledges from their phone. | < 15 min to live. Ack dashboard shows 100 % with timestamps. |
| J13 | **Bulk policy signing**: HR sends the updated Code of Conduct to 4 000 employees for signature. Reminders run automatically, non-signers escalate to managers after 14 days, and each signed PDF carries a certificate of completion. | Async bulk send; per-recipient status; every signed PDF verifies; overdue export by department. |
| J14 | **Announcement with ack**: Admin posts an "Office move" announcement to the Bangalore location, requiring ack in 3 days. Ack % by department is visible live. | Audience snapshot recorded; escalation fires once; read vs ack distinguished. |
| J11 | **File lifecycle**: a requester attaches an invoice PDF; it's scanned and previewed; the approver views it; the file is locked when the request completes; an admin later finds it in the Files view under retention class `execution_record`. | Quarantined files are never served; locked files can't be deleted; download events are audited. |

## 7. Non-functional requirements (summary; detail in 06 and 14)

| Area | Target |
|---|---|
| Availability | Pilot 99.9 %/month; GA hosted 99.95 % after HA. |
| Performance | P95 read < 300 ms, P95 transition < 500 ms (excl. connectors) at reference load (100 workspaces, 10 k waiting instances, 100 transitions/s burst). |
| Durability | RPO ≤ 15 min, RTO ≤ 2 h (tested quarterly). |
| Correctness | 0 lost/duplicated transitions in conformance + chaos suites; ≥ 99.9 % accepted transitions succeed. |
| Security | 0 cross-workspace leaks; OWASP ASVS 5.0 L2 (L3 for auth/session/crypto); external pentest before GA. |
| Accessibility | WCAG 2.2 AA audited before GA. |
| UX | SUS ≥ 80 for requesters/approvers, ≥ 70 for designers in pilot; first decision untrained ≥ 80 %. |
| Portability | Runs on x86-64 and ARM64; Postgres 16+; any S3-compatible store. |

## 8. Out of scope / non-goals (initial release)

Jira-style project management; payroll/ERP implementation; full BPMN 2.0 conformance;
user scripting; AI auto-approval or AI people-scoring; arbitrary-log process discovery;
native mobile apps; offline approvals; cross-workspace reporting for workspace users
(System Admin sees usage only).

## 9. Success measures and instrumentation

| Metric | Target | Instrument |
|---|---|---|
| Time to first published process (template) | ≤ 30 min | Product event `process.published` − `workspace.created` |
| Untrained task completion | ≥ 80 % | Usability study + funnel `work.opened → decision.recorded` |
| Accepted transition success | ≥ 99.9 % | Runtime metrics |
| Cross-workspace leaks | 0 | Security suite in CI + pentest |
| Published changes attributable | 100 % | Audit completeness check job |
| P95 read latency | < 300 ms | OTel histograms at reference load |
| Weekly active approvers / members | track | Opt-in, anonymous for self-hosted; never content |

Product analytics in self-hosted builds are **off by default** and never include content.

## 10. Open product questions

Tracked with owners and decide-by dates in [17-EXECUTION-TRACKER §Open questions](17-EXECUTION-TRACKER.md#open-questions).
