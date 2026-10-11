# 22 — Unknowns, assumptions and open questions

Status: Living register · Owner: Product + Tech lead · Reviewed at every milestone planning.

Purpose: to make sure **nothing turns into a wild card**. Every uncertainty that could change scope, architecture, cost or timeline is listed here. Each entry has an owner, a resolution method and a decide-by milestone. An item can't stay open past its decide-by milestone without an explicit, recorded extension.

## 1. Open product questions (need a decision from the owner)

| ID | Question | Recommendation | Owner | Decide by |
|---|---|---|---|---|
| OQ-01 | ~~Product name~~ **Decided 2026-10-11: Procera.** Remaining: trademark/domain search (`procera.dev` is a placeholder in error-type URIs), public repository name (local dir is `bpms`) | Run a trademark search in target markets (IN, EU, US) before the public launch; register the domain | Product owner | R1 |
| OQ-02 | Hosted SaaS at GA, or self-host only first? | Self-host at GA, hosted pilot at M10, so the architecture is ready (residency, quotas) | Product owner | M7 |
| OQ-03 | First regulated industry to target (pharma / BFSI / public sector)? | BFSI/IT-services in India + EU SMEs first; pharma (Part 11) as a design constraint, sold after M9 | Product owner | M7 |
| OQ-04 | Can a single user belong to workspaces of *different* customers on a hosted deployment (like Slack Connect)? | Yes (global accounts), but no cross-workspace data, ever | Product owner | M2 |
| OQ-05 | Expected scale of the first customers (members, active instances)? | Plan for the reference load (03 §12); validate in interviews | Product | M0 |
| OQ-06 | Business model: hosted service, support subscriptions, enterprise add-ons? Is any feature non-OSS? | Fully Apache-2.0 core; revenue from hosted, support, and validation packs/LTS | Product owner | M7 |
| OQ-07 | Which chat/collaboration platform integration ships first after GA (Slack, Teams, Google Chat, Mattermost…), or is it purely customer-driven? | Customer-driven demand gate; contract designed now (13 §4) | Product owner | M8 |
| OQ-08 | Offline/mobile native apps ever? | PWA only through GA; revisit with usage data | Product | M10 |
| OQ-09 | GA UI languages beyond English? | English + Hindi + one EU language (German or French), chosen from the pilot pipeline | Product | M8 |
| OQ-10 | Should e-sign target legal-grade use in India (Aadhaar eSign via ESP) at GA? | No. Ship SES/AES-style with a strong evidence certificate; Aadhaar/QES providers in M12 | Product + Legal | M8 |
| OQ-11 | Pricing anchor for hosted (per author vs per member)? | Free unlimited readers/attesters/requesters; charge per "maker" seat + workspace tier | Product owner | M10 |
| OQ-12 | **Admin full visibility vs confidential HR/whistleblower cases.** Must admins always see everything? | Default: admins see everything, with access-logging. Cases flagged *restricted* need an audited admin self-grant that the case team sees. Separation mode removes admin content access entirely. | **Product owner (you)** | M3 |
| OQ-13 | Is a public/anonymous intake (whistleblowing, public forms) in GA scope? | Public forms P1 (M8); anonymous two-way whistleblowing P2 | Product | M8 |
| OQ-14 | Who curates the global template gallery long-term (in-house SMEs, community, partners)? | In-house for Waves 1–2; community contributions with review from Wave 3 | Product | M6 |

## 2. Technical unknowns (resolved by spikes or research)

| ID | Unknown | Why it matters | Resolution | Decide by |
|---|---|---|---|---|
| U-01 | Native interpreter vs Temporal vs Operaton adapter | Core runtime architecture | SP-01…SP-06 + ops-cost comparison → ADR-0015 | G1 (M1) |
| U-02 | FEEL subset (dsntk) vs CEL: typing, WASM build for browser, TCK coverage | Expression language is stored in every definition | SP-08 → ADR-0010 | G1 |
| U-03 | Cedar partial evaluation maturity for list filters | List performance and correctness of scoped admins | SP-09; fall back to hand-compiled scope predicates | G1 |
| U-04 | Audit chain append contention at high data-access volume | Hot read paths must not serialise | SP-10 (batched sealer) | G1 |
| U-05 | Postgres-only bus throughput ceiling | When NATS/Kafka becomes necessary | SP-01 metrics; document a threshold | M5 |
| U-06 | Temporal Rust SDK worker-versioning support | Only matters if Temporal wins U-01 | Vendor docs / spike | G1 |
| U-07 | PDF stamping / PAdES library choice in Rust (licence-clean) | E-sign finalisation | Research spike in M5 (lopdf/pdf-writer + signing crate vs sandboxed external tool) | M5 |
| U-08 | DOCX→PDF conversion engine (LibreOffice headless = MPL, heavy; OpenDoc engine; others) | Previews, DOCX e-sign | Spike in M4; sandbox mandatory | M4 |
| U-09 | Object Lock support in SeaweedFS/RustFS | WORM audit export (AUD-04) | Test in M9; else document "use AWS/compatible store" | M9 |
| U-10 | Block editor library for SOP/policy (licence, a11y, collaborative-ready) | Core authoring UX | Evaluate in M1 (TipTap core MIT / ProseMirror MIT / Lexical MIT) | M1 |
| U-11 | PDF field-placement editor (render with pdf.js Apache-2.0 + own overlay) | E-sign UX | Prototype in M4 | M4 |
| U-12 | Real-time collaboration on drafts (co-editing) needed? | Big architecture impact if yes | Assume **no** for GA (locking + comments); revisit at M7 | M7 |
| U-13 | APQC PCF exact licence text | Template taxonomy attribution | Legal review | M3 |
| U-14 | Email deliverability for self-hosters (SPF/DKIM/DMARC guidance, bounce handling) | Notifications are core in standalone mode | Admin guide + bounce webhook support for common relays | M6 |
| U-16 | Brand assets are raster concept extractions; production vector masters and a small-size symbol are needed | Logo/favicon quality, print, app icons | Commission or produce SVG masters (BPMS-034); re-export icon and favicon sets | R1 |
| U-15 | Browser support matrix | QA cost | Decide: last 2 versions of Chromium/Firefox/Safari; no IE/legacy Edge | M1 |

## 3. Assumptions (true until proven otherwise; each has a check)

| ID | Assumption | Check | If false |
|---|---|---|---|
| A-01 | Customers can provide PostgreSQL 16+, an S3-compatible store and SMTP | Interviews (M0), install telemetry (opt-in) | Ship bundled defaults in Compose (already planned) |
| A-02 | A step builder covers ≥ 90 % of the processes buyers want to automate | Model the 5 reference processes + 13 Wave-1 templates in IR | Extend IR before M5 |
| A-03 | Pinning plus cancel-and-restart is acceptable in place of instance migration until P2 | Pilot feedback | Pull the migration tool forward |
| A-04 | Locking plus comments are enough for co-authoring drafts (no real-time co-edit) | Pilot feedback | U-12 |
| A-05 | Workspace admins seeing all content is acceptable to most buyers (with Separation mode for the rest) | Interviews with HR/compliance | OQ-12 |
| A-06 | One campaign engine covers acks, quizzes, signatures and attestations | Design review at M3 | Split envelope engine |
| A-07 | SES/AES-style signatures with a certificate of completion meet most enterprise HR/policy needs | Legal review per launch market | QES/Aadhaar providers earlier |
| A-08 | The AI-assisted team (~8–9 people + agents, 07 §5) is available | Hiring plan at P0 | Keep release order; stretch each release to 11–12 weeks |
| A-11 | AI coding agents roughly double implementation throughput without lowering quality | Measure cycle time and escaped defects per release (R1 baseline) | Re-plan R3+ durations at the G-R1 review |
| A-12 | Customers accept PostgreSQL as a hard requirement (no MariaDB) | Interviews; install guide | Offer the managed-Postgres guide; not negotiable (ADR-0002) |
| A-09 | AI is optional and off by default; the product is complete without it | Pilot | — |
| A-10 | 3 UI languages at GA are enough | Pipeline | More locales post-GA |
| A-13 | Campaign timing (BPMS-046): reminder offsets and `campaign.escalation.to_manager_after` both count from each member's assignment; escalation happens once; settings are copied onto a campaign at creation; joiners keep the campaign's due window; policy acknowledgements need `policy:view` (no `policy:acknowledge` in the catalogue) | Product-owner review of docs/04 §2.7 "As built"; pilot feedback | Count escalation from the due date; add `policy:acknowledge` to docs/12 §5 |
| A-18 (BPMS-065) | System console: (1) the member quota counts active members plus open invitations, checked when inviting (accepting a reserved seat is never refused; lowering a quota below current use blocks new invitations only); (2) quota defaults are per configuration profile and apply to new workspaces only; (3) the system plane's step-up window is the docs/12 §11 default of 5 minutes (it has no workspace settings) and covers suspend, reactivate, schedule deletion, workspace quotas and quota defaults; (4) scheduling deletion requires a reason, stored as the workspace's state reason; (5) health is "degraded" when the oldest pending event or queued email is older than 5 minutes, any email has failed, the cache is degraded, no dispatcher runs, or migrations are missing; (6) "last activity" is the newest event on the workspace's own audit chain; (7) the system audit names system administrators only, other actors stay pseudonymous | Product-owner review of docs/05 §3.20 and docs/20 §3 "As built"; operator feedback | Count invitations separately; per-plan defaults; a platform setting for the system step-up window |
| A-14 | Library visibility (BPMS-045): members who can only `view` a kind (no `create`/`edit_draft`/`review`/`approve`/`publish`) see an artifact only once it has a published version, and only its published history (published, deprecated, retired), never drafts, reviews or scheduled versions. This matches the UI promise "Nobody else sees it until it's published". A `restricted` policy is additionally hidden (404, also in search) from readers outside its applicability. Authors and admins see everything | Product-owner review; pilot feedback | Show drafts to readers through explicit visibility settings (docs/12 §8) |
| A-15 (BPMS-048) | Quiet hours apply every day (no per-day selection yet); digest and quiet-hour times use whole hours in the member's time zone; which emails are "urgent" is fixed per kind (campaign actions and acknowledgement requests urgent, plain announcements normal, critical announcements critical) | Pilot feedback | Per-day quiet hours; per-kind member choices (CFG) |

## 4. External dependencies

| Dependency | Risk | Mitigation |
|---|---|---|
| ClamAV signatures/updates | Stale signatures in air-gapped installs | Offline signature bundle process; scanner interface is pluggable |
| Public TSA availability (RFC 3161) | Outage delays checkpoint stamping | Async retry; multiple TSAs configurable |
| SMTP relays | Deliverability | Bounce handling, digest, in-app fallback |
| Upstream licences (Cedar, React Flow, diagram-js, dsntk, TanStack, etc.) | Licence change | CI licence gate, pinned versions, quarterly review (SP-07 repeated) |
| Standards drafts (Annex 11 final, ISO 9001:2026, EAA harmonised standard update) | Requirement drift | Compliance review at M8 and M10 |

## 5. Explicit non-goals (so they can't creep in)

The following are out of scope:
- Jira-style project management
- payroll/ERP
- full BPMN conformance
- user scripting
- AI auto-approval or people-scoring
- arbitrary-log process discovery
- native mobile apps and offline approvals (before demand gate)
- SCORM/LMS
- payment collection
- certified mass-notification
- real-time co-editing (GA)
- review-sets eDiscovery
- becoming a QTSP/ESP ourselves

Changing any of them requires the change process ([21 §17](21-FEATURE-CATALOGUE.md)).
