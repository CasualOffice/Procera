# Procera — documentation pack v0.2

Date: 2026-10-11 · Status: **design-complete for G1 review; no code yet** · Supersedes the
v0.1 blueprint (archived verbatim in [`archive/v0.1/`](archive/v0.1/)).

## How to read this pack

| If you are… | Read in this order |
|---|---|
| Product owner / stakeholder | 02 PRD → 21 Feature catalogue → 10 Competitive → 07 Roadmap → 22 Open questions |
| Architect / tech lead | 09 Gap analysis → 01 Research & ADRs → 03 HLD → 04 LLD → 11 IR → 12 Access → adr/ |
| Backend engineer | 04 LLD → 11 IR → 12 Access → 08 API → 06 Security → 15 Tests |
| Frontend / designer | 05 UX & design → 02 PRD §5.8–5.10 → 21 → 19 Templates → 20 Config |
| Security / compliance | 06 Security & ops → 14 Data governance & compliance → 12 Access → 16 AI |
| Delivery / PM | 07 Roadmap → 17 Tracker (+ xlsx) → 22 Register → 23 E2E → 15 Tests |

## Documents

| # | Document | What it settles |
|---|---|---|
| 00 | This index | — |
| 01 | [Research & decisions](01-RESEARCH-AND-DECISIONS.md) | Verified engine/UI/infra landscape and licences; ADR summary; spikes SP-01…SP-10 |
| 02 | [PRD](02-PRD.md) | Vision, audience (startup→MNC), personas, every functional requirement with acceptance criteria, journeys J0–J14, NFRs |
| 03 | [HLD](03-HLD.md) | Planes, bounded contexts, processes, topologies, key flows, isolation layers |
| 04 | [LLD](04-LLD.md) | Code layout, full schema with RLS, state machines, algorithms, data-grid engine, keys |
| 05 | [UX & design](05-UX-AND-DESIGN.md) | IA, screen specs, DataGrid, dashboards, design system and WCAG tokens, anti-patterns |
| 06 | [Security & operations](06-SECURITY-AND-OPERATIONS.md) | Threat model, protocols, crypto, SDLC, SLOs, DR, SEC-01…SEC-37 |
| 07 | [Roadmap & releases](07-ROADMAP-AND-MILESTONES.md) | Phased usable releases P0 → R1–R5 → GA (Oct 2027), exit checklists, AI-assisted delivery model, team, scope cuts |
| 08 | [API spec](08-API-SPEC.md) | Conventions, errors, resource map, events, examples |
| 09 | [Gap analysis](09-GAP-ANALYSIS.md) | What v0.1 lacked and where v0.2 closes it |
| 10 | [Competitive analysis](10-COMPETITIVE-ANALYSIS.md) | Market map, capability matrix, positioning, pricing anchors |
| 11 | [Workflow IR spec](11-WORKFLOW-IR-SPEC.md) | Node kinds, variables, expressions, forms, rules, SLAs, BPMN subset, compiler checks |
| 12 | [Tenancy, identity & access](12-AUTHORIZATION-MODEL.md) | Workspace tenancy, system plane, permission catalogue, roles, delegation, SoD |
| 13 | [Integrations](13-INTEGRATIONS.md) | Standalone baseline, ports, connector SDK, phased catalogue, chat/host-platform pattern (P2) |
| 14 | [Data governance & compliance](14-DATA-GOVERNANCE-AND-COMPLIANCE.md) | ESIG/AUD/CMP/QMS/PRV/A11Y/WH/SSRF requirement ids; erasure vs immutability |
| 15 | [Test strategy & conformance](15-TEST-STRATEGY-AND-CONFORMANCE.md) | Conformance suite, model-based tests, chaos, CI gates, T01–T40 |
| 16 | [AI assistance](16-AI-ASSISTANCE.md) | Governed, optional AI; invariants; EU AI Act posture |
| 17 | [Execution tracker](17-EXECUTION-TRACKER.md) | Milestones, 99 core epics, risks, decisions, RACI (generated) |
| 18 | [Glossary](18-GLOSSARY.md) | Canonical terms and code names |
| 19 | [Process template library](19-PROCESS-TEMPLATE-LIBRARY.md) | ~55 industry-standard templates, package format, Wave 1 |
| 20 | [Workspace configuration](20-WORKSPACE-CONFIGURATION.md) | Profiles, settings layering, full settings catalogue, config-as-code |
| 21 | [Feature catalogue](21-FEATURE-CATALOGUE.md) | **The complete system inventory**, with priority and milestone; change process |
| 22 | [Unknowns, assumptions & open questions](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) | Every open item with owner and decide-by |
| 23 | [E2E test plan](23-E2E-TEST-PLAN.md) | Harness, test-only controls, 74 scenarios across R1–R5 + GA, CI and flake policy |
| 24 | [Engineering rules](24-ENGINEERING-RULES.md) | **Binding** rules: design-first, tracker updates, architecture/code, tests, security, UX, git/PR, docs, releases, AI agents |
| adr/ | [Architecture decision records](adr/README.md) | ADR-0001…0019 |
| — | [`../assets/brand/`](../assets/brand/README.md) | Brand copy, palette with contrast, logo files and usage rules |
| — | [`../trackers/README.md`](../trackers/README.md) | **Tracker dashboard**: progress, current release, active items, release checklists (generated) |
| — | [`../trackers/Delivery_Tracker.xlsx`](../trackers/Delivery_Tracker.xlsx) | Backlog, milestones, risks, decisions, tests, OQs, RACI, burndown |

## Product direction (owner decisions, 2026-10-11)

1. Standalone and independent first. Integrations come later.
2. Multi-tenant: **workspace = tenant**. A System Admin plane manages all workspaces.
3. Configurable, role-based access per workspace (built-in templates + custom roles).
4. Workspace admins can see and control everything in their workspace, with dashboards, data tables, monitoring, complete audit trails and investigations.
5. Pre-designed, industry-standard process templates.
6. File handling as a first-class capability.
7. For every organisation size and department. Includes policies, employee document signing, and announcements with acknowledgements.
8. Industry-leading, easy yet powerful UX. It must not look AI-generated or templated, and must follow WCAG 2.2 AA with configurable branding.
9. A proper LLD and proper security protocols.
10. Thorough research so nothing is a wild card. That is why this pack includes the catalogue (21) and the register (22).
11. Build with Claude Code and other AI agents. Ship in usable phases (R1 in Jan 2027). Gate every release with end-to-end tests (07 v0.3, 23).
12. **PostgreSQL** is the database, not MariaDB (ADR-0002). **Valkey/Redis** is the cache (ADR-0019).

## Key principle (kept from v0.1)

A process **model** ≠ a published **definition** ≠ a running **instance** ≠ a governed **SOP/policy** ≠ an ad hoc **case**. Keep these boundaries everywhere.
