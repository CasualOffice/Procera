# Procera — tracker dashboard

> **Generated** by `scripts/build_tracker.py`. Don't edit by hand. To change status, edit the `STATUS` map (or `CORE`) in the script and re-run it ([rules](../docs/24-ENGINEERING-RULES.md#2-tracker-rules)).
> Detail: [docs/17](../docs/17-EXECUTION-TRACKER.md) · [Delivery_Tracker.xlsx](Delivery_Tracker.xlsx) · [backlog.csv](backlog.csv) · [domain_backlog.csv](domain_backlog.csv) · Roadmap: [docs/07](../docs/07-ROADMAP-AND-MILESTONES.md)

## At a glance

```text
Overall   ██████████░░░░░░░░░░░░░░░░░░░░  35/109 epics done  (32%)
Partial   ████████████░░░░░░░░░░░░░░░░░░  41% counting in-progress epics as half done
Tests     245 automated E2E scenarios · 422 Rust tests (incl. property tests)
Status    done 35 · in review 0 · in progress 19 · blocked 0 · not started 55 · deferred 0
Now       P0 — Spikes + G1 + design system  (2026-11-02 → 2026-11-20, gate G1)
Evidence  automated tests 40 · code merged 14  (separate from status; none user-validated yet)
```

> **Status vs evidence.** Status says how far the work has got; evidence says what is proven: code merged → automated tests (cited E2E scenarios run in the full suite) → demonstrated → user validated → released. Pilots and usability studies raise evidence; merging code doesn't.

## Releases

| Release | Name | Window | Gate | Progress | Done |
|---|---|---|---|---|---|
| **P0** ◀ now | Spikes + G1 + design system | 2026-11-02 → 2026-11-20 | G1 | `█████████░░░` | 10/14 |
| **R1** | Workspace & Policies | 2026-11-23 → 2027-01-15 | G-R1 | `█████████░░░` | 24/32 |
| **R2** | Requests & Approvals | 2027-01-18 → 2027-03-12 | G-R2 | `░░░░░░░░░░░░` | 0/17 |
| **R3** | Operate & Sign | 2027-03-15 → 2027-05-07 | G-R3 | `██░░░░░░░░░░` | 1/8 |
| **R4** | Enterprise Connect | 2027-05-10 → 2027-07-02 | G-R4 | `░░░░░░░░░░░░` | 0/13 |
| **R5** | Cases & Compliance | 2027-07-05 → 2027-08-27 | G-R5 | `░░░░░░░░░░░░` | 0/11 |
| **GA** | 1.0 hardening + release | 2027-08-30 → 2027-10-15 | G4/G5 | `░░░░░░░░░░░░` | 0/8 |
| **POST** | Post-GA expansion | 2027-10-18 → 2028-03-31 | Demand | `░░░░░░░░░░░░` | 0/6 |

## Active right now

| ID | Item | Status | Evidence | Note |
|---|---|---|---|---|
| BPMS-030 | Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK | **In progress** | Automated tests | procera-crypto: LocalKey root, AES-256-GCM seal/open, envelope_* (per-value DEK wrapped by a versioned KEK; rewrap without re-encryption). procera-keys (migration 0026): per-workspace versioned KEKs root-sealed, lazy v1, rotation (reason + step-up, audited), background rewrap of registered envelope columns, unused retired keys destroyed, crypto-shred; email bodies moved to workspace envelopes (legacy still opens); Encryption admin page. Root key rotation: `procera rotate-master-key` (migration 0039 discovery fn) re-seals KEKs, TOTP secrets, invitation tokens and pre-envelope email bodies in one verified transaction, audited key.root_rotated; access tokens and signed file links change with the key. E2E-R1-KEY-01, E2E-R1-UI-29, keys integration tests (incl. root_rotation). Remaining: KMS / OpenBao root adapters, yearly auto-rotation, files and restricted fields on envelopes as they land |
| BPMS-031 | App shell, workspace switcher, system console shell, ⌘K | **In progress** | Automated tests | Web app on the Notion/Attio direction (owner choice): warm paper palette in tokens (contrast-checked), full-bleed pages with a warm icon sidebar, workspace + account menus (persisted theme), action-first Home (acknowledgements, reviews, announcements with inline actions, library table, activity, people), Library (filters, Attio-style table, New document dialog), document reading view with properties, lifecycle actions (edit, submit, approve, request changes, publish with step-up, new version), acknowledgement callout, Announcements feed with read/ack, Members with invite dialog; split sign-in with product preview; single-flight token refresh. E2E-R1-UI-01..10 with axe. Second UI pass: Geist type, state kit (skeletons, empty states, load errors with retry), ⌘K palette over search + commands (E2E-R1-UI-12..13), admin screens: Manage announcements, Campaigns, Settings, Audit log, Overview, People & access (E2E-R1-UI-14..17, 23); Policies portal and document governance (E2E-R1-UI-18..22). Keyboard-only journey (E2E-R1-19) and system console shell done. Remaining: persona IA redesign approved 2026-10-12 (docs/design/ia-competitive-analysis.md: persona sidebar, Admin hub, Needs-you Home, workspace onboarding) done: persona sidebar, Admin hub and admin mode, Library and Announcements views, Needs you Home, workspace onboarding checklist and /welcome (E2E-R1-UI-62..63). Home role cards (needs attention, active campaigns); 0.1 polish from a small-company walkthrough (review choice in setup, approver named, authors not offered their own review, merged notifications, document page layout, animated empty-state illustrations). Remaining: mobile bottom bar |
| BPMS-034 | Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search | **In progress** | Code merged | Vector masters (assets/brand/vector, generated by build.py: P-ribbon symbol, flat small symbol, horizontal and stacked lockups light/dark, app icons, banner; wordmark outlined from Geist, OFL); PNGs, favicons and ICO re-exported from them; the app uses the SVG symbol and an SVG favicon. Remaining: trademark search (OQ-01), professional mark review before GA Marketing site (site/, static, generated from tracker data and real screenshots: home, features, user flows, comparison, roadmap, security, open source); not published yet. Revamped site: animated product story, original illustrations, scroll motion, persona tabs, tamper demo, Get started page rendering the guides; published to GitHub Pages. |
| BPMS-042 | SOP/policy block editor + renderer (web/PDF) | **In progress** | Automated tests | Block editor (TipTap core, MIT; U-10 closed): headings, bold/italic/links, bullet and numbered lists, callouts, tables, accessible single-tab-stop toolbar, markdown shortcuts; block model validated server-side (ADR-0013 as built); shared renderer; HTML (print to tagged PDF in the browser) and Markdown export with a superseded banner, audited. E2E-R1-LIB-14, E2E-R1-UI-07, 28. Word (.docx) export with heading styles, numbering, tables and links, reproducible bytes; Export menu. E2E-R1-UI-44. Image blocks (alt text required, attached clean images only, embedded in print HTML; A-30) inserted from the editor. E2E-R1-LIB-15, UI-45. Remaining: deterministic server-side tagged PDF, DOCX import, pictures inside DOCX, variables blocks. |
| BPMS-060 | Files pipeline: resumable upload, hash, sniff, ClamAV, quarantine, signed URLs | **In progress** | Automated tests | procera-files (migration 0027): resumable chunked upload (offset-checked, resume from received), size limit and storage quota, SHA-256, magic-byte type with a platform deny list (executables, scripts, HTML, SVG), clamd INSTREAM scanner (built-in EICAR-only fallback, warned), quarantine and rejection, per-file DEK under the workspace KEK (rotated and shredded with it), local storage port, 5-minute signed download links re-authorized per request (nosniff, CSP sandbox), audited; attachments on draft versions fixed once past draft. E2E-R1-FILE-01..03, E2E-R1-UI-32..33. Request attachments (migration 0043): files linked to requests (object type request, execution_record retention, undeletable while held), preview/download for anyone who may open the request (E2E-R2-41/42). Remaining: S3-compatible storage backend, EXIF stripping at upload (previews done in BPMS-061), attachments on cases. |
| BPMS-061 | Previews (image/PDF) in sandbox; Office preview spike | **In progress** | Automated tests | Image/PDF previews: GET /files/{id}/preview-url (kind image|pdf|none) and GET /files/preview/{token} (own preview HMAC purpose, 2-minute TTL, re-authorized per request; clean PNG/JPEG/GIF/WebP/PDF only by recorded type plus fresh sniff, else 415; inline under CSP sandbox, nosniff, CORP same-origin, no-store; audited file.previewed); JPEG Exif/XMP/IPTC stripped from previews (files.exif_strip); one Preview dialog (attachments, Admin › Files) with pdf.js, docx-preview in a script-less sandboxed frame, SheetJS CE and CSV tables, download fallback; DOCX/XLSX checked against the zip central directory. E2E-R1-FILE-04, E2E-R1-UI-34, E2E-R1-UI-66..69. Remaining: separate preview origin, EXIF strip at upload. |
| BPMS-062 | Workspace Files view, quotas, orphan cleanup | **In progress** | Automated tests | Files admin view: usage against the storage quota, scan states, every file with uploader, use and scan result, delete (blocked while held by a version past draft), orphan report and purge (files.orphan_purge_after). E2E-R1-FILE-03, E2E-R1-UI-33. Files view on the DataGrid: retention class per file (from what holds it), last accessed (migration 0030, set on download/preview), bulk delete; audit-export files held by their job. E2E-R1-UI-43. Remaining: legal hold column (holds engine is BPMS-164). |
| BPMS-080 | IR v1 model + compiler (§11 checks, diagnostics, semantic diff) | **In progress** | Code merged | procera-ir crate: IR v1 model, compiler with the docs/11 §11 checks (IR-E001..E034, E100, W001-W002, I001; JSON-pointer diagnostics), JCS canonical form + sha256 checksum, semantic diff with apply; a test per code, property tests, conformance/compiler fixtures, golden checksums for the reference processes (A-21). Expressions type-checked with procera-expr (check 7 complete; IR-E035/E036, A-26); checked forms in Compiled::expressions; reference fixtures in CEL with regenerated checksums. Remaining: type checking with the BPMS-081 engine, environment wiring at publish (BPMS-084), declared perpetual waits. |
| BPMS-081 | Expression engine integration (typed, budgeted, WASM build) | **In progress** | Code merged | procera-expr: a typed CEL subset of our own (A-22): static checker with null policy and exact decimal/money, budgeted pure evaluator (10k steps, 1 MiB, 64 KiB strings, 10k lists; exhaustion is IR-R010), stdlib, read sets with the restricted-in-event-facing check; wasm32 build checked; unit + property tests; docs/11 §6.1. Wired end to end: the IR compiler type-checks with it and the runtime CelEvaluator runs stored checked expressions (injected now/tz, budget → IR-R010). Remaining: business-time functions with calendars (BPMS-083). |
| BPMS-082 | Interpreter: advance loop, tokens, splits/joins, end/terminate | **In progress** | Code merged | procera-runtime: pure deterministic interpreter (Program, InstanceState, Stimulus) -> (state, commands) with injected Clock/IdGen; advance loop with a step budget (ContinueAdvance) and loop guard (IR-R004); start/end/terminate, set, decision (+rule tables via the Evaluator port), parallel split/join released exactly once, human.* work items, wait.timer/signal, service.call with typed error boundaries, notify, subprocess.call, case.open, timeout boundaries, cancel; commands with deterministic idempotency keys; unit tests per node kind, the five reference processes end to end, 22 conformance/runtime fixtures, property tests (replay, budgets, joins, terminate); docs/11 §15, A-23. Runs real CEL via CelEvaluator; conformance (24 cases) green on CEL and the stand-in; determinism property over compiled CEL. Remaining: persistence and timers (BPMS-083), work items (BPMS-085), incidents (BPMS-088), E2E once wired to the API. |
| BPMS-083 | Jobs + timers + scheduler with leases/fencing; calendars; SLA clocks | **In progress** | Automated tests | procera-engine + migration 0031: instances persisted (state JSONB, optimistic revision) with an append-only stimulus log and replay; advance = interpreter + state + log + effects (timers, advance jobs, outbox, pending effects) + SLA clocks + audit in one transaction; jobs with SKIP LOCKED claims, 30 s leases, fencing tokens checked on completion, backoff, dead letter, idempotency keys; scheduler run_once fires due timers once, lateness histogram for the 60 s SLO; business calendars (fixed offset, working days/hours, holidays) with property-tested arithmetic; SLA clocks start/pause/resume/stop/breach; unit, property and PostgreSQL integration tests; docs/04 §4.4, A-27..A-29. Wired (0.2): serve --all runs the scheduler every second, /__test/drain drains it, EffectHandler port consumes work items/notifications in the advance transaction, user audit actor (A-28 (4)), pinned checksums, lazy default calendar. Handler timers (0.2): TimerPurpose reminder/escalation armed by the effect port, fired through EffectHandler::fire under a savepoint. Proofs (0.2): restart durability, atomic advance with a failing effect handler, lost-lease re-claim with fencing (E2E-R2-SYS-01) and version pinning — StaticPrograms keeps every registered build by checksum so pinned instances finish on their build while new starts use the new one, audit names the checksum (E2E-R2-SYS-02). Decision trace (PR-08): migration 0042 instance_decisions (branch, conditions, inputs with values except restricted, build) written in the advance transaction. Remaining: release bundles (BPMS-084), encryption of restricted instance data, at-risk SLA state, IANA zones. |
| BPMS-085 | Work items, assignment strategies, manager chain, DoA matrix, quorum, per-group | **In progress** | Automated tests | crates/requests + migration 0040: work items and materialised candidates; assignment resolution (user from principal expressions incl. manager chain, group, role incl. group grants, position; excludeInitiator, excludeActorsOf; fallback; unassigned when nobody remains, A-45); claim strategy, quorum one; decisions lock instance then item (one winner, already_decided, If-Match), SoD re-check; work_assigned/request_decided/request_completed notices; GET /work, POST /work/{id}/decisions; DB tests + E2E-R2-04/-18. Reminders and escalations (0.2): built-in IRs remind before/at due and escalate a business day after (reassign to the manager's manager, or notify the requester's manager), armed as handler timers and cancelled on decision; work_reminder/work_escalated/work_reassigned; timeline Reminded/Escalated; DB tests + E2E-R2-21/-22. Proofs (0.2): 20 concurrent decisions on one step have exactly one winner (E2E-R2-SYS-03); open steps and reminder timers survive a restart and a reminder fires once (E2E-R2-SYS-01). Resolution snapshots record the rule behind each candidate source and why SoD excluded each member. Remaining: claim/unclaim, round-robin/least-loaded/all, quorum, DoA matrix.; a failing reminder/escalation handler is retried with backoff (1m→4h, 5 retries) then marked failed with its error (migration 0044) |
| BPMS-086 | Delegation, OOO substitution, SoD re-check, send-back | **In progress** | Automated tests | Out-of-office substitution (0.2, migration 0041): one out-of-office per member (inclusive UTC dates, delegate, note); the resolver hands an away candidate's new work items to the delegate (snapshot records it), the delegate may decide existing open items, decisions record completed_on_behalf_of ('X on behalf of Y'); access.delegation.max_depth (default 1, cycles stop), SoD re-checked for the delegate, the initiator's own steps never handed on, expiry fails closed. GET/PUT/DELETE /me/out-of-office and /members/{id}/out-of-office (admin or manager), audited member.ooo_set/ooo_cleared; Profile and Person page UI; Away chips in My work and the stepper. DB tests + E2E-R2-33/-34, E2E-R2-UI-11. Remaining: per-item delegation (admin reassignment is BPMS-088), a process filter on out-of-office, send-back to an earlier step.; status at a glance (E2E-R2-UI-12): the account menu sets Available / Out of office, an orange dot on avatars, a Home banner with "I'm back", and others see it on Home › People hover cards, Members and the Person page |
| BPMS-088 | Incidents, retry/skip/cancel, instance admin ops | **In progress** | Automated tests | Oversight first slice (0.2): GET /work?state=unassigned|overdue and POST /work/{id}/reassign {membership_id, reason} (work:reassign; SoD re-run, 422 sod_excluded; audited work_item.reassigned; work_assigned/work_reassigned notices); Admin › Work › Requests oversight page and Home Needs attention lines; DB tests + E2E-R2-23, E2E-R2-UI-05, AUD-01 step. Remaining: incidents, retry/skip/cancel, instance admin ops. |
| BPMS-101 | Form builder + renderer (calc, prefill, repeating, lookups, PDF of submission) | **In progress** | Automated tests | Server-side forms subset (0.2): start and step forms derived from IR variables (string, text, enum, money, date, number, boolean, principal), required/type/unknown-field checks with 422 errors[], principals shaped from the directory; E2E-R2-03/-19. File fields (`file`, multiple): ids checked for workspace, uploader, clean scan and allowed type (images, PDF, Word, Excel, CSV), stored as file refs (E2E-R2-41). Remaining: form artifacts, builder, renderer, calc, conditional, repeating, prefill, PDF. |
| BPMS-103 | Request portal/catalogue, start form, request detail, comments/@mentions/watchers | **In progress** | Automated tests | Request API (0.2): GET /request-types, POST /requests (Idempotency-Key, display ids LR-2026-000123), GET /requests?scope=mine|all, GET /requests/{id} (steps, timeline, SLA), POST /requests/{id}/cancel; status derived per stimulus; audited request.started/cancelled with the runtime record; DB tests + e2e/packs/r2 (E2E-R2-17, -20, -ISO). Why this route (PR-08): GET /requests/{id} returns why[{at, step, text, facts}] (threshold routes and assignment reasons) and release{version, checksum}; GET /work items carry why_you; request page Why this route disclosure and decision panel Why you (E2E-R2-24, E2E-R2-UI-01). Remaining: web portal UI, comments/@mentions/watchers, cursor pages, query endpoint. web: requests catalogue, forms, my/all requests, request page with stepper, My work inbox with decisions (Requests area with views as links; /requests/new/{type} rendered from the form schema with required checks and 422 field errors; /requests/{id} with steps, answers, timeline and cancel; Home quick action and requests in progress; E2E-R2-UI-01..04). Server API in progress alongside. Workspace request settings: requests.types.enabled / requests.types.audience / requests.currency (curated ISO list, default EUR; money-bearing built-ins compiled per currency, thresholds read in it); GET /request-types lists only enabled types the caller is in the audience of, POST /requests refuses others with 403; Settings › Requests with 'Offer <type>' switches and 'Who can ask' (E2E-R2-30..32, E2E-R2-UI-10). Slice 3: Who approves this — POST /requests/preview (a read, audit-exempt) dry-runs the routing with the real resolver and words later steps from IR conditions, shown live on the form (E2E-R2-40, E2E-R2-UI-20); attachments on every built-in and with decisions (E2E-R2-41/42, E2E-R2-UI-21); requests.types.policy links a published policy/SOP, shown to deciders, the decision records the version shown (E2E-R2-43, E2E-R2-UI-22). |
| BPMS-105 | My Work 3-pane inbox (tasks, acks, signatures), bulk with preview | **In progress** | Automated tests | web: requests catalogue, forms, my/all requests, request page with stepper, My work inbox with decisions (/work list + detail with Open/Done, J/K, decision panel with If-Match and required comment to send back or reject, sidebar count "My work, N open", Home "Needs you" approvals, request notifications); Overdue tag by the server clock; Admin › Requests oversight (Unassigned, Overdue, All open; Reassign… with SoD errors on the field; E2E-R2-UI-05). Bulk with preview and acks/signatures tabs remain. |
| BPMS-106 | Template gallery + install wizard + Wave-1 templates (13) | **In progress** | Automated tests | P0 subset (A-38): built-in template packs (employee handbook, starter policies) installed as drafts with the right kind permissions, Templates gallery page; E2E-R1-18. Built-in request types (0.2, docs/19 §5.6): leave, expense claim, purchase requisition, access request compiled at boot and pinned. Remaining: install wizard, the other Wave-1 process templates, role mapping at install. |
| BPMS-108 | All requests / all work / workload / process health / ops monitor | **In progress** | Automated tests | Workload (FR-A03) shipped early with 0.2: GET /work/workload?scope=team|all (per person open / overdue / due in 7 days / oldest, away + delegate, coverage gaps) and /work/workload/{member} (their open steps); Team page for managers (team = reporting line, derived from the directory, no grant needed) and Requests oversight › Workload for admins; bulk reassign with a preview, refusals per step; a manager reassigns within their team without work:reassign (docs/12 §6). E2E-R2-UI-13. Not yet: all-requests/process health/ops monitor. |

## Done

- [x] **BPMS-002** Model 5 reference processes in IR on paper — 5 reference processes as IR v1 fixtures (all 16 node kinds) + JSON Schema + structural checker IR-E001..E016 with negative tests; 12 IR spec clarifications (docs/spikes/BPMS-002)
- [x] **BPMS-003** SP-01…SP-06 runtime spikes (native vs Temporal vs Operaton) — SP-01..06 native runtime: 10k instances under 1329 crashes 0 lost; race exactly-once; join once; 410 retries 0 dup effects; 30-day pinning; BPMN 54-node lossless round trip — docs/spikes/SP-01-06; ADR-0015 recommended
- [x] **BPMS-004** SP-07 licence/SBOM review of candidate deps — SP-07: Rust + JS dependency licences reviewed; approved set; JS licence gate (scripts/js-licence-gate.mjs) + CDLA-Permissive-2.0 for CA roots; fonts OFL — docs/spikes/SP-07
- [x] **BPMS-005** SP-08 expression language (dsntk FEEL vs CEL, incl. WASM) — SP-08: recommend CEL + decimal/stdlib/type-checker/budgets; FEEL rejected at runtime (quadratic loops, no WASM) — docs/spikes/SP-08; ADR-0010 recommended
- [x] **BPMS-006** SP-09 Cedar + RLS + sqlx pool + partial eval — SP-09: Cedar 6/6 correct, 5.6µs P50; RLS via pool 0 leaks/5000; LLD fix NULLIF in RLS policy; partial-eval→SQL viable with fallback — docs/spikes/SP-09
- [x] **BPMS-007** SP-10 audit chain 10M events: append + explorer — SP-10: head-lock 9.7k ev/s (P99 11ms), batched sealer 65k ev/s, explorer P95 <0.6ms at 10M, verifier 536k ev/s + tamper detected — docs/spikes/SP-10
- [x] **BPMS-010** Design tokens + core components + visual direction — Tokens (41 contrast checks + forbidden-pair guard), Button/TextField/Checkbox/Notice/Select/ConfirmDialog/Tabs/DataGrid shell on React Aria + TanStack, app shell, gallery; axe light/dark, keyboard, focus trap, aria-sort, reflow, targets (E2E-P0-05..13)
- [x] **BPMS-011** Monorepo, CI gates (fmt, clippy, deny, tests, a11y, SBOM) — Rust workspace, kernel, procera binary, CI (fmt/clippy/test/deny/SBOM/E2E smoke) + web check/build, JS licence gate and axe a11y job (added with BPMS-010)
- [x] **BPMS-013** E2E harness: Playwright + compose.e2e (PG, SeaweedFS, Valkey, Mailpit, ClamAV), seed A/B, personas, __test clock/drain/fault (feature-gated) — e2e feature + /__test controls, Playwright harness, P0 smoke pack (E2E-P0-01..04, REL), compose.e2e stack, CI e2e-smoke job; on main, CI green. Seed A/B + personas land with the first workspace APIs (BPMS-032); full compose boot pending a Docker run Self-hosting: deploy/compose.yml (web+API behind Caddy, Postgres, migrate, optional cache/scan/mail profiles), procera healthcheck, SIGTERM shutdown; guides for Docker Compose install, first-time setup and configuration (docs/guides).
- [x] **BPMS-014** AGENTS.md/CLAUDE.md contract, PR template (spec id + E2E id), CODEOWNERS for protected areas — CLAUDE.md, AGENTS.md, PR template, CODEOWNERS, docs/24 rules on main
- [x] **BPMS-020** System plane: bootstrap, system roles, workspaces lifecycle, quotas — procera-platform + /sysapi/v1: one-time bootstrap token (printed at startup until bootstrapped), system.owner/admin/operator/auditor fixed authz table, MFA required for system plane, workspace create/list/get/rename/suspend/restore/schedule-deletion with If-Match + ETag, profile quotas, platform setting floors seeded, all audited; E2E-R1-SYS-01..05 Workspace rename from workspace Settings (PATCH /api/v1/workspace, audited on the system and workspace chains), E2E-R1-UI-47.
- [x] **BPMS-021** Local auth (Argon2id), email verify, lockout, sessions — procera-identity + /api/v1/auth: users, Argon2id (policy, rehash, enumeration-safe), JWT access (ADR-0020) + rotating refresh with reuse detection, DB login throttling + per-IP rate limit, logout, sign-out-everywhere, deactivation via token_version; audited; E2E-R1-AUTH-01..05,07 Account recovery (migration 0036, docs/06 §3.6.1, A-39/A-40): forgot password with hashed single-use 30-minute reset links (no enumeration, rate limited), reset revokes sessions and keeps two-step, change password, admin-required change at next sign-in; admin account controls on the person page (member:security.manage, step-up and reason, only-workspace rule): send reset link, require password change, sign out everywhere, reset two-step; Account › Profile (name, time zone, language). E2E-R1-UI-46, 48..50, AUD-01.
- [x] **BPMS-022** MFA TOTP + WebAuthn, recovery codes, step-up — TOTP + recovery codes + step-up + MFA by role (earlier); passkeys (WebAuthn L3, migration 0020): registration, sign-in second factor and step-up, list/rename/remove with step-up, phishing-origin and replay protection, Account › Security page, Use a passkey at sign-in and in step-up. E2E-R1-AUTH-06, 20..23, E2E-R1-UI-30..31. Passkey-only sign-in is P1 (A-19) No second sign-in after turning on two-step: the confirmed code upgrades the session and its refresh family.
- [x] **BPMS-023** Memberships, invites, directory (departments/groups/positions/managers), CSV import — procera-members + /api/v1: memberships, invitations (hashed + sealed tokens, 7-day expiry, email-bound, single use; list, revoke, resend), register-by-invite, owner onboarding activates the workspace, workspace switch (ws-scoped JWT), deactivation guards, state gate (423). Directory (migration 0008): department tree (uuid[] paths, sibling-unique names, no moves under descendants), static groups, positions (behind directory.positions.enabled), manager links with cycle rejection, deterministic manager resolution (explicit → nearest department head), per-field source attribution, CSV import with plan/apply. All audited; E2E-R1-MEM-01..03,05..07, E2E-R1-DIR-01..04 Invitation page /invite (GET /invitations/preview, A-24): create account and join, or sign in and join; deep links survive sign-in. E2E-R1-FLOW-01, 02. Directory import UI with a check step and per-row reasons (E2E-R1-04); deactivate from the person page with immediate session exit (E2E-R1-16); person page with department, manager and location. Directory UI: Departments page (tree, add, rename, move, head, delete only when empty), Groups (list, group page, members), editable person profile with groups, reactivate with step-up and reason (A-41), resend and revoke invitations, bulk set department and add to group. E2E-R1-UI-51..55. Later: people list per department, Used by, group owners. 0.1 gaps: copy an invitation link when email isn't set up (members and the owner invite), leaving the organisation as a 3-step flow (review, hand over, confirm), department people and Used by. E2E-R1-UI-71..72, 74..75.
- [x] **BPMS-024** Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar — procera-access: permission catalogue + 12 role templates; custom roles (no escalation on create/edit/grant/revoke, built-ins immutable, clone lineage); grants to members or static groups (migration 0009), scoped (workspace/department/collection/process/case) with expiry, compiled to Cedar; members inherit their department subtree so department-scoped admins reach only it; invariants (workspace isolation, active members only); last-owner protection; role change preview; access_revisions cache; all changes audited. Property tests: no escalation (64 cases) and preview matches effect (48 cases), both against Postgres. E2E-R1-MEM-04,08,09 Roles & access UI: roles list and drawer, custom roles, grants with scope and expiry, collection-scoped library grants (A-34); the Member baseline is always granted on accept, so specialist roles add to it (E2E-R1-05).
- [x] **BPMS-025** Forced RLS + repository WsCtx + CI cross-workspace sweep — procera-db: migrations (0001-0003), roles bootstrap (app/migrator/discovery), WsTx with tx-local set_config, procera_enable_workspace_rls() with NULLIF, procera_rls_violations guard (tested + enforced at startup), discovery.* functions with column grants; 6 DB tests
- [x] **BPMS-026** Audit chain append + coverage contract test — procera-audit: JCS/SHA-256 per-workspace chain in the business tx, batched sealer, verifier, monthly partitions with forced RLS, append-only grants + trigger; tamper test. Route audit contract (audit_contract.rs): a unit test fails on any mutating route without an entry; E2E-R1-AUD-01 checks every route's appended events against it. Gaps closed: TOTP enrolment start, workspace selection, role/grant changes. data_access events follow with restricted data (BPMS-040+)
- [x] **BPMS-027** Settings layering, floors, profiles, explainer — Catalogue, floors, profiles, explainer (earlier); department overrides as layer 4 with tree inheritance (0021), profile switch plan/apply with If-Match, reason and step-up (CFG-017), effective-settings cache with generation invalidation over Valkey, never authoritative (CFG-004); Settings tabs for departments and profile. E2E-R1-CFG-01..08, E2E-R1-UI-16, 35..36 (A-17) Settings redesign (owner: crowded and confusing; docs/design/ia-competitive-analysis.md §6): seven category pages with search, Advanced sections, unbuilt-feature keys and choices hidden, department overrides on each setting, starting-point banner. E2E-R1-UI-61, 35, 36. 0.2: an eighth category, Requests (request types, who can ask, currency, out of office); new per-item audiences kind, validated server-side.
- [x] **BPMS-028** Shared DataGrid v1 (sort, filter builder, saved views, export, keyboard) — DataGrid v1 (web/src/ui/components/DataGrid.tsx): quick search, typed filter builder with chips and plain-language summary, multi-sort with order numbers, columns chooser, density, formula-safe CSV export audited via POST /exports, saved views per list (personal/shared, default, modified marker; migration 0028; /views API) with URL state, selection with shift-range and bulk actions, Enter/Space keyboard, live count, cards below 40rem; adopted by Library and Members. E2E-R1-VIEW-01, E2E-R1-UI-38, E2E-P0-10. v2: AND/OR filter groups, views shared with roles (migration 0034), XLSX export (E2E-R1-13). Later: server-driven query + cursor paging, async export, column resize.
- [x] **BPMS-029** Outbox + dispatcher + inbox dedup + idempotency keys — Outbox/inbox/idempotency tables + helpers (SKIP LOCKED claims, replay/conflict). Dispatcher (procera_db::dispatch, migration 0010): due-workspace discovery, per-workspace advisory lock, per-aggregate ordered delivery to a Bus, at-least-once with exponential backoff (max 5 min) and hold-back behind a failed event, NOTIFY trigger wake-ups + 1 s polling; runs in serve --all; /__test/drain really drains. Tests: order/backoff/hold-back/workspaces + NOTIFY wake-up; E2E-R1-MEM-07. Consumers (mailer) arrive with BPMS-048
- [x] **BPMS-032** E2E-R1 pack (R1-01…19 + ISO) — R1 gate pack started in the browser (packs/r1/gate.ui.spec.ts): E2E-R1-01 setup page, E2E-R1-02 new workspaces with owner onboarding, E2E-R1-05 roles with collection scope, E2E-R1-03 accent branding with contrast-safe shades (logo and terminology pending), E2E-R1-04 directory import, E2E-R1-06 controlled policy SoD, step-up approve, scheduled go-live, superseded; E2E-R1-10 announcement approval and expiry, E2E-R1-07 re-ack at scale with evidence, E2E-R1-11 resumable 50 MB upload and links, E2E-R1-12 Files retention and held files, E2E-R1-13 DataGrid groups, shared views and exports, E2E-R1-18 startup day one with template packs, E2E-R1-08 new joiner gets open campaigns, E2E-R1-09 J14 location audience with confirm-by, campaign engine, one escalation, archive closes, E2E-R1-16 deactivation, E2E-R1-14 audit trail in the browser, E2E-R1-15 overview drill-downs, E2E-R1-17 read-only workspace with banner, E2E-R1-19 keyboard only, E2E-R1-ISO sweep (21 probes → 404, each audited as access.denied, A-31); plus complete flows E2E-R1-FLOW-01..06. All 19 R1 scenarios green in the browser. Notes: R1-01 full bootstrap needs a fresh database (covered by E2E-R1-SYS-01); R1-18 plans the first campaign rather than creating it.
- [x] **BPMS-033** Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback — procera-cache: memory + Valkey/Redis (redis 1.7, BSD-3) with ws:/sys: key namespacing, TTL-only writes, Lua fixed-window rate limit with in-process fallback, degraded health; wired into procera (/readyz), CI Valkey service
- [x] **BPMS-040** Artifacts, versions, lifecycle state machine, immutability trigger, SoD — procera-library (migration 0012): artifacts + versions for SOPs and policies; lifecycle draft → in_review → approved → scheduled/published → deprecated → retired with guarded transitions; one open draft per artifact; reviews in numbered rounds, approvals from governance.review.* settings, author never reviews own version, distinct approvers, comment required to reject; content_hash = SHA-256(JCS) frozen at approval and a trigger forbids changing approved content or moving back (T07); publish supersedes, future effective_at schedules (worker + drain), reason for change on later versions, retire; step-up for controlled approve/publish; outbox artifact.published.v1; every transition audited. E2E-R1-LIB-01..05. Process artifacts arrive with the designer/compiler Review notifications (A-25): reviewers told on submit (in-app + email), author told the outcome with the comment. E2E-R1-FLOW-03, 05. Clean reading view for plain readers (details tucked away); authors aren't offered review of their own version. E2E-R1-UI-73.
- [x] **BPMS-041** Landscape, collections, owners, RACI, visibility — Collections (migration 0025): a tree with uuid[] paths, sibling-unique names, moves that rewrite the subtree, empty-only deletion; per-collection visibility (audience) hiding filed documents from view-only readers in lists, reads, search, the policy portal and the tree (A-20); document owner hand-over; RACI per document (members or groups, one accountable) with formula-safe CSV export (audited); Library collection rail with filtering and management, document-page Collection/Owner/RACI rows. E2E-R1-LIB-11..13, E2E-R1-UI-27. Later: landscape for processes (domain › capability › process) with the designer, collection-scoped grants (A-20).
- [x] **BPMS-043** Semantic diff across artifact kinds — UI: Compare page (/library/:id/compare) with word-level ins/del, version pickers, E2E-R1-UI-20. procera-library diff module: block-level LCS alignment of SOP/policy content with word-level diffs inside changed blocks (removals and additions paired by position within an unmatched run, same block type → changed); GET /versions/{a}/diff/{b} (same artifact only); unit + property test (applying a diff always rebuilds the target) + E2E-R1-LIB-06. Process/form/rule bundle artifacts diff once their editors land (BPMS-050+).
- [x] **BPMS-044** Reviews, change requests, effective-date scheduling, renewal date, verification badge — UI: document governance panel (review status, recertify, schedule, verify with step-up, suggested changes with accept/decline/implemented), E2E-R1-UI-18..19. Migration 0016: review cadence (per-artifact override or governance.review_cadence.default/.controlled), next_review_at set on every publish, recertification, worker flag of passed review dates (once; artifact.review_overdue.v1 → owner in-app notification), verification badge with governance.verification.validity expiry, change requests (open → accepted/declined/implemented, linked to the implementing version); all audited and in the audit contract; E2E-R1-LIB-07..10. Effective-date scheduling shipped with BPMS-040.
- [x] **BPMS-045** Policy library + employee policy portal + applicability — Category, applicability and restricted policies; Policies portal with what changed (migration 0017, A-14); re-acknowledgement on publish (0019): major/minor semver fixed at publish or scheduling, governance.reack.policy/window, campaign to the applicable audience bound to the exact version hash (immediate in the publish transaction, scheduled on go-live via the bus), publish preview and dialog. E2E-R1-POL-01..05, E2E-R1-UI-21..22, 24. Later: per-artifact acknowledgement requirement (docs/05), closing older campaigns (A-16)
- [x] **BPMS-046** Campaign engine: ack/quiz-lite, audience snapshot, reminders, escalation, new joiners, evidence export — procera-campaigns (migration 0013): acknowledgement campaigns for a published SOP/policy version; audience via the shared resolver (snapshot or dynamic), assignments with in-app + email notices, acknowledgement bound to version id + content hash + statement + auth method, reminder ladder and one-time manager escalation by the scheduler, dynamic new joiners, close/cancel, progress, audited evidence CSV; settings campaign.*; E2E-R1-CMP-01..07. Later: quiz-lite, exemptions, signed PDF evidence, auto re-ack on major publish Campaign subjects include announcements (0033). Leavers' pending acknowledgements close as left organisation (migration 0038), excluded from progress, never marked acknowledged.
- [x] **BPMS-047** Announcements: compose, targeting, schedule/expiry, pin, required ack, approval, analytics — procera-announcements (migration 0014): drafts with block content, audience snapshot, approval per announcements.approval_required with SoD, scheduling + derived expiry, pinned/critical, channels from settings, in-app + email delivery via the outbox, read and acknowledgement receipts, analytics, archive; settings announcements.*; E2E-R1-ANN-01..06. Later: poster scoping, reminders/escalation for unacked, comments/reactions, reach preview Approval loop notifications (A-25): approvers told on submit, author told approved or sent back. E2E-R1-FLOW-04, 06. Announcement acknowledgements run on the campaign engine (migration 0033, A-32): confirm-by date, reminder ladder, one manager escalation, archive closes; composer Confirm by and drawer link. E2E-R1-09.
- [x] **BPMS-048** In-app notification centre + SMTP email + templates + digests — procera-notify (migrations 0011, 0023): templates in code (minijinja, strict, HTML auto-escaped), in-app inbox, sealed email queue with dedupe, SMTP delivery via lettre with backoff; digests (daily/weekly at the member's hour and time zone; only normal emails are digested), quiet hours (emails held until the window ends; critical override), bounce suppression on permanent SMTP rejection (global email_suppressions); member preferences with workspace defaults (notify.digest.*, notify.quiet_hours.*); Notification preferences page. E2E-R1-NOT-01..05, E2E-R1-UI-25. Later: localisation + terminology (CFG-008), per-day quiet hours (A-15), more event kinds as features land
- [x] **BPMS-049** Postgres FTS + trgm, permission-filtered search — procera-search (migration 0015): Postgres FTS (english + simple) + pg_trgm fuzzy/prefix over SOPs, policies (artifact tsv kept current by triggers), members, departments, groups; one ranked list with escaped <mark> snippets; permission-filtered per type with Cedar before querying; RLS isolation; E2E-R1-SRCH-01..03
- [x] **BPMS-063** Workspace overview dashboard + people & access monitor — API: GET /admin/overview (needs-attention ranking: overdue acknowledgements, admins without MFA, documents past review, drafts in review, announcements to approve; acknowledgement metrics, published-versions series, security strip from the audit chain, departments; requests/storage reported as null with guidance until those modules ship) and GET /admin/people (MFA, last active, roles, dormant, privileged, expiring grants, pending invites, filters); migration 0018 discovery.member_login_failures. UI: Overview (typographic summary, ranked list, metric rows, security strip; time range + department in the URL) and People & access (filter chips). E2E-R1-ADM-01..02, E2E-R1-UI-23.
- [x] **BPMS-064** Audit explorer, timelines, saved queries, export — procera_audit::explorer and query: filtered newest-first keyset paging, view-own scope, member names, artifact timelines (own and version events), chain verify, saved queries, capped synchronous CSV/JSONL exports with step-up; POST /audit/query (AND/OR tree, allow-listed fields, depth 2); background export jobs (migration 0029) stored as the requester's encrypted file with notification; Audit log UI with filters, timelines and background exports. E2E-R1-AUDX-01..08, E2E-R1-AUD-01, E2E-R1-UI-17, 39. Later: verify-proof per sequence, the P95 < 2 s at 10 M events benchmark Events name their object (“Expense claims v1”, a person, a campaign).
- [x] **BPMS-065** System console: workspaces grid, quotas, health, system audit — System console (0022): workspaces grid with usage and quotas, suspend/reactivate/schedule deletion with step-up on the system chain, quota defaults per profile with the member quota enforced at invite, platform health, system audit explorer; console shell for system admins (metadata only, never workspace content). E2E-R1-SYS-01..09, E2E-R1-UI-40..41 (A-18) Email & platform (migration 0037): SMTP settings with the password sealed and never returned, test send with plain-language failures, hot reload without restart, environment wins; public URL, storage and scanning status; first-time setup continues to Email and the first workspace. E2E-R1-UI-64..65. Policy floors page: every floor grouped by Settings category with plain-words meaning; edit with reason, step-up and If-Match; auditors read-only. E2E-R1-UI-76..77.
- [x] **BPMS-066** Retention policy per record class (policy only) — Retention classes honoured by jobs (migration 0024, docs/14 §4): draft (deleted after retention.draft.inactive_after without edits; authors notified retention.draft.notice_before ahead; a never-published artifact goes with its only draft; drafts linked to change requests kept), operational (sent/failed emails, read notifications, sent digest items after retention.operational.keep_for), governance_record and audit never deleted by the app (disposition with certificates is P1); hourly in the scheduler, every run recorded and audited (retention.purged); Retention admin page with due drafts and period changes (reason + step-up). E2E-R1-RET-01..02, E2E-R1-UI-26.

## Release checklists

Legend: `[x]` done · `[ ]` open. The status is shown in brackets when the item isn't simply not started.

<details open>
<summary><b>P0 — Spikes + G1 + design system</b> · 2026-11-02 → 2026-11-20 · gate G1 · 10/14 done</summary>

_SP-01…SP-10, ADRs, tokens, CI + E2E harness smoke (old M0–M1)_

- [ ] **BPMS-001** (P0, M) User interviews (12+, startup/SME/MNC × HR/IT/Fin/Compliance/Admin) + synthesis
- [x] **BPMS-002** (P0, S) Model 5 reference processes in IR on paper
- [x] **BPMS-003** (P0, L) SP-01…SP-06 runtime spikes (native vs Temporal vs Operaton)
- [x] **BPMS-004** (P0, S) SP-07 licence/SBOM review of candidate deps
- [x] **BPMS-005** (P0, M) SP-08 expression language (dsntk FEEL vs CEL, incl. WASM)
- [x] **BPMS-006** (P0, M) SP-09 Cedar + RLS + sqlx pool + partial eval
- [x] **BPMS-007** (P0, M) SP-10 audit chain 10M events: append + explorer
- [ ] **BPMS-008** (P0, M) Clickable prototype of Wave-1 flows + 5-user test
- [ ] **BPMS-009** (P0, S) G1: accept ADR-0010/0011/0015; freeze IR v1
- [x] **BPMS-010** (P0, L) Design tokens + core components + visual direction
- [x] **BPMS-011** (P0, M) Monorepo, CI gates (fmt, clippy, deny, tests, a11y, SBOM)
- [ ] **BPMS-012** (P0, S) Threat model review
- [x] **BPMS-013** (P0, M) E2E harness: Playwright + compose.e2e (PG, SeaweedFS, Valkey, Mailpit, ClamAV), seed A/B, personas, __test clock/drain/fault (feature-gated)
- [x] **BPMS-014** (P0, S) AGENTS.md/CLAUDE.md contract, PR template (spec id + E2E id), CODEOWNERS for protected areas

</details>

<details>
<summary><b>R1 — Workspace & Policies</b> · 2026-11-23 → 2027-01-15 · gate G-R1 · 24/32 done</summary>

_System console, auth+MFA, roles, settings, DataGrid, SOP/policy library, campaigns, announcements, files, audit, dashboard (old M2–M4 part)_

- [x] **BPMS-020** (P0, L) System plane: bootstrap, system roles, workspaces lifecycle, quotas
- [x] **BPMS-021** (P0, M) Local auth (Argon2id), email verify, lockout, sessions
- [x] **BPMS-022** (P0, M) MFA TOTP + WebAuthn, recovery codes, step-up
- [x] **BPMS-023** (P0, M) Memberships, invites, directory (departments/groups/positions/managers), CSV import
- [x] **BPMS-024** (P0, L) Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar
- [x] **BPMS-025** (P0, M) Forced RLS + repository WsCtx + CI cross-workspace sweep
- [x] **BPMS-026** (P0, M) Audit chain append + coverage contract test
- [x] **BPMS-027** (P0, M) Settings layering, floors, profiles, explainer
- [x] **BPMS-028** (P0, L) Shared DataGrid v1 (sort, filter builder, saved views, export, keyboard)
- [x] **BPMS-029** (P0, M) Outbox + dispatcher + inbox dedup + idempotency keys
- [ ] **BPMS-030** (P0, M) Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK **[In progress]**
- [ ] **BPMS-031** (P0, M) App shell, workspace switcher, system console shell, ⌘K **[In progress]**
- [x] **BPMS-032** (P0, L) E2E-R1 pack (R1-01…19 + ISO)
- [x] **BPMS-033** (P0, M) Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback
- [ ] **BPMS-034** (P0, S) Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search **[In progress]**
- [x] **BPMS-040** (P0, L) Artifacts, versions, lifecycle state machine, immutability trigger, SoD
- [x] **BPMS-041** (P0, M) Landscape, collections, owners, RACI, visibility
- [ ] **BPMS-042** (P0, L) SOP/policy block editor + renderer (web/PDF) **[In progress]**
- [x] **BPMS-043** (P0, M) Semantic diff across artifact kinds
- [x] **BPMS-044** (P0, M) Reviews, change requests, effective-date scheduling, renewal date, verification badge
- [x] **BPMS-045** (P0, M) Policy library + employee policy portal + applicability
- [x] **BPMS-046** (P0, L) Campaign engine: ack/quiz-lite, audience snapshot, reminders, escalation, new joiners, evidence export
- [x] **BPMS-047** (P0, L) Announcements: compose, targeting, schedule/expiry, pin, required ack, approval, analytics
- [x] **BPMS-048** (P0, M) In-app notification centre + SMTP email + templates + digests
- [x] **BPMS-049** (P0, M) Postgres FTS + trgm, permission-filtered search
- [ ] **BPMS-060** (P0, L) Files pipeline: resumable upload, hash, sniff, ClamAV, quarantine, signed URLs **[In progress]**
- [ ] **BPMS-061** (P0, M) Previews (image/PDF) in sandbox; Office preview spike **[In progress]**
- [ ] **BPMS-062** (P0, M) Workspace Files view, quotas, orphan cleanup **[In progress]**
- [x] **BPMS-063** (P0, L) Workspace overview dashboard + people & access monitor
- [x] **BPMS-064** (P0, L) Audit explorer, timelines, saved queries, export
- [x] **BPMS-065** (P0, M) System console: workspaces grid, quotas, health, system audit
- [ ] **BPMS-120** (P0, L) Pilot workspaces (Starter + Enterprise profile), 3–5 real processes, 4 weeks

</details>

<details>
<summary><b>R2 — Requests & Approvals</b> · 2027-01-18 → 2027-03-12 · gate G-R2 · 0/17 done</summary>

_Runtime core, forms, designer, portal, inbox, delegation, SLA, 6 templates (old M5–M6 part)_

- [ ] **BPMS-080** (P0, L) IR v1 model + compiler (§11 checks, diagnostics, semantic diff) **[In progress]**
- [ ] **BPMS-081** (P0, M) Expression engine integration (typed, budgeted, WASM build) **[In progress]**
- [ ] **BPMS-082** (P0, L) Interpreter: advance loop, tokens, splits/joins, end/terminate **[In progress]**
- [ ] **BPMS-083** (P0, L) Jobs + timers + scheduler with leases/fencing; calendars; SLA clocks **[In progress]**
- [ ] **BPMS-084** (P0, M) Release bundles + publish integration
- [ ] **BPMS-085** (P0, L) Work items, assignment strategies, manager chain, DoA matrix, quorum, per-group **[In progress]**
- [ ] **BPMS-086** (P0, M) Delegation, OOO substitution, SoD re-check, send-back **[In progress]**
- [ ] **BPMS-087** (P0, M) Rule tables (UNIQUE/FIRST/COLLECT)
- [ ] **BPMS-088** (P0, M) Incidents, retry/skip/cancel, instance admin ops **[In progress]**
- [ ] **BPMS-089** (P0, L) Conformance suite + model-based tests + deterministic simulation harness
- [ ] **BPMS-100** (P0, XL) Step builder + React Flow canvas + properties + validate/test-run/review/publish
- [ ] **BPMS-101** (P0, XL) Form builder + renderer (calc, prefill, repeating, lookups, PDF of submission) **[In progress]**
- [ ] **BPMS-103** (P0, L) Request portal/catalogue, start form, request detail, comments/@mentions/watchers **[In progress]**
- [ ] **BPMS-105** (P0, L) My Work 3-pane inbox (tasks, acks, signatures), bulk with preview **[In progress]**
- [ ] **BPMS-106** (P0, XL) Template gallery + install wizard + Wave-1 templates (13) **[In progress]**
- [ ] **BPMS-111** (P0, L) E2E-R2 pack (R2-01…16 + ISO) + Firefox
- [ ] **BPMS-121** (P0, M) Usability round + SUS measurement + fixes

</details>

<details>
<summary><b>R3 — Operate & Sign</b> · 2027-03-15 → 2027-05-07 · gate G-R3 · 1/8 done</summary>

_Admin console, DoA, master data, recall/edit, e-sign v1, 13 templates, PWA, BPMN import (old M4 rest, M6 rest, M7)_

- [x] **BPMS-066** (P0, M) Retention policy per record class (policy only)
- [ ] **BPMS-102** (P0, M) Master-data tables (CSV import, lookups, versioned)
- [ ] **BPMS-104** (P0, M) Recall/withdraw, edit-after-submit, request-on-behalf, numbering
- [ ] **BPMS-107** (P0, XL) E-sign v1: PDF, recipients/roles, routing, bulk, fields, consent step, ceremony, certificate
- [ ] **BPMS-108** (P0, L) All requests / all work / workload / process health / ops monitor **[In progress]**
- [ ] **BPMS-109** (P0, M) PWA (360px) for My Work, requests, acks, signing
- [ ] **BPMS-110** (P0, M) BPMN 2.0 XML import (subset) + compatibility report
- [ ] **BPMS-112** (P0, L) E2E-R3 pack (R3-01…12 + MOB + ISO) + WebKit

</details>

<details>
<summary><b>R4 — Enterprise Connect</b> · 2027-05-10 → 2027-07-02 · gate G-R4 · 0/13 done</summary>

_SSO/SCIM, scoped admins, webhooks/API/CLI, email approvals, guests, e-sign v2 (old M8)_

- [ ] **BPMS-140** (P1, L) OIDC SSO, SAML-broker, SSO enforcement, JIT
- [ ] **BPMS-141** (P1, M) SCIM 2.0 server
- [ ] **BPMS-142** (P1, M) Scoped/delegated admins; IP allowlists
- [ ] **BPMS-143** (P1, L) Webhooks (Standard Webhooks), HTTP connector via egress proxy, inbound events
- [ ] **BPMS-144** (P1, M) Public API hardening, OpenAPI, CLI, TS/Python SDKs
- [ ] **BPMS-145** (P1, M) Email-actionable approvals, reply-by-email, web push
- [ ] **BPMS-146** (P1, M) Guests + external signers (magic link + OTP); self-serve signing links
- [ ] **BPMS-147** (P1, L) E-sign v2: DOCX + merge, anchor tags, conditional fields, delegate, correction, embedded
- [ ] **BPMS-148** (P1, M) Public forms (privacy CAPTCHA), save & resume, partials, form signature field
- [ ] **BPMS-149** (P1, L) Subprocesses, reusable sub-flows, duplicate detection, bulk import, env promotion
- [ ] **BPMS-150** (P1, M) Auto-generated process documentation; policy exceptions; policy templates
- [ ] **BPMS-151** (P1, M) Emergency alerts, content validation period, moderation, quiz
- [ ] **BPMS-152** (P1, L) E2E-R4 pack incl. Keycloak, SCIM, webhook receiver

</details>

<details>
<summary><b>R5 — Cases & Compliance</b> · 2027-07-05 → 2027-08-27 · gate G-R5 · 0/11 done</summary>

_Cases, investigations, legal hold, Part 11, reports, AI optional (old M9)_

- [ ] **BPMS-160** (P1, XL) Case types, queues, macros, SLAs, link/merge, restricted cases
- [ ] **BPMS-161** (P1, L) Investigations: pins, legal hold, signed export, conflict-of-interest
- [ ] **BPMS-162** (P1, M) Checkpoints, RFC 3161, WORM export, procera-verify CLI
- [ ] **BPMS-163** (P1, M) Attestation campaigns, control mapping, access reviews
- [ ] **BPMS-164** (P1, M) Retention disposition + certificates, DSAR template, legal hold engine
- [ ] **BPMS-165** (P1, L) Part 11 mode, PAdES-LTV, TSA; in-person/kiosk signing
- [ ] **BPMS-166** (P1, XL) Alert centre, custom dashboards, report builder, scheduled reports, KPI goals, usage analytics, CSAT
- [ ] **BPMS-167** (P1, M) Separation mode; break-glass
- [ ] **BPMS-168** (P1, M) Role-based onboarding/training paths
- [ ] **BPMS-169** (P1, L) E2E-R5 pack incl. investigations, Part 11, break-glass
- [ ] **BPMS-186** (P1, L) AI assistance P1 set (drafting, summaries, translate, Q&A) behind kill switch

</details>

<details>
<summary><b>GA — 1.0 hardening + release</b> · 2027-08-30 → 2027-10-15 · gate G4/G5 · 0/8 done</summary>

_Load/HA/DR, pentest, a11y/VPAT, i18n, docs, full E2E regression (old M10–M11)_

- [ ] **BPMS-180** (P1, L) Load test at reference load; HA failover; RPO/RTO restore drill
- [ ] **BPMS-181** (P1, M) External pentest + ASVS L2 self-assessment + fixes
- [ ] **BPMS-182** (P1, M) WCAG 2.2 AA audit + VPAT 2.5 INT
- [ ] **BPMS-183** (P1, M) 3 UI languages + RTL + pseudo-locale
- [ ] **BPMS-184** (P1, L) Helm chart, air-gapped bundle, upgrade N-1→N tests, dedicated-DB tier
- [ ] **BPMS-185** (P1, M) Docs site, admin guide, API reference, trust pack
- [ ] **BPMS-187** (P1, M) E2E-GA regression: 3 browsers × 2 viewports, cache-off, upgrade-path R1→1.0, air-gapped
- [ ] **BPMS-190** (P1, S) GA release: signed artefacts, SBOM, provenance, runbooks

</details>

<details>
<summary><b>POST — Post-GA expansion</b> · 2027-10-18 → 2028-03-31 · gate Demand · 0/6 done</summary>

_Chat platforms, HRIS/ERP, QES/Aadhaar, BPMN/DMN, mining, AI agents (old M12+)_

- [ ] **BPMS-200** (P2, XL) Chat/host-platform connectors (identity, embed, cards, notifications, directory)
- [ ] **BPMS-201** (P2, XL) Chat cards (Slack/Teams), M365/Google, HRIS, ERP connectors
- [ ] **BPMS-202** (P2, L) QES via QTSP; Aadhaar eSign via ESP; SMS/IDV providers
- [ ] **BPMS-203** (P2, XL) BPMN view (diagram-js), DMN import/export, migration tool, simulation
- [ ] **BPMS-204** (P2, M) Event-log export OCEL 2.0 / XES
- [ ] **BPMS-205** (P2, L) Governed AI agent steps

</details>

## Domain backlogs

| Backlog | Items | Source |
|---|---|---|
| Templates (TPL) | 20 | [19-PROCESS-TEMPLATE-LIBRARY.md](../docs/19-PROCESS-TEMPLATE-LIBRARY.md) |
| UX (UX) | 49 | [05-UX-AND-DESIGN.md](../docs/05-UX-AND-DESIGN.md) |
| Integrations (INT) | 32 | [13-INTEGRATIONS.md](../docs/13-INTEGRATIONS.md) |
| Config (CFG) | 25 | [20-WORKSPACE-CONFIGURATION.md](../docs/20-WORKSPACE-CONFIGURATION.md) |
| AI (AI) | 25 | [16-AI-ASSISTANCE.md](../docs/16-AI-ASSISTANCE.md) |
| E2E scenarios (E2E) | 300 | [23-E2E-TEST-PLAN.md](../docs/23-E2E-TEST-PLAN.md) |
| Security tests | 37 | [06](../docs/06-SECURITY-AND-OPERATIONS.md) |
| Test matrix | 40 | [15](../docs/15-TEST-STRATEGY-AND-CONFORMANCE.md) |
| Open questions / unknowns / assumptions | 15 / 16 / 45 | [22](../docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) |
