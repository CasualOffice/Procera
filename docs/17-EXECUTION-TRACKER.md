# 17 — Execution tracker

Status: Living · Owner: Product + Tech lead · **Generated** by `scripts/build_tracker.py` (edit the script's CORE list or the domain backlog tables in the docs, then re-run). Spreadsheet: [`trackers/Delivery_Tracker.xlsx`](../trackers/Delivery_Tracker.xlsx).

Release = phased release in 07 v0.3 (domain backlogs below still cite old M-codes; map them with: M0–M1→P0, M2–M4→R1, M5–M6→R2, M7→R3, M8→R4, M9→R5, M10–M11→GA, M12+→POST, except items moved per 07 §2). Status values: `Not started · In progress · Blocked · In review · Done · Deferred`. Size: S ≤ 3 d · M ≤ 2 wk · L ≤ 4 wk · XL > 4 wk (split before start).

## Releases (phased; see 07)

| Release | Name | Start | End | Gate | Scope |
|---|---|---|---|---|---|
| P0 | Spikes + G1 + design system | 2026-11-02 | 2026-11-20 | G1 | SP-01…SP-10, ADRs, tokens, CI + E2E harness smoke (old M0–M1) |
| R1 | Workspace & Policies | 2026-11-23 | 2027-01-15 | G-R1 | System console, auth+MFA, roles, settings, DataGrid, SOP/policy library, campaigns, announcements, files, audit, dashboard (old M2–M4 part) |
| R2 | Requests & Approvals | 2027-01-18 | 2027-03-12 | G-R2 | Runtime core, forms, designer, portal, inbox, delegation, SLA, 6 templates (old M5–M6 part) |
| R3 | Operate & Sign | 2027-03-15 | 2027-05-07 | G-R3 | Admin console, DoA, master data, recall/edit, e-sign v1, 13 templates, PWA, BPMN import (old M4 rest, M6 rest, M7) |
| R4 | Enterprise Connect | 2027-05-10 | 2027-07-02 | G-R4 | SSO/SCIM, scoped admins, webhooks/API/CLI, email approvals, guests, e-sign v2 (old M8) |
| R5 | Cases & Compliance | 2027-07-05 | 2027-08-27 | G-R5 | Cases, investigations, legal hold, Part 11, reports, AI optional (old M9) |
| GA | 1.0 hardening + release | 2027-08-30 | 2027-10-15 | G4/G5 | Load/HA/DR, pentest, a11y/VPAT, i18n, docs, full E2E regression (old M10–M11) |
| POST | Post-GA expansion | 2027-10-18 | 2028-03-31 | Demand | Chat platforms, HRIS/ERP, QES/Aadhaar, BPMN/DMN, mining, AI agents (old M12+) |

## Core backlog (epics)

| ID | Area | Work item | Pri | Release | Owner | Refs | Acceptance | Size | Depends | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| BPMS-001 | DISC | User interviews (12+, startup/SME/MNC × HR/IT/Fin/Compliance/Admin) + synthesis | P0 | P0 | Product/UXR | OQ-05, A-* | Synthesis published; personas validated | M |  | Not started |
| BPMS-002 | DISC | Model 5 reference processes in IR on paper | P0 | P0 | Runtime lead | A-02 | All 5 expressible or IR gaps logged | S |  | Done |
| BPMS-003 | SPIKE | SP-01…SP-06 runtime spikes (native vs Temporal vs Operaton) | P0 | P0 | Runtime | U-01, ADR-0015 | Reports in docs/spikes/ | L |  | Done |
| BPMS-004 | SPIKE | SP-07 licence/SBOM review of candidate deps | P0 | P0 | Tech lead | ADR-0014 | No blocked licence | S |  | Done |
| BPMS-005 | SPIKE | SP-08 expression language (dsntk FEEL vs CEL, incl. WASM) | P0 | P0 | Runtime | U-02, ADR-0010 | Engine passes IR expression suite | M |  | Done |
| BPMS-006 | SPIKE | SP-09 Cedar + RLS + sqlx pool + partial eval | P0 | P0 | Platform | U-03, ADR-0011 | 0 leaks; list P95 in budget | M |  | Done |
| BPMS-007 | SPIKE | SP-10 audit chain 10M events: append + explorer | P0 | P0 | Platform | U-04 | ≥2k ev/s; P95<2s | M |  | Done |
| BPMS-008 | UX | Clickable prototype of Wave-1 flows + 5-user test | P0 | P0 | Design | 05 | Findings logged | M |  | Not started |
| BPMS-009 | ARCH | G1: accept ADR-0010/0011/0015; freeze IR v1 | P0 | P0 | Architect | G1 | Signed off | S | BPMS-003..007 | Not started |
| BPMS-010 | UX | Design tokens + core components + visual direction | P0 | P0 | Design/FE | 05 §7, UX-001 | axe clean; approved | L |  | Done |
| BPMS-011 | ENG | Monorepo, CI gates (fmt, clippy, deny, tests, a11y, SBOM) | P0 | P0 | DevOps | 03 §11 | All gates blocking | M |  | Done |
| BPMS-012 | SEC | Threat model review | P0 | P0 | Security | 06 §2 | Reviewed; tests mapped | S |  | Not started |
| BPMS-013 | E2E | E2E harness: Playwright + compose.e2e (PG, SeaweedFS, Valkey, Mailpit, ClamAV), seed A/B, personas, __test clock/drain/fault (feature-gated) | P0 | P0 | SDET | 23 §2–4 | Smoke runs in CI; endpoints absent in release build | M | BPMS-011 | Done |
| BPMS-014 | ENG | AGENTS.md/CLAUDE.md contract, PR template (spec id + E2E id), CODEOWNERS for protected areas | P0 | P0 | Tech lead | 07 §3 | In repo | S |  | Done |
| BPMS-020 | PLAT | System plane: bootstrap, system roles, workspaces lifecycle, quotas | P0 | R1 | Platform | FR-S01–S04 | J8 passes | L | BPMS-011 | Done |
| BPMS-021 | IAM | Local auth (Argon2id), email verify, lockout, sessions | P0 | R1 | Platform | 12 §3, FX-I01 | SEC tests green | M | BPMS-020 | Done |
| BPMS-022 | IAM | MFA TOTP + WebAuthn, recovery codes, step-up | P0 | R1 | Platform | 12 §3/§11 | MFA required by role works | M | BPMS-021 | In progress |
| BPMS-023 | IAM | Memberships, invites, directory (departments/groups/positions/managers), CSV import | P0 | R1 | Platform | FR-W01/W02 | Manager cycles rejected | M | BPMS-020 | Done |
| BPMS-024 | IAM | Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar | P0 | R1 | Platform | FR-W03, ADR-0011 | No-escalation property test | L | BPMS-023 | Done |
| BPMS-025 | SEC | Forced RLS + repository WsCtx + CI cross-workspace sweep | P0 | R1 | Platform | 03 §7, SEC-01/02 | Sweep green in CI | M | BPMS-020 | Done |
| BPMS-026 | AUD | Audit chain append + coverage contract test | P0 | R1 | Platform | FR-T01, AUD-01/07 | Every mutating route emits 1 event | M | BPMS-025 | Done |
| BPMS-027 | CFG | Settings layering, floors, profiles, explainer | P0 | R1 | Platform | FR-S05, FR-W04/W06, CFG-001..003 | Never looser than floor | M | BPMS-020 | In progress |
| BPMS-028 | UX | Shared DataGrid v1 (sort, filter builder, saved views, export, keyboard) | P0 | R1 | FE | FR-V01–V09 | Used by Members & Workspaces | L | BPMS-010 | Not started |
| BPMS-029 | ENG | Outbox + dispatcher + inbox dedup + idempotency keys | P0 | R1 | Platform | ADR-0005 | T02 skeleton passes | M | BPMS-011 | Done |
| BPMS-030 | ENG | Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK | P0 | R1 | Platform/Sec | 04 §8, 06 §4 | Rotation + shred tested | M | BPMS-020 | In progress |
| BPMS-031 | UX | App shell, workspace switcher, system console shell, ⌘K | P0 | R1 | FE | UX-002/003 | Keyboard nav | M | BPMS-010 | Not started |
| BPMS-032 | E2E | E2E-R1 pack (R1-01…19 + ISO) | P0 | R1 | SDET + feature owners | 23 §5 | Green on Chromium + mobile | L | BPMS-013 | Not started |
| BPMS-033 | OPS | Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback | P0 | R1 | Platform | ADR-0019 | E2E passes with cache off | M | BPMS-020 | Done |
| BPMS-034 | BRAND | Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search | P0 | R1 | Design + Product | assets/brand, OQ-01, U-16 | SVGs in assets/brand/vector; clearance memo | S |  | In progress |
| BPMS-040 | LIB | Artifacts, versions, lifecycle state machine, immutability trigger, SoD | P0 | R1 | Backend | FR-G04, T07 | T07 passes | L | BPMS-024,026 | Not started |
| BPMS-041 | LIB | Landscape, collections, owners, RACI, visibility | P0 | R1 | Fullstack | FR-G01/G02 | Search authorized-only | M | BPMS-040 | Not started |
| BPMS-042 | LIB | SOP/policy block editor + renderer (web/PDF) | P0 | R1 | FE | FR-G03, ADR-0013, U-10 | Tagged PDF export | L | BPMS-040 | Not started |
| BPMS-043 | LIB | Semantic diff across artifact kinds | P0 | R1 | Fullstack | FR-G05 | Diff covers bundle artifacts | M | BPMS-040 | Not started |
| BPMS-044 | LIB | Reviews, change requests, effective-date scheduling, renewal date, verification badge | P0 | R1 | Fullstack | FR-G05/G07, FX-G01/G02 | Expired flagged | M | BPMS-040 | Not started |
| BPMS-045 | LIB | Policy library + employee policy portal + applicability | P0 | R1 | Fullstack | FR-PL01–PL03 | Applicability enforced | M | BPMS-042 | Not started |
| BPMS-046 | CAMP | Campaign engine: ack/quiz-lite, audience snapshot, reminders, escalation, new joiners, evidence export | P0 | R1 | Backend | FR-AK01–04, ADR-0017 | 5k recipients/min | L | BPMS-029 | Not started |
| BPMS-047 | COMMS | Announcements: compose, targeting, schedule/expiry, pin, required ack, approval, analytics | P0 | R1 | Fullstack | FR-AN01–06, FX-C01 | J14 passes | L | BPMS-046 | Not started |
| BPMS-048 | NOTIF | In-app notification centre + SMTP email + templates + digests | P0 | R1 | Fullstack | FR-N01/N02/N04 | Bounce handling | M | BPMS-029 | In progress |
| BPMS-049 | SRCH | Postgres FTS + trgm, permission-filtered search | P0 | R1 | Backend | 04 §6 | No restricted leakage | M | BPMS-040 | Not started |
| BPMS-060 | FILE | Files pipeline: resumable upload, hash, sniff, ClamAV, quarantine, signed URLs | P0 | R1 | Backend | FR-F01–F03, F05 | EICAR quarantined; SEC-10 | L | BPMS-030 | Not started |
| BPMS-061 | FILE | Previews (image/PDF) in sandbox; Office preview spike | P0 | R1 | Backend | FR-F04, U-08 | No active content | M | BPMS-060 | Not started |
| BPMS-062 | FILE | Workspace Files view, quotas, orphan cleanup | P0 | R1 | FE | FR-F06/F07 | Held files undeletable | M | BPMS-060,028 | Not started |
| BPMS-063 | ADMIN | Workspace overview dashboard + people & access monitor | P0 | R1 | FE | FR-A01/A06 | Every tile drills down | L | BPMS-028 | Not started |
| BPMS-064 | AUD | Audit explorer, timelines, saved queries, export | P0 | R1 | Fullstack | FR-T03/T04 | P95<2s @10M | L | BPMS-026 | Not started |
| BPMS-065 | PLAT | System console: workspaces grid, quotas, health, system audit | P0 | R1 | FE | FR-S02–S09 | — | M | BPMS-020 | Not started |
| BPMS-120 | PILOT | Pilot workspaces (Starter + Enterprise profile), 3–5 real processes, 4 weeks | P0 | R1 | Product | G3 | G3 checklist | L | M6 | Not started |
| BPMS-080 | RT | IR v1 model + compiler (§11 checks, diagnostics, semantic diff) | P0 | R2 | Runtime | 11, ADR-0003 | All checks covered | L | BPMS-009 | Not started |
| BPMS-081 | RT | Expression engine integration (typed, budgeted, WASM build) | P0 | R2 | Runtime | ADR-0010 | Budget exhaustion = incident | M | BPMS-009 | Not started |
| BPMS-082 | RT | Interpreter: advance loop, tokens, splits/joins, end/terminate | P0 | R2 | Runtime | 04 §4 | Conformance green | L | BPMS-080 | Not started |
| BPMS-083 | RT | Jobs + timers + scheduler with leases/fencing; calendars; SLA clocks | P0 | R2 | Runtime | 04 §4.4, FR-E12 | T06; lateness SLO | L | BPMS-082 | Not started |
| BPMS-084 | RT | Release bundles + publish integration | P0 | R2 | Runtime | ADR-0012 | T03 passes | M | BPMS-040,080 | Not started |
| BPMS-085 | WORK | Work items, assignment strategies, manager chain, DoA matrix, quorum, per-group | P0 | R2 | Backend | FR-E04/E05, FX-D01/D02, FX-E01 | T01 passes | L | BPMS-082,023 | Not started |
| BPMS-086 | WORK | Delegation, OOO substitution, SoD re-check, send-back | P0 | R2 | Backend | 12 §9/§10, FX-E05 | T05 passes | M | BPMS-085 | Not started |
| BPMS-087 | RT | Rule tables (UNIQUE/FIRST/COLLECT) | P0 | R2 | Runtime | FR-E10 | Deterministic | M | BPMS-081 | Not started |
| BPMS-088 | RT | Incidents, retry/skip/cancel, instance admin ops | P0 | R2 | Backend | FR-E13 | One open incident per failure | M | BPMS-082 | Not started |
| BPMS-089 | QA | Conformance suite + model-based tests + deterministic simulation harness | P0 | R2 | SDET/Runtime | 15 §4–6 | Nightly chaos green | L | BPMS-082 | Not started |
| BPMS-100 | DES | Step builder + React Flow canvas + properties + validate/test-run/review/publish | P0 | R2 | FE | FR-E01/E11, ADR-0018 | T10 keyboard-only | XL | BPMS-080 | Not started |
| BPMS-101 | FORM | Form builder + renderer (calc, prefill, repeating, lookups, PDF of submission) | P0 | R2 | FE | FR-E03, FX-F01–F04 | Server = client validation | XL | BPMS-081 | Not started |
| BPMS-103 | REQ | Request portal/catalogue, start form, request detail, comments/@mentions/watchers | P0 | R2 | Fullstack | FR-R01/R02, FX-R01/R02 | J0/J3 pass | L | BPMS-082 | Not started |
| BPMS-105 | WORK | My Work 3-pane inbox (tasks, acks, signatures), bulk with preview | P0 | R2 | FE | FR-R03, FR-AK02 | Decision ≤3 interactions | L | BPMS-085,046 | Not started |
| BPMS-106 | TPL | Template gallery + install wizard + Wave-1 templates (13) | P0 | R2 | Fullstack/SMEs | FR-G08, TPL-001.. | All W1 test-run green | XL | BPMS-100,101 | Not started |
| BPMS-111 | E2E | E2E-R2 pack (R2-01…16 + ISO) + Firefox | P0 | R2 | SDET + feature owners | 23 §5 | Green; race/restart/timer stable | L | BPMS-032 | Not started |
| BPMS-121 | UX | Usability round + SUS measurement + fixes | P0 | R2 | Design | 02 §9 | SUS targets | M | BPMS-120 | Not started |
| BPMS-066 | AUD | Retention policy per record class (policy only) | P0 | R3 | Backend | PRV-04 | Jobs honour classes | M | BPMS-026 | Not started |
| BPMS-102 | DATA | Master-data tables (CSV import, lookups, versioned) | P0 | R3 | Fullstack | FX-E02 | Cascading dropdowns | M | BPMS-101 | Not started |
| BPMS-104 | REQ | Recall/withdraw, edit-after-submit, request-on-behalf, numbering | P0 | R3 | Backend | FX-E03/E04/E06 | Audited | M | BPMS-103 | Not started |
| BPMS-107 | SIGN | E-sign v1: PDF, recipients/roles, routing, bulk, fields, consent step, ceremony, certificate | P0 | R3 | Fullstack | FR-DS01–07, FX-S01/02/05/06/07 | J13 passes | XL | BPMS-046,060 | Not started |
| BPMS-108 | ADMIN | All requests / all work / workload / process health / ops monitor | P0 | R3 | FE | FR-A02–A05 | Bulk audited | L | BPMS-063,085 | Not started |
| BPMS-109 | UX | PWA (360px) for My Work, requests, acks, signing | P0 | R3 | FE | FR-P07 | Lighthouse PWA pass | M | BPMS-105 | Not started |
| BPMS-110 | LIB | BPMN 2.0 XML import (subset) + compatibility report | P0 | R3 | Runtime | FR-G09, SP-06 | Unsupported listed | M | BPMS-080 | Not started |
| BPMS-112 | E2E | E2E-R3 pack (R3-01…12 + MOB + ISO) + WebKit | P0 | R3 | SDET + feature owners | 23 §5 | Green; bulk-sign 1000 | L | BPMS-111 | Not started |
| BPMS-140 | IAM | OIDC SSO, SAML-broker, SSO enforcement, JIT | P1 | R4 | Platform | FR-P02, INT-001/002 | Deprovision test | L |  | Not started |
| BPMS-141 | IAM | SCIM 2.0 server | P1 | R4 | Platform | INT-003 | <5 min deprovision | M | BPMS-140 | Not started |
| BPMS-142 | IAM | Scoped/delegated admins; IP allowlists | P1 | R4 | Platform | CFG-018, FR-W06 | No escalation | M | BPMS-024 | Not started |
| BPMS-143 | INT | Webhooks (Standard Webhooks), HTTP connector via egress proxy, inbound events | P1 | R4 | Backend | INT-*, WH/SSRF | SEC SSRF tests | L | BPMS-029 | Not started |
| BPMS-144 | INT | Public API hardening, OpenAPI, CLI, TS/Python SDKs | P1 | R4 | Backend | 08 | Spec published | M |  | Not started |
| BPMS-145 | NOTIF | Email-actionable approvals, reply-by-email, web push | P1 | R4 | Fullstack | FR-N03/N05, FX-R03 | Replay rejected | M | BPMS-048 | Not started |
| BPMS-146 | IAM | Guests + external signers (magic link + OTP); self-serve signing links | P1 | R4 | Fullstack | FR-P08, FX-S10 | Single-use | M | BPMS-107 | Not started |
| BPMS-147 | SIGN | E-sign v2: DOCX + merge, anchor tags, conditional fields, delegate, correction, embedded | P1 | R4 | Fullstack | FX-S03/04/08/09/12 | Audited corrections | L | BPMS-107 | Not started |
| BPMS-148 | FORM | Public forms (privacy CAPTCHA), save & resume, partials, form signature field | P1 | R4 | Fullstack | FX-F05–F07 | Rate-limited | M | BPMS-101 | Not started |
| BPMS-149 | EXEC | Subprocesses, reusable sub-flows, duplicate detection, bulk import, env promotion | P1 | R4 | Runtime | FR-E08, FX-E07–E09, FR-P09 | — | L | BPMS-082 | Not started |
| BPMS-150 | LIB | Auto-generated process documentation; policy exceptions; policy templates | P1 | R4 | Fullstack | FX-G04, FR-PL04, FX-G03 | — | M | BPMS-042 | Not started |
| BPMS-151 | COMMS | Emergency alerts, content validation period, moderation, quiz | P1 | R4 | Fullstack | FX-C02–C04 | — | M | BPMS-047 | Not started |
| BPMS-152 | E2E | E2E-R4 pack incl. Keycloak, SCIM, webhook receiver | P1 | R4 | SDET + feature owners | 23 §5 | Green | L | BPMS-112 | Not started |
| BPMS-160 | CASE | Case types, queues, macros, SLAs, link/merge, restricted cases | P1 | R5 | Fullstack | FR-C01, FX-K01–K06 | J4 passes | XL | BPMS-082 | Not started |
| BPMS-161 | AUD | Investigations: pins, legal hold, signed export, conflict-of-interest | P1 | R5 | Fullstack | FR-T05/T06, FX-K08, FX-L01 | J10 passes | L | BPMS-064,160 | Not started |
| BPMS-162 | AUD | Checkpoints, RFC 3161, WORM export, procera-verify CLI | P1 | R5 | Platform | AUD-02–05 | Verifier detects tamper | M | BPMS-026 | Not started |
| BPMS-163 | GOV | Attestation campaigns, control mapping, access reviews | P1 | R5 | Fullstack | FR-G06, FR-W11 | Evidence export | M | BPMS-046 | Not started |
| BPMS-164 | GOV | Retention disposition + certificates, DSAR template, legal hold engine | P1 | R5 | Backend | PRV-01/03/04 | Holds block deletion | M | BPMS-066 | Not started |
| BPMS-165 | SIGN | Part 11 mode, PAdES-LTV, TSA; in-person/kiosk signing | P1 | R5 | Fullstack | ESIG-01–05, FR-DS05, FX-S11 | Verification passes | L | BPMS-107 | Not started |
| BPMS-166 | ADMIN | Alert centre, custom dashboards, report builder, scheduled reports, KPI goals, usage analytics, CSAT | P1 | R5 | Fullstack | FR-A07–A09, FX-A01–A04, FX-R04 | RLS at run time | XL | BPMS-063 | Not started |
| BPMS-167 | IAM | Separation mode; break-glass | P1 | R5 | Platform | 12 §6.1, FR-S07 | Visible to workspace | M | BPMS-024 | Not started |
| BPMS-168 | LIB | Role-based onboarding/training paths | P1 | R5 | Fullstack | FX-G07 | Completion reports | M | BPMS-046 | Not started |
| BPMS-169 | E2E | E2E-R5 pack incl. investigations, Part 11, break-glass | P1 | R5 | SDET + feature owners | 23 §5 | Green | L | BPMS-152 | Not started |
| BPMS-186 | AI | AI assistance P1 set (drafting, summaries, translate, Q&A) behind kill switch | P1 | R5 | Fullstack | 16, ADR-0006 | Eval thresholds | L |  | Not started |
| BPMS-180 | OPS | Load test at reference load; HA failover; RPO/RTO restore drill | P1 | GA | SRE | 06, T09 | Targets met | L |  | Not started |
| BPMS-181 | SEC | External pentest + ASVS L2 self-assessment + fixes | P1 | GA | Security | 06 | 0 crit/high | M |  | Not started |
| BPMS-182 | UX | WCAG 2.2 AA audit + VPAT 2.5 INT | P1 | GA | Design/QA | A11Y-01/03 | Audit pass | M |  | Not started |
| BPMS-183 | I18N | 3 UI languages + RTL + pseudo-locale | P1 | GA | FE | FR-P04, OQ-09 | Checks pass | M |  | Not started |
| BPMS-184 | OPS | Helm chart, air-gapped bundle, upgrade N-1→N tests, dedicated-DB tier | P1 | GA | DevOps | FR-P06, FR-S10 | Upgrade with live instances | L |  | Not started |
| BPMS-185 | DOC | Docs site, admin guide, API reference, trust pack | P1 | GA | Writer | FX-P06 | Published | M |  | Not started |
| BPMS-187 | E2E | E2E-GA regression: 3 browsers × 2 viewports, cache-off, upgrade-path R1→1.0, air-gapped | P1 | GA | SDET | 23 §5 | Green; flake < 0.5 % | M | BPMS-169 | Not started |
| BPMS-190 | REL | GA release: signed artefacts, SBOM, provenance, runbooks | P1 | GA | Tech lead | G5 | Released | S | M10 | Not started |
| BPMS-200 | INT | Chat/host-platform connectors (identity, embed, cards, notifications, directory) | P2 | POST | Backend | 13 §4 | Demand gate | XL |  | Not started |
| BPMS-201 | INT | Chat cards (Slack/Teams), M365/Google, HRIS, ERP connectors | P2 | POST | Backend | INT-* | — | XL |  | Not started |
| BPMS-202 | SIGN | QES via QTSP; Aadhaar eSign via ESP; SMS/IDV providers | P2 | POST | Backend | FX-S13, FX-S07 | — | L |  | Not started |
| BPMS-203 | EXEC | BPMN view (diagram-js), DMN import/export, migration tool, simulation | P2 | POST | Runtime/FE | ADR-0018, FR-E07/E10, FR-I04 | — | XL |  | Not started |
| BPMS-204 | INS | Event-log export OCEL 2.0 / XES | P2 | POST | Data | FR-I03 | Schema-valid | M |  | Not started |
| BPMS-205 | AI | Governed AI agent steps | P2 | POST | Fullstack | 16 | Human gate | L |  | Not started |

**Progress:** Done: 18 · In review: 0 · In progress: 5 · Blocked: 0 · Not started: 86 · Deferred: 0 (of 109)

### Active and recently completed

| ID | Work item | Status | Note |
|---|---|---|---|
| BPMS-002 | Model 5 reference processes in IR on paper | Done | 5 reference processes as IR v1 fixtures (all 16 node kinds) + JSON Schema + structural checker IR-E001..E016 with negative tests; 12 IR spec clarifications (docs/spikes/BPMS-002) |
| BPMS-003 | SP-01…SP-06 runtime spikes (native vs Temporal vs Operaton) | Done | SP-01..06 native runtime: 10k instances under 1329 crashes 0 lost; race exactly-once; join once; 410 retries 0 dup effects; 30-day pinning; BPMN 54-node lossless round trip — docs/spikes/SP-01-06; ADR-0015 recommended |
| BPMS-004 | SP-07 licence/SBOM review of candidate deps | Done | SP-07: Rust + JS dependency licences reviewed; approved set; JS licence gate (scripts/js-licence-gate.mjs) + CDLA-Permissive-2.0 for CA roots; fonts OFL — docs/spikes/SP-07 |
| BPMS-005 | SP-08 expression language (dsntk FEEL vs CEL, incl. WASM) | Done | SP-08: recommend CEL + decimal/stdlib/type-checker/budgets; FEEL rejected at runtime (quadratic loops, no WASM) — docs/spikes/SP-08; ADR-0010 recommended |
| BPMS-006 | SP-09 Cedar + RLS + sqlx pool + partial eval | Done | SP-09: Cedar 6/6 correct, 5.6µs P50; RLS via pool 0 leaks/5000; LLD fix NULLIF in RLS policy; partial-eval→SQL viable with fallback — docs/spikes/SP-09 |
| BPMS-007 | SP-10 audit chain 10M events: append + explorer | Done | SP-10: head-lock 9.7k ev/s (P99 11ms), batched sealer 65k ev/s, explorer P95 <0.6ms at 10M, verifier 536k ev/s + tamper detected — docs/spikes/SP-10 |
| BPMS-010 | Design tokens + core components + visual direction | Done | Tokens (41 contrast checks + forbidden-pair guard), Button/TextField/Checkbox/Notice/Select/ConfirmDialog/Tabs/DataGrid shell on React Aria + TanStack, app shell, gallery; axe light/dark, keyboard, focus trap, aria-sort, reflow, targets (E2E-P0-05..13) |
| BPMS-011 | Monorepo, CI gates (fmt, clippy, deny, tests, a11y, SBOM) | Done | Rust workspace, kernel, procera binary, CI (fmt/clippy/test/deny/SBOM/E2E smoke) + web check/build, JS licence gate and axe a11y job (added with BPMS-010) |
| BPMS-013 | E2E harness: Playwright + compose.e2e (PG, SeaweedFS, Valkey, Mailpit, ClamAV), seed A/B, personas, __test clock/drain/fault (feature-gated) | Done | e2e feature + /__test controls, Playwright harness, P0 smoke pack (E2E-P0-01..04, REL), compose.e2e stack, CI e2e-smoke job; on main, CI green. Seed A/B + personas land with the first workspace APIs (BPMS-032); full compose boot pending a Docker run |
| BPMS-014 | AGENTS.md/CLAUDE.md contract, PR template (spec id + E2E id), CODEOWNERS for protected areas | Done | CLAUDE.md, AGENTS.md, PR template, CODEOWNERS, docs/24 rules on main |
| BPMS-020 | System plane: bootstrap, system roles, workspaces lifecycle, quotas | Done | procera-platform + /sysapi/v1: one-time bootstrap token (printed at startup until bootstrapped), system.owner/admin/operator/auditor fixed authz table, MFA required for system plane, workspace create/list/get/rename/suspend/restore/schedule-deletion with If-Match + ETag, profile quotas, platform setting floors seeded, all audited; E2E-R1-SYS-01..05 |
| BPMS-021 | Local auth (Argon2id), email verify, lockout, sessions | Done | procera-identity + /api/v1/auth: users, Argon2id (policy, rehash, enumeration-safe), JWT access (ADR-0020) + rotating refresh with reuse detection, DB login throttling + per-IP rate limit, logout, sign-out-everywhere, deactivation via token_version; audited; E2E-R1-AUTH-01..05,07 |
| BPMS-022 | MFA TOTP + WebAuthn, recovery codes, step-up | In progress | TOTP (RFC 6238, replay-protected, sealed secrets) + 10 recovery codes + MFA login (E2E-R1-AUTH-06). Step-up: POST /session/step-up (TOTP code, or password without MFA) re-issues the token with a fresh auth_time; enforced against auth.stepup.window (401 step_up_required) for step-up settings, role create/edit and grant/revoke (E2E-R1-CFG-03). MFA required by role (auth.mfa.required_roles) enforced at workspace entry, 403 mfa_required (E2E-R1-MEM-10). Remaining: WebAuthn/passkeys |
| BPMS-023 | Memberships, invites, directory (departments/groups/positions/managers), CSV import | Done | procera-members + /api/v1: memberships, invitations (hashed + sealed tokens, 7-day expiry, email-bound, single use; list, revoke, resend), register-by-invite, owner onboarding activates the workspace, workspace switch (ws-scoped JWT), deactivation guards, state gate (423). Directory (migration 0008): department tree (uuid[] paths, sibling-unique names, no moves under descendants), static groups, positions (behind directory.positions.enabled), manager links with cycle rejection, deterministic manager resolution (explicit → nearest department head), per-field source attribution, CSV import with plan/apply. All audited; E2E-R1-MEM-01..03,05..07, E2E-R1-DIR-01..04 |
| BPMS-024 | Permission catalogue, role templates, custom roles, grants, scopes, preview → Cedar | Done | procera-access: permission catalogue + 12 role templates; custom roles (no escalation on create/edit/grant/revoke, built-ins immutable, clone lineage); grants to members or static groups (migration 0009), scoped (workspace/department/collection/process/case) with expiry, compiled to Cedar; members inherit their department subtree so department-scoped admins reach only it; invariants (workspace isolation, active members only); last-owner protection; role change preview; access_revisions cache; all changes audited. Property tests: no escalation (64 cases) and preview matches effect (48 cases), both against Postgres. E2E-R1-MEM-04,08,09 |
| BPMS-025 | Forced RLS + repository WsCtx + CI cross-workspace sweep | Done | procera-db: migrations (0001-0003), roles bootstrap (app/migrator/discovery), WsTx with tx-local set_config, procera_enable_workspace_rls() with NULLIF, procera_rls_violations guard (tested + enforced at startup), discovery.* functions with column grants; 6 DB tests |
| BPMS-026 | Audit chain append + coverage contract test | Done | procera-audit: JCS/SHA-256 per-workspace chain in the business tx, batched sealer, verifier, monthly partitions with forced RLS, append-only grants + trigger; tamper test. Route audit contract (audit_contract.rs): a unit test fails on any mutating route without an entry; E2E-R1-AUD-01 checks every route's appended events against it. Gaps closed: TOTP enrolment start, workspace selection, role/grant changes. data_access events follow with restricted data (BPMS-040+) |
| BPMS-027 | Settings layering, floors, profiles, explainer | In progress | procera-settings: catalogue in code (P0 subset, 38 keys) with comparators; platform floors + workspace values + append-only revisions (migration 0007, RLS); 4 configuration profiles applied at workspace creation; resolution with clamping when a floor is raised; explainer; /api/v1/settings (list, explainer, PUT per key with If-Match, history) and /sysapi/v1/settings floors; reasons for ⚡ keys, step-up = MFA session (interim); audited (setting.changed, platform.floor_changed); property test (512 cases, never looser than floor); E2E-R1-CFG-01..05. Remaining: department/collection overrides (needs directory), profile switch plan/apply (CFG-017), settings cache invalidation (CFG-004) |
| BPMS-029 | Outbox + dispatcher + inbox dedup + idempotency keys | Done | Outbox/inbox/idempotency tables + helpers (SKIP LOCKED claims, replay/conflict). Dispatcher (procera_db::dispatch, migration 0010): due-workspace discovery, per-workspace advisory lock, per-aggregate ordered delivery to a Bus, at-least-once with exponential backoff (max 5 min) and hold-back behind a failed event, NOTIFY trigger wake-ups + 1 s polling; runs in serve --all; /__test/drain really drains. Tests: order/backoff/hold-back/workspaces + NOTIFY wake-up; E2E-R1-MEM-07. Consumers (mailer) arrive with BPMS-048 |
| BPMS-030 | Key hierarchy: KeyProvider (sealed local, OpenBao, KMS), per-workspace KEK | In progress | procera-crypto: LocalKey (env root key, HMAC-derived purpose keys), AES-256-GCM seal/open bound to context, token helpers, procera keygen. Remaining: KMS/OpenBao adapters, per-workspace KEKs, rotation |
| BPMS-033 | Valkey cache port (rate limit, session cache, policy/settings cache, SSE fan-out) + cache-off fallback | Done | procera-cache: memory + Valkey/Redis (redis 1.7, BSD-3) with ws:/sys: key namespacing, TTL-only writes, Lua fixed-window rate limit with in-process fallback, degraded health; wired into procera (/readyz), CI Valkey service |
| BPMS-034 | Brand vector masters (SVG symbol/wordmark/lockups, small-size symbol), favicon/PWA icon set from vector, trademark & domain search | In progress | Raster assets + brand guide merged; SVG masters and trademark search pending |
| BPMS-048 | In-app notification centre + SMTP email + templates + digests | In progress | procera-notify (migration 0011): templates in code (minijinja, strict, HTML auto-escaped), in-app inbox (list/unread/read/read-all), email queue with sealed bodies and dedupe, SMTP delivery via lettre (STARTTLS/TLS) with backoff (max 1 h, 12 tries), workspace sender name; outbox consumers: invitation email with accept link, member-joined in-app to the inviter. Flags --public-url, --smtp-url, --mail-from. E2E-R1-NOT-01..02. Remaining: digests + quiet hours (FR-N04), localisation + terminology (CFG-008), more event kinds as features land |

## Domain backlogs (detail, owned by their docs)

- **Templates (TPL)** — 20 items (TPL-001…TPL-020), source: `19-PROCESS-TEMPLATE-LIBRARY.md`
- **UX (UX)** — 49 items (UX-001…UX-049), source: `05-UX-AND-DESIGN.md`
- **Integrations (INT)** — 32 items (INT-001…INT-028), source: `13-INTEGRATIONS.md`
- **Config (CFG)** — 25 items (CFG-001…CFG-025), source: `20-WORKSPACE-CONFIGURATION.md`
- **AI (AI)** — 25 items (AI-001…AI-025), source: `16-AI-ASSISTANCE.md`
- **E2E scenarios (E2E)** — 122 items (E2E-P0-01…E2E-R5-ISO), source: `23-E2E-TEST-PLAN.md`
- **Security tests** — 37 (06) · **Test matrix** — 40 (15)

## Risks

| ID | Risk | Severity | Likelihood | Mitigation | Owner |
|---|---|---|---|---|---|
| R1 | Execution correctness | Critical | High | IR spec, conformance, property/model tests, chaos | Runtime lead |
| R2 | Scope creep (BPMN/features) | High | High | Non-goals, catalogue change process, scope-cut list | Product |
| R3 | Cross-workspace data leak | Critical | Medium | Layered isolation, CI sweep, pentest | Security |
| R4 | Adoption / UX quality | High | Medium | Templates, Starter profile, usability per milestone, SUS gate | Design |
| R5 | Wrong runtime choice | High | Medium | Spikes before G1, adapter seam | Architect |
| R6 | Licence contamination | High | Medium | CI licence gate, SP-07 quarterly | Tech lead |
| R7 | Instance migration demand | High | High | Pinning; cancel-and-restart; P2 tool | Runtime |
| R8 | Duplicate external side effects | High | High | Outbox, idempotency keys | Backend |
| R9 | Analytics inaccuracy | Medium | Medium | Event semantics, reconciliation tests | Data |
| R10 | Staffing below plan | High | Medium | Option B timeline; scope cuts | Product |
| R11 | E-sign legal validity per jurisdiction | Medium | Medium | Legal posture doc, certificate, QES/Aadhaar providers P2 | Legal |
| R12 | Admin visibility vs confidential cases | Medium | Medium | Restricted cases + Separation mode (OQ-12) | Product |
| R13 | Scanner/converter/object store availability | Medium | Low | Fail-closed + degraded modes | SRE |
| R14 | Email deliverability for self-hosters | Medium | Medium | Guides, bounce handling, in-app fallback | Backend |
| R15 | Accessibility regressions in designer/canvas | Medium | Medium | Keyboard alternative, axe in CI, audits | Design |
| R16 | AI-generated code quality / subtle bugs | High | Medium | Spec-first tasks, small PRs, CI gates, 2-human review on protected areas, E2E per feature | Tech lead |
| R17 | E2E flakiness slows releases | Medium | Medium | Test clock + drain, seeded data, 1 retry max, quarantine lane, flake budget | SDET |

## Decisions (ADRs)

| ADR | Decision | Status |
|---|---|---|
| [ADR-0001](adr/ADR-0001-governance-execution-separation.md) | Separate governance/catalog from execution | Accepted (pending G1) |
| [ADR-0002](adr/ADR-0002-postgres-system-of-record.md) | PostgreSQL 16+ is the only system of record (not MariaDB/MySQL) | Accepted |
| [ADR-0003](adr/ADR-0003-versioned-ir.md) | Canonical, versioned workflow IR | Accepted |
| [ADR-0004](adr/ADR-0004-step-builder-first.md) | Step builder is the default authoring UX; BPMN is optional | Accepted |
| [ADR-0005](adr/ADR-0005-outbox-idempotency.md) | Transactional outbox/inbox and idempotency keys | Accepted |
| [ADR-0006](adr/ADR-0006-ai-human-gate.md) | AI outputs are drafts behind a human gate | Accepted |
| [ADR-0007](adr/ADR-0007-modular-monolith.md) | Modular monolith with separate worker/scheduler processes | Accepted |
| [ADR-0008](adr/ADR-0008-workspace-tenancy.md) | Workspace is the tenant; layered isolation | Accepted (revised per product direction 2026-10-11) |
| [ADR-0009](adr/ADR-0009-broker-optional.md) | Message broker is optional | Accepted |
| [ADR-0010](adr/ADR-0010-expression-language.md) | Typed, total expression language | Recommended: CEL with Procera extensions (SP-08, 2026-10-11); final acceptance at G1 |
| [ADR-0011](adr/ADR-0011-authorization-engine.md) | Cedar embedded for authorization | Validated by SP-09 (2026-10-11); acceptance at G1 |
| [ADR-0012](adr/ADR-0012-release-bundle.md) | Release bundle pins every dependency of a definition | Accepted |
| [ADR-0013](adr/ADR-0013-sop-content-model.md) | Structured block model for SOP/policy content | Accepted |
| [ADR-0014](adr/ADR-0014-licensing-and-contribution.md) | Apache-2.0, DCO, permissive-only dependencies | Proposed (OQ-06) |
| [ADR-0015](adr/ADR-0015-runtime-selection.md) | Runtime: native Rust interpreter primary | Recommended: native interpreter (SP-01…06 all pass, 2026-10-11); final acceptance at G1 |
| [ADR-0016](adr/ADR-0016-standalone-first.md) | Standalone first; integrations later via ports | Accepted (product direction 2026-10-11) |
| [ADR-0017](adr/ADR-0017-unified-campaign-engine.md) | One acknowledgement & signature campaign engine | Accepted |
| [ADR-0018](adr/ADR-0018-ui-stack-and-canvas.md) | UI stack and canvas libraries | Accepted |
| [ADR-0019](adr/ADR-0019-valkey-redis-cache.md) | Redis-protocol cache (Valkey default) as a standard, non-authoritative component | Accepted |
| [ADR-0020](adr/ADR-0020-jwt-access-tokens.md) | JWT access tokens with rotating refresh tokens (no server-side sessions) | Accepted |

## Open questions

See [22 §1](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md). Decide-by milestones are tracked in the spreadsheet `Open Questions` sheet.

## RACI (delivery-level)

| Activity | Process Owner | Designer | Reviewer/Compliance | Workspace Admin | System Admin |
|---|---|---|---|---|---|
| Draft process/SOP/policy | A | R | C | I | — |
| Approve controlled change | C | I | A/R | I | — |
| Publish standard process | A/R | C | C | I | — |
| Configure roles & settings | I | — | C | A/R | — |
| Install template | A | R | C | R | — |
| Configure integration secret | I | C | C | A/R | C (allowed list) |
| Run attestation campaign | C | — | A/R | R | — |
| Investigate audit event | I | — | A/R | R | — |
| Create/suspend workspace | — | — | — | I | A/R |
| Approve break-glass | — | — | C | A | R (requester) |

## Cadence

- Weekly: tracker review (status, blockers). - Per milestone: exit checklist review ([07 §3](07-ROADMAP-AND-MILESTONES.md)), register review ([22](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md)), risk re-score, usability round.
