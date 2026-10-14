# 21 — Complete feature catalogue (system inventory)

Status: v0.2 · 2026-10-11 · Owner: Product. This is the **single inventory of everything the system does**. It combines the PRD ([02](02-PRD.md)) with two feature-parity sweeps of the category leaders, run on 2026-10-11:
- workflow, case, admin, audit and platform
- e-sign, policy, comms, forms and SOP

Anything not listed here is **out of scope until added through the change process** (§8). That rule is what stops wild cards later.

Key:
- **Pri:** P0 = pilot (M6/M7) · P1 = GA (M11) · P2 = expansion (M12+) · ✗ = explicit non-goal.
- **M:** target work package. Releases group them per [07 v0.3](07-ROADMAP-AND-MILESTONES.md): M2–M4 → R1 (Jan 2027), M5–M6 → R2, M7 → R3, M8 → R4, M9 → R5, M10–M11 → GA. Items moved explicitly in 07 §2 take precedence; for example DoA, master data, recall/edit-after-submit and e-sign v1 ship in R3.
- **Ref:** PRD requirement id. New ids from the sweep are prefixed `FX-`.

## 1. Platform & system administration

| Feature | Pri | M | Ref |
|---|---|---|---|
| Install bootstrap (one-time token, mandatory MFA) | P0 | M2 | FR-S01 |
| Workspace lifecycle: create, suspend (read-only or locked), restore, scheduled delete, crypto-shred | P0 | M2 | FR-S02 |
| Workspace owners assignment | P0 | M2 | FR-S03 |
| Quotas and usage per workspace | P0 | M4 | FR-S04 |
| Platform settings and policy floors (MFA, password, sessions) | P0 | M2 | FR-S05 |
| Global template gallery management | P0 | M6 | FR-S06 |
| Break-glass with workspace approval and visibility | P1 | M9 | FR-S07 |
| Platform health console and Prometheus/OTel metrics | P0 | M4 | FR-S08 |
| System audit log | P0 | M2 | FR-S09 |
| Isolation tiers (dedicated DB) | P1 | M10 | FR-S10 |
| Configuration profiles: Starter / Standard / Enterprise / Regulated | P0 | M2 | 20 §0 |
| Sandbox workspace cloned from prod config (no data) | P1 | M8 | FX-P01 |
| Full workspace export and verified deletion certificate | P1 | M9 | FR-W10, FX-P02 |
| Per-workspace point-in-time restore (dedicated tier) | P2 | — | FX-P03 |
| BYOK through an external KMS per workspace; HYOK | P1 / P2 | M10 | FX-P04 |
| Data residency (region pinning) for the hosted offering | P1 | M10 | PRV-05 |
| Seat hygiene: dormant-member report, auto-deactivate after N days (opt-in) | P2 | — | FX-P05 |
| Trust pack: DPA template, subprocessors, CAIQ-Lite pre-fill, security.txt, VDP | P1 | M10 | FX-P06 |
| Status page (hosted) and health endpoints (self-host) | P1 | M10 | FX-P07 |

## 2. Identity & access

| Feature | Pri | M | Ref |
|---|---|---|---|
| Local accounts (Argon2id), email verification, lockout | P0 | M2 | 12 §3 |
| MFA: TOTP, WebAuthn/passkeys, recovery codes; MFA required by role | P0 | M2 | 12 §3 |
| Sessions: idle and absolute timeouts, list/revoke own sessions, admin revoke | P0 | M2 | FX-I01 |
| API tokens and service accounts (scoped, expiring), token inventory and revoke per user | P0 | M2 | FR-P10, FX-I02 |
| OIDC SSO per workspace, JIT provisioning, SSO enforcement (break-glass local login only) | P1 | M8 | FR-P02 |
| SAML via broker IdP | P1 | M8 | FR-P02 |
| SCIM 2.0 users, groups and manager attribute; deprovision < 5 min | P1 | M8 | FR-P02 |
| IP allowlists (workspace and system plane) | P1 | M8 | FR-W06 |
| Permission catalogue, built-in role templates, custom roles, scopes, preview, time-boxed grants | P0 | M2 | FR-W03 |
| **Scoped / delegated admins** (department or collection admin) that can't grant beyond their own rights | P1 | M8 | CFG-018 |
| Separation mode (regulated workspaces) | P1 | M9 | 12 §6.1 |
| Access review / recertification campaigns | P1 | M9 | FR-W11 |
| Delegation per item, out-of-office substitution, admin reassignment | P0 | M5 | FR-E04 |
| Step-up authentication for sensitive actions | P0 | M2 | 12 §11 |
| Guests / external participants (magic link + OTP) | P1 | M8 | FR-P08 |

## 3. Directory & organisation model

| Feature | Pri | M | Ref |
|---|---|---|---|
| Members, profile, bulk CSV import/export | P0 | M2 | FR-W01 |
| Department/team tree, groups (static/rule), positions, manager links, locations | P0 | M2 | FR-W02 |
| **Position-based approval authority (DoA matrix)**: approval limits by position/role and amount/category, versioned | P0 | M6 | FX-D01 |
| Manager-chain traversal ("up to N levels", "first manager with limit ≥ amount") | P0 | M5 | FX-D02 |
| Per-location business calendars (assignee's location used for SLA by default) | P0 | M5 | FR-W05, FX-D03 |
| Org chart view | P1 | M8 | FX-D04 |

## 4. Library, governance, SOPs & policies

| Feature | Pri | M | Ref |
|---|---|---|---|
| Landscape hierarchy, collections, owners, RACI, tags, criticality, visibility | P0 | M3 | FR-G01 |
| Process detail (as-is/to-be, KPIs, dependencies, links) | P0 | M3 | FR-G02 |
| SOP block editor (steps, roles, exceptions, records, references, images, video embed) | P0 | M3 | FR-G03 |
| Policy library with applicability, employee policy portal, "what changed" summaries | P0 | M3 | FR-PL01–02 |
| Canonical lifecycle with SoD, step-up, effective dates | P0 | M3 | FR-G04 |
| Immutable versions, **side-by-side semantic diff** (text, steps, forms, rules, roles) | P0 | M3 | FR-G05 |
| **Required review/renewal date to publish**; reminders; auto-flag expired | P0 | M3 | FR-G07, FX-G01 |
| **Page verification with expiry** (verified badge ranks higher in search) | P0 | M3 | FX-G02 |
| Re-attestation on major version (per policy) | P0 | M3 | FR-PL03 |
| Change requests and feedback on published versions | P0 | M3 | FR-G05 |
| Policy exception requests (approve, expire, compensating control) | P1 | M8 | FR-PL04 |
| Framework/control mapping (ISO 27001, SOC 2, ISO 9001 ids) and evidence export | P1 | M9 | FR-G06, CMP-01 |
| Policy templates (SOC 2 / ISO-aligned starter policies, not copied text) | P1 | M8 | FX-G03 |
| Templates gallery and install wizard with role mapping and upgrade diff | P0 | M6 | FR-G08 |
| Import: BPMN XML (P0); CSV landscape, DOCX/Markdown SOP (P1); Visio/ARIS/Signavio/Bizagi (P2) | P0–P2 | M6/M8 | FR-G09 |
| Export: PDF (tagged, watermarked); DOCX; XLSX landscape | P0 / P1 | M3 | FR-G10 |
| **Auto-generated process documentation** from a published definition (diagram, steps, roles, SLAs, fields) as an SOP draft | P1 | M8 | FX-G04 |
| Improvement proposals ("Idea") | P1 | M8 | FR-G12 |
| Public read-only policy portal (external trust centre) | P2 | — | FX-G05 |
| Click-capture step recorder (browser extension) with PII blur | P2 | — | FX-G06 |
| Role-based onboarding/training paths (SOP and policy playlists by role/department, due dates, completion) | P1 | M9 | FX-G07 |
| Certificates with expiry and recertification cycles | P2 | — | FX-G08 |
| AI Q&A over published content with citations (RBAC-filtered) | P1 | M10 | 16 |
| SCORM/xAPI import | ✗ (P2 demand gate) | — | — |

As built (BPMS-106 P0 subset, A-38): built-in **template packs** in code, versioned and reviewed. `policy-ack` 1.0.0 ("Employee handbook & acknowledgement") creates an Employee handbook and four starter policies (Code of conduct, Acceptable use of IT, Leave and time off, Workplace safety), each opening with a "Template to adapt" notice, and suggests an acknowledgement campaign for the handbook (everyone, due 14 days after launch). `core-sops` 1.0.0 ("Core procedures") creates two SOPs (New starter onboarding, Reporting an incident). Installing (Templates page, `POST /templates/{key}/install`) creates the chosen documents as drafts owned by the installer; an existing key is skipped and reported unless a numbered copy is asked for, so a second install never duplicates. The campaign is returned as a plan to start once the handbook is published. The gallery service, role mapping, test runs, lineage and upgrade diffs, and the 13 Wave-1 process templates remain BPMS-106.

## 5. Process design & execution

| Feature | Pri | M | Ref |
|---|---|---|---|
| Step builder, flow canvas, keyboard authoring, undo/redo, autosave | P0 | M6 | FR-E01 |
| IR node kinds v1 (+ `human.sign`, `human.acknowledge` v1.1) | P0 | M5 | 11 |
| Approval modes: sequential, parallel all / any / quorum, **per-group one-of-each** | P0 | M5 | FR-E05, FX-E01 |
| Rule tables (UNIQUE/FIRST/COLLECT) | P0 | M5 | FR-E10 |
| **Master-data tables** (cost centres, vendors, GL codes): CSV import, used as form lookups and cascading dropdowns, versioned | P0 | M6 | FX-E02 |
| Launch: manual, API, schedule (P0); inbound event (P1) | P0 / P1 | M5 | FR-E02 |
| **Request on behalf of** (requester ≠ beneficiary) | P0 | M6 | FX-E03 |
| **Recall/withdraw** by requester; restart as linked copy | P0 | M6 | FX-E04 |
| **Send back to any earlier step** (choose last actor or whole step) | P0 | M5 | FX-E05 |
| **Edit after submit** with per-step editable fields and change diff shown to approvers | P0 | M6 | FX-E06 |
| Request numbering per type (prefix, year, sequence; gap-free option) | P0 | M5 | FR-W04 |
| SLA definitions, business calendars, pause states, breach actions | P0 | M5 | FR-E12 |
| Escalations (remind, reassign, auto-outcome where allowed) | P0 | M5 | 11 §3.2 |
| Subprocesses, linked cases | P1 | M8 | FR-E08 |
| Reusable sub-flows ("DoA approval" block) | P1 | M8 | FX-E07 |
| Test run (no side effects) with trace | P0 | M6 | FR-E11 |
| Version pinning (release bundle); cancel-and-restart (P0); governed migration tool (P2) | P0 / P2 | M5 | FR-E07 |
| Ops incidents with retry/skip/cancel | P0 | M5 | FR-E13 |
| **Duplicate detection** on submit (configurable key fields and window) | P1 | M8 | FX-E08 |
| Bulk import of requests (CSV) | P1 | M8 | FX-E09 |
| Environment promotion (sandbox → prod bundle with diff) | P1 | M8 | FR-P09 |
| BPMN view (diagram-js, own renderer) | P2 | — | ADR-0018 |
| DMN import/export | P2 | — | FR-E10 |
| Statistical simulation | P2 | — | FR-I04 |
| Governed AI agent steps | P2 | — | 16 |
| Unbounded user scripting | ✗ | — | 02 §8 |

## 6. Forms

| Feature | Pri | M | Ref |
|---|---|---|---|
| Typed fields, layout, sections, conditional visibility, validation (min/max/regex) | P0 | M6 | FR-E03 |
| **Calculations** (numeric/text/date) | P0 | M6 | FX-F01 |
| **Hidden fields and URL prefill**; answer piping | P0 | M6 | FX-F02 |
| Repeating sections / line-item tables | P0 | M6 | FX-F03 |
| Lookups to master data and directory (person/department pickers) | P0 | M6 | FX-E02 |
| File upload fields (count/size/type limits, scanned) | P0 | M6 | FR-F01 |
| Submission → PDF (template) | P0 | M6 | FX-F04 |
| Field classification (privacy) | P0 | M6 | 11 §5 |
| Save draft and resume; partial-submission capture (with notice) | P1 | M8 | FX-F05 |
| Public / anonymous forms: CAPTCHA (privacy-friendly providers: hCaptcha, Turnstile, self-hosted PoW), rate limit, response cap, close date | P1 | M8 | FX-F06 |
| Signature field inside a form (light sign-off) | P1 | M8 | FX-F07 |
| Form translations | P1 | M10 | FR-P04 |
| Offline mobile, kiosk mode, QR | P2 | — | FX-F08 |
| Payment fields (hosted provider fields only) | P2 | — | FX-F09 |

## 7. Requests, work, portal & notifications

| Feature | Pri | M | Ref |
|---|---|---|---|
| **Request portal / service catalogue** (categories, search, favourites, "my requests") | P0 | M6 | FR-R01, FX-R01 |
| Request detail (stepper, owner, ETA, comments, attachments, history, cancel) | P0 | M6 | FR-R02 |
| Comments with **@mentions**, internal vs requester-visible, **watchers/followers**, CC | P0 | M6 | FX-R02 |
| My Work 3-pane inbox, keyboard triage, saved views; Acknowledgements and Signatures tabs | P0 | M6 | FR-R03, FR-AK02 |
| Safe bulk approve / reassign with preview | P0 | M6 | FR-E05 |
| In-app notification centre; email (localised templates, merge fields); digests; quiet hours | P0 | M6 | FR-N01/02/04 |
| **Email-actionable approvals** (signed single-use links → confirm page) | P1 | M8 | FR-N03 |
| Reply-by-email becomes a comment | P1 | M8 | FX-R03 |
| Web push (PWA) | P1 | M8 | FR-N05 |
| **CSAT** survey on completion; scores in reports | P1 | M9 | FX-R04 |
| Chat cards (Slack/Teams/other platforms via the 13 §4 contract) | P2 | M12 | FR-N06 |
| SMS/WhatsApp notifications (bring-your-own provider; DLT/10DLC registration is the customer's responsibility) | P2 | — | FX-R05 |
| Native mobile apps | ✗ (P2 demand gate) | — | — |

## 8. Communications: announcements & campaigns

| Feature | Pri | M | Ref |
|---|---|---|---|
| Compose (rich text, media, attachments, links to policies/SOPs), categories, urgent | P0 | M3 | FR-AN01 |
| Audience targeting (all, departments, groups, roles, locations, directory rule) with count preview | P0 | M3 | FR-AN02 |
| Schedule, **expiry**, pin; comments on/off; reactions | P0 | M3 | FR-AN03 |
| Required acknowledgement, deadline, reminders, manager escalation | P0 | M3 | FR-AN04 |
| **Post approval workflow** (draft → review → publish) for selected categories | P0 | M3 | FX-C01 |
| Campaign analytics (reach, read %, ack % by department/location) | P0 | M3 | FR-AN06 |
| Campaign engine (ack, quiz, sign), new-joiner auto-enrolment, evidence export | P0 | M3 | FR-AK01–04 |
| Quiz with pass mark, scores per version | P1 | M8 | FR-G03 |
| **Emergency alerts** that bypass digest/quiet hours (explicitly not a certified mass-notification system) | P1 | M8 | FX-C02 |
| Content validation period (author re-confirms, otherwise auto-archive) | P1 | M8 | FX-C03 |
| Comment moderation (flag, hide, profanity list) | P1 | M8 | FX-C04 |
| Translations of announcements (manual variants; machine translation optional with a "source prevails" note) | P1 | M10 | FX-C05 |
| Multi-channel publish (Teams/Slack/other chat platforms), newsletters, signage | P2 | M12 | FX-C06 |

As built (0033, BPMS-046/047): required acknowledgement runs on the shared campaign engine (one engine, FR-AK01). An announcement with `require_ack` gets an optional deadline (`ack_by`, default go-live + `campaign.default_due`, never after expiry). When it goes live, a campaign with `subject_kind = announcement` starts with its recipients as a fixed snapshot (no new-joiner enrolment). Reminders follow `campaign.reminders.cadence`, and the manager is escalated to once (`campaign.escalation.to_manager_after`, never before the deadline). Acknowledging the announcement completes the assignment (one record). The evidence CSV covers it, and archiving closes the campaign. Quiz and `announcements.escalate_unacked_to_manager` are not built yet. See docs/04 §2.7 and A-32.

## 9. Document signing (e-signature)

| Feature | Pri | M | Ref |
|---|---|---|---|
| Upload PDF (P0) / DOCX (P1) → send; template documents with merge fields (P1) | P0 / P1 | M6 | FR-DS01 |
| **Recipient roles**: signer, approver, viewer (confirm viewed), CC, assistant (fills on behalf) | P0 (signer/approver/CC) / P1 | M6 | FX-S01 |
| Routing: sequential, parallel, **mixed** (same order number = parallel group) | P0 | M6 | FX-S02 |
| Bulk send to groups / CSV with merge data | P0 | M6 | FR-DS02 |
| Field placement (signature, initials, date, name, text, checkbox, dropdown, attachment) with a keyboard alternative | P0 | M6 | FR-DS03 |
| **Anchor/text-tag auto-placement** (`{{sig:signer1}}` in the document) | P1 | M8 | FX-S03 |
| Conditional and calculated fields, field validation | P1 | M8 | FX-S04 |
| Signer attachments (upload ID or certificates) | P1 | M8 | FR-DS03 |
| **Electronic-records consent / disclosure step** (ESIGN §101(c)) before signing | P0 | M6 | FX-S05 |
| Per-template **"not e-signable" flag** with jurisdiction guidance (wills, PoA, immovable property in India, etc.) | P0 | M6 | FX-S06 |
| Recipient authentication: session (members), access code/PIN, email OTP (P0); SMS OTP via own provider (P1); ID verification provider plugin, off by default (P2) | P0–P2 | M6/M8 | FX-S07 |
| Signing ceremony (mobile-first, accessible), decline with reason | P0 | M6 | FR-DS04 |
| **Signer delegate/reassign** (when the sender allows) | P1 | M8 | FX-S08 |
| Reminders, expiry, void, resend | P0 | M6 | FR-DS06 |
| **Envelope correction in flight** (fix recipient email or fields without voiding; audited) | P1 | M8 | FX-S09 |
| Certificate of completion, tamper-evident seal (P0); PAdES-LTV + RFC 3161 (P1) | P0 / P1 | M6/M9 | FR-DS05 |
| Self-serve signing links / QR (public NDA, waivers, visitor sign-in) | P1 | M8 | FX-S10 |
| In-person / kiosk signing (host hands over device) | P1 | M9 | FX-S11 |
| Embedded signing (iframe) + webhooks | P1 | M8 | FX-S12 |
| Signing as a process step (`human.sign`) | P1 | M8 | FR-DS07 |
| QES via QTSP connector (EU); **Aadhaar eSign via licensed ESP** (India) as pluggable providers | P2 | M12 | FR-DS08, FX-S13 |
| Witness role | P2 | — | FX-S14 |
| Payment collection at signing, pricing tables, view analytics | ✗ (P2 demand gate) | — | — |

## 10. Cases & investigations

| Feature | Pri | M | Ref |
|---|---|---|---|
| Case types/templates, stages, case file, parties, milestones, discretionary tasks | P1 | M9 | FR-C01 |
| Queues with counts and "next case" pull mode | P1 | M9 | FX-K01 |
| Assignment rules: round-robin, load-balanced (P1); skills-based (P2) | P1 / P2 | M9 | FX-K02 |
| Macros / canned responses | P1 | M9 | FX-K03 |
| SLA policies per priority (first response, resolution) | P1 | M9 | FX-K04 |
| Link, merge (both audit/SLA histories kept), parent/child | P1 | M9 | FX-K05 |
| **Restricted/confidential cases**: excluded from global search, reports, notifications and the AI index; admin access only via audited self-grant visible to the case team (OQ-12) | P1 | M9 | FX-K06 |
| Anonymous reporter with two-way messaging (case key + password), whistleblowing intake | P2 | — | FX-K07 |
| Investigation workspace (allegations, subjects, witnesses, interviews, findings, substantiation, remediation) | P1 | M9 | FR-T05 |
| Evidence chain of custody (hash on upload, access log, sealed export) | P1 | M9 | FR-C02 |
| Conflict-of-interest check (investigator ≠ subject or subject's manager) | P1 | M9 | FX-K08 |
| Side conversations (internal threads with external parties) | P2 | — | FX-K09 |
| Knowledge linking (attach SOP, create SOP draft from case) | P1 | M9 | FX-K10 |

## 11. Files & documents

FR-F01–F09 (P0/P1, M4). Additional items:
- Office-document preview in a sandbox (P1).
- Watermarked downloads of controlled documents (P1).
- Orphan cleanup (P0).

## 12. Admin console, dashboards, reporting

| Feature | Pri | M | Ref |
|---|---|---|---|
| Workspace overview dashboard with drill-down | P0 | M4 | FR-A01 |
| All requests / all work / workload / process health / ops monitor / people & access | P0 | M4–M6 | FR-A02–A06 |
| Shared DataGrid everywhere (sort, filter builder, saved views, bulk, export, keyboard) | P0 | M2 | FR-V01–V09 |
| Alert centre (rules → alert with assignee/status → link to investigation) | P1 | M9 | FR-A07, FX-A01 |
| Custom dashboards (P1), report builder with grouping/pivot/calculated fields (P1), scheduled reports and subscriptions with RLS re-applied at run time (P1) | P1 | M9–M10 | FR-A08/A09, FX-A02 |
| KPI goals/targets with RAG status | P1 | M9 | FX-A03 |
| Usage analytics (adoption by department, active users) | P1 | M9 | FX-A04 |
| Dashboard embedding (signed) | P2 | — | FX-A05 |

## 13. Audit, retention, legal hold

| Feature | Pri | M | Ref |
|---|---|---|---|
| Complete audit trail incl. data-access events; hash chain; DB append-only | P0 | M2 | FR-T01/T02 |
| Audit explorer, timelines, saved queries | P0 | M4 | FR-T03/T04 |
| Audit export (P0); SIEM streaming via webhook/syslog/OTel (P1) | P0 / P1 | M4/M8 | FR-T08 |
| Signed checkpoints, RFC 3161, offline verifier `procera-verify` | P1 (verifier pulled forward if possible) | M9 | AUD-02–05 |
| Retention policies per record class (P0 policy, P1 disposition review with certificates) | P0 / P1 | M4/M9 | PRV-04 |
| **Legal hold on users, records or cases**; custodian hold notices with ack tracking (reuses campaigns) | P1 | M9 | PRV-03, FX-L01 |
| Matter container (saved searches, held custodians, exports, own audit) = investigation | P1 | M9 | FR-T05 |
| Anomaly signals (rule-based) | P1 | M9 | FR-T07 |
| Review sets / near-duplicate | ✗ | — | — |

## 14. Integrations (post-standalone)

These are covered in [13-INTEGRATIONS](13-INTEGRATIONS.md) (INT-001…030).
- **P1:** SSO, SCIM, webhooks, HTTP connector, inbound events, iCal, CLI/SDKs.
- **P2:** chat platforms, M365/Google, HRIS, ERP, QTSP/Aadhaar ESP, OpenDoc, SIEM, SMS/IDV/CAPTCHA/payment providers.

The rule: **pluggable providers, never hard-wired vendors.**

## 14a. HR: time off, holidays, shifts and attendance

Added through §17 on 2026-10-14 (owner decision: a full time-and-attendance area, because Procera already
runs onboarding). Design: [25-HR-TIME-AND-ATTENDANCE](25-HR-TIME-AND-ATTENDANCE.md). Release 0.5.

| Feature | Pri | M | Ref |
|---|---|---|---|
| HR-01 Work locations (IANA time zone, work week), holiday calendars by year, optional holidays, iCal/CSV import | P1 | 0.5 | docs/25 §2.1, BPMS-222 |
| HR-02 Leave types and policies (allowance, up-front or monthly accrual, pro-rating, carry-over and expiry), assignment by audience | P1 | 0.5 | docs/25 §2.2, BPMS-223 |
| HR-03 Balances from an append-only ledger; audited admin adjustments with a reason; yearly rollover | P1 | 0.5 | docs/25 §3, BPMS-223 |
| HR-04 Leave requests on the process engine: day counting (work week, holidays, half days, sandwich rule), balance hold, cancel after approval | P1 | 0.5 | docs/25 §4, §7, BPMS-224 |
| HR-05 Team calendar (who's out, by team, department, location) and org chart; colleagues see "Out", not the type | P1 | 0.5 | docs/25 §6, BPMS-225 |
| HR-06 Shift patterns and weekly rosters (draft, publish, notify) | P1 | 0.5 | docs/25 §2.5, BPMS-226 |
| HR-07 Attendance: clock in/out, punch import (CSV, API), derived daily status with grace rules | P1 | 0.5 | docs/25 §5, BPMS-227 |
| HR-08 Regularization requests (manager approval, original punches kept) | P1 | 0.5 | docs/25 §7, BPMS-228 |
| HR-09 Attendance register and reports, month close, payroll-ready export | P1 | 0.5 | docs/25 §2.8, BPMS-229 |
| Payroll calculation (future) | P2 | POST | docs/25 §11, BPMS-230 |
| Releasing documents to people: payslips, letters (future) | P2 | POST | docs/25 §11, BPMS-231 |

## 15. Experience qualities (cross-cutting)

- WCAG 2.2 AA and the normative design tokens (05 §7).
- No AI-generic or cookie-cutter visual style.
- Configurable branding with contrast enforcement.
- Personal display preferences.
- PWA at 360 px.
- i18n framework from P0, with 3 languages at GA.
- Command palette.
- Startup-to-MNC scaling behaviours (05 §1.2).

## 16. Gap-sweep traceability

| Sweep finding | Disposition |
|---|---|
| Email/Teams approve links | FR-N03 P1 (email); chat P2 |
| DoA matrix + org hierarchy + positions | FX-D01/D02 **P0** |
| Recall, send-back, edit-after-submit | FX-E04/E05/E06 **P0** |
| Request-on-behalf | FX-E03 **P0** |
| Master-data tables | FX-E02 **P0** |
| Request portal / catalogue; public forms | FX-R01 **P0**; FX-F06 P1 |
| Scoped admins | CFG-018 P1 |
| Confidential cases vs admin visibility | FX-K06 P1 + OQ-12 |
| Legal hold / retention / disposition | PRV-03/04, FX-L01 P1 |
| Offline audit verifier | AUD-05 P1 |
| Auto process documentation | FX-G04 P1 |
| Access recertification | FR-W11 P1 |
| CSAT | FX-R04 P1 |
| Full tenant export & deletion | FR-W10, FX-P02 P1 |
| Trust pack / VPAT | FX-P06, A11Y-03 P1 |
| Envelope correction; recipient roles; anchor tags | FX-S09/S01/S03 |
| ESIGN consent step; "not e-signable" flag | FX-S05/S06 **P0** |
| Policy renewal date; re-attestation per version | FX-G01 **P0**, FR-PL03 |
| Policy exceptions | FR-PL04 P1 |
| Post/SOP expiry & validation | FR-AN03, FX-C03, FX-G02 |
| Form prefill, partials, calculations | FX-F01/F02 P0, FX-F05 P1 |
| PII blur in captures | FX-G06 P2 |
| Pluggable SMS/IDV/QTSP/ESP/payments/CAPTCHA | §14 rule |

## 17. Change process

A new feature enters through the steps below. This is how "nothing becomes a wild card":
1. A row is added here with Pri = `proposed`.
2. Impact is assessed on IR, authz, data model, audit, a11y and security.
3. Product and the tech lead decide at the milestone planning review.
4. The tracker gets an entry.
5. If architecture changes, an ADR is written.
