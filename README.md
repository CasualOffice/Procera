<p align="center">
  <img src="assets/brand/marketing/procera-banner.png" alt="Procera — Turn processes into progress. Design · Execute · Govern · Improve">
</p>

<p align="center">
  <b>Turn processes into progress.</b><br>
  Open-source business process management for modeling, automating, governing and improving workflows,<br>
  where the approved SOP is the running process.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="Status: design phase" src="https://img.shields.io/badge/status-design%20phase-orange.svg">
  <img alt="Rust + PostgreSQL" src="https://img.shields.io/badge/stack-Rust%20%C2%B7%20PostgreSQL%20%C2%B7%20React-0F172A.svg">
  <img alt="WCAG 2.2 AA" src="https://img.shields.io/badge/a11y-WCAG%202.2%20AA-067647.svg">
</p>

---

> **Status: design phase.** This repository currently holds Procera's complete design and planning pack: PRD, system design, LLD, security, UX, test plans, roadmap and trackers. Implementation starts after the P0 architecture gate, and the first usable release (**R1, Jan 2027**) follows. See the [roadmap](docs/07-ROADMAP-AND-MILESTONES.md).

## What is Procera?

Procera is a modern, open-source business process management platform. It helps organizations design, automate, govern and continuously improve their business processes, end to end. It is built for any size, from a 10-person startup to a multinational, and for every department.

| **Design** | **Execute** | **Govern** | **Improve** |
|---|---|---|---|
| Model and document processes, SOPs and policies | Automate work across people and systems | Ensure compliance, control and visibility | Gain insights and continuously optimize |

In one place, Procera brings together:

- **Policies & SOPs** on a governed lifecycle (draft → review → approved → scheduled → published → retired), with segregation of duties, version diffs, review dates and PDF export.
- **Workflows without code.** A step builder, typed forms, approvals (sequential, parallel, quorum, approval-limit matrices), SLAs on business calendars, delegation and out-of-office, recall and send-back.
- **Employee document signing.** Bulk send to groups, accessible signing on any device, and a certificate of completion.
- **Announcements & acknowledgements.** Targeted, scheduled, mandatory reads with reminders and manager escalation.
- **Cases & investigations.** Flexible cases, restricted HR cases, evidence chain of custody, legal hold.
- **Admin control centre.** Dashboards, and every list as a sortable, filterable data grid with saved views and export.
- **Complete audit trail.** Every action and every sensitive read is recorded in a tamper-evident hash chain, with an audit explorer and verifiable evidence exports.
- **Industry-standard templates.** About 55 ready-made processes across HR, Finance, IT, GRC, Quality, Legal and Operations, plus industry packs.

Procera is **standalone**. It needs only PostgreSQL, S3-compatible storage and SMTP. Integrations (SSO/SCIM, webhooks, chat platforms, HRIS, ERP, qualified e-signatures) are optional layers added later.

## Why Procera

- **Governance and execution live in separate tools today.** Process documentation suites don't run the work, and workflow tools don't govern it. Procera does both, in the open.
- **The open-source BPM field has emptied.** Major OSS engines have gone end-of-life or source-available, and the remaining ones ship without a business UI ([analysis](docs/10-COMPETITIVE-ANALYSIS.md)).
- **Self-hosting and sovereignty.** Apache-2.0, workspace-level isolation and encryption keys, air-gapped installs, and no telemetry by default.

## Highlights of the design

| Area | Approach |
|---|---|
| Multi-tenancy | **Workspace = tenant**, with a System Admin console over all workspaces. Isolation is enforced in the app, by PostgreSQL row-level security and by per-workspace encryption keys. |
| Access control | A permission catalogue, built-in role templates and **custom roles per workspace**, scoped to departments or collections, evaluated with Cedar policies |
| Runtime | A versioned workflow IR, pinned release bundles, a transactional outbox, idempotent commands and leased timers. Deterministic and conformance-tested. |
| Data | **PostgreSQL 16+** is the only system of record ([why not MariaDB](docs/adr/ADR-0002-postgres-system-of-record.md)). **Valkey/Redis** is a cache only. Any S3-compatible object store. |
| Security | OWASP ASVS 5.0 L2 (L3 for auth, sessions and crypto), MFA/passkeys, step-up auth, Standard Webhooks, an SSRF egress proxy, malware scanning, signed releases with SBOMs |
| UX | Easy for everyone, powerful for owners. WCAG 2.2 AA design tokens, configurable branding with contrast enforcement, a calm editorial look (no generic "AI-SaaS" styling). |
| Quality | Unit, property, conformance and chaos tests, plus **end-to-end test packs that gate every release** |

## Roadmap

| Release | Target | What you can do |
|---|---|---|
| P0 | Nov 2026 | Architecture gate, design system, test harness |
| **R1: Workspace & Policies** | **Jan 2027** | Policy/SOP library, announcements with acknowledgements, files, audit, admin dashboard |
| R2: Requests & Approvals | Mar 2027 | Leave, expense, purchase, access and onboarding workflows |
| R3: Operate & Sign | May 2027 | Full admin console, approval-limit matrices, employee e-signing, all Wave-1 templates |
| R4: Enterprise Connect | Jul 2027 | SSO/SCIM, scoped admins, webhooks/API, email approvals, external signers |
| R5: Cases & Compliance | Aug 2027 | Cases, investigations, legal hold, regulated-industry signatures, reporting |
| **1.0** | **Oct 2027** | Hardened, audited, multi-language GA |

Dates are indicative. Every release is installable, upgradeable in place and gated by its [E2E pack](docs/23-E2E-TEST-PLAN.md).

## Documentation

Start at **[docs/00-INDEX.md](docs/00-INDEX.md)**.

| Topic | Docs |
|---|---|
| Product | [PRD](docs/02-PRD.md) · [Feature catalogue](docs/21-FEATURE-CATALOGUE.md) · [Templates](docs/19-PROCESS-TEMPLATE-LIBRARY.md) · [Competitive analysis](docs/10-COMPETITIVE-ANALYSIS.md) |
| System design | [HLD](docs/03-HLD.md) · [LLD](docs/04-LLD.md) · [Workflow IR](docs/11-WORKFLOW-IR-SPEC.md) · [Access model](docs/12-AUTHORIZATION-MODEL.md) · [API](docs/08-API-SPEC.md) · [ADRs](docs/adr/README.md) |
| Security & compliance | [Security & operations](docs/06-SECURITY-AND-OPERATIONS.md) · [Data governance & compliance](docs/14-DATA-GOVERNANCE-AND-COMPLIANCE.md) · [AI assistance](docs/16-AI-ASSISTANCE.md) |
| UX | [UX & design system](docs/05-UX-AND-DESIGN.md) · [Workspace configuration](docs/20-WORKSPACE-CONFIGURATION.md) |
| Delivery | **[Tracker dashboard](trackers/README.md)** · [Roadmap](docs/07-ROADMAP-AND-MILESTONES.md) · [Tracker detail](docs/17-EXECUTION-TRACKER.md) ([xlsx](trackers/Delivery_Tracker.xlsx)) · [Test strategy](docs/15-TEST-STRATEGY-AND-CONFORMANCE.md) · [E2E plan](docs/23-E2E-TEST-PLAN.md) · [Open questions](docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) |
| Ways of working | [Engineering rules](docs/24-ENGINEERING-RULES.md) · [Contributing](CONTRIBUTING.md) · [Governance](GOVERNANCE.md) |

## Repository layout

```text
docs/        numbered design pack, adr/, archive/v0.1/
assets/      brand/ — logos, icons, favicons, banner, palette (see assets/brand/README.md)
trackers/    README.md dashboard + Delivery_Tracker.xlsx + CSV exports (generated, do not hand-edit)
scripts/     build_tracker.py (regenerates the tracker from docs)
.github/     PR/issue templates, CODEOWNERS
crates/      Rust library crates (kernel; domain/infra crates added per tracker item)
apps/        procera binary (api · worker · scheduler · admin CLI)
# coming: web/ e2e/ conformance/ migrations/ deploy/
```

## Contributing

Contributions are welcome, from humans and from AI-assisted workflows. Please read:
- [CONTRIBUTING.md](CONTRIBUTING.md)
- the binding [engineering rules](docs/24-ENGINEERING-RULES.md)
- the [Code of Conduct](CODE_OF_CONDUCT.md)

Every commit is DCO signed-off. Security issues go privately via [SECURITY.md](SECURITY.md).

## License

Procera's source code is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE). The Procera name, logo and brand assets in [`assets/brand/`](assets/brand/README.md) are trademarks of CasualOffice and are **not** covered by the Apache licence ([governance](GOVERNANCE.md#7-trademark)).
