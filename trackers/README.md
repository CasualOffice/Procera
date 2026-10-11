# Procera — tracker dashboard

> **Generated** by `scripts/build_tracker.py`. Don't edit by hand. To change status, edit the `STATUS` map (or `CORE`) in the script and re-run it ([rules](../docs/24-ENGINEERING-RULES.md#2-tracker-rules)).
> Detail: [docs/17](../docs/17-EXECUTION-TRACKER.md) · [Delivery_Tracker.xlsx](Delivery_Tracker.xlsx) · [backlog.csv](backlog.csv) · [domain_backlog.csv](domain_backlog.csv) · Roadmap: [docs/07](../docs/07-ROADMAP-AND-MILESTONES.md)

## At a glance

```text
Overall   ███████░░░░░░░░░░░░░░░░░░░░░░░  25/109 epics done  (23%)
Partial   ████████░░░░░░░░░░░░░░░░░░░░░░  27% counting in-progress epics as half done
Tests     107 automated E2E scenarios · 91 Rust tests (incl. property tests)
Status    done 25 · in review 0 · in progress 8 · blocked 0 · not started 76 · deferred 0
Now       P0 — Spikes + G1 + design system  (2026-11-02 → 2026-11-20, gate G1)
```

## Releases

| Release | Name | Window | Gate | Progress | Done |
|---|---|---|---|---|---|
| **P0** ◀ now | Spikes + G1 + design system | 2026-11-02 → 2026-11-20 | G1 | `█████████░░░` | 10/14 |
| **R1** | Workspace & Policies | 2026-11-23 → 2027-01-15 | G-R1 | `██████░░░░░░` | 15/32 |
| **R2** | Requests & Approvals | 2027-01-18 → 2027-03-12 | G-R2 | `░░░░░░░░░░░░` | 0/17 |
| **R3** | Operate & Sign | 2027-03-15 → 2027-05-07 | G-R3 | `░░░░░░░░░░░░` | 0/8 |
| **R4** | Enterprise Connect | 2027-05-10 → 2027-07-02 | G-R4 | `░░░░░░░░░░░░` | 0/13 |
| **R5** | Cases & Compliance | 2027-07-05 → 2027-08-27 | G-R5 | `░░░░░░░░░░░░` | 0/11 |
| **GA** | 1.0 hardening + release | 2027-08-30 → 2027-10-15 | G4/G5 | `░░░░░░░░░░░░` | 0/8 |
| **POST** | Post-GA expansion | 2027-10-18 → 2028-03-31 | Demand | `░░░░░░░░░░░░` | 0/6 |

## Active right now

| ID | Item | Status | Note |
|---|---|---|---|
| BPMS-022 | MFA TOTP + WebAuthn, recovery codes, step-up | **In progress** | TOTP (RFC 6238, replay-protected, sealed secrets) + 10 recovery codes + MFA login (E2E-R1-AUTH-06). Step-up: POST /session/step-up (TOTP code, or password without MFA) re-issues the token with a fresh auth_time; enforced against auth.stepup.window (401 step_up_required) for step-up settings, role create/edit and grant/revoke (E2E-R1-CFG-03). MFA required by role (auth.mfa.required_roles) enforced at workspace entry, 403 mfa_required (E2E-R1-MEM-10). Remaining: WebAuthn/passkeys |
| BPMS-027 | Settings layering, floors, profiles, explainer | **In progress** | procera-settings: catalogue in code (P0 subset, 38 keys) with comparators; platform floors + workspace values + append-only revisions (migration 0007, RLS); 4 configuration profiles applied at workspace creation; resolution with clamping when a floor is raised; explainer; /api/v1/settings (list, explainer, PUT per key with If-Match, history) and /sysapi/v1/settings floors; reasons for ⚡ keys, step-up = MFA session (interim); audited (setting.changed, platform.floor_changed); property test (512 cases, never looser than floor); E2E-R1-CFG-01..05. Remaining: department/collection overrides (needs directory), profile switch plan/apply (CFG-017), settings cache invalidation (CFG-004) |
| BPMS-030 | Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK | **In progress** | procera-crypto: LocalKey (env root key, HMAC-derived purpose keys), AES-256-GCM seal/open bound to context, token helpers, procera keygen. Remaining: KMS/OpenBao adapters, per-workspace KEKs, rotation |
| BPMS-031 | App shell, workspace switcher, system console shell, ⌘K | **In progress** | Web app on the Notion/Attio direction (owner choice): warm paper palette in tokens (contrast-checked), full-bleed pages with a warm icon sidebar, workspace + account menus (persisted theme), action-first Home (acknowledgements, reviews, announcements with inline actions, library table, activity, people), Library (filters, Attio-style table, New document dialog), document reading view with properties, lifecycle actions (edit, submit, approve, request changes, publish with step-up, new version), acknowledgement callout, Announcements feed with read/ack, Members with invite dialog; split sign-in with product preview; single-flight token refresh. E2E-R1-UI-01..10 with axe. Remaining: ⌘K command palette (search API is ready), system console shell, mobile bottom bar Second UI pass: Geist type, state kit (skeletons, empty states, load errors with retry), ⌘K palette over search + commands (E2E-R1-UI-12..13), admin screens: Manage announcements, Campaigns, Settings, Audit log, Overview, People & access (E2E-R1-UI-14..17, 23); Policies portal and document governance (E2E-R1-UI-18..22). Remaining: system console shell, keyboard nav audit |
| BPMS-034 | Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search | **In progress** | Raster assets + brand guide merged; SVG masters and trademark search pending |
| BPMS-045 | Policy library + employee policy portal + applicability | **In progress** | UI: Policies portal (search, category chips, what changed → compare) and applicability control on the document page, E2E-R1-UI-21..22. Migration 0017: policy category, applicability (audience; NULL = everyone) and restricted flag; procera_audience_contains SQL twin of the audience resolver; reader visibility (A-14): view-only readers see published history only, restricted policies hidden outside their applicability in lists, reads, diff, change requests and search; PUT /artifacts/{id}/applicability (audited); GET /me/policies portal with what-changed summaries vs the previous published version, text + category filters; E2E-R1-POL-01..03. Remaining: re-acknowledgement on major publish (FR-PL03), superseded watermark in exports. |
| BPMS-048 | In-app notification centre + SMTP email + templates + digests | **In progress** | procera-notify (migration 0011): templates in code (minijinja, strict, HTML auto-escaped), in-app inbox (list/unread/read/read-all), email queue with sealed bodies and dedupe, SMTP delivery via lettre (STARTTLS/TLS) with backoff (max 1 h, 12 tries), workspace sender name; outbox consumers: invitation email with accept link, member-joined in-app to the inviter. Flags --public-url, --smtp-url, --mail-from. E2E-R1-NOT-01..02. Remaining: digests + quiet hours (FR-N04), localisation + terminology (CFG-008), more event kinds as features land |
| BPMS-064 | Audit explorer, timelines, saved queries, export | **In progress** | API done: procera_audit::explorer — filtered newest-first keyset paging, view-own scope, member names, artifact-aware object timelines, chain verify, private audited saved queries, capped CSV/JSONL exports with step-up that are themselves audited; E2E-R1-AUDX-01..06. UI: Audit log screen (chain-verified badge, filters, keyset paging, expandable records, object timelines in a drawer; E2E-R1-UI-17). Remaining: async export job, POST /audit/query |

## Done

- [x] **BPMS-002** Model 5 reference processes in IR on paper — 5 reference processes as IR v1 fixtures (all 16 node kinds) + JSON Schema + structural checker IR-E001..E016 with negative tests; 12 IR spec clarifications (docs/spikes/BPMS-002)
- [x] **BPMS-003** SP-01…SP-06 runtime spikes (native vs Temporal vs Operaton) — SP-01..06 native runtime: 10k instances under 1329 crashes 0 lost; race exactly-once; join once; 410 retries 0 dup effects; 30-day pinning; BPMN 54-node lossless round trip — docs/spikes/SP-01-06; ADR-0015 recommended
- [x] **BPMS-004** SP-07 licence/SBOM review of candidate deps — SP-07: Rust + JS dependency licences reviewed; approved set; JS licence gate (scripts/js-licence-gate.mjs) + CDLA-Permissive-2.0 for CA roots; fonts OFL — docs/spikes/SP-07
- [x] **BPMS-005** SP-08 expression language (dsntk FEEL vs CEL, incl. WASM) — SP-08: recommend CEL + decimal/stdlib/type-checker/budgets; FEEL rejected at runtime (quadratic loops, no WASM) — docs/spikes/SP-08; ADR-0010 recommended
- [x] **BPMS-006** SP-09 Cedar + RLS + sqlx pool + partial eval — SP-09: Cedar 6/6 correct, 5.6µs P50; RLS via pool 0 leaks/5000; LLD fix NULLIF in RLS policy; partial-eval→SQL viable with fallback — docs/spikes/SP-09
- [x] **BPMS-007** SP-10 audit chain 10M events: append + explorer — SP-10: head-lock 9.7k ev/s (P99 11ms), batched sealer 65k ev/s, explorer P95 <0.6ms at 10M, verifier 536k ev/s + tamper detected — docs/spikes/SP-10
- [x] **BPMS-010** Design tokens + core components + visual direction — Tokens (41 contrast checks + forbidden-pair guard), Button/TextField/Checkbox/Notice/Select/ConfirmDialog/Tabs/DataGrid shell on React Aria + TanStack, app shell, gallery; axe light/dark, keyboard, focus trap, aria-sort, reflow, targets (E2E-P0-05..13)
- [x] **BPMS-011** Monorepo, CI gates (fmt, clippy, deny, tests, a11y, SBOM) — Rust workspace, kernel, procera binary, CI (fmt/clippy/test/deny/SBOM/E2E smoke) + web check/build, JS licence gate and axe a11y job (added with BPMS-010)
- [x] **BPMS-013** E2E harness: Playwright + compose.e2e (PG, SeaweedFS, Valkey, Mailpit, ClamAV), seed A/B, personas, __test clock/drain/fault (feature-gated) — e2e feature + /__test controls, Playwright harness, P0 smoke pack (E2E-P0-01..04, REL), compose.e2e stack, CI e2e-smoke job; on main, CI green. Seed A/B + personas land with the first workspace APIs (BPMS-032); full compose boot pending a Docker run
- [x] **BPMS-014** AGENTS.md/CLAUDE.md contract, PR template (spec id + E2E id), CODEOWNERS for protected areas — CLAUDE.md, AGENTS.md, PR template, CODEOWNERS, docs/24 rules on main
- [x] **BPMS-020** System plane: bootstrap, system roles, workspaces lifecycle, quotas — procera-platform + /sysapi/v1: one-time bootstrap token (printed at startup until bootstrapped), system.owner/admin/operator/auditor fixed authz table, MFA required for system plane, workspace create/list/get/rename/suspend/restore/schedule-deletion with If-Match + ETag, profile quotas, platform setting floors seeded, all audited; E2E-R1-SYS-01..05
- [x] **BPMS-021** Local auth (Argon2id), email verify, lockout, sessions — procera-identity + /api/v1/auth: users, Argon2id (policy, rehash, enumeration-safe), JWT access (ADR-0020) + rotating refresh with reuse detection, DB login throttling + per-IP rate limit, logout, sign-out-everywhere, deactivation via token_version; audited; E2E-R1-AUTH-01..05,07
- [x] **BPMS-023** Memberships, invites, directory (departments/groups/positions/managers), CSV import — procera-members + /api/v1: memberships, invitations (hashed + sealed tokens, 7-day expiry, email-bound, single use; list, revoke, resend), register-by-invite, owner onboarding activates the workspace, workspace switch (ws-scoped JWT), deactivation guards, state gate (423). Directory (migration 0008): department tree (uuid[] paths, sibling-unique names, no moves under descendants), static groups, positions (behind directory.positions.enabled), manager links with cycle rejection, deterministic manager resolution (explicit → nearest department head), per-field source attribution, CSV import with plan/apply. All audited; E2E-R1-MEM-01..03,05..07, E2E-R1-DIR-01..04
- [x] **BPMS-024** Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar — procera-access: permission catalogue + 12 role templates; custom roles (no escalation on create/edit/grant/revoke, built-ins immutable, clone lineage); grants to members or static groups (migration 0009), scoped (workspace/department/collection/process/case) with expiry, compiled to Cedar; members inherit their department subtree so department-scoped admins reach only it; invariants (workspace isolation, active members only); last-owner protection; role change preview; access_revisions cache; all changes audited. Property tests: no escalation (64 cases) and preview matches effect (48 cases), both against Postgres. E2E-R1-MEM-04,08,09
- [x] **BPMS-025** Forced RLS + repository WsCtx + CI cross-workspace sweep — procera-db: migrations (0001-0003), roles bootstrap (app/migrator/discovery), WsTx with tx-local set_config, procera_enable_workspace_rls() with NULLIF, procera_rls_violations guard (tested + enforced at startup), discovery.* functions with column grants; 6 DB tests
- [x] **BPMS-026** Audit chain append + coverage contract test — procera-audit: JCS/SHA-256 per-workspace chain in the business tx, batched sealer, verifier, monthly partitions with forced RLS, append-only grants + trigger; tamper test. Route audit contract (audit_contract.rs): a unit test fails on any mutating route without an entry; E2E-R1-AUD-01 checks every route's appended events against it. Gaps closed: TOTP enrolment start, workspace selection, role/grant changes. data_access events follow with restricted data (BPMS-040+)
- [x] **BPMS-029** Outbox + dispatcher + inbox dedup + idempotency keys — Outbox/inbox/idempotency tables + helpers (SKIP LOCKED claims, replay/conflict). Dispatcher (procera_db::dispatch, migration 0010): due-workspace discovery, per-workspace advisory lock, per-aggregate ordered delivery to a Bus, at-least-once with exponential backoff (max 5 min) and hold-back behind a failed event, NOTIFY trigger wake-ups + 1 s polling; runs in serve --all; /__test/drain really drains. Tests: order/backoff/hold-back/workspaces + NOTIFY wake-up; E2E-R1-MEM-07. Consumers (mailer) arrive with BPMS-048
- [x] **BPMS-033** Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback — procera-cache: memory + Valkey/Redis (redis 1.7, BSD-3) with ws:/sys: key namespacing, TTL-only writes, Lua fixed-window rate limit with in-process fallback, degraded health; wired into procera (/readyz), CI Valkey service
- [x] **BPMS-040** Artifacts, versions, lifecycle state machine, immutability trigger, SoD — procera-library (migration 0012): artifacts + versions for SOPs and policies; lifecycle draft → in_review → approved → scheduled/published → deprecated → retired with guarded transitions; one open draft per artifact; reviews in numbered rounds, approvals from governance.review.* settings, author never reviews own version, distinct approvers, comment required to reject; content_hash = SHA-256(JCS) frozen at approval and a trigger forbids changing approved content or moving back (T07); publish supersedes, future effective_at schedules (worker + drain), reason for change on later versions, retire; step-up for controlled approve/publish; outbox artifact.published.v1; every transition audited. E2E-R1-LIB-01..05. Process artifacts arrive with the designer/compiler
- [x] **BPMS-043** Semantic diff across artifact kinds — UI: Compare page (/library/:id/compare) with word-level ins/del, version pickers, E2E-R1-UI-20. procera-library diff module: block-level LCS alignment of SOP/policy content with word-level diffs inside changed blocks (removals and additions paired by position within an unmatched run, same block type → changed); GET /versions/{a}/diff/{b} (same artifact only); unit + property test (applying a diff always rebuilds the target) + E2E-R1-LIB-06. Process/form/rule bundle artifacts diff once their editors land (BPMS-050+).
- [x] **BPMS-044** Reviews, change requests, effective-date scheduling, renewal date, verification badge — UI: document governance panel (review status, recertify, schedule, verify with step-up, suggested changes with accept/decline/implemented), E2E-R1-UI-18..19. Migration 0016: review cadence (per-artifact override or governance.review_cadence.default/.controlled), next_review_at set on every publish, recertification, worker flag of passed review dates (once; artifact.review_overdue.v1 → owner in-app notification), verification badge with governance.verification.validity expiry, change requests (open → accepted/declined/implemented, linked to the implementing version); all audited and in the audit contract; E2E-R1-LIB-07..10. Effective-date scheduling shipped with BPMS-040.
- [x] **BPMS-046** Campaign engine: ack/quiz-lite, audience snapshot, reminders, escalation, new joiners, evidence export — procera-campaigns (migration 0013): acknowledgement campaigns for a published SOP/policy version; audience via the shared resolver (snapshot or dynamic), assignments with in-app + email notices, acknowledgement bound to version id + content hash + statement + auth method, reminder ladder and one-time manager escalation by the scheduler, dynamic new joiners, close/cancel, progress, audited evidence CSV; settings campaign.*; E2E-R1-CMP-01..07. Later: quiz-lite, exemptions, signed PDF evidence, auto re-ack on major publish
- [x] **BPMS-047** Announcements: compose, targeting, schedule/expiry, pin, required ack, approval, analytics — procera-announcements (migration 0014): drafts with block content, audience snapshot, approval per announcements.approval_required with SoD, scheduling + derived expiry, pinned/critical, channels from settings, in-app + email delivery via the outbox, read and acknowledgement receipts, analytics, archive; settings announcements.*; E2E-R1-ANN-01..06. Later: poster scoping, reminders/escalation for unacked, comments/reactions, reach preview
- [x] **BPMS-049** Postgres FTS + trgm, permission-filtered search — procera-search (migration 0015): Postgres FTS (english + simple) + pg_trgm fuzzy/prefix over SOPs, policies (artifact tsv kept current by triggers), members, departments, groups; one ranked list with escaped <mark> snippets; permission-filtered per type with Cedar before querying; RLS isolation; E2E-R1-SRCH-01..03
- [x] **BPMS-063** Workspace overview dashboard + people & access monitor — API: GET /admin/overview (needs-attention ranking: overdue acknowledgements, admins without MFA, documents past review, drafts in review, announcements to approve; acknowledgement metrics, published-versions series, security strip from the audit chain, departments; requests/storage reported as null with guidance until those modules ship) and GET /admin/people (MFA, last active, roles, dormant, privileged, expiring grants, pending invites, filters); migration 0018 discovery.member_login_failures. UI: Overview (typographic summary, ranked list, metric rows, security strip; time range + department in the URL) and People & access (filter chips). E2E-R1-ADM-01..02, E2E-R1-UI-23.

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
<summary><b>R1 — Workspace & Policies</b> · 2026-11-23 → 2027-01-15 · gate G-R1 · 15/32 done</summary>

_System console, auth+MFA, roles, settings, DataGrid, SOP/policy library, campaigns, announcements, files, audit, dashboard (old M2–M4 part)_

- [x] **BPMS-020** (P0, L) System plane: bootstrap, system roles, workspaces lifecycle, quotas
- [x] **BPMS-021** (P0, M) Local auth (Argon2id), email verify, lockout, sessions
- [ ] **BPMS-022** (P0, M) MFA TOTP + WebAuthn, recovery codes, step-up **[In progress]**
- [x] **BPMS-023** (P0, M) Memberships, invites, directory (departments/groups/positions/managers), CSV import
- [x] **BPMS-024** (P0, L) Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar
- [x] **BPMS-025** (P0, M) Forced RLS + repository WsCtx + CI cross-workspace sweep
- [x] **BPMS-026** (P0, M) Audit chain append + coverage contract test
- [ ] **BPMS-027** (P0, M) Settings layering, floors, profiles, explainer **[In progress]**
- [ ] **BPMS-028** (P0, L) Shared DataGrid v1 (sort, filter builder, saved views, export, keyboard)
- [x] **BPMS-029** (P0, M) Outbox + dispatcher + inbox dedup + idempotency keys
- [ ] **BPMS-030** (P0, M) Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK **[In progress]**
- [ ] **BPMS-031** (P0, M) App shell, workspace switcher, system console shell, ⌘K **[In progress]**
- [ ] **BPMS-032** (P0, L) E2E-R1 pack (R1-01…19 + ISO)
- [x] **BPMS-033** (P0, M) Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback
- [ ] **BPMS-034** (P0, S) Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search **[In progress]**
- [x] **BPMS-040** (P0, L) Artifacts, versions, lifecycle state machine, immutability trigger, SoD
- [ ] **BPMS-041** (P0, M) Landscape, collections, owners, RACI, visibility
- [ ] **BPMS-042** (P0, L) SOP/policy block editor + renderer (web/PDF)
- [x] **BPMS-043** (P0, M) Semantic diff across artifact kinds
- [x] **BPMS-044** (P0, M) Reviews, change requests, effective-date scheduling, renewal date, verification badge
- [ ] **BPMS-045** (P0, M) Policy library + employee policy portal + applicability **[In progress]**
- [x] **BPMS-046** (P0, L) Campaign engine: ack/quiz-lite, audience snapshot, reminders, escalation, new joiners, evidence export
- [x] **BPMS-047** (P0, L) Announcements: compose, targeting, schedule/expiry, pin, required ack, approval, analytics
- [ ] **BPMS-048** (P0, M) In-app notification centre + SMTP email + templates + digests **[In progress]**
- [x] **BPMS-049** (P0, M) Postgres FTS + trgm, permission-filtered search
- [ ] **BPMS-060** (P0, L) Files pipeline: resumable upload, hash, sniff, ClamAV, quarantine, signed URLs
- [ ] **BPMS-061** (P0, M) Previews (image/PDF) in sandbox; Office preview spike
- [ ] **BPMS-062** (P0, M) Workspace Files view, quotas, orphan cleanup
- [x] **BPMS-063** (P0, L) Workspace overview dashboard + people & access monitor
- [ ] **BPMS-064** (P0, L) Audit explorer, timelines, saved queries, export **[In progress]**
- [ ] **BPMS-065** (P0, M) System console: workspaces grid, quotas, health, system audit
- [ ] **BPMS-120** (P0, L) Pilot workspaces (Starter + Enterprise profile), 3–5 real processes, 4 weeks

</details>

<details>
<summary><b>R2 — Requests & Approvals</b> · 2027-01-18 → 2027-03-12 · gate G-R2 · 0/17 done</summary>

_Runtime core, forms, designer, portal, inbox, delegation, SLA, 6 templates (old M5–M6 part)_

- [ ] **BPMS-080** (P0, L) IR v1 model + compiler (§11 checks, diagnostics, semantic diff)
- [ ] **BPMS-081** (P0, M) Expression engine integration (typed, budgeted, WASM build)
- [ ] **BPMS-082** (P0, L) Interpreter: advance loop, tokens, splits/joins, end/terminate
- [ ] **BPMS-083** (P0, L) Jobs + timers + scheduler with leases/fencing; calendars; SLA clocks
- [ ] **BPMS-084** (P0, M) Release bundles + publish integration
- [ ] **BPMS-085** (P0, L) Work items, assignment strategies, manager chain, DoA matrix, quorum, per-group
- [ ] **BPMS-086** (P0, M) Delegation, OOO substitution, SoD re-check, send-back
- [ ] **BPMS-087** (P0, M) Rule tables (UNIQUE/FIRST/COLLECT)
- [ ] **BPMS-088** (P0, M) Incidents, retry/skip/cancel, instance admin ops
- [ ] **BPMS-089** (P0, L) Conformance suite + model-based tests + deterministic simulation harness
- [ ] **BPMS-100** (P0, XL) Step builder + React Flow canvas + properties + validate/test-run/review/publish
- [ ] **BPMS-101** (P0, XL) Form builder + renderer (calc, prefill, repeating, lookups, PDF of submission)
- [ ] **BPMS-103** (P0, L) Request portal/catalogue, start form, request detail, comments/@mentions/watchers
- [ ] **BPMS-105** (P0, L) My Work 3-pane inbox (tasks, acks, signatures), bulk with preview
- [ ] **BPMS-106** (P0, XL) Template gallery + install wizard + Wave-1 templates (13)
- [ ] **BPMS-111** (P0, L) E2E-R2 pack (R2-01…16 + ISO) + Firefox
- [ ] **BPMS-121** (P0, M) Usability round + SUS measurement + fixes

</details>

<details>
<summary><b>R3 — Operate & Sign</b> · 2027-03-15 → 2027-05-07 · gate G-R3 · 0/8 done</summary>

_Admin console, DoA, master data, recall/edit, e-sign v1, 13 templates, PWA, BPMN import (old M4 rest, M6 rest, M7)_

- [ ] **BPMS-066** (P0, M) Retention policy per record class (policy only)
- [ ] **BPMS-102** (P0, M) Master-data tables (CSV import, lookups, versioned)
- [ ] **BPMS-104** (P0, M) Recall/withdraw, edit-after-submit, request-on-behalf, numbering
- [ ] **BPMS-107** (P0, XL) E-sign v1: PDF, recipients/roles, routing, bulk, fields, consent step, ceremony, certificate
- [ ] **BPMS-108** (P0, L) All requests / all work / workload / process health / ops monitor
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
| E2E scenarios (E2E) | 182 | [23-E2E-TEST-PLAN.md](../docs/23-E2E-TEST-PLAN.md) |
| Security tests | 37 | [06](../docs/06-SECURITY-AND-OPERATIONS.md) |
| Test matrix | 40 | [15](../docs/15-TEST-STRATEGY-AND-CONFORMANCE.md) |
| Open questions / unknowns / assumptions | 14 / 16 / 14 | [22](../docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) |
