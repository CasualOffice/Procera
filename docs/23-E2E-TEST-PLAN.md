# 23 — End-to-end (E2E) test plan

Status: Draft v0.1 · 2026-10-11 · Owner: QA/SDET · Complements [15-TEST-STRATEGY](15-TEST-STRATEGY-AND-CONFORMANCE.md) (unit, property, conformance, chaos) and [06 security tests](06-SECURITY-AND-OPERATIONS.md).

E2E tests prove that **whole user journeys work across the real stack**:
- browser → API → Postgres → workers/scheduler → object store → scanner → email
- across **two workspaces at once**, so isolation is tested in every run

Each release ships only when its E2E pack and all earlier packs are green ([07 §4](07-ROADMAP-AND-MILESTONES.md#4-phase-level-e2e-gates-summary)).

## 1. Principles

1. **Journey-first.** Every PRD journey (J0–J14) and every P0/P1 feature in the catalogue has at least one E2E scenario. The PRD id is tagged in the test title (`@FR-DS04`).
2. **Written with the feature.** An agent or engineer implementing a tracker item writes or extends the E2E scenario in the same PR. The PR template asks for the scenario id.
3. **Real stack, deterministic time.** No mocks of our own services. The clock is controlled through a **test-only time-travel endpoint**, so a 30-day timer runs in seconds (§4).
4. **Two tenants always.** The seed creates workspace **A** (Starter profile) and **B** (Enterprise profile). Every pack ends with an isolation sweep: B's users try A's ids and must get 404, with the denial audited.
5. **Accessible by default.** Each scenario runs axe on every page it visits. Key flows also run keyboard-only.
6. **Fast feedback, full regression nightly.** PRs run the smoke pack plus the affected packs. Nightly runs everything on 3 browsers and 2 viewports.

## 2. Tooling

| Concern | Choice | Licence |
|---|---|---|
| Browser automation | **Playwright** (TypeScript), with the test runner, traces, video on failure | Apache-2.0 |
| API-level E2E | Playwright `request` context + generated TS client from OpenAPI | — |
| Accessibility | `@axe-core/playwright` | MPL-2.0 (dev only) |
| Visual regression | Playwright screenshots with masked dynamic regions; per-component stories in Storybook + Playwright | Apache/MIT |
| Email capture | **Mailpit** container (SMTP sink + REST API) to read invites, notifications, magic links, email approvals | MIT |
| Malware scan | **ClamAV** container. EICAR test file must be quarantined | GPL-2.0 (separate container, dev/test) |
| Object store | SeaweedFS (or RustFS) container | Apache-2.0 |
| Cache | Valkey container; also a **cache-off** run | BSD-3 |
| IdP (R4+) | **Keycloak** container with a seeded realm (OIDC + SAML broker) | Apache-2.0 |
| Webhook receiver | Small local receiver (records deliveries, verifies Standard Webhooks signatures) | ours |
| Load (not E2E, related) | k6, dev/CI only | AGPL (never shipped) |
| Orchestration | `docker compose -f deploy/compose.e2e.yml` with health-checked services; same file locally and in CI | — |

## 3. Environment and data

- **Ephemeral stack per CI job.** Compose builds the release images once and shares them across jobs. Postgres runs from a template DB for fast resets.
- **Seed** (`e2e/seed/`): created through the public API (not SQL), so seeding itself exercises the API. It includes:
  - a system owner
  - workspaces A and B
  - about 40 users across departments, groups, positions and managers
  - business calendars (IN, DE)
  - Wave-1 templates installed in A
  - master data (cost centres, vendors)
  - sample policies
- **Personas as fixtures:** `systemAdmin`, `wsOwnerA`, `wsAdminA`, `processOwnerA`, `designerA`, `reviewerA`, `complianceA`, `managerA`, `employeeA1..n`, `hrAdminA`, `investigatorA`, `guestSigner`, `wsAdminB`, `employeeB1`. Each has a stored auth state, and MFA is handled by fixture TOTP secrets.
- **Isolation between tests:** each test creates its own objects with unique prefixes. Destructive tests (suspend workspace, deletion) run in dedicated workspaces created per test.
- **Secrets** are test-only and generated at stack start. No real external services are ever called.

## 4. Test-only controls (compiled out of release builds)

Built only with the `e2e` cargo feature, which the release pipeline verifies is absent (a binary symbol check plus an endpoint probe that must return 404 in production images):

| Endpoint | Purpose |
|---|---|
| `POST /__test/clock {advance:"P30D"}` | Advance the injected `Clock` for timers, SLA, reminders, expiry, retention |
| `POST /__test/drain` | Wait until the outbox, jobs and timers are idle (removes sleeps and flakiness) |
| `POST /__test/fault {kind:"kill_worker_after_commit"}` | Fault injection for restart/race scenarios |
| `GET /__test/audit/verify` | Run the chain verifier and return the result |

## 5. Scenario catalogue

ID format: `E2E-<release>-<nn>`. Every row lists the PRD/catalogue refs it proves. **Packs are cumulative**: a release must pass its own pack and every earlier one.

### E2E-P0: harness smoke (gate G1)

| ID | Scenario | Proves |
|---|---|---|
| E2E-P0-01 | `/healthz` and `/readyz` return 200 | Harness boots the stack |
| E2E-P0-02 | `/version` reports name and server time | Clock wiring |
| E2E-P0-03 | `__test/clock` advances only forward and drives server time; invalid durations get a 422 problem+json | docs/23 §4 controls |
| E2E-P0-04 | `__test/drain` reports idle; unknown routes return 404 | Drain control; routing baseline |
| E2E-P0-REL | The release binary returns 404 for `/__test/clock` (CI step) | docs/24 §5.4 |
| E2E-P0-05 | Design-system gallery: zero axe violations for WCAG 2.0/2.1/2.2 A+AA (light theme) | BPMS-010, A11Y-01 |
| E2E-P0-06 | Same in the dark theme | docs/05 §7.1.1 |
| E2E-P0-07 | Keyboard: skip link is the first tab stop and moves focus to main; every following tab stop shows a visible focus outline | WCAG 2.4.1, 2.4.7, 2.4.11 |
| E2E-P0-08 | Reflows at 320 px with no horizontal scrolling | WCAG 1.4.10 |
| E2E-P0-09 | Every visible interactive target is ≥ 24×24 px (visually hidden native inputs are measured through their label) | WCAG 2.5.8 |
| E2E-P0-10 | The root size follows the display width, not the window: 16px on displays up to 1440 wide and on small screens, about 17.7px at 1920, 20px at 2560 (also in a half-width window) | docs/05 §7.1.0 |
| E2E-P0-10 | Confirm dialog: Cancel focused first, focus trapped, axe-clean, Esc closes and restores focus to the trigger; confirm performs the action | WCAG 2.4.3, 2.1.2; docs/05 §7.1.1 |
| E2E-P0-11 | Data grid: `aria-sort` none→ascending→descending, filter result count announced in a live region, select-all count | FR-V01/V02/V06/V08 |
| E2E-P0-12 | Select operable by keyboard only (open, move, choose) | WCAG 2.1.1 |
| E2E-P0-13 | Tabs: arrow keys move focus and selection; the panel follows | WAI-ARIA tabs pattern |

Implemented in `e2e/packs/p0/harness.api.spec.ts` and `e2e/packs/p0/design-system.ui.spec.ts`. CI jobs: `e2e-smoke` and `e2e-a11y`.

### E2E-R1-AUTH: authentication (BPMS-021/022, ADR-0020) — implemented

| ID | Scenario | Proves |
|---|---|---|
| E2E-R1-AUTH-01 | Login returns a ≤ 15 min Bearer JWT + HttpOnly/SameSite=Strict refresh cookie, `Cache-Control: no-store`; `/me` works only with a valid token; forged token → 401 problem+json | §3.2 |
| E2E-R1-AUTH-02 | Wrong password and unknown account return the same error code | enumeration resistance |
| E2E-R1-AUTH-03 | Refresh rotates the cookie; replaying the old refresh token → `token_reused` and the legitimate session is revoked too | SEC-14 |
| E2E-R1-AUTH-04 | Cookie endpoints without `X-Requested-With: procera` → 403 `csrf` | CSRF |
| E2E-R1-AUTH-05 | Logout revokes the family and clears the cookie | §3.2 |
| E2E-R1-AUTH-06 | TOTP enrolment (otpauth URI), MFA login (`amr=[pwd,otp]`), MFA token rejected as access token, TOTP replay rejected, recovery code single-use | §3.6 |
| E2E-R1-AUTH-07 | Sign-out-everywhere rejects another device's unexpired JWT and refresh; system audit chain verifies | SEC-04, AUD-01 |
| E2E-R1-AUTH-20 | Register a passkey: options, single-use challenge, recovery codes once, duplicates refused | BPMS-022 |
| E2E-R1-AUTH-21 | Sign in with password + passkey; phishing origin and replayed assertions fail | BPMS-022 |
| E2E-R1-AUTH-22 | Step up with a passkey; a sign-in challenge cannot be reused for step-up | BPMS-022 |
| E2E-R1-AUTH-23 | Removing a passkey needs step-up; another user’s passkey is 404; the last one turns MFA off | BPMS-022 |

Implemented in `e2e/packs/r1/auth.api.spec.ts`.

### E2E-R1-SYS: system plane (BPMS-020): implemented

| ID | Scenario | Proves |
|---|---|---|
| E2E-R1-SYS-01 | Bootstrap: wrong token 401; one-time token creates `system.owner`; reuse 409; no further tokens | FR-S01 (= E2E-R1-01) |
| E2E-R1-SYS-02 | System API returns 404 to non-system users and 403 `mfa_required` to system users without MFA | docs/12 §1, docs/06 §3.6 |
| E2E-R1-SYS-03 | Create workspaces A (starter) and B (enterprise): 201 + Location + ETag, profile quotas, duplicate slug 409, invalid slug 422, listing | FR-S02/S04 (= E2E-R1-02) |
| E2E-R1-SYS-04 | Suspend read-only with reason; stale `If-Match` → 412; restore with current ETag; invalid transition 409; schedule deletion (grace validated) | FR-S02 (= E2E-R1-17) |
| E2E-R1-SYS-05 | `system.operator` can read but not write; system audit chain verifies | docs/12 §4.1, FR-S09 |
| E2E-R1-SYS-06 | Workspaces grid lists metadata, quotas and usage counts with filters | BPMS-065 |
| E2E-R1-SYS-07 | Suspend, reactivate and schedule deletion need step-up and land on the system chain | BPMS-065 |
| E2E-R1-SYS-08 | Quota defaults per profile and the member quota enforced at invite time | BPMS-065 |
| E2E-R1-SYS-09 | Platform health and the system audit explorer; auditors read, never change | BPMS-065 |

Implemented in `e2e/packs/r1/system.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-MEM-01 | Owner registers through the system-created invitation; workspace becomes `active`; owner holds owner permissions; invitation is single-use | FR-S01, FR-W01 |
| E2E-R1-MEM-02 | Invite → accept only by the invited email; the token is never returned to the inviter; member appears in the list | FR-W01, docs/06 §3 |
| E2E-R1-MEM-03 | A non-member cannot switch into another workspace (404); workspace APIs need a selected workspace | docs/12 §1, SEC-01 |
| E2E-R1-MEM-04 | Custom role: no escalation, built-ins immutable, grant/revoke takes effect on the next request | FR-W03, ADR-0011 |
| E2E-R1-MEM-05 | Admin cannot invite an owner, cannot remove the last owner or themselves; a deactivated member loses access at once | FR-W01, docs/12 §6 |
| E2E-R1-MEM-06 | Read-only suspension blocks workspace writes with 423, reads still work; restore re-enables writes | FR-S02 |
| E2E-R1-MEM-07 | Pending invitations listed; revoking kills the token (404); resend issues a new working token; `/__test/drain` dispatches the outbox to idle | FR-W01, docs/04 §2.11 |
| E2E-R1-MEM-08 | An admin cannot revoke the owner's grant (escalation); the last owner keeps the role; role preview shows added/removed/affected members; role and grant changes are on the verified audit chain | FR-W03, docs/12 §5, FR-T01 |
| E2E-R1-MEM-09 | A role granted to a group applies while the member is in it and stops when removed; owner role cannot go to groups; a department-scoped admin reaches members in the subtree only | docs/12 §5, FR-W03 |
| E2E-R1-MEM-10 | MFA by role: in a Standard workspace an admin signed in without MFA is refused at workspace entry (403 `mfa_required`); a plain member is not | docs/20 §2.3 `auth.mfa.required_roles`, FR-W06 |

Implemented in `e2e/packs/r1/members.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-AUD-01 | Every mutating route is called once and appends exactly the audit actions in its route contract (`/__test/audit/contract`); exempt routes append nothing; no route goes unexercised (bootstrap is one-shot and checked by the platform integration test) | FR-T01, docs/24 §3.5 |

Implemented in `e2e/packs/r1/audit.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-CFG-01 | The creation profile seeds values (Starter: 8 h idle); the explainer lists default → platform floor → profile; unknown keys 404 | FR-W04, docs/20 §0-1 |
| E2E-R1-CFG-02 | Looser than a floor → 422 `setting_below_floor`; ⚡ keys need a reason; stricter value saved with `ETag`/`If-Match` (stale → 412); floor-locked key 403; invalid value 422; history records the change | FR-S05, FR-W06 |
| E2E-R1-CFG-03 | Step-up keys need a recent authentication: a 10-minute-old sign-in gets 401 `step_up_required`; a wrong code fails; `POST /session/step-up` with a current TOTP code issues a fresh token that succeeds | docs/12 §11, SEC-17 |
| E2E-R1-CFG-04 | A member without settings permissions can neither view nor change settings | docs/12 §5 |
| E2E-R1-CFG-05 | Raising a platform floor clamps a looser workspace value at resolution (stored value kept, `clamped: true`); floors need a reason; keys without floors are rejected | docs/20 §1 rules 4-5 |
| E2E-R1-CFG-06 | Department overrides: set on a parent, inherited by a child, explained, cleared; catalogue and workspace isolation enforced | BPMS-027 |
| E2E-R1-CFG-07 | Profile switch: the plan shows current, new and floor values; apply needs If-Match, a reason and step-up; then nothing is left to change | BPMS-027, CFG-017 |
| E2E-R1-CFG-08 | A settings change is visible on the very next read, through the cache when one is configured | BPMS-027, CFG-004 |

Implemented in `e2e/packs/r1/settings.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-DIR-01 | Department tree: sibling names unique (409), moving under a descendant rejected (422 `cycle`), deleting a non-empty department refused | FR-W02 |
| E2E-R1-DIR-02 | Manager cycles rejected; resolution is the explicit manager, else the nearest department head that is not the member; field sources recorded | FR-W02 (deterministic resolution) |
| E2E-R1-DIR-03 | Static groups (duplicate member 409); positions blocked until `directory.positions.enabled`; members read the directory but cannot change it | FR-W02, docs/20 §2.5 |
| E2E-R1-DIR-04 | CSV import: `mode=plan` changes nothing; `mode=apply` creates departments by path and links managers listed later in the file; unknown members reported per row; unknown columns 422 | FR-W02 |

Implemented in `e2e/packs/r1/directory.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-NOT-01 | An invitation queues an email (subject, HTML button, link); the link's token registers the invitee; the inviter gets an in-app "joined" notification; read / unread / read-all work | FR-N01, FR-N02, FR-W01 |
| E2E-R1-NOT-02 | A member cannot mark another member's notification (404) | FR-N01, SEC-01 |
| E2E-R1-NOT-03 | Preferences fall back to the workspace defaults; a member's overrides apply to them only; invalid digest/hour → 422; null resets | BPMS-048, FR-N04 |
| E2E-R1-NOT-04 | With a daily digest due now, two plain announcements produce no individual emails and exactly one digest listing both (a second drain sends nothing more); in-app notifications still arrive | BPMS-048, FR-N04 |
| E2E-R1-NOT-05 | During quiet hours a plain announcement email is held until the window ends; a critical one is not held | BPMS-048, FR-N04 |

Implemented in `e2e/packs/r1/notifications.api.spec.ts` (mail is read through `/__test/mail`, the local stand-in for Mailpit).

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-LIB-01 | SOP draft → edit with `If-Match` (stale → 412) → submit (no review needed: approved, content hash frozen) → published; editing an approved version 409; content and hash identical on retrieval; one open draft per artifact | FR-G04, FR-G05, T07 |
| E2E-R1-LIB-02 | Controlled policy needs a review: the author's own approval is refused (403 `sod_violation`); a reviewer cannot publish; a reviewer approves; the owner publishes | FR-G04 SoD, docs/12 §6 |
| E2E-R1-LIB-03 | Changes requested needs a comment and returns the draft; withdraw; later versions need a reason for change; publishing supersedes (previous → deprecated) | FR-G04, docs/20 §2.6 |
| E2E-R1-LIB-04 | A scheduled version goes live when its effective time passes (test clock + drain) and supersedes the live one; retire | FR-G05, FR-PL01 |
| E2E-R1-LIB-05 | Members read published policies but cannot author; lists contain only kinds the member may view | docs/12 §5 |
| E2E-R1-LIB-06 | Semantic diff between two versions: one changed block with word-level detail, one added list, unchanged blocks kept; the reverse direction mirrors it; versions of different artifacts → 422; members may compare; another workspace gets 404 | BPMS-043, FR-G05 |
| E2E-R1-LIB-07 | Publishing sets `next_review_at = published + 365 d` (standard); a per-artifact cadence of 20 days moves it and makes it `due_soon`; 0 → 422; `null` restores the default; an unpublished artifact cannot be recertified (409) | BPMS-044, FR-G07, FX-G01 |
| E2E-R1-LIB-08 | A passed review date reads `overdue` immediately; the worker flags it once (two drains) and the owner receives one `review_overdue` notification linking the document; recertifying clears the flag and starts a new period; both are audited | BPMS-044, FR-G07, FX-G01 |
| E2E-R1-LIB-09 | Verification lasts `governance.verification.validity` (90 d) and expires on its own; members cannot verify (403); removing it twice → 409 | BPMS-044, FX-G02 |
| E2E-R1-LIB-10 | Change requests: a member proposes (blank title 422) but cannot resolve (403); declining needs a reason; accepted → implemented by a version of the same artifact (another artifact's version 422); resolved requests are final (409); list filters by state | BPMS-044, FR-G05 |
| E2E-R1-LIB-11 | Collection tree: sibling names unique (409), moving rewrites paths, no move under itself (422), lists filter by subtree, only empty collections delete (409/204) | BPMS-041, FR-G01 |
| E2E-R1-LIB-12 | A restricted collection hides the documents filed under it (also in sub-collections) from readers outside its audience in lists, reads, search and the tree; members cannot manage collections | BPMS-041, FR-G01 "search authorized-only", A-20 |
| E2E-R1-LIB-13 | Owner hand-over (publishers only); RACI with at most one accountable, readable by readers; CSV export with the exact row and an audit event | BPMS-041, FR-G01 "RACI exportable" |
| E2E-R1-LIB-14 | Rich blocks (heading, formatted paragraph with an in-app link, numbered list, warning callout, table) save; unknown blocks, `javascript:` links and runs that don't add up to the text → 422; HTML export is faithful and escaped, Markdown export exact, unknown format 422; a superseded version's export says so; exports are audited | BPMS-042, ADR-0013 |
| E2E-R1-LIB-15 | Image blocks: refused unless a clean image attached to the draft with a description; HTML export embeds it, Markdown references it; a new version keeps the attachment | BPMS-042 |
| E2E-R1-POL-01 | Readers never see drafts: absent from lists, reads and search until published; a later draft stays hidden (version 404, diff 404, versions list shows v1 only); authors see drafts in search | BPMS-045, A-14 |
| E2E-R1-POL-02 | Applicability: empty audience or restricted without one → 422; members cannot set it (403); a restricted policy is hidden from members outside its group in lists, reads, search, the portal and change requests (404), and shown to those inside; an unrestricted one is listed for everyone but only "applicable" to its audience | BPMS-045, FR-PL01/PL02 |
| E2E-R1-POL-03 | The portal lists published policies that apply to the member (no drafts), ordered by category, with what changed since the previous version (added/changed counts, reason) and text/category filters | BPMS-045, FR-PL02 |
| E2E-R1-POL-04 | A major version asks the applicable audience to re-acknowledge that exact version; a minor one does not (major_only) | BPMS-045, FR-PL03 |
| E2E-R1-POL-05 | Every_publish asks on minors, none never asks, and a scheduled major asks when it goes live | BPMS-045, FR-PL03 |
| E2E-R1-ADM-01 | Overview: overdue acknowledgements rank first in "needs attention", followed by an admin without two-step verification; acknowledgement metrics and a daily published series; the security strip counts a failed sign-in, members without MFA and the new admin grant; requests/storage are null; members get 403; an inverted range → 422 | BPMS-063, FR-A01 |
| E2E-R1-ADM-02 | People & access: counts (members, no MFA, privileged, pending invites), member row (MFA off, last active set, not dormant), no-MFA and admin filters; unknown filter → 422; members get 403 | BPMS-063, FR-A06 |
| E2E-R1-RET-01 | Retention classes are listed (draft and operational automatic; governance records and audit never). Drafts untouched for 170 of 180 days are due and their author is notified once; editing one resets its clock; 11 days later the untouched drafts are deleted (a never-published artifact with its draft, a v2 draft beside a published v1 that stays); the run is recorded and audited (`retention.purged`) | BPMS-066, PRV-04 |
| E2E-R1-RET-02 | A read notification older than the operational period is removed by the job; members cannot see retention (403) | BPMS-066, PRV-04 |
| E2E-R1-KEY-01 | An invitation email is envelope-encrypted under the workspace KEK; rotation needs a reason (422) and is audited; the email stays readable right after rotation and after the background rewrap moves it to v2; the unused v1 is destroyed; members cannot see or rotate keys (403). Crypto-shredding is proven in the `procera-keys` integration test | BPMS-030 |
| E2E-R1-FILE-01 | A chunked upload refuses a wrong offset (409) and resumes from `received`; completion before all bytes 409; the file is hashed (SHA-256 matches), typed by content (PDF despite a .txt name and text/plain declared) and clean; the signed link downloads without a bearer with nosniff and the exact bytes; a tampered link 403; another member gets 404; upload and download are audited | BPMS-060, FR-F01–F03 |
| E2E-R1-FILE-02 | EICAR is quarantined and its download link refused (409); an executable named .pdf and an HTML file named .txt are rejected by content; 500 MB → 413; empty → 422; usage counts states | BPMS-060, FR-F02 |
| E2E-R1-FILE-03 | A draft takes an attachment; after publishing it can't be unlinked or the file deleted (409); a member reading the published SOP downloads it; an unattached file past the orphan period is listed, members can't purge (403), the owner purges it | BPMS-060/062, FR-F05/F07 |
| E2E-R1-FILE-04 | A PNG, a PDF and a JPEG get preview links that serve inline with the strict headers (CSP sandbox, nosniff, CORP same-origin, no-store) and a ≤ 2-minute expiry; the JPEG's Exif is removed; a text file gets kind none and no URL; a tampered token and a download token on the preview route are 403; another member gets 404; previews are audited | BPMS-061, FR-F04/F02 |
| E2E-R1-VIEW-01 | Saved views: personal and shared, one default per member (setting a new default clears the old), invalid list or name 422; another member sees only shared views and can't edit or delete them (403) or touch personal ones (404); unsharing hides it; exports are recorded (`data.exported`), unknown format 422 | BPMS-028, FR-V05/V06 |

Implemented in `e2e/packs/r1/library.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-UI-01 | Sign in through the browser (password + recovery code) lands on Home; navigation is permission-shaped with the current page marked; axe clean on sign-in and Home; reload keeps the session | docs/05 §2.1, §3.1, WCAG 2.2 AA |
| E2E-R1-UI-02 | Wrong password and wrong code produce clear, non-revealing messages (role=alert); axe clean | docs/06 §3, WCAG 3.3.1 |
| E2E-R1-UI-03 | With several workspaces the chooser appears; choosing one and switching from the header changes the context and the navigation (no Admin for a plain member) | docs/05 §2.1 workspace switcher |
| E2E-R1-UI-04 | The bell announces the unread count; the inbox lists the "joined" notification; mark all as read | FR-N01 |
| E2E-R1-UI-05 | Sign out returns to sign-in; reloading stays signed out | docs/06 §3 |

Implemented in `e2e/packs/r1/shell.ui.spec.ts` and `journeys.ui.spec.ts` (Chromium, against the built app behind the Vite proxy).

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-UI-06 | Theme switched from the account menu persists across reload; dark theme passes axe | docs/05 §7, WCAG 2.2 AA |
| E2E-R1-UI-07 | Author a policy in the browser: New document dialog → write content (headings, lists) → save → submit → publish; the library row shows Published; axe clean | FR-G04, docs/05 §3.2/§3.21 |
| E2E-R1-UI-08 | A campaign task on Home leads to the document; “I acknowledge” records the acknowledgement (campaign progress +1) and the callout disappears | FR-AK01, docs/05 §3.1 |
| E2E-R1-UI-09 | Invite a colleague from Members with a role; the invitation appears in the table as Invited | FR-W01, docs/05 §3.14 |
| E2E-R1-UI-10 | Read and acknowledge a critical, pinned announcement in the feed | FR-AN04, docs/05 §3.23 |
| E2E-R1-UI-11 | An admin without two-step verification is guided through setup (QR + key, confirm a code, recovery codes) and then signs in with MFA and lands on Home | docs/06 §3, docs/20 §2.3 |
| E2E-R1-UI-12 | ⌘K finds a document by its content and opens it; commands navigate; recent items are offered; keyboard only | BPMS-031, BPMS-049 |
| E2E-R1-UI-13 | A new workspace shows composed empty states; an unknown page offers a way back; a failed load shows the error with a retry | BPMS-031 (UX-005) |
| E2E-R1-UI-14 | Manage announcements: compose a draft; a workspace-wide post is sent for approval; a targeted draft is published and shows receipts | BPMS-031, BPMS-047 |
| E2E-R1-UI-15 | Campaigns: create from a published document with an audience, check the reach, launch, follow progress, download the evidence CSV | BPMS-031, BPMS-046 |
| E2E-R1-UI-16 | Settings: change a value in place with a reason and see why it applies; a step-up key asks to confirm identity | BPMS-031, BPMS-027 |
| E2E-R1-UI-17 | Audit log: chain verified; filter by category; expand a record; open an object timeline; no-match state | BPMS-031, BPMS-064 |
| E2E-R1-UI-18 | The owner sets a review schedule, recertifies when it falls due, verifies and removes verification | BPMS-044 |
| E2E-R1-UI-19 | A member suggests a change; the owner accepts it and marks it implemented in v2 | BPMS-044 |
| E2E-R1-UI-20 | Compare v1 and v2 from the version history: word-level changes and the summary | BPMS-043 |
| E2E-R1-UI-21 | A member opens Policies, filters by category and follows "what changed" to the comparison | BPMS-045 |
| E2E-R1-UI-22 | A publisher restricts a policy to a role; a member outside it can't find it in Policies or open it | BPMS-045, A-14 |
| E2E-R1-UI-23 | An admin opens Overview: the summary and "Needs attention" show overdue acknowledgements, and the security strip shows one member without two-step verification; drilling in opens People & access filtered to them; the Admins filter shows the owner | BPMS-063 |
| E2E-R1-UI-24 | An owner publishes v2 as a major change, sees who will be asked, then finds the campaign | BPMS-045 |
| E2E-R1-UI-25 | A member opens Notifications → Preferences, picks a daily digest and quiet hours, saves, sees them after a reload, and resets to the workspace defaults | BPMS-048 |
| E2E-R1-UI-26 | An admin opens Retention, sees each record class and a draft due for deletion, and shortens the draft period with a reason | BPMS-066 |
| E2E-R1-UI-27 | An admin creates a collection from the Library rail, files a document in it from the document page, sets the accountable person in RACI, and filters the Library by the collection | BPMS-041 |
| E2E-R1-UI-28 | An author uses the editor toolbar (bold, numbered steps, callout, table), saves, and the document renders them; the stored blocks match; axe on the editor and the result. UI-07 types markdown shortcuts into the editor | BPMS-042 |
| E2E-R1-UI-29 | An admin opens Encryption, sees version 1 in use, rotates the key with a reason, and sees version 2 in use | BPMS-030 |
| E2E-R1-UI-32 | An author attaches a file to a draft from the document page, sees it clean, and downloads it | BPMS-060 |
| E2E-R1-UI-33 | An EICAR upload on a document is refused with a quarantine message and shows as Quarantined (no download) in the Files view | BPMS-060/062 |
| E2E-R1-UI-34 | An author opens Preview on an image attachment: a dialog named after the file shows the image (axe clean); a text attachment offers no Preview; Escape closes it and focus returns | BPMS-061 |
| E2E-R1-UI-38 | On the Library grid: build a Status filter (chip + count), sort by name descending, save the view, reload and get the same filter and order back with no modified marker, export a CSV with exactly the visible columns and rows | BPMS-028 |
| E2E-R1-UI-39 | An admin filters the Audit log to content events, starts a background export (dialog says it uses the current filters), sees it become Ready in Your exports, and downloads the CSV | BPMS-064 |
| E2E-R1-UI-30 | Add a passkey from Account › Security, sign out, sign in with password + passkey | BPMS-022 |
| E2E-R1-UI-31 | Removing a passkey asks to confirm it’s you; the passkey itself passes step-up | BPMS-022 |
| E2E-R1-UI-35 | Departments: override a setting for a parent department, see a child inherit it, then clear it | BPMS-027 |
| E2E-R1-UI-36 | Configuration profile: preview a switch with every change, then switch with a reason | BPMS-027, CFG-017 |
| E2E-R1-UI-40 | A system admin finds a workspace, makes it read-only with a reason, and reactivates it | BPMS-065 |
| E2E-R1-UI-41 | Quotas, platform health and the system audit; non-system users never see the console | BPMS-065 |
| E2E-R1-UI-42 | With the API slowed down, every main page (Home, Library, Members, Campaigns, Manage announcements, Overview, People & access, Settings, Audit log, Notifications) shows a content-shaped skeleton, then its content; a reload shows the app frame, never a blank page | UX-005 |
| E2E-R1-UI-43 | Files: a draft attachment shows retention class Draft and last accessed Never; downloading it updates last accessed; two unattached files are selected and deleted in bulk | BPMS-062 |
| E2E-R1-UI-44 | A reader opens Export on a published SOP (Print or PDF, Word, Markdown) and downloads a .docx | BPMS-042 |
| E2E-R1-UI-45 | An author inserts an image from the editor (upload, required description, caption), saves, and readers see it with its alt text after a reload | BPMS-042 |
| E2E-R1-FLOW-01 | Onboarding start to finish: the owner invites from Members; the invitee opens the link from the email, creates an account (password confirmation checked) and lands on Home in the workspace; the link then no longer works; the owner sees them Active and opens their person page | BPMS-023 |
| E2E-R1-FLOW-02 | Someone with an account opens the invitation email, signs in from the page, comes back to it and joins with one click; an unknown link explains itself | BPMS-023 |
| E2E-R1-FLOW-03 | Policy to evidence: the owner writes and publishes a policy and launches a campaign; a colleague (invited the same way) sees the task on Home, opens the acknowledgement email link, acknowledges on the document; the owner sees them Done, 1 of 2, and downloads the evidence CSV | BPMS-040, 046 |
| E2E-R1-FLOW-04 | Announcement: the owner composes a critical post that needs acknowledgement and publishes it; a colleague opens the email link while signed out, signs in, lands on the open post and confirms; the owner sees the receipt | BPMS-047 |
| E2E-R1-FLOW-05 | Review loop: the owner submits a controlled policy; the reviewer (invited as Reviewer) gets the review email, opens it and requests changes with a comment; the owner gets a notification quoting it, edits and resubmits; the reviewer approves from Home; the owner is told it is approved and publishes | BPMS-040 |
| E2E-R1-FLOW-06 | Announcement approval: a second admin joins from the invitation and turns on two-step verification; the owner sends a workspace-wide post for approval; the admin gets the approval email, opens it and sends it back with a note; the owner gets a notification with the note, edits and resubmits; the admin approves and publishes; the owner is told; the post is in the feed | BPMS-047 |
| E2E-R1-CMP-01 | Launching a campaign snapshots the audience (department) and notifies each assignee in-app and by email; non-audience members get nothing; draft preview counts the audience | FR-AK01, FR-AK02, FR-AN05 |
| E2E-R1-CMP-02 | Acknowledging records the exact version id, content hash and statement; a stale hash is refused (409 content_changed); progress counts update | FR-AK01, FR-PL03, FR-AK04 |
| E2E-R1-CMP-03 | Only the assignee can acknowledge (others 404), only once (409), only while active; close/cancel rules | FR-AK01, docs/12 §5 |
| E2E-R1-CMP-04 | Reminder ladder and manager escalation run by the scheduler via drain; overdue list and counts; overdue can still be acknowledged | FR-AK01, FR-AN04 |
| E2E-R1-CMP-05 | A new member matching a dynamic audience is enrolled (own window); at-launch campaigns stay fixed | FR-AK03 |
| E2E-R1-CMP-06 | Evidence CSV lists every assignee with department, timestamps, version, content hash and auth method; members cannot export | FR-AK04, CMP-03 |
| E2E-R1-CMP-07 | Members cannot manage or track campaigns; a compliance officer can; draft version 409, empty audience 422; another workspace gets 404 | docs/12 §5 |
| E2E-R1-ANN-01 | Compose → edit with If-Match → publish to a department: only its members see it; first read recorded; in-app + email delivery after drain; edits after publish 409; content hash frozen | FR-AN01..03, FR-AN05 |
| E2E-R1-ANN-02 | Workspace-wide posts need approval; self-approval 403 sod_violation; publisher approves; reject needs a comment and returns the draft | FX-C01, FR-AN02 |
| E2E-R1-ANN-03 | Scheduled announcement goes live after clock + drain; expiry removes it from feeds (state stays published, expired: true) | FR-AN03 |
| E2E-R1-ANN-04 | Required ack on a critical announcement; idempotent ack; analytics reconcile by department; archive needs a reason | FR-AN04, FR-AN06 |
| E2E-R1-ANN-05 | Permissions and isolation: members read the feed but cannot compose; authors see only their drafts; other workspaces 404 | docs/12 §5 |
| E2E-R1-ANN-06 | Settings drive approval, ack default, expiry default and the enabled switch | docs/20 §2.17 |
| E2E-R1-SRCH-01 | Ranked results across SOPs, policies, people, departments, groups; escaped highlighted snippets; typo and prefix matches; type filter; validation errors | BPMS-049, docs/05 §2.4 |
| E2E-R1-SRCH-02 | A SOP-only role sees only SOP results; other types filtered on the server | docs/12 §3 |
| E2E-R1-SRCH-03 | Another workspace's content is never found; draft edits are searchable at once | docs/06 T-22 |
| E2E-R1-AUDX-01 | Explorer filters, newest-first keyset pagination without duplicates, actor names, validation | BPMS-064, FR-T03 |
| E2E-R1-AUDX-02 | audit:view_own sees only own events; verify/export forbidden; no permission 403 | docs/12 §5 |
| E2E-R1-AUDX-03 | Artifact timeline covers all versions, oldest first | FR-T04 |
| E2E-R1-AUDX-04 | Chain verify reports intact | FR-T02 |
| E2E-R1-AUDX-05 | Saved queries: create, duplicate 409, validation 422, private per member, delete; audited | FR-T03, FR-T01 |
| E2E-R1-AUDX-06 | CSV and JSONL exports honour filters, need step-up, and are audited | FR-T01, docs/12 §11 |
| E2E-R1-AUDX-07 | `POST /audit/query`: an AND group (category, action prefix, within P30D) sorted ascending pages by cursor; an OR group returns only matching events; unknown fields or values, bad durations, nesting deeper than 2 and unsupported sorts → 422 | BPMS-064, docs/08 §7.5 |
| E2E-R1-AUDX-08 | A background export (202) runs as a job, finishes `done` with rows, its CSV downloads through the requester's signed file link, the requester is notified, and both `audit.export_requested` and `audit.exported` are recorded | BPMS-064, FR-T04 |

Implemented in `e2e/packs/r1/campaigns.api.spec.ts`, `announcements.api.spec.ts` and `search-audit.api.spec.ts`.

### E2E-R1: Workspace & Policies

| ID | Scenario | Proves |
|---|---|---|
| E2E-R1-01 | Bootstrap: one-time token → system owner enrols MFA → no default credentials work | FR-S01 |
| E2E-R1-02 | System admin creates workspace A (Starter) and B (Enterprise), invites owners; owners accept via Mailpit link and enrol MFA | FR-S02/S03, FR-W01 |
| E2E-R1-03 | Workspace owner sets branding; a low-contrast accent is auto-adjusted or rejected; terminology override ("Request" → "Ticket") shows in UI and email | FR-W04, 05 §7 |
| E2E-R1-04 | Directory CSV import (departments, managers); a manager cycle is rejected with a clear error (browser: Members › Import directory › Check shows each row; cycle and unknown member refused; Apply saves the rest; person pages show department, manager and location) | FR-W02 |
| E2E-R1-05 | Clone "Process Designer" → "HR Designer" scoped to the HR collection; preview count is correct; the scope is enforced on UI and API; adding a permission the editor lacks is refused | FR-W03, J6 |
| E2E-R1-06 | Policy authored → submitted → self-approval blocked (SoD) → reviewer approves with step-up → scheduled → published at effective date (test clock) → old version watermarked as superseded in PDF | FR-G04/G05/G10, FR-PL01 |
| E2E-R1-07 | Major version publish triggers a re-ack campaign; 50 employees ack (10 at mobile viewport); evidence export lists version hash and auth method | FR-PL03, FR-AK01–04 |
| E2E-R1-08 | New joiner added to a department automatically receives outstanding applicable campaigns | FR-AK03 |
| E2E-R1-09 | Announcement to location "Bangalore" with ack in 3 days; reminders; manager escalation after deadline (test clock); ack % by department correct | FR-AN01–06, J14 |
| E2E-R1-10 | Announcement approval workflow: draft → reviewer → publish; expiry archives it | FX-C01, FR-AN03 |
| E2E-R1-11 | File upload (resumable 50 MB): clean file previews; **EICAR quarantined** and undownloadable; signed URL expires and fails from workspace B | FR-F01–F03, SEC-10 |
| E2E-R1-12 | Files view: filter by scan state/type/size, bulk retention change, held file deletion refused | FR-F06, FR-V* |
| E2E-R1-13 | DataGrid: multi-sort, typed filters, AND/OR group, saved view shared to a role, URL deep link restores view, CSV/XLSX export matches view and redaction | FR-V01–V07 |
| E2E-R1-14 | Audit: perform 10 actions → audit explorer filter by actor → user timeline shows them in order → `/__test/audit/verify` OK (browser: Person filter shows only that person's events; a version's timeline is oldest first: three edits, submitted, published; the page shows Chain verified) | FR-T01–T04 |
| E2E-R1-15 | Workspace overview dashboard: every tile drills into a pre-filtered grid with matching counts | FR-A01 |
| E2E-R1-16 | Member deactivated → sessions revoked immediately → cannot log in or act (browser: owner deactivates from the person page; the member's open tab is sent to the chooser on the next page; signing in again offers no workspace; the API refuses entry) | SEC-04, FR-W01 |
| E2E-R1-17 | System admin suspends workspace B (read-only) → writes return 423 with banner → restore (browser: reads keep working; the first change is refused and a read-only banner appears on every page; after restore the banner is gone and changes work) | FR-S02, J8 |
| E2E-R1-18 | Startup day-one (J12): Starter profile → install policy-ack pack → publish handbook → 20 employees ack on phones in < 15 min wall-clock | J12 |
| E2E-R1-19 | Keyboard-only: publish a policy and acknowledge it without a mouse (browser: Tab/Enter/typing only, author from Library to Published, colleague from sign-in to Acknowledged) | A11Y-01, T10 |
| E2E-R1-ISO | Isolation sweep: B users probe every A object id captured during the pack → 404 + audit `denied` (built: B's owner probes 21 reads and writes on A's documents, versions, exports, campaigns, announcements, collections, departments, views, files and members → all 404, A unchanged; not yet: the `denied` audit record for cross-workspace probes) | SEC-01/02 |

### E2E-R2: Requests & Approvals

| ID | Scenario | Proves |
|---|---|---|
| E2E-R2-01 | Install "Purchase requisition", map role placeholders, test run (no emails sent, nothing in metrics), publish | FR-G08, FR-E11 |
| E2E-R2-02 | Purchase 30 000 → manager approves; 120 000 → rule table routes to finance; rule version recorded in timeline | FR-E10, J3 |
| E2E-R2-03 | Duplicate submit with the same Idempotency-Key returns the same request; a different body returns 409 | 08 §2 |
| E2E-R2-04 | **Double approval race:** two approvers decide concurrently → exactly one commit, the other gets `already_decided` | T01 |
| E2E-R2-05 | **Kill worker after commit** (fault) → instance still progresses after restart; no duplicate email | T02 |
| E2E-R2-06 | 30-day wait timer: publish v2 mid-flight → advance clock 30 d → instance completes on pinned v1 | T03, FR-E07 |
| E2E-R2-07 | Onboarding (J1): HR starts, manager approves, parallel IT + facilities tasks join, employee tracks progress | J1 |
| E2E-R2-08 | Out-of-office: manager sets a substitute → new approvals route to them → "on behalf of" recorded → expiry restores routing → substitute who is the initiator is excluded (SoD) | FR-E04, J7, T05 |
| E2E-R2-09 | Send back to an earlier step with comment → requester edits → resubmits → approver sees diff | FX-E05 |
| E2E-R2-10 | SLA on business calendar (IN holidays) → at-risk → breach notification to process owner (test clock) | FR-E12 |
| E2E-R2-11 | Escalation reassign after P2D; controlled process forbids auto-approve | 11 §3.2 |
| E2E-R2-12 | Form: calculation, conditional section, repeating line items, URL prefill, server rejects tampered payload | FR-E03, FX-F01–F03 |
| E2E-R2-13 | Designer keyboard-only: build a 4-step process, validation errors link to nodes, publish | FR-E01, T10 |
| E2E-R2-14 | Failed step raises an incident → admin retries → instance continues; exactly one open incident | FR-E13 |
| E2E-R2-15 | My Work: j/k/a triage, decision in ≤ 3 interactions, acks tab shows pending campaigns | FR-R03 |
| E2E-R2-16 | Restricted variable (salary) hidden from approver without grant in UI, API, export, email body and search | SEC-03 |
| E2E-R2-ISO | Isolation sweep incl. work items, requests, forms, rule tables | SEC-01/02 |

### E2E-R3: Operate & Sign

| ID | Scenario | Proves |
|---|---|---|
| E2E-R3-01 | Admin monitoring (J9): overview → 12 overdue in Finance → filtered grid → bulk reassign with reason and preview → per-item audit → save view | J9, FR-A02/A03 |
| E2E-R3-02 | DoA matrix: amount and position limits route to the correct approver; matrix version pinned | FX-D01/D02 |
| E2E-R3-03 | Recall/withdraw then restart as a linked copy; edit-after-submit with per-step editable fields | FX-E04/E06 |
| E2E-R3-04 | Request on behalf of: beneficiary sees status; initiator ≠ beneficiary in audit | FX-E03 |
| E2E-R3-05 | Master data: CSV import of cost centres → cascading dropdown in form → version change doesn't alter running requests | FX-E02 |
| E2E-R3-06 | E-sign: upload PDF, place fields (mouse and keyboard), sequential then parallel signers, consent step, sign on mobile, certificate of completion, tamper check fails after byte change | FR-DS01–DS06 |
| E2E-R3-07 | **Bulk sign 1 000 recipients** (J13): async progress, reminders, decline with reason, manager escalation, overdue export | J13 |
| E2E-R3-08 | "Not e-signable" template flag blocks sending with guidance | FX-S06 |
| E2E-R3-09 | File lifecycle (J11): invoice attached, scanned, previewed, locked on completion, visible in Files view with retention class | J11 |
| E2E-R3-10 | BPMN import of a sample with an inclusive gateway → compatibility report lists the element id | FR-G09 |
| E2E-R3-11 | Process health and ops monitor numbers reconcile with raw events | FR-A04/A05 |
| E2E-R3-12 | All 13 Wave-1 templates: install → test run green (data-driven) | TPL-*, 19 |
| E2E-R3-MOB | Mobile pack (360×740, touch): start request, approve, ack, sign | FR-P07 |
| E2E-R3-ISO | Isolation sweep incl. envelopes, master data, files | SEC-01/02/10 |

### E2E-R4: Enterprise Connect

| ID | Scenario | Proves |
|---|---|---|
| E2E-R4-01 | OIDC SSO via Keycloak; SSO enforcement blocks local login except break-glass owner | FR-P02 |
| E2E-R4-02 | SCIM create/update/deactivate → access revoked and work re-routed < 5 min | INT-003, SEC-04 |
| E2E-R4-03 | Scoped admin (Finance dept) cannot see or grant outside scope; escalation attempt denied | CFG-018 |
| E2E-R4-04 | Webhook delivered with valid Standard Webhooks signature; replay outside tolerance rejected by receiver; retries on 500; disabled on 410 | WH-01–04 |
| E2E-R4-05 | HTTP connector to `169.254.169.254` and to private IP refused (SSRF); allowlisted host OK | SSRF-01–06 |
| E2E-R4-06 | Email-actionable approval: link → confirm page → approve; reusing link fails; a GET never mutates | FR-N03 |
| E2E-R4-07 | External signer via magic link + email OTP; link single-use | FR-DS02, FR-P08 |
| E2E-R4-08 | Envelope correction (change recipient email) audited; DOCX + merge fields; anchor tags place fields | FX-S03/S09 |
| E2E-R4-09 | Public form with CAPTCHA + rate limit; save & resume | FX-F05/F06 |
| E2E-R4-10 | Config export from sandbox → import into prod workspace shows diff; secrets absent | FR-W12, FR-P09 |
| E2E-R4-11 | API token scoped to `request:start` cannot read audit | 12 §5 |
| E2E-R4-ISO | Isolation sweep incl. tokens, webhooks, connectors | SEC-* |

### E2E-R5: Cases & Compliance

| ID | Scenario | Proves |
|---|---|---|
| E2E-R5-01 | Incident case (J4): intake → evidence → ad hoc tasks → closure blocked until checklist done → audit export | J4, FR-C01/C02 |
| E2E-R5-02 | Restricted HR case invisible in global search, reports, dashboards, notifications; admin self-grant is audited and visible to case team | FX-K06, OQ-12 |
| E2E-R5-03 | Investigation (J10): filter `data_access` on `salary` → pin events/files → legal hold → signed export → `procera-verify` offline passes; edited bundle fails | J10, FR-T05/T06, AUD-05 |
| E2E-R5-04 | Legal hold blocks retention purge and file deletion; release by authorised role resumes | PRV-03 |
| E2E-R5-05 | Part 11 mode: approval requires re-auth; manifestation shows name/time/meaning; signature bound to record hash | ESIG-01–05 |
| E2E-R5-06 | Attestation campaign + access review; unconfirmed grants expire | FR-G06, FR-W11 |
| E2E-R5-07 | Break-glass: system admin request → workspace approval → read-only session banner → auto-expiry → visible in workspace audit | FR-S07 |
| E2E-R5-08 | Separation mode on → workspace admin loses content access → audited self-grant flow | 12 §6.1 |
| E2E-R5-09 | Scheduled report re-applies permissions at run time (user lost access → no data sent) | FX-A02 |
| E2E-R5-10 | AI drafting (when enabled): output is a draft, labelled, provenance in audit; cannot publish or approve | ADR-0006 |
| E2E-R5-ISO | Isolation sweep incl. cases, investigations, evidence | SEC-* |

### E2E-GA: release regression

All packs run on Chromium, Firefox and WebKit, at desktop and mobile viewports. The regression also includes:
- **cache-off** run (Valkey disabled)
- **upgrade path** run (install R1 → seed → upgrade through each release → run all packs)
- **air-gapped** image run
- pseudo-locale and RTL smoke

## 6. CI integration

| Trigger | Runs | Budget |
|---|---|---|
| Every PR | Smoke (10 critical scenarios) + packs tagged for touched areas, Chromium | ≤ 12 min (sharded ×4) |
| Merge to `main` | Current release pack + isolation sweep, Chromium | ≤ 20 min |
| Nightly | All packs × 3 browsers × 2 viewports; cache-off; chaos-tagged scenarios | ≤ 90 min (sharded) |
| Release candidate | E2E-GA style regression for that release + upgrade-path run | Gate |

Artefacts (Playwright traces, videos, axe reports, Mailpit dumps, server logs with trace ids) are kept 14 days. A failing nightly opens a tracker item automatically.

## 7. Flake policy

- Use only `drain`/condition waits. **Fixed sleeps are forbidden**, and a lint enforces this.
- Retry at most once in CI. A test that passes only on retry is marked flaky and goes to the quarantine lane within 24 h. The owner gets 5 working days to fix it, otherwise the feature's release gate is blocked.
- The flake rate target is < 0.5 % of runs. The SDET reports it weekly.

## 8. Repository layout

```text
e2e/
  playwright.config.ts      # projects: chromium/firefox/webkit × desktop/mobile
  fixtures/                 # personas, auth states, api client, mailpit, clock, drain
  seed/                     # API-driven seed for workspaces A/B
  packs/r1/ … packs/r5/ packs/ga/
  pages/                    # page objects (thin; prefer role-based locators)
  support/webhook-receiver/
deploy/compose.e2e.yml
```

Locators must be **role- or label-based** (`getByRole`, `getByLabel`), which keeps tests aligned with accessibility. `data-testid` is allowed only where no accessible name exists.

## 9. Ownership

- **SDET** owns the harness, the seed, the flake budget and release regression.
- **Feature owners** (human or agent) own the scenarios for their features.
- **Release gate:** the tech lead and SDET sign off the E2E report.
