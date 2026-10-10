# 09 — Gap analysis of the v0.1 blueprint

Date: 2026-10-11 · Input: the v0.1 pack (archived verbatim in [`archive/v0.1/`](archive/v0.1/)) ·
Output: this v0.2 pack.

The v0.1 pack is a strong *architecture proposal*: its central principle (model ≠
definition ≠ instance ≠ SOP ≠ case), its reliability stance (transactional outbox,
idempotency, version pinning, at-least-once side effects) and its honesty about unverified
claims are all kept. What it lacked was the material an engineering team needs to start
building without re-deciding things: a specified execution language, an authorization
model, an integration contract, a reconciled lifecycle, a complete data model, and a
tracker that can actually be scheduled.

Each gap below is rated **Critical** (blocks Phase 1/2 start), **High** (blocks a phase
exit gate) or **Medium** (needed before GA), and points at the v0.2 document that closes it.

## A. Contradictions and inconsistencies in v0.1

| # | Finding | Sev | Resolution |
|---|---|---|---|
| A1 | Two different governance lifecycles: PRD FR-G04 `Draft > Review > Approved > Published > Retired` vs UX doc `Idea > Draft > In review > Approved > Scheduled > Published > Deprecated > Retired`. | Critical | One canonical artifact lifecycle with `Scheduled` and `Deprecated`; `Idea` becomes a separate *improvement proposal* object, not a lifecycle state. [04-LLD §3](04-LLD.md), [05-UX §4](05-UX-AND-DESIGN.md). |
| A2 | Human-work state `REJECTED` conflates a *work-item state* with a *decision outcome*. A rejection is a completed work item whose outcome is `rejected`. | High | Work item states: `OFFERED, CLAIMED, DELEGATED, COMPLETED, CANCELLED, EXPIRED`; outcome is a separate typed field. [04-LLD §4](04-LLD.md). |
| A3 | Instance state machine missing transitions: `CREATED → CANCELLED`, `FAILED → RUNNING` (retry), `WAITING → COMPLETED` is impossible but `WAITING → RUNNING → COMPLETED` is implied, no `INCIDENT` concept for ops. | High | Full transition table with guards; failures raise **incidents** attached to tokens, instance remains `RUNNING` unless policy says `FAILED`. [04-LLD §4](04-LLD.md). |
| A4 | Kafka is "the" async integration bus (ADR-002) yet the Compose trial lists only a "broker adapter"; Kafka is heavy for a self-hosted trial. | High | Broker is optional. Default: Postgres outbox + in-process dispatcher (`LISTEN/NOTIFY` wakeups). Kafka / NATS JetStream adapters for scale. [ADR-0009](adr/ADR-0009-broker-optional.md). |
| A5 | "Simulate" appears in the designer toolbar and FR-I02, but simulation is a P2 item. | Medium | Split: **test run** (P0, dry-run of a draft with fake identities, no side effects) vs **statistical simulation** (P2). [02-PRD FR-E11, FR-I04](02-PRD.md). |
| A6 | `RACI` sample says Analyst is `R` for "Approve controlled SOP" — an analyst cannot be responsible for approving their own work under segregation of duties. | Medium | Corrected matrix. [05-UX §5](05-UX-AND-DESIGN.md). |
| A7 | The tracker's priorities don't match the PRD: PRD P0 includes G05 (diffs/change requests) but no backlog item covers semantic diff; I01 is P0 but its acceptance is "accurate SLA dashboards" with no SLA model. | High | Tracker rebuilt from PRD IDs, 1:1 traceable. [17-EXECUTION-TRACKER](17-EXECUTION-TRACKER.md). |

## B. Missing specifications (engineering cannot start without these)

| # | Gap | Sev | Resolution |
|---|---|---|---|
| B1 | **No workflow IR specification.** Only a 3-node sample. Node kinds, edge semantics, variable typing, error/boundary events, loops, subprocess calls, versioning and canonical serialization are all unspecified. | Critical | [11-WORKFLOW-IR-SPEC](11-WORKFLOW-IR-SPEC.md) — full IR v1. |
| B2 | **No expression language chosen.** Conditions, assignment rules, rule tables and SLA formulas all need one; "no arbitrary code evaluation" is a constraint, not a choice. | Critical | [ADR-0010](adr/ADR-0010-expression-language.md): a typed, total, side-effect-free expression language (FEEL subset vs CEL decided by spike SP-08). |
| B3 | **No authorization model.** Personas are listed, but there is no permission catalogue, no policy engine, no ABAC attributes, no field-level policy format, no delegation rules. | Critical | [12-AUTHORIZATION-MODEL](12-AUTHORIZATION-MODEL.md) + [ADR-0011](adr/ADR-0011-authorization-engine.md). |
| B4 | **No organization model.** "Manager" assignment requires a reporting line; "role/group" requires groups. There are no users, groups, org units, positions or manager tables. | Critical | Org model + identity tables in [04-LLD §2](04-LLD.md); sources: SCIM, HRIS connector, chat-platform directory, manual. |
| B5 | **No form schema specification**, and no rule for how forms are versioned relative to process definitions. | Critical | JSON Schema 2020-12 data schema + UI schema; forms are pinned into the **release bundle**. [11-IR §7](11-WORKFLOW-IR-SPEC.md), [ADR-0012](adr/ADR-0012-release-bundle.md). |
| B6 | **Dependent-artifact pinning undefined.** A definition is pinned, but the forms, rule tables, calendars, connector configs and SOP it references can change underneath a running instance. | Critical | **Release bundle**: an immutable, content-addressed manifest of every artifact version a definition depends on. [ADR-0012](adr/ADR-0012-release-bundle.md). |
| B7 | **No business-calendar model** although timers and SLAs depend on business days. | High | `business_calendars` + versioned holiday sets, pinned at timer creation. [04-LLD §2](04-LLD.md). |
| B8 | **No SLA model** (what is measured, start/stop/pause clocks, breach actions). | High | SLA definitions on process/step with pause states; breaches as events. [02-PRD FR-I01](02-PRD.md), [04-LLD](04-LLD.md). |
| B9 | **No API error model, versioning policy, rate limiting or OpenAPI plan.** | High | RFC 9457 problem details, URL major version + additive evolution, OpenAPI 3.1 generated from code. [08-API-SPEC](08-API-SPEC.md). |
| B10 | **Audit log is under-specified** and conflated with `instance_events`. | High | Separate `audit_log` with per-tenant hash chain and periodic anchored checkpoints; instance events are domain history, audit is accountability. [04-LLD §6](04-LLD.md), [14-DATA-GOVERNANCE](14-DATA-GOVERNANCE-AND-COMPLIANCE.md). |
| B11 | **No ops "incident" model** for failed jobs/connectors: who sees them, how to retry, DLQ admin. | High | Incidents table + operator console. [04-LLD §5](04-LLD.md), [05-UX](05-UX-AND-DESIGN.md). |
| B12 | **Instance variables stored as one `jsonb` blob** while FR-E03 requires field-level privacy. | High | Variables split by classification; `restricted` fields envelope-encrypted and policy-gated; size limits. [04-LLD §2](04-LLD.md). |
| B13 | **Tenancy hierarchy undefined** (organization vs workspace). | High | `organization → workspace`; organization is the isolation boundary, workspaces partition catalog/admin. [12-AUTHZ §2](12-AUTHORIZATION-MODEL.md). |
| B14 | **Electronic signature semantics undefined** ("signed decision audit"). | High | Re-authentication at signing, signature meaning, record-signature linkage (Part 11-ready mode). [14-DATA-GOVERNANCE §3](14-DATA-GOVERNANCE-AND-COMPLIANCE.md). |

## C. Missing product scope (buyers expect these)

| # | Gap | Sev | Resolution |
|---|---|---|---|
| C1 | **Integration contract absent**: v0.1's "deep optional integration" was a single connector line. | Medium (re-rated: owner decided standalone-first; chat/platform integrations are P2) | [13-INTEGRATIONS](13-INTEGRATIONS.md): identity federation, embed/host contract, actionable approval cards, notification API, directory sync. |
| C2 | **Notifications** are 4 lines: no channel model, templates, digests, email-actionable approvals, localisation. | High | FR-N01–N06. [02-PRD](02-PRD.md). |
| C3 | **Out-of-office / vacation delegation** not covered (only per-item delegation). | High | FR-E04 extended; delegation rules with scope + expiry. |
| C4 | **Template gallery** mentioned in success metrics but never specified. | High | FR-G08 + starter packs (5 pilot processes). |
| C5 | **Import/migration**: no BPMN/Visio/Excel/CSV import of existing landscapes and SOPs; no `.docx` SOP import. | High | FR-G09; `.docx` import via the CasualOffice OpenDoc engine (optional). |
| C6 | **Mobile approvals** asked as an open question only. | High | Responsive PWA in P0 for My Work; native/offline out of scope. FR-P07. |
| C7 | **Reporting/exports** vague. | Medium | Fixed P0 reports + CSV/XLSX export; report builder P2. FR-I05. |
| C8 | **Permission-aware search** not specified. | Medium | Postgres FTS with policy filter pushed into SQL. [03-HLD](03-HLD.md). |
| C9 | **AI features** are a single P2 line. | Medium | [16-AI-ASSISTANCE](16-AI-ASSISTANCE.md): scoped assistants, provider abstraction incl. local models, EU AI Act posture. |
| C10 | **SOP content format undefined** (rich text? structured? docx?). | High | Structured block model (portable JSON) as the source of truth, PDF/DOCX export; optional OpenDoc embedding. [ADR-0013](adr/ADR-0013-sop-content-model.md). |
| C11 | **Read-and-understood / training acknowledgement** semantics (quiz, per-version re-ack) missing. | Medium | FR-G03 extended. |
| C12 | **Localization**: languages, RTL, per-tenant terminology. | Medium | FR-P04 extended; English first, ICU MessageFormat. |

## D. Missing delivery / business material

| # | Gap | Sev | Resolution |
|---|---|---|---|
| D1 | **Tracker is a 22-row list** with no estimates, dependencies, milestones, DoD, owners, or traceability to tests and risks. | Critical | [17-EXECUTION-TRACKER](17-EXECUTION-TRACKER.md) + rebuilt `trackers/Delivery_Tracker.xlsx` (backlog, milestones, roadmap, risks, decisions, tests, open questions, RACI). |
| D2 | **No milestones** inside phases; gates G0–G5 have no measurable checklist. | Critical | [07-ROADMAP-AND-MILESTONES](07-ROADMAP-AND-MILESTONES.md): M0–M12 with exit checklists. |
| D3 | **Schedule realism**: Phase 2 (designer + runtime + forms + timers + inbox + outbox + connectors + metrics) in 12 weeks with 2 Rust engineers is not credible. | High | Re-planned; Phase 2 split into 2a (runtime core) and 2b (pilot surface), +6 weeks buffer; explicit scope-cut list. |
| D4 | **Licensing / business model** undecided (open core? hosted? CLA vs DCO? trademark). | High | Open question OQ-06 with a recommendation; [ADR-0014](adr/ADR-0014-licensing-and-contribution.md). |
| D5 | **Infra license traps not flagged** (object storage and cache defaults changed licence in 2024–2025). | High | [01-RESEARCH §4](01-RESEARCH-AND-DECISIONS.md); S3 API only, no bundled server dependency in the product. |
| D6 | **No developer environment / CI / release engineering plan** (monorepo layout, CI gates, SBOM, signing, upgrade/migration testing). | High | [03-HLD §11](03-HLD.md), [15-TEST-STRATEGY](15-TEST-STRATEGY-AND-CONFORMANCE.md), [CLAUDE.md](../CLAUDE.md). |
| D7 | **Success metrics lack instrumentation and baselines.** | Medium | Product analytics plan (opt-in for self-hosted) in [02-PRD §9](02-PRD.md). |
| D8 | **No formal verification plan** for the runtime despite R1 being the top risk. | High | Model-based testing (state-machine models + property tests) and a deterministic simulation harness; optional TLA+ for the join/timer protocol. [15-TEST-STRATEGY](15-TEST-STRATEGY-AND-CONFORMANCE.md). |
| D9 | **Unverified research claims** in v0.1 (e.g. engine release status) were stated as fact. | Medium | Re-verified 2026-10-11 with sources in [01-RESEARCH](01-RESEARCH-AND-DECISIONS.md). |

## F. Gaps added by the owner's direction (2026-10-11)

| # | Direction | Closed in |
|---|---|---|
| F1 | Standalone first; integrations later | [ADR-0016](adr/ADR-0016-standalone-first.md), [13](13-INTEGRATIONS.md) |
| F2 | Workspace = tenant + System Admin plane | [12 §2/§4](12-AUTHORIZATION-MODEL.md), [ADR-0008](adr/ADR-0008-workspace-tenancy.md), FR-S01–S10 |
| F3 | Configurable roles per workspace | [12 §5–6](12-AUTHORIZATION-MODEL.md), FR-W03, [20](20-WORKSPACE-CONFIGURATION.md) |
| F4 | Industry-standard templates | [19](19-PROCESS-TEMPLATE-LIBRARY.md), FR-G08 |
| F5 | Files; admin sees all; dashboards, data tables, audit & investigations | FR-F*, FR-A*, FR-V*, FR-T*, [04 §2.9–2.10, §5](04-LLD.md) |
| F6 | All org sizes and departments; policies, e-signing, announcements + acks | [02 §1.1, §5.6a](02-PRD.md), [ADR-0017](adr/ADR-0017-unified-campaign-engine.md) |
| F7 | Leading, non-AI-looking UX; WCAG tokens; configurable branding | [05 §7](05-UX-AND-DESIGN.md) |
| F8 | Proper LLD and security protocols | [04](04-LLD.md), [06 §3–4](06-SECURITY-AND-OPERATIONS.md) |
| F9 | Thorough research, no wild cards | [21](21-FEATURE-CATALOGUE.md) (two parity sweeps), [22](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) |

## G. Gaps found by the feature-parity sweeps (2026-10-11)

These are ~35 features that leaders ship and buyers expect, all of which were missing from v0.1 and from the first v0.2 draft. Examples:
- DoA approval matrix
- recall, send-back and edit-after-submit
- request on behalf
- master-data tables
- request portal
- envelope correction
- ESIGN consent step
- policy renewal dates
- post expiry
- form prefill and calculations
- restricted cases
- legal hold
- CSAT

Each has an `FX-` id, a priority and a milestone in [21 §16](21-FEATURE-CATALOGUE.md#16-gap-sweep-traceability).

## E. What v0.2 deliberately does *not* decide

These stay open because they are business decisions, not engineering ones. Each has a
recommendation and a "decide by" milestone in [17-EXECUTION-TRACKER §Open questions](17-EXECUTION-TRACKER.md#open-questions):
public brand/name, hosted-offering pricing, first regulated industry, single- vs multi-org
deployments for the first customer, and the runtime choice (decided by spikes at M1/G1).
