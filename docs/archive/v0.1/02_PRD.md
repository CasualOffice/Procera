# Product Requirements Document (PRD)
Version 0.1 | Product: working title **Procera** | Target: standalone OSS enterprise BPMS

## Vision and positioning
One accessible workspace to **discover, document, govern, execute, measure and improve** organizational processes. Distinct from task trackers: work is an output of processes, not the product's primary object.

## Personas and permissions
| Persona | Main jobs | Default permissions |
|---|---|---|
| Employee/requester | Discover process, submit request, track outcome | Read published public processes; own instances |
| Process participant | Complete assigned approvals/forms | Assigned work only |
| Process owner | Define SOP, accountable metrics, propose changes | Edit assigned drafts; see scoped analytics |
| Business analyst | Model, compare as-is/to-be, simulate | Edit models; no unilateral publish |
| Compliance officer | Control mappings, attestations, evidence, exceptions | Read audit/evidence; approve controlled changes |
| Workspace admin | Manage people, policies, integrations | Tenant admin, not necessarily sensitive case content |
| Platform operator | Run system, troubleshoot infrastructure | Operational telemetry; break-glass audited access |

## Business capabilities / feature requirements
### Process catalog and governance
FR-G01: Hierarchical process landscape (domain > capability > process > subprocess), owners, RACI, tags, criticality, search.
FR-G02: Document as-is and to-be models with rich descriptions, diagrams, inputs/outputs, KPIs and dependencies.
FR-G03: SOP editor with attachments, linked policy/control, checklists, acknowledgements, review schedule, retention.
FR-G04: Draft > Review > Approved > Published > Retired lifecycle, configurable segregation of duties.
FR-G05: Immutable published versions, semantic diffs, review comments, change requests and effective dates.
FR-G06: Attestation campaigns, evidence collection, exception register and audit exports.
FR-G07: Ownership handover, periodic recertification, review overdue alerts.

### Execution and human work
FR-E01: Visual no-code builder: start, form, approval, assignment, condition, parallel, wait, timer, webhook, subprocess, end.
FR-E02: Launch manually, via API, scheduled trigger or authenticated event.
FR-E03: Dynamic forms, typed variables, validation, file upload and field-level privacy.
FR-E04: Role/group/manager assignment, delegation, substitute, reassignment, due date, escalation.
FR-E05: Approve/reject/request changes; signed decision audit with comments and attachments.
FR-E06: Durable instances; pause, resume, cancel, retry; idempotent APIs; recovery after crashes.
FR-E07: Version-pinned instances; explicitly governed migration for active instances.
FR-E08: Correlated sub-processes and linked cases; comments, participants and timeline.
FR-E09: API, outbound webhooks, chat messages/notifications, email and generic HTTP connectors.
FR-E10: Rule tables and conditions; later DMN-compatible modeler.

### Case management and improvement
FR-C01: Case intake, flexible stages, case file, related parties, milestones and discretionary activities.
FR-C02: Exceptions, investigations, evidence, case closure controls.
FR-I01: Metrics: cycle time, wait time, rework, SLA compliance, abandonment, cost proxy, bottlenecks.
FR-I02: As-is/to-be comparison, improvement proposals, experiments, before/after KPI review.
FR-I03: Event-log export suitable for process mining; process mining itself later.

### Administration and platform
FR-P01: Multi-tenant organizations/workspaces, tenant-bound resources, custom roles and policies.
FR-P02: OIDC/SAML SSO, SCIM provisioning, API tokens/service identities; local auth for evaluation deployments.
FR-P03: Configurable data retention, residency/deployment options, export, deletion workflows and legal holds.
FR-P04: Accessible, responsive web UI; keyboard operation; localization-ready text and date/time handling.
FR-P05: Tenant-scoped audit trail, reporting, observability and backup/restore.
FR-P06: Full self-hosted install via Compose for trial; Kubernetes/Helm for production.

## Priorities
P0 (pilot): G01-G05, E01-E07, E09 (notifications/webhook), I01 basic, P01/P04/P05/P06.
P1: G06-G07, E08/E10, C01-C02, I02, P02/P03 enterprise features.
P2: I03/mining, broad BPMN/DMN/CMMN interchange, advanced simulations, AI-assisted design.

## User journeys and acceptance criteria
J1 Employee onboarding: HR starts a published template, manager approves, IT provisioning fires once, employee tracks status. AC: approval race safe; state survives restart; every decision attributed; tenant cannot read another tenant's data.
J2 SOP recertification: owner receives review reminder, proposes edit, reviewer approves, version publishes at effective date. AC: old version retrievable, diff and approval evidence retained, expired SOP flagged.
J3 Purchase request: requester submits typed form; amount threshold chooses approver; rejection returns reason. AC: rule evaluation deterministic, request not orphaned on reassignment, duplicate submission returns same instance for same idempotency key.
J4 Incident case: analyst opens flexible case, adds evidence and ad hoc steps, closes with mandatory findings. AC: case permissions enforced per file and field; full audit export available.
J5 Improvement review: owner filters bottleneck dashboard and compares median cycle time between versions. AC: metrics show sample sizes and exclusions, timezone-consistent timestamps.

## Success measures (targets to validate with pilot)
- First useful process published by nontechnical admin in <= 30 minutes with template.
- >= 80% of pilot participants complete assigned action without training.
- >= 99.9% successful accepted transitions (excluding validated user/business failures).
- 0 cross-tenant data leaks in automated security suite.
- 100% of published changes attributable to actor, reviewer and version.
- 95th percentile common API reads < 300 ms at pilot load; define hardware/load before commitment.

## Non-goals for initial release
General-purpose Jira clone; payroll/ERP implementation; complete BPMN conformance; unbounded user scripting; AI auto-approval; process mining discovery from arbitrary enterprise logs.

## Open product questions
Public standalone brand and repository? Single-org vs multi-org deployments? Which industries require regulated records? Expected active instances and tenants? Must offline/mobile approvals be supported? Licensing for paid hosted offering? These are decision inputs, not blockers for core design.
