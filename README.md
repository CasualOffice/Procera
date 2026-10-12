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
  <img alt="Status: pre-release, 0.1 in progress" src="https://img.shields.io/badge/status-pre--release%20%C2%B7%200.1-orange.svg">
  <img alt="Rust + PostgreSQL" src="https://img.shields.io/badge/stack-Rust%20%C2%B7%20PostgreSQL%20%C2%B7%20React-0F172A.svg">
  <img alt="WCAG 2.2 AA" src="https://img.shields.io/badge/a11y-WCAG%202.2%20AA-067647.svg">
</p>

---

> **Status: pre-release.** Procera 0.1 is being polished: the governed-document loop below works end to end in the browser and is covered by end-to-end tests, but there is no packaged release yet. Build from source to try it. Website: <https://casualoffice.github.io/Procera/>

## What works today (0.1)

```
Write a policy or SOP → review (separation of duties) → approve → publish → everyone acknowledges → audit
```

- **Library:** policies and SOPs with a block editor, versions with word-level compare, review cycles, collections, applicability, starter template packs, Word/HTML/Markdown export.
- **Review and publish:** reviewers never approve their own work, step-up for controlled documents, scheduled go-live, reason for change.
- **Acknowledgements:** campaigns bound to the exact version, reminders and manager escalation, new joiners included, evidence export; announcements with confirm-by dates.
- **Audit:** every change in a tamper-evident hash chain, an audit explorer with filters, timelines and exports.
- **Running a workspace:** guided setup, people, departments and groups, roles (built-in, custom and job presets), settings by category, branding, encryption keys, retention; leaving the organisation hands documents and reports over and closes their acknowledgements as "left".
- **Files:** attachments with malware scanning, in-app preview of PDF, Word and spreadsheet files.
- **Accounts:** passwords with reset links, two-step verification (authenticator apps, passkeys), admin account controls; invitation links can be copied when email isn't set up.
- **Self-hosting:** Docker Compose stack and guides, email (SMTP) set up from the system console, many workspaces on one server, PostgreSQL row-level isolation per workspace.
- **Works on phones:** a bottom bar for the everyday pages.

- **Requests and approvals (0.2, first slice):** leave, expense, purchase and access requests with forms; approvals routed to the requester's manager, finance, the CFO or IT; a My Work inbox to approve, reject or send back; a request page with steps and timeline. Designing your own workflows, e-signing and cases are planned, **not built yet**.
- **Team workload (0.2):** managers get a Team page (their reporting line from the directory): open, overdue and due-this-week steps per person, who is away holding work, and bulk reassign within the team; admins see everyone under Requests oversight › Workload.
- **Out of office and request settings (0.2):** set an out-of-office with a delegate from the account menu (your avatar shows it, and colleagues see it on hover cards and Members) (or an admin or manager sets it for you); new approvals go to the delegate, who can also decide what's already waiting, recorded "on behalf of". Admins choose which request types are offered, who can ask for each, and the currency amounts are in.
- **Why this route (0.2):** every request page explains in plain words why it went where it went ("Sent to Finance approval because the amount 640.00 EUR is over 500") and why each person decides; approvers see why a step is theirs.
- **Who approves, attachments and policy (0.2):** the request form says who will approve before you send ("Olivia Owner (your manager), then Finance if the amount is over 500 EUR"); every request type takes files (receipts, quotes, certificates, photos), scanned and previewable by the people on the request, and approvers can attach files with their decision; a request type can link its governing policy, and each decision records the version the approver saw.
- **Reminders and escalations (0.2):** approvers are reminded before and at the due date; an overdue manager approval moves up to the next manager; admins see unassigned and overdue steps under Admin › Requests oversight and reassign them with a reason.

**Evidence, not just status:** everything above is covered by automated end-to-end tests that run on every full test pass; nothing has been validated in a pilot with outside users yet. The [tracker](trackers/README.md) shows status and evidence separately.

## Try it

Needs Rust (see `rust-toolchain.toml`), Node 20+ and PostgreSQL 16 binaries on your PATH.

```bash
./scripts/demo.sh        # throwaway database + API + web app, seeded "Acme" workspace; prints sign-in details
./scripts/test-all.sh    # the full gate: fmt, clippy, Rust tests, web checks, browser end-to-end tests
```

## Self-hosting

Run Procera on your own server with Docker Compose (PostgreSQL 16, the server and the web app; Valkey, ClamAV and a test mail inbox are optional). Files are stored on local disk today; S3-compatible storage is not built yet.

```bash
cp deploy/.env.example deploy/.env      # fill in the passwords and the master key
docker compose -f deploy/compose.yml up -d --build
docker compose -f deploy/compose.yml logs procera | grep bootstrap_token   # then open /setup
```

- [Install with Docker Compose](docs/guides/install-docker-compose.md): prerequisites, HTTPS, backups, upgrades, troubleshooting
- [First-time setup](docs/guides/first-time-setup.md): from the setup token to your first published policy
- [Configuration](docs/guides/configuration.md): every server option, settings in the app, security notes

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

## Design highlights

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

| Version | What it adds |
|---|---|
| **0.1** (done) | The governed-document loop above, polished for small companies |
| **0.2** (now) | Requests and approvals: built-in leave, expense, purchase and access requests, reminders, escalations, out of office and request settings are in; onboarding next |
| 0.3 | Visual workflow designer and template gallery |
| 0.4 | Automation and integrations: SSO/SCIM, webhooks and API, email approvals, e-signing |
| 0.5 | Analytics and improvement: reports, process insights, cases and investigations |
| **1.0** | Hardened, audited, multi-language |

Live progress: the [tracker dashboard](trackers/README.md) and the [website roadmap](https://casualoffice.github.io/Procera/roadmap.html). Each release is gated by its [end-to-end pack](docs/23-E2E-TEST-PLAN.md).

## Documentation

Start at **[docs/00-INDEX.md](docs/00-INDEX.md)**.

| Topic | Docs |
|---|---|
| Product | [PRD](docs/02-PRD.md) · [Feature catalogue](docs/21-FEATURE-CATALOGUE.md) · [Templates](docs/19-PROCESS-TEMPLATE-LIBRARY.md) · [Competitive analysis](docs/10-COMPETITIVE-ANALYSIS.md) |
| System design | [HLD](docs/03-HLD.md) · [LLD](docs/04-LLD.md) · [Workflow IR](docs/11-WORKFLOW-IR-SPEC.md) · [Access model](docs/12-AUTHORIZATION-MODEL.md) · [API](docs/08-API-SPEC.md) · [ADRs](docs/adr/README.md) |
| Security & compliance | [Security & operations](docs/06-SECURITY-AND-OPERATIONS.md) · [Data governance & compliance](docs/14-DATA-GOVERNANCE-AND-COMPLIANCE.md) · [AI assistance](docs/16-AI-ASSISTANCE.md) |
| UX | [UX & design system](docs/05-UX-AND-DESIGN.md) · [Workspace configuration](docs/20-WORKSPACE-CONFIGURATION.md) |
| Self-hosting | [Install with Docker Compose](docs/guides/install-docker-compose.md) · [First-time setup](docs/guides/first-time-setup.md) · [Configuration](docs/guides/configuration.md) |
| Delivery | **[Tracker dashboard](trackers/README.md)** · [Roadmap](docs/07-ROADMAP-AND-MILESTONES.md) · [Tracker detail](docs/17-EXECUTION-TRACKER.md) ([xlsx](trackers/Delivery_Tracker.xlsx)) · [Test strategy](docs/15-TEST-STRATEGY-AND-CONFORMANCE.md) · [E2E plan](docs/23-E2E-TEST-PLAN.md) · [Open questions](docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) |
| Ways of working | [Engineering rules](docs/24-ENGINEERING-RULES.md) · [Contributing](CONTRIBUTING.md) · [Governance](GOVERNANCE.md) |

## Repository layout

```text
apps/procera/   the server binary: API, worker, scheduler, admin CLI
crates/         Rust crates: access, audit, identity, members, library, campaigns, announcements,
                files, keys, crypto, notify, search, settings, platform, db, cache, engine, runtime, ir, expr
migrations/     PostgreSQL migrations (forced row-level security on every workspace table)
web/            React + TypeScript app (React Aria)
e2e/            Playwright end-to-end packs (API and browser)
conformance/    workflow runtime conformance fixtures
deploy/         Docker Compose (self-hosting and E2E), web/proxy image, SQL roles
site/           the website (static, generated from tracker data)
docs/           design pack (00–24) and ADRs
assets/brand/   logos and brand guide (not Apache-licensed)
scripts/        demo, full test gate, tracker generator
trackers/       generated delivery tracker (do not hand-edit)
```

## Contributing

Contributions are welcome, from humans and from AI-assisted workflows. Please read:
- [CONTRIBUTING.md](CONTRIBUTING.md)
- the binding [engineering rules](docs/24-ENGINEERING-RULES.md)
- the [Code of Conduct](CODE_OF_CONDUCT.md)

Every commit is DCO signed-off. Security issues go privately via [SECURITY.md](SECURITY.md).

## License

Procera's source code is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE). The Procera name, logo and brand assets in [`assets/brand/`](assets/brand/README.md) are trademarks of CasualOffice and are **not** covered by the Apache licence ([governance](GOVERNANCE.md#7-trademark)).
