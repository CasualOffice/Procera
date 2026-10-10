# UX, process governance and product information architecture

## UX principles
Default to business language (Request, Step, Owner, Decision, Deadline), with advanced BPMN mode for analysts. Reduce navigation depth and avoid dashboard clutter. Process execution and process documentation are linked but visibly distinct. Every action shows current state, responsible person, expected next step and audit consequence. Use white/neutral surfaces, one restrained accent colour; product branding must be configurable. WCAG 2.2 AA target.

## Primary navigation
Home | Process Library | My Work | Requests | Cases | Improve | Reports | Administration.

## Screen specifications
**Home**: compact assigned work, processes needing review, SLA risk, recent activity, favorites. No empty KPI cards when no data; first-run guided template.
**Process Library**: hierarchical taxonomy, search, filters (department, owner, lifecycle, compliance), cards/list, dependencies, ownership badges.
**Process Detail**: overview, interactive map, SOP, owner/RACI, inputs/outputs, controls, KPIs, versions, execution launch; single-page anchored tabs.
**Designer**: left step palette, center graph, right properties panel, top validate/simulate/review/publish; inline errors; keyboard shortcuts; draft autosave and undo/redo; basic mode + advanced BPMN mode.
**My Work**: action-focused inbox, due date, requester/context, decision form, delegated work; batch actions only when safe and explicitly permitted.
**Request Detail**: progress stepper, visible next actor, comments, attachments, history and cancellation rights.
**SOP Editor**: structured document with ownership, prerequisites, steps, exceptions, evidence, revision diff, approval chain and acknowledgement.
**Cases**: flexible checklist/timeline, linked incidents and evidence, activities, due dates, milestones, closure requirements.
**Improve**: baseline KPIs, proposed process change, simulation/impact hypothesis, approvals, post-release measurements.
**Admin**: org, SSO, roles, connectors, calendars, data retention, audit exports and system health.

## Governance lifecycle
Idea -> Draft -> In review -> Approved -> Scheduled -> Published -> Deprecated -> Retired. Process owner authors; independent reviewer approves controlled processes. Enforce effective date, change reason, affected SOP/control links, stakeholder notification, archived prior version, and re-acknowledgement policy. Running instances remain pinned to definition versions unless approved migration is explicitly executed.

## Sample RACI
| Activity | Owner | Analyst | Compliance | Admin |
|---|---|---|---|---|
| Draft model | A | R | C | I |
| Approve controlled SOP | C | R | A | I |
| Publish standard process | A | R | C | I |
| Configure integration secret | I | C | C | A/R |
| Review control evidence | C | I | A/R | I |

## Usability testing
Recruit 5–8 participants per persona for formative studies. Tasks: locate SOP, start purchase request, approve in inbox, change step owner, publish revised process, export evidence. Measure completion, time-on-task, misclicks, perceived confidence, accessibility keyboard navigation. Test narrow screens and 200% zoom; no hover-only essential controls.

## Notifications
Only actionable changes by default; grouped daily reminders; critical SLA escalation can override digest by tenant policy. Deep link to authorized work item, not to unrestricted data. All notification actions revalidate permissions and current revision.
