# Delivery plan, staffing, risks and release gates

## Delivery strategy
Time ranges are indicative for a dedicated cross-functional team (2 Rust backend, 2 frontend, 1 QA/SDET, shared DevOps/product/design/security). They are not delivery promises. Prefer a modular monolith for catalog/governance/API and separate background workers; only split services after measuring contention and ownership needs.

### Phase 0 — Discovery and spikes (weeks 1–4)
Interview 8–12 users across HR, IT, finance, compliance; define 5 reference processes; research license matrix; prototype IR and modeler; benchmark native Rust vs Operaton/Flowable/Temporal adapter; produce ADR signoff. Exit: workflow semantics test suite, license review and selected runtime approach.

### Phase 1 — Governance-first alpha (weeks 5–12)
Tenant/workspace identity, catalog, process maps, SOP versioning, ownership/RACI, search, reviews, templates, audit, starter UI. Exit: business owner publishes reviewed SOP with audit trail; role isolation tests pass.

### Phase 2 — Execution beta (weeks 13–24)
Graph designer, typed forms, approvals, branches, durable instances, inbox, timers, escalation, outbox, notification connector, analytics basics, idempotency. Exit: 3 real internal processes live; restart/race tests pass; no critical security defects.

### Phase 3 — Enterprise release candidate (weeks 25–36)
SSO/SCIM, governance controls, case management baseline, integration framework, reporting, backups/restore, HA/load testing, docs and accessibility audit. Exit: tenant isolation verified, RPO/RTO rehearsal, disaster recovery and UAT acceptance.

### Phase 4 — Expansion (months 10–18+)
Advanced BPMN/DMN, process simulation, process mining, marketplace, AI-assisted design, version migration tools, industry packs. Gate each against real customer demand.

## Release gates
G0 Product charter approved; G1 domain model and runtime ADR approved; G2 governance alpha with SOP audit; G3 workflow beta correctness suite; G4 security/HA/compliance evidence; G5 general availability readiness.

## Critical dependencies
Identity/authorization before tenant features; immutable versions before runtime; transactional outbox before external side effects; forms and work inbox before human process pilot; event schema before analytics; retention model before compliance exports.

## Top risks
R1 execution correctness (high): formalize transition semantics and property-based tests.
R2 scope creep/full BPMN (high): subset + compatibility matrix.
R3 security/cross-tenant access (critical): tenant-safe keys and continuous negative testing.
R4 governance adoption (medium): templates and owner accountability.
R5 engine lock-in (high): versioned IR and adapter boundary, migration spike.
R6 third-party licenses (high): dependency-level review before adoption.
R7 workflow migration (high): immutable running versions; migration tools later.
R8 connector side effects (high): idempotency and compensation contracts.
R9 process analytics inaccuracies (medium): explicit event semantics and sample size.
R10 staffing/UX quality (medium): dedicated UX research and phased rollout.

## Definition of done
PRD acceptance criterion satisfied; schema migration and rollback strategy; unit/integration/property/load tests; security review; observability; user-facing help; localization and accessibility checks; migration/backup behavior documented; no unresolved critical defects.

## Suggested internal pilot processes
Purchase approval; employee onboarding; access provisioning/deprovisioning; policy/SOP recertification; incident exception investigation. Each has owner, success KPI, initial SOP and target baseline.
