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

Implemented in `e2e/packs/r1/auth.api.spec.ts`.

### E2E-R1-SYS: system plane (BPMS-020): implemented

| ID | Scenario | Proves |
|---|---|---|
| E2E-R1-SYS-01 | Bootstrap: wrong token 401; one-time token creates `system.owner`; reuse 409; no further tokens | FR-S01 (= E2E-R1-01) |
| E2E-R1-SYS-02 | System API returns 404 to non-system users and 403 `mfa_required` to system users without MFA | docs/12 §1, docs/06 §3.6 |
| E2E-R1-SYS-03 | Create workspaces A (starter) and B (enterprise): 201 + Location + ETag, profile quotas, duplicate slug 409, invalid slug 422, listing | FR-S02/S04 (= E2E-R1-02) |
| E2E-R1-SYS-04 | Suspend read-only with reason; stale `If-Match` → 412; restore with current ETag; invalid transition 409; schedule deletion (grace validated) | FR-S02 (= E2E-R1-17) |
| E2E-R1-SYS-05 | `system.operator` can read but not write; system audit chain verifies | docs/12 §4.1, FR-S09 |

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

Implemented in `e2e/packs/r1/notifications.api.spec.ts` (mail is read through `/__test/mail`, the local stand-in for Mailpit).

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-LIB-01 | SOP draft → edit with `If-Match` (stale → 412) → submit (no review needed: approved, content hash frozen) → published; editing an approved version 409; content and hash identical on retrieval; one open draft per artifact | FR-G04, FR-G05, T07 |
| E2E-R1-LIB-02 | Controlled policy needs a review: the author's own approval is refused (403 `sod_violation`); a reviewer cannot publish; a reviewer approves; the owner publishes | FR-G04 SoD, docs/12 §6 |
| E2E-R1-LIB-03 | Changes requested needs a comment and returns the draft; withdraw; later versions need a reason for change; publishing supersedes (previous → deprecated) | FR-G04, docs/20 §2.6 |
| E2E-R1-LIB-04 | A scheduled version goes live when its effective time passes (test clock + drain) and supersedes the live one; retire | FR-G05, FR-PL01 |
| E2E-R1-LIB-05 | Members read published policies but cannot author; lists contain only kinds the member may view | docs/12 §5 |

Implemented in `e2e/packs/r1/library.api.spec.ts`.

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-UI-01 | Sign in through the browser (password + recovery code) lands on Home; navigation is permission-shaped with the current page marked; axe clean on sign-in and Home; reload keeps the session | docs/05 §2.1, §3.1, WCAG 2.2 AA |
| E2E-R1-UI-02 | Wrong password and wrong code produce clear, non-revealing messages (role=alert); axe clean | docs/06 §3, WCAG 3.3.1 |
| E2E-R1-UI-03 | With several workspaces the chooser appears; choosing one and switching from the header changes the context and the navigation (no Admin for a plain member) | docs/05 §2.1 workspace switcher |
| E2E-R1-UI-04 | The bell announces the unread count; the inbox lists the "joined" notification; mark all as read | FR-N01 |
| E2E-R1-UI-05 | Sign out returns to sign-in; reloading stays signed out | docs/06 §3 |

Implemented in `e2e/packs/r1/shell.ui.spec.ts` (Chromium, against the built app behind the Vite proxy).

| ID | Scenario | Covers |
|---|---|---|
| E2E-R1-UI-06 | Theme switched from the account menu persists across reload; dark theme passes axe | docs/05 §7, WCAG 2.2 AA |
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

Implemented in `e2e/packs/r1/campaigns.api.spec.ts`, `announcements.api.spec.ts` and `search-audit.api.spec.ts`.

### E2E-R1: Workspace & Policies

| ID | Scenario | Proves |
|---|---|---|
| E2E-R1-01 | Bootstrap: one-time token → system owner enrols MFA → no default credentials work | FR-S01 |
| E2E-R1-02 | System admin creates workspace A (Starter) and B (Enterprise), invites owners; owners accept via Mailpit link and enrol MFA | FR-S02/S03, FR-W01 |
| E2E-R1-03 | Workspace owner sets branding; a low-contrast accent is auto-adjusted or rejected; terminology override ("Request" → "Ticket") shows in UI and email | FR-W04, 05 §7 |
| E2E-R1-04 | Directory CSV import (departments, managers); a manager cycle is rejected with a clear error | FR-W02 |
| E2E-R1-05 | Clone "Process Designer" → "HR Designer" scoped to the HR collection; preview count is correct; the scope is enforced on UI and API; adding a permission the editor lacks is refused | FR-W03, J6 |
| E2E-R1-06 | Policy authored → submitted → self-approval blocked (SoD) → reviewer approves with step-up → scheduled → published at effective date (test clock) → old version watermarked as superseded in PDF | FR-G04/G05/G10, FR-PL01 |
| E2E-R1-07 | Major version publish triggers a re-ack campaign; 50 employees ack (10 at mobile viewport); evidence export lists version hash and auth method | FR-PL03, FR-AK01–04 |
| E2E-R1-08 | New joiner added to a department automatically receives outstanding applicable campaigns | FR-AK03 |
| E2E-R1-09 | Announcement to location "Bangalore" with ack in 3 days; reminders; manager escalation after deadline (test clock); ack % by department correct | FR-AN01–06, J14 |
| E2E-R1-10 | Announcement approval workflow: draft → reviewer → publish; expiry archives it | FX-C01, FR-AN03 |
| E2E-R1-11 | File upload (resumable 50 MB): clean file previews; **EICAR quarantined** and undownloadable; signed URL expires and fails from workspace B | FR-F01–F03, SEC-10 |
| E2E-R1-12 | Files view: filter by scan state/type/size, bulk retention change, held file deletion refused | FR-F06, FR-V* |
| E2E-R1-13 | DataGrid: multi-sort, typed filters, AND/OR group, saved view shared to a role, URL deep link restores view, CSV/XLSX export matches view and redaction | FR-V01–V07 |
| E2E-R1-14 | Audit: perform 10 actions → audit explorer filter by actor → user timeline shows them in order → `/__test/audit/verify` OK | FR-T01–T04 |
| E2E-R1-15 | Workspace overview dashboard: every tile drills into a pre-filtered grid with matching counts | FR-A01 |
| E2E-R1-16 | Member deactivated → sessions revoked immediately → cannot log in or act | SEC-04, FR-W01 |
| E2E-R1-17 | System admin suspends workspace B (read-only) → writes return 423 with banner → restore | FR-S02, J8 |
| E2E-R1-18 | Startup day-one (J12): Starter profile → install policy-ack pack → publish handbook → 20 employees ack on phones in < 15 min wall-clock | J12 |
| E2E-R1-19 | Keyboard-only: publish a policy and acknowledge it without a mouse | A11Y-01, T10 |
| E2E-R1-ISO | Isolation sweep: B users probe every A object id captured during the pack → 404 + audit `denied` | SEC-01/02 |

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
