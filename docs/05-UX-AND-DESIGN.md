# 05 — UX, information architecture and design system

Status: **Draft v0.2** · Owner: Design lead · Supersedes [v0.1 UX & governance](archive/v0.1/05_UX_AND_GOVERNANCE.md).
Closes gap A6 (RACI). Covers the UX side of FR-S, FR-W, FR-G, FR-E, FR-R, FR-F, FR-A, FR-V and FR-T in [02-PRD](02-PRD.md), plus the Policies, Document signing, Announcements and Acknowledgement & Signature campaign modules (§3.21–3.24).

## 1. North star and principles

**North star:** *A new employee can start a request or approve one in under a minute without training, and a process owner can take a process from template to published in under 30 minutes. Neither ever wonders "what happens next?" or "who did that?"*

The product has two audiences with opposite needs:
- **Many occasional users** (requesters, approvers, SOP readers) need it effortless.
- **Few power users** (owners, designers, admins, compliance) need speed, density and control.

Both must be served without forking the product.

| # | Principle | What it means in practice |
|---|---|---|
| P1 | **Progressive disclosure** | Defaults are expressed in business language (Request, Step, Owner, Decision, Deadline). Advanced options (rule tables, boundary events, SoD tuning, BPMN view, raw JSON) sit behind an "Advanced" disclosure. They are never removed, and never in the way. |
| P2 | **Templates first** | Every "create" entry point offers templates before a blank canvas. Empty states link to the gallery. |
| P3 | **One obvious next action** | Each screen has exactly one primary button. Secondary actions go in a menu. Colour is reserved for the primary action and for status. |
| P4 | **State · Owner · Next · Consequence** | Every object header answers four questions: what state is it in, who holds it now, what happens next, and what this action will record. Example: "Approving will notify Finance and is recorded in the audit trail." |
| P5 | **Keyboard-first power** | Global ⌘K, list navigation with `j`/`k`, single-key actions in the inbox, and shortcuts shown in tooltips and menus. Nothing is mouse-only or hover-only. |
| P6 | **Zero dead ends** | Every empty, error and no-permission state explains why and offers a next step: install a template, ask an owner, request access, retry. |
| P7 | **One grid, one filter language** | Every list uses the same DataGrid (§4). Learn it once, then use it for requests, files, members and audit alike. |
| P8 | **Visible trust** | Immutable versions show a lock and checksum. Signed decisions show the signature manifestation. Admin access to content shows an "access is logged" cue. A break-glass session shows a persistent banner. Test runs are visibly marked "TEST". |

### 1.1 Benchmarks — what we borrow

| Product | Borrow | Avoid |
|---|---|---|
| **Linear** | Speed, ⌘K palette, keyboard triage, optimistic UI, calm density | Assuming everyone is a power user |
| **Notion** | Block editing for SOPs, slash commands, inline mentions | Unstructured pages: SOPs need structure for governance |
| **Stripe Dashboard** | Data tables with chip filters, drill-down from metrics, readable audit/event logs, test-mode toggle | — |
| **Process Street** | Checklist-style run view, "SOP is the workflow" mental model | Shallow governance and audit |
| **Power Automate Approvals** | Single-screen approve with context, approve from email/chat/mobile | Licence and complexity sprawl |
| **Signavio Collaboration Hub** | Read-only "process portal" for all employees, landscape navigation | Modeller-centric jargon |
| **Retool tables** | Column-level filters, saved views, bulk actions, CSV export | Developer-oriented configuration |

### 1.2 Audience and scale

The audience is companies of every size and every department: admin, IT, HR, finance, ops, quality and legal. The same product has to feel light for a 15-person startup and hold up for a 50,000-person multinational. Scale is handled through **defaults and progressive surfaces, not separate editions**.

| Dimension | Startup mode (default for new workspaces < 50 members) | MNC mode (unlocked by configuration, never forced) |
|---|---|---|
| Setup | Zero-config. The first-run wizard asks 3 questions (company name, departments, who approves money), installs 3–5 recommended templates and goes live. | Config-as-code import, sandbox → prod promotion, staged rollout per department |
| Workspaces | One workspace; the switcher is hidden until there is a second membership | Many workspaces (per region or business unit). System Admin console, quotas |
| Identity | Email + password + MFA, invite links | SSO/SCIM, domain auto-join, delegated admins scoped to departments |
| Roles | 4 visible roles (Admin, Owner, Approver, Member); the rest collapsed under "More roles" | Full permission matrix, custom roles, directory-rule assignment, access reviews |
| Governance | Single reviewer, no scheduling, re-ack off | Independent review, SoD, effective dates, Separation mode, Part 11 signatures |
| Data volume | Lists of tens or hundreds: comfortable density, cards on small screens | Tables of millions: compact density, saved views, server-side filters, async exports |
| Language | Plain terms, no ISO or BPMN vocabulary | Compliance mappings, framework tags, BPMN view available |

Rules:
- A screen must work at both ends. Power features appear when the data or configuration needs them; for example, grouping, bulk actions and saved views are promoted once a list passes 50 rows.
- Startup mode never shows empty admin sections. Sections with nothing configured collapse to a one-line "Set up …" link.
- Per-screen scaling notes are marked **Scale:** in §3.

## 2. Information architecture

### 2.1 Workspace app (every member)

```
┌ Workspace switcher ▾ │ ⌘K Search & commands │ (+) New │ 🔔 │ Avatar ┐
│ Home                                                             │
│ Library        Processes · SOPs · Policies · collections         │
│ Requests       start · my requests · (team)                      │
│ My Work        Tasks · Acknowledgements · Signatures · delegated │
│ Documents      send for signature · sent · templates             │
│ Announcements  feed · (compose, campaigns — if permitted)        │
│ Cases          (if permitted)                                    │
│ Reports        reports · dashboards · improvement proposals      │
│ Admin          (Workspace Admin / Owner only)                    │
└──────────────────────────────────────────────────────────────────┘
```

Documents, Announcements and Cases are **modules** that a workspace can switch on or off in Settings. Startup mode enables Library, Requests, My Work and Announcements by default.

- Navigation is **permission-shaped**: items the user cannot use are hidden, not greyed out. Admin shows only for admin roles.
- **Workspace switcher** lists the user's memberships with role badges. Switching reloads with a new workspace-scoped session (12-AUTHZ §2), and the workspace accent changes so users notice the context switch.
- **(+) New** is contextual: Request, Process (template or blank), SOP, Case.
- Mobile (≤ 640 px): bottom bar with Home · My Work · Requests · Search · More.

### 2.2 Workspace Admin console (`/admin`)

```
Overview · Requests (all) · Work & workload · Process health · Campaigns · Files · Ops monitor
People: Members · Roles · Directory · Access reviews
Configuration: Settings · Modules · Branding & terminology · Calendars · Notifications · Security · Governance policy · Retention
Trust: Audit explorer · Investigations · Alerts · Export & data
```

### 2.3 System Admin console (separate realm, `admin.<host>` or `/system`)

```
Workspaces · Quotas & usage · Template gallery · Platform health · Platform settings · System audit · Break-glass · System admins
```

It is visually distinct: a dark chrome and a "SYSTEM" label in the header. It has no navigation into workspace content, and break-glass is the only bridge (§3.20).

### 2.4 Command palette and search (⌘K / Ctrl K)

- Actions and objects are mixed, for example "Start purchase request", "Go to Members", "Open PR-2026-0142", "Approve next item".
- Results are scoped to the active workspace and permission-filtered on the server.
- Recent and frequent items show first. Fuzzy matching. Each result shows its type icon and shortcut.
- Full search page: type facets (process, SOP, request, case, file, person), filters, and highlighted snippets. Restricted fields are never indexed.
- **As built (R1, BPMS-031):**
  - A "Search ⌘K" field sits under the workspace switcher; on narrow screens it becomes an icon button.
  - Empty query: **Recent** (documents opened in this browser, per workspace, at most 6), **Go to** (the same permission-shaped list as the sidebar) and **Create** (New policy, New SOP, New announcement, New campaign, Invite people, each only with its permission).
  - Typing: server results (`GET /api/v1/search`, documents and people) come first, with the matched words highlighted in the snippet. Up to 5 matching commands follow. Command matching is loose on labels ("gtm" finds "Go to Members") and exact on keywords.
  - Keyboard: ↑/↓ move, Enter opens, Esc closes. The input is a combobox over a listbox (`aria-activedescendant`), and a polite live region announces the result count.
  - A person opens Members filtered to that person until a member detail page exists.

## 3. Screen specifications

For each screen: **Purpose · User · Components · States · Interactions · Mobile**. States always include *empty*, *loading* (skeletons, never spinners for layout), *error* (retry plus error id), *partial* (some sections unavailable) and *no-permission* (explanation plus "Request access" to the owner).

**State kit as built (`web/src/ui/States.tsx`, UX-005):**
- `Skeleton` comes in list, table and document shapes that match the final layout.
- `Empty` has an icon, one line saying what the area is, one line saying how it fills, and at most one action. A filtered list that is empty offers "Clear filters" instead of the first-run copy.
- `LoadError` replaces the content (never an empty list) and shows what happened plus "Try again". A 403 says who can help instead of offering a retry.
- Copy rules: plain functional sentences, no em-dashes, sentence case. An empty cell stays blank, or names the absence ("No owner").

### 3.1 Home
- **Purpose:** what needs me today.
- **User:** everyone.
- **Components:** "Needs your action" (top 5 work items with inline decide), "My open requests" with status steppers, "Due for review / acknowledge" (SOPs), favourites, recent activity.
- **Role-aware sections:** process owners see SLA risk for their processes; admins see a link to Overview.
- **Empty:** the first-run checklist. For a workspace admin it is: install a template → invite people → publish. For members it is: "Browse the library".
- **Mobile:** single column, action cards first.

### 3.2 Library
- **Purpose:** find and understand processes and SOPs. This is the employee "process portal".
- **Components:**
  - Left: collection tree (domain › capability › process).
  - Centre: card/list toggle using the DataGrid.
  - Filters: department, owner, lifecycle, criticality, compliance tag, "can start".
  - Badges: owner, version, review due, controlled 🔒.
- **Interactions:** star a process, start directly from the card when permitted, hover/focus preview.
- **Empty:** "No processes yet. Install from the template gallery." No-permission collections are hidden.
- **Mobile:** search-first list. The tree becomes a drill-down.

### 3.3 Process detail
- **Purpose:** the single page that tells you everything about a process.
- **Header:** name, lifecycle state, version, owner, criticality, and the primary action (`Start request` for members, `Edit draft` for owners).
- **Anchored tabs:** Overview · Map (read-only diagram) · SOP · Roles & RACI · Inputs/Outputs · Controls · KPIs · Versions · Runs (permitted).
- **Versions tab:**
  - A timeline of published versions with effective dates, change reasons and approvers.
  - "Compare" opens the semantic diff across steps, rules, forms, roles and SOP text, side by side, colour plus symbol (not colour alone).
- **Trust cue:** published versions show a 🔒 icon and the checksum on hover.
- **Mobile:** tabs become accordion sections. The Start button stays sticky.

### 3.4 SOP editor
- **Purpose:** author governed procedures.
- **Editor:**
  - Structured block editor with fixed sections (Purpose, Scope, Roles, Definitions, Procedure steps, Exceptions, Records, References).
  - Within sections, free blocks: text, checklist, table, image, callout, file, link to a process step.
  - Slash menu and @mentions.
- **Side panel:** owner, reviewers, review cadence, linked controls, retention class, acknowledgement policy (re-ack on major version, optional quiz).
- **Top bar:** state chip · autosave indicator · `Preview` · `Submit for review` (primary).
- **Review mode:**
  - Inline comments, suggestion mode, and a diff against the published version.
  - The reviewer's primary action is `Approve` or `Request changes`, with a mandatory comment for changes.
- **Export:** PDF (tagged) and DOCX. Superseded and draft copies are watermarked.
- **Mobile:** read and acknowledge only. Editing is desktop/tablet.

### 3.5 Designer
- **Purpose:** build the executable process. It serves both designers (power) and owners (simple edits).
- **Layout:** left palette (step kinds from IR v1, in business language), centre **step builder** (vertical list of steps with branches as nested lanes; the default view), toggle to a **flow canvas** (graph view; React Flow), and a right properties panel.
- **Top bar:** draft state · `Validate` · `Test run` · `Submit for review` / `Publish` (whichever policy allows).
- **Properties panel sections:**
  - Basics (name, description)
  - Who (assignment builder with live "resolves to 3 people" preview)
  - Form
  - Deadline & escalation
  - Outcomes
  - Advanced (boundary events, SoD exclusions, loop bound)
- **Validation:** inline errors on the step itself plus a problems panel. Each problem links to its node. The IR codes (`IR-E00x`) show only in Advanced mode.
- **Test run:** split view. The process runs on the left with simulated participants (switch "act as"); a trace timeline shows on the right. A "TEST" ribbon is shown, and no real notifications are sent.
- **Non-drag alternative (WCAG 2.5.7):**
  - Every step has "Add step after…", "Move up/down", "Move to branch…" menu items, with shortcuts `Alt+↑/↓`.
  - The canvas supports keyboard node selection (Tab / arrows) and connection via a "Connect to…" dialog.
- Autosave, undo/redo (`⌘Z`/`⇧⌘Z`), and a version-conflict banner if another editor saves.
- **Mobile:** read-only view. Editing needs ≥ 1024 px.

### 3.6 Form builder
- **Layout:** field palette (typed: text, number, money, date, person, choice, file, table/repeating group, section). Canvas preview toggles between desktop and mobile widths. Field properties: label, help, required, validation, default, visibility rule (expression builder with plain-language preview), **classification** (public/internal/confidential/restricted, with a lock icon).
- **Bindings:** fields bind to process variables. Unbound fields are flagged.
- **Preview** as a specific role, to see field-level redaction.
- **Accessibility built in:** labels are mandatory, error text sits next to the field, and logical tab order is checked by a lint.

### 3.7 Template gallery and install wizard
- **Gallery:** cards with industry and domain filters, a "Wave 1 / recommended" badge, framework alignment tags, and a preview (diagram, form, SOP skeleton, KPIs).
- **Install wizard (4 steps, progress bar):**
  1. Choose collection and name.
  2. **Map roles:** each `{{role:…}}` placeholder maps to a group, a directory rule or a person. Live "resolves to N people" preview and warnings for unmapped roles.
  3. Configure: thresholds, calendars, SLA defaults, optional steps on/off.
  4. Review → creates a **draft**, then offers `Test run now`.
- **Upgrade offers:** a banner on installed processes ("Template v1.3 available"). It opens a 3-way diff (template old → template new vs your copy) where the user picks changes. Nothing applies automatically.

### 3.8 Start request
- Entry points: Library card, ⌘K, Home, and a deep link (`/start/purchase-request`).
- One-page form, or a stepper for long forms. Draft autosave. "What happens next" preview: "Your manager approves, then Finance if > ₹50,000. Typical time: 2 days."
- On submit: confirmation with request id, a link to track it, and an undo window (configurable, default 10 s) before the first step activates.
- **Mobile:** full-screen form, camera upload for files.

### 3.9 Request detail
- **Header:** id, title, state, and the **current holder + due date** in large text.
- **Progress stepper:** done / current / upcoming, with the actor shown per step.
- **Tabs:** Details (form, with redaction cues) · Activity (comments, mentions, attachments) · History (event timeline) · Files.
- **Actions:** contextual, e.g. `Cancel request` for the initiator, or admin actions behind a "Manage" menu with a required reason.

### 3.10 My Work (inbox)
- **3-pane layout (desktop):**
  - Left: views (Assigned, Claimable, Delegated to me, Due today, Overdue, saved views).
  - Centre: the list (DataGrid, compact).
  - Right: the item with its decision panel.
- **Keyboard triage:**
  - `j`/`k` next/previous item
  - `a` approve
  - `r` reject
  - `c` request changes
  - `e` open full detail
  - `d` delegate
  - `/` search
  - `?` shortcut help
- Destructive or comment-required decisions open the comment field first.
- **Decision in ≤ 3 interactions** (FR-R03).
- Bulk approve appears only when all selected items are eligible: no restricted fields, the process allows bulk, and no signatures are required. Preview: "Approve 8 items".
- Out-of-office toggle in the header, with date range and substitute picker.
- **Three sections, one inbox:** **Tasks** (approvals and forms), **Acknowledgements** (SOPs, policies, announcements, attestations) and **Signatures** (documents to sign). Each shows a count and a due date. The default "All" view sorts everything by due date, so an employee has exactly one place to clear (§3.24).
- **Mobile:** list → item → decision sheet, with swipe actions as optional shortcuts (buttons are always present).
- **Scale:** a startup user sees a single list. At volume, the views rail, saved views and claimable queues appear.

### 3.11 Cases
- **Case board/list:** DataGrid or kanban by stage.
- **Case page:**
  - Header (stage, owner, due, milestones)
  - Checklist of planned and discretionary activities ("+ Add activity")
  - Case file (evidence with hashes and chain-of-custody log)
  - Parties
  - Linked requests and investigations
  - Closure panel listing unmet mandatory items

### 3.12 Improve / Reports
- **Standard reports:** volume, cycle time percentiles, SLA attainment, bottleneck steps, rework, abandonment, attestation coverage.
- **Version compare:** median cycle time v3 vs v4, with sample sizes and confidence notes.
- **Proposals:** improvement ideas with baseline KPI, hypothesis and status. A proposal can spawn a draft.

### 3.13 Admin · Overview dashboard
- **Tiles** (each drills into a pre-filtered table, §5):
  - Active requests
  - Overdue work
  - SLA at risk
  - Open incidents / stuck instances
  - Throughput trend
  - Pending reviews and attestations
  - Storage and quota
  - Security: failed logins, members without MFA, new admins this week, break-glass sessions
- **Filters:** time range and department, persisted in the URL.
- **Layout:** not a uniform grid of identical KPI cards (§7.1). The top row is a short typographic summary ("12 overdue approvals, 7 in Finance") with links. Below it sit two asymmetric columns: a ranked "Needs attention" list (left, wide) and compact metric rows with sparklines (right). Security and quota sit in a divider-separated strip at the bottom.
- **Scale:** in startup mode it shows "Needs attention" and activity only. Department breakdowns appear once departments exist.

### 3.14 Admin · Members
DataGrid with columns: name, email, department, roles, MFA, last active, status. Actions: invite (email, link, CSV), deactivate, change roles, resend invite. The detail drawer shows roles with their scopes, groups, OOO, sessions (with revoke) and the audit timeline for that person.

### 3.15 Admin · Roles editor
- **List:** built-in templates (lock icon, "clone to customise") and custom roles, with holder counts.
- **Editor:**
  - **Permission matrix:** resources as rows, actions as columns, with checkbox, search and "select all in row".
  - Permissions the editor doesn't hold are disabled, with a tooltip explaining no-escalation.
  - Default scope picker.
  - Assignment tab: users, groups and directory rules, plus an expiry option.
- **Effect preview before save:** "+14 people gain `request:view_all` in Finance; 2 lose `process:publish`". Expandable list. Save requires step-up auth and records a diff in the audit trail.

### 3.16 Admin · Directory, Settings, Calendars
- **Directory:** department tree with drag-free moves (a "Move to…" menu), groups, positions, a manager chain viewer, CSV import with dry-run, and cycle detection errors.
- **Settings:** sectioned forms with a sticky save bar showing unsaved changes. Platform-floor-locked fields show "Set by system admin". Branding includes accent colour with a live contrast check, blocking save below 4.5:1.
- **Calendars:** year view of holidays, working hours per weekday, iCal import, and versions (only edits after publish create a new version).
- **Settings as built (R1):**
  - One page at `/admin/settings`: a section index on the left, and areas (Security and sign-in, Access, Documents and reviews, Campaigns, Announcements, Notifications, Directory, Branding, Language and region, Files, Requests) grouped by key prefix. The API does not return the catalogue `group`.
  - Each setting is one row: a plain label and hint, the effective value, and its source when it isn't the product default.
  - It is edited in place, one at a time, instead of through a page-wide save bar, because security keys need their own reason and step-up. The control follows the kind: switch, number, number plus unit, segmented choice (≤ 4 values) or select, chip set (with "Everyone" for `*`), one item per line, colour.
  - "Why this value" opens the explainer (`GET /settings/effective?key=`): every layer, the one in effect, and a clamped note.
  - Immediate (security) keys ask for a reason up front. Step-up keys open "Confirm it’s you" and retry. Keys without `editable_with` show "Set by the platform".

### 3.17 Admin · Files
DataGrid of all workspace files with columns: name, type, size, owner, linked object, scan status, retention class, legal hold, last accessed.
- **Filters:** quarantined, orphaned, > size, type.
- **Bulk actions:** set retention, apply/release hold, delete (blocked rows explain why), export list.
- **Quota bar:** breakdown by process, user and type.
- **Quarantine tab:** restricted to `file:manage_quarantine`.

### 3.18 Admin · Requests (all), Work & workload, Process health, Ops monitor
- **Requests (all):** DataGrid with columns state, process@version, initiator, current step, assignee, age, SLA, department. Row actions reassign, suspend/resume, cancel, retry, comment. Every admin action takes a reason. Opening restricted details shows "Your access is logged."
- **Workload:** per person and team, with open / overdue / due this week, OOO coverage gaps, and bulk reassign with preview.
- **Process health:** one row per process, showing volume, p50/p90 cycle time, SLA %, rework, incidents, last review, owner status (orphan flag).
- **Ops monitor:** incidents (failed steps), email bounces, connector failures (P1), timer lateness. Retry / skip / cancel in the row, with a reason.

### 3.19 Admin · Audit explorer, Investigations, Alerts
- **Audit explorer:**
  - DataGrid over audit events. Filters: actor, on-behalf-of, action category, object type/id, result (allowed/denied), IP, time range, severity.
  - Row expand shows the before→after diff (hashed for restricted values), session, trace id and the policy ids matched.
  - **Timeline modes:** per user, per object, per session.
  - A chain-verified badge with the last checkpoint time.
- **Investigations:** list plus investigation page with:
  - pinned evidence (events, files and requests captured as hashed snapshots)
  - notes, findings, assignees
  - a legal-hold toggle on linked records
  - status (Open → In progress → Findings → Closed)
  - `Export signed bundle`
  - "Pin to investigation" is available from any audit row, file or request.
- **Alerts:** a rule list (condition, threshold, recipients) with a "matches in the last 30 days" preview while editing, an alert inbox with acknowledgement, and anomaly signals (rule-based) linking to their events.
- **Audit log as built (R1, `/admin/audit`, read-only):**
  - A chain badge (`/audit/verify`).
  - A filter bar (category, result, person, action, date range). Selects apply at once; text and dates apply with "Apply".
  - 50 rows per page, with "Load older events" for keyset paging.
  - A row expands in place to the full record: action key, reason, object, IP and agent, policies, trace, sequence and hash, and the redacted diff.
  - The object cell opens that object's timeline in a side drawer.
  - With only `audit:view_own`, a note says the list is limited to the member's own activity.

### 3.20 System Admin console
- **Workspaces:** DataGrid with columns name, status, tier, members, active instances, storage, created, owner contact. Actions: create, suspend (read-only / locked), restore, schedule deletion (typed-confirmation plus grace period), change quotas.
- **Workspace detail:** usage graphs, quotas, owners, lifecycle history, export. No content.
- **Quotas & usage:** a cross-workspace table with sorting by usage % and soft/hard limit indicators.
- **Template gallery management:** draft → lint/fixtures status → publish / deprecate, version history, install counts.
- **Platform health:** job backlog, timer lateness, outbox lag, error rates, DB/storage capacity, per-workspace hot spots.
- **System audit:** the same explorer component, scoped to system-plane events.
- **Break-glass:**
  - Request form: workspace, reason, ticket, duration ≤ 4 h, read-only default.
  - The approval status is shown.
  - The active session has a red persistent banner and countdown in both consoles, and is visible to the workspace's admins.

### 3.21 Policies (Library › Policies)
- **Purpose:** the organisation's governing policies (Code of Conduct, InfoSec, Leave, Travel, Anti-bribery). They are distinct from SOPs: a policy states *what and why*, an SOP states *how*. Policies run on the same governed lifecycle (§6).
- **User:** employees read. Policy owners author. Compliance and HR approve.
- **Employee read view:** a document-like page, not a dashboard.
  - Title, owner, effective date, version, "applies to" (departments, locations).
  - A 70ch reading column with a sticky table of contents.
  - Linked SOPs and processes ("How to request an exception").
  - Primary action: `Acknowledge` when an acknowledgement is pending.
  - A public portal view (all members) and an optional unauthenticated **public link** for externally published policies (e.g. supplier code). It is off by default and audited.
- **Authoring:** the same block editor as SOPs, plus fields for policy type, applicability, review cadence, exception process, linked controls, and an acknowledgement requirement (none / on publish / on major version / annual).
- **Publishing:** a published major version can launch an acknowledgement campaign automatically for the "applies to" audience. The publish dialog previews this: "Publishing v3 asks 1,240 people to re-acknowledge by 30 Nov".
- **States:** empty ("Start from a policy template": Code of Conduct, Acceptable Use, Leave…); superseded versions watermarked and read-only.
- **Mobile:** fully readable and acknowledgeable at 360 px.
- **Scale:** startup mode shows a flat list. MNC mode adds applicability by entity and location, localised variants (one policy, several language versions linked) and per-region effective dates.
- **As built (R1, BPMS-043/044/045):**
  - **Policies** (`/policies`) is in the sidebar for everyone with `policy:view`. It lists the published policies that apply to me (`GET /me/policies`), with a search box and category chips. The chips come from the unfiltered list so they don't vanish while filtering.
  - Each row shows verified, "Updated" (v2 and later published in the last 30 days) and review-overdue tags. A "What changed in vN" box gives the counts and change reason and links to the comparison.
  - **Document properties** add Review (status tag, next date, schedule, with "Recertify" and "Change schedule" for publishers), Verified ("Verify" or "Remove verification" for approvers; controlled documents ask to confirm identity) and, for policies, Applies to (audience summary, category, "Only they can see it").
  - A due or overdue review also shows a callout with "Recertify now" to publishers.
  - **Applies to** is edited in a dialog with category, the shared audience picker, and "Only these people can see it" (only with a specific audience).
  - **Suggested changes** sit under the document. Anyone who can read it can "Suggest a change" (title, optional details). Publishers can Accept, Decline (reason required) or Mark implemented (pick the version). The state is shown as a tag; members never see resolve actions.
  - **Compare** (`/library/:id/compare?from=&to=`, version numbers) is linked as "Compare with vN" from the version history. It shows the newer version as a document: added blocks tinted green with an underline, removed blocks red and struck through, and word-level `<ins>`/`<del>` inside changed blocks (a left rule marks the block).
  - The compare page also has a summary line, the change reason and two version pickers. Changes are named for screen readers, so colour is never the only signal.

### 3.22 Document signing (Documents)
- **Purpose:** send any document (PDF/DOCX; DOCX converted to PDF at send time and frozen) to employees or external people for e-signature.
- **User:** HR, legal, finance and admins send. Anyone signs.
- **Send flow (4 steps, one page with sections):**
  1. **Document:** upload or choose from the template library (saved field layouts).
  2. **Recipients:** individuals, groups, departments, roles, or external email addresses. **Bulk send** creates one envelope per recipient from a single setup ("Everyone in the company signs the Code of Conduct"). Roles per recipient: signer, approver, CC.
  3. **Order:** sequential (drag-free ordering with up/down and number inputs) or parallel, or mixed groups.
  4. **Fields:** a page view of the PDF with a field palette (signature, initials, date signed, text, checkbox, name, title). Fields are placed by click-to-place (not drag-only: WCAG 2.5.7), positioned with arrow keys, and assigned to a recipient by colour plus label. Required/optional, validation. Then review and send: message, deadline, reminder cadence, expiry.
- **Signing ceremony (mobile-first):**
  - Open from email, the My Work › Signatures inbox, or a magic link for externals.
  - Intro screen: who sent it, why, the deadline, and "Review and sign" (primary). Consent to electronic signature appears on first use.
  - Document view with a "Next field" guided navigator showing remaining field counts. Fields are focusable in order; keyboard and screen-reader accessible.
  - Signature: type (rendered in a script font), draw, or upload. Adopt once per session.
  - **Part 11 mode:** re-authentication before finishing (password + MFA or passkey), plus a signature meaning picker (e.g. "I have read and agree").
  - `Finish` → confirmation with a download of the signed copy. `Decline` requires a reason and notifies the sender.
- **Outputs:** a flattened signed PDF (with a PAdES signature by a platform/workspace certificate, P1) plus a **certificate of completion** page. The certificate lists each signer's name, email, auth method, IP, timestamps, document hash before and after, and the event log. Both are stored as immutable files and linked to the audit trail.
- **Tracking:** Documents › Sent is a DataGrid of envelopes with status per recipient (sent / viewed / signed / declined / expired), filters, remind now, void with reason, resend, and download the completion bundle. Bulk sends roll up into a campaign (§3.24).
- **Scale:** a startup sends a one-off offer letter in two minutes. An MNC bulk-sends to 20,000 people with progress by department and automated reminders.

### 3.23 Announcements
- **Purpose:** official communication that can require acknowledgement. Examples: a policy change, an office closure, a safety notice.
- **Compose:**
  - Title, rich text (headings, lists, links, images), attachments.
  - **Audience:** all, departments, groups, roles, locations, or combinations, with a live "reaches N people" count.
  - Schedule publish and expiry, pin to top, importance (normal / important; "urgent" is limited by policy).
  - Optional **"Require acknowledgement"** with a deadline, reminder cadence and **escalation to managers** for non-acknowledgement after X days.
  - Preview as a selected employee, on desktop and mobile.
- **Employee view:**
  - Home shows the most recent important and pinned items.
  - The Announcements feed is a chronological list in reading layout (not cards in a grid). Unread items have a weight change and a dot (not colour alone).
  - When acknowledgement is required, the item also appears in My Work › Acknowledgements.
- **Read receipts:** the sender sees viewed and acknowledged counts. Individual read lists are visible only to `announcement:view_receipts` holders (privacy).
- **Campaign dashboard:** the shared tracker (§3.24) shows completion % by department and an overdue list.
- **Scale:** startup mode is "post to everyone". MNC mode adds location and entity targeting, localised versions and approval before publishing for company-wide posts (configurable).
- **As built (R1):**
  - The feed shows a two-line preview, an unread dot with heavier weight, and the acknowledgement at the end of the expanded post. `?open=<id>` deep-links from Home.
  - **Manage announcements** (`/admin/announcements`, Administration) has status tabs and an Attio-style table: title, status, audience, author, reach, updated.
  - A row opens a side drawer with what happens next, properties, receipts (read and acknowledged %, by department) and the message. Actions depend on state and permission: Edit, Publish now or Schedule, Send for approval, Request changes, Approve and publish, Archive (reason).
  - When a direct publish needs approval, the drawer explains why and offers "Send for approval".
  - The composer has a title, a message (the same light markup as documents), the audience picker (Everyone, or departments, groups and roles as toggle chips), and options: acknowledgement, critical, pin, publish at, expires on. It always saves a draft first.

### 3.24 Acknowledgement & Signature campaigns (shared engine)
One engine powers SOP and policy re-acknowledgement, announcement acknowledgement, attestations and bulk signing. The UX is therefore identical everywhere.

- **Campaign** = a source item (SOP version, policy version, announcement, document, attestation question set) + an audience snapshot + a required action (`acknowledge`, `acknowledge + quiz`, `attest`, `sign`) + a deadline + reminders + escalation.
- **Employee side (My Work › Acknowledgements / Signatures):**
  - One list sorted by due date: item type, title, from, due.
  - Opening an item shows the content in reading layout with the action at the end. It is sticky on mobile, enabled once the content is opened. A "scroll to end" requirement is optional, never forced for screen-reader users.
  - Quizzes are short (≤ 5 questions), retryable, and don't punish errors.
  - After acting: a receipt ("You acknowledged InfoSec Policy v3 on 12 Oct 10:42").
- **Admin / HR / compliance side (campaign tracker):**
  - **Campaigns list:** a DataGrid with source, type, audience size, completion %, overdue, deadline, owner.
  - **Campaign page:**
    - A completion bar.
    - A **completion by department** table, sortable, with a horizontal bar per row (colour-blind safe, with values shown).
    - The overdue list with `Remind`, `Escalate to managers`, `Exempt with reason`.
    - Audience changes: joiners after launch are auto-added if the policy says so.
    - Export the evidence (who / when / which version / method), signed (CMP-03).
- **States:** a draft campaign shows the audience preview. A closed campaign is frozen as evidence.
- **Scale:** the same screens serve 10 people or 50,000. At volume, the department table becomes a drill-down tree (entity › department › team).
- **As built (R1):**
  - **Campaigns** (`/admin/campaigns`) lists campaign, document and version, status, an acknowledged meter ("3 of 7"), overdue, audience and due date.
  - **New campaign** offers a list of published documents (the current published version is used), a name prefilled from the document, the shared audience picker and an optional due date.
  - The campaign page:
    - Draft: shows the audience preview ("N people will be asked to acknowledge") and the statement, then Launch or Cancel.
    - Active: a progress bar, four figures (acknowledged, pending, overdue, launched), an acknowledged-by-department table computed from recipients, and a people table with All, Pending, Overdue and Done filters.
    - Actions: Remind pending, Close (reason), and the evidence CSV.

## 4. Shared DataGrid specification (FR-V01–V09)

Built once (`@procera/ui/DataGrid`) on **TanStack Table + TanStack Virtual** (MIT). It is server-driven, so all sort, filter and pagination run on the API.

| Feature | UX |
|---|---|
| **Toolbar** | Quick search · `Filter` · view selector ▾ · `Columns` · density · `Export` · overflow |
| **Filter chips** | Each active filter is a chip `Department: Finance ×`. Clicking edits it in a popover. "Clear all". |
| **Filter builder** | `+ Add filter` → field picker (search, grouped) → operator typed by field (text: contains/is/is not/empty; number/date: =, ≠, <, >, between; **relative dates**: last N days, this quarter; enum: any of / none of; person: is / is me / in my team; boolean). **Advanced**: AND/OR groups with nesting depth ≤ 2. A plain-language summary appears above the grid. |
| **Sorting** | Click a header to sort, shift-click for multi-sort. Sort indicators show the order number. Only allowlisted sortable columns are offered. |
| **Columns** | Chooser with search, drag or "move up/down", pin left/right, resize (keyboard resizable). |
| **Saved views** | Personal or shared to roles. Set default. A "modified" dot appears when the current state differs from the saved view (`Save` / `Save as`). The URL always encodes the state. |
| **Selection & bulk** | Checkbox column, shift-range select, "select all N matching". Bulk action → **impact preview** modal (counts, ineligible rows with reasons) → reason field if required → run async → per-row results report. |
| **Export** | Current view (filters + columns). CSV or XLSX. A modal shows the row count and redaction notice. Large exports run async with a notification when ready. The download is audited. |
| **Pagination** | Cursor-based with virtual scrolling. Exact counts up to 10k, then "10k+ (approx.)". |
| **Grouping** (P1) | Group by a column, with collapsible groups and counts. |
| **Empty / no results** | Distinguishes "nothing here yet" from "no matches — clear filters". |
| **Accessibility** | WAI-ARIA grid pattern. Arrow-key cell navigation, `Enter` to open, `Space` to select. Sort and filter changes are announced through a live region. |
| **Responsive** | ≤ 640 px renders cards (primary field, status, 2–3 key fields). Filters go in a sheet, sort in a select. |

## 5. Dashboard design rules

1. **Every number drills down** to a pre-filtered DataGrid showing exactly the records behind it. No dead tiles.
2. **Show denominators and sample sizes:** "SLA 92% (n = 214)". Hide percentages when n < 10.
3. **No vanity tiles.** Each tile maps to a decision or an action, e.g. reassign, review or investigate.
4. **Permission-aware:** the same dashboard renders different data per viewer, and there is no tile-level leakage.
5. **Colour-blind safe** palette (validated). Status always uses colour plus icon plus text. Trends show absolute values, not just arrows.
6. **Consistent time semantics:** workspace timezone shown, and the period stated on every tile.
7. **Empty guidance:** a new workspace shows "Data appears after your first requests", not zeros.
8. **Fast:** tiles load independently with skeletons, and the page is usable before every tile has loaded.

## 6. Governance lifecycle UX and RACI

Canonical lifecycle: **Draft → In review → Approved → Scheduled → Published → Deprecated → Retired**.

| State | Shown as | Who acts | UX rule |
|---|---|---|---|
| Draft | Grey chip "Draft" | Owner / Designer | Editable, autosaved, can test-run |
| In review | Blue "In review · 2 of 3 reviewers" | Reviewers | Locked for edits. Author can withdraw. |
| Approved | Teal "Approved" | Owner | Set the effective date, or publish now |
| Scheduled | Teal "Scheduled · 1 Nov" | — | Countdown. Can be cancelled back to Approved. |
| Published | Green "Published · v4 🔒" | — | Immutable. "Create new draft from this version". |
| Deprecated | Amber "Deprecated" | Owner | No new starts. Running instances continue. Banner points to the replacement. |
| Retired | Dark "Retired" | Owner / Compliance | Read-only archive. Retained per retention class. |

Publishing a version requires all of:
- a change reason
- affected SOPs and controls shown
- stakeholder notification preview
- the re-acknowledgement policy confirmed
- the statement "Running requests stay on v3 unless migrated"

An *Idea* is a separate improvement-proposal object, not a lifecycle state.

### 6.1 Corrected RACI (fixes gap A6)

| Activity | Process Owner | Designer (Analyst) | Reviewer | Compliance | Workspace Admin |
|---|---|---|---|---|---|
| Draft model / SOP | A | R | C | C | I |
| Review a standard process | A | C | R | I | I |
| Approve a **controlled** SOP / process | C | C (cannot approve own work) | R | **A** | I |
| Publish a standard process | A/R | C | C | I | I |
| Configure roles and permissions | C | I | I | C | A/R |
| Configure integration secrets (P1) | I | C | I | C | A/R |
| Review control evidence | C | I | C | A/R | I |
| Run an investigation | I | I | I | A/R | C (R if no compliance role) |

## 7. Design system

### 7.0 Visual language: calm, editorial, enterprise

The product should look like a well-made reference tool that people trust with records, not like a generated SaaS template. The craft benchmarks are Linear, Stripe Dashboard, the GOV.UK Design System, IBM Carbon, Atlassian and Notion's document pages. We learn from their discipline but copy none of them.

**What the language is:**
- **Typography over boxes.** Hierarchy comes from size, weight and spacing, not from wrapping everything in cards. A page reads top to bottom like a well-set document.
- **Content-driven, asymmetric layouts.** List + detail, document + margin rail, a wide main column with a narrow context column. Layout follows the content's shape, not a 12-column card grid.
- **Document-like reading pages** for SOPs, policies and announcements: a 70ch measure, generous leading, a sticky table of contents.
- **Real density where it helps:** tables, timelines and audit logs are dense and scannable. Forms and reading views breathe.
- **Dividers and borders over floating cards.** Sections separate with a 1px rule and whitespace. Elevation is reserved for things that truly float: menus, popovers, dialogs.
- **A restrained neutral palette with one accent.** Colour carries meaning (action, status, focus), never decoration.
- **Purposeful whitespace,** set on the spacing scale, never arbitrary.
- **Icons are functional:** one line-icon family at a consistent stroke, always with a text label in navigation and actions.

### 7.0.1 Anti-patterns we will not ship

Design review rejects any of these:

| ✗ Anti-pattern | Instead |
|---|---|
| Purple/indigo gradients, mesh or "aurora" backgrounds, glow effects | Flat neutral surfaces; the accent used sparingly |
| Glassmorphism (blur and translucency panels) | Opaque surfaces with a hairline border |
| Oversized hero cards and marketing-style banners inside the app | A plain page title + one-line description + primary action |
| Uniform bento grids of identical KPI cards | A typographic summary line + ranked lists + compact metric rows (§3.13) |
| Emoji as icons or status ("🚀 Published!") | Line icons + text status chips. (The 🔒 in this doc is shorthand for the lock icon.) |
| "✨ AI" sparkles, magic-wand branding, rainbow AI accents | AI features labelled in words ("Draft with assistant"), styled like any other secondary action, with provenance shown (16-AI) |
| Rounded-everything (pill buttons, 16–24px radii) with heavy drop shadows | Small radii (§7.1); borders, not shadows |
| Decorative illustrations in every empty state | A short sentence + one action; illustration only in first-run onboarding, monochrome |
| Centred, narrow "app in a card" layouts on wide screens | Full-width working surfaces with readable measures inside |
| Colour-only status, low-contrast grey-on-grey text | Colour + icon + text; contrast tokens (§7.1) |
| Skeuomorphic gauges, 3D charts, donut charts for > 3 categories | Bars, lines, tables with numbers |
| Animated counters, confetti, gratuitous transitions | ≤ 200ms functional transitions only |

### 7.1 Foundations: normative tokens

These values are **normative**: components consume tokens only, and CI lint blocks raw values in component CSS.

**Typography**

| Token | Value | Standard |
|---|---|---|
| Font stack (default) | `"Geist", system-ui, -apple-system, "Segoe UI", Roboto, "Noto Sans", sans-serif`; mono `"Geist Mono", ui-monospace, monospace` | Geist and Geist Mono (2026-10-11, replacing Inter: Inter read as a generic default), Inter, IBM Plex and Source Sans 3 are all **SIL OFL 1.1** (self-hosted via `@fontsource-variable`, no CDN). Noto covers non-Latin scripts. Verify in SP-07. |
| Vetted workspace font choices | Geist · Inter · IBM Plex Sans · Source Sans 3 · System UI | Only these (metrics tested for layout and scripts) |
| Type scale (px) | 12 · 14 · **16** · 18 · 20 · 24 · 30 · 36 | Modular, ~1.125–1.25 |
| Body default | **16px**, line-height **1.5** | Readability; WCAG 1.4.12 tolerance |
| Dense tables | 14px, line-height 1.43 (20px), only in DataGrid compact density and with a user toggle back to 16 | — |
| Minimum size | 12px, only for metadata/captions, never for body or controls | — |
| Headings | 36/30/24/20/18 at weight 600, line-height 1.2–1.3, letter-spacing −0.01em ≥ 24px | Hierarchy via type |
| Measure | max **70ch** for reading text; forms ≤ 640px wide | — |
| Text spacing | Layouts must survive line-height 1.5×, paragraph spacing 2×, letter spacing 0.12em, word spacing 0.16em with no clipping | WCAG 1.4.12 |
| Numerals | Tabular figures in tables, timers and money | — |

**Spacing and layout**

| Token | Value |
|---|---|
| Base grid | 4px |
| Scale | 4 · 8 · 12 · 16 · 24 · 32 · 48 · 64 |
| Control padding | Inputs and buttons 8px × 12px (compact) / 10px × 16px (default); min height 32 / 40px |
| Section spacing | 32px between page sections, 16–24px within, 48–64px around page titles on reading pages |
| Breakpoints | 360 · 640 · 1024 · 1280 · 1600 |
| Page gutters | 16px (< 640) · 24px (< 1280) · 32px (≥ 1280) |

**Targets, focus and contrast**

| Token | Value | Standard |
|---|---|---|
| Minimum target | **24 × 24 CSS px** (or 24px spacing) | WCAG 2.5.8 |
| Recommended target | **44 × 44** for primary actions and all touch layouts | Platform HIG |
| Text contrast | **≥ 4.5:1**; large text (≥ 24px, or ≥ 18.66px bold) ≥ 3:1 | WCAG 1.4.3 |
| UI components and graphics | **≥ 3:1** against adjacent colours (borders of inputs, chart lines, icons) | WCAG 1.4.11 |
| Focus indicator | **≥ 2px** solid ring at ≥ 3:1 contrast, 2px offset, never removed; not obscured | WCAG 2.4.7, 2.4.11, 2.4.13 (AAA aspiration) |
| Status | Never colour alone: colour + icon + text | WCAG 1.4.1 |

**Colour, radius, elevation, motion**

| Token | Value |
|---|---|
| Neutrals | 11-step neutral ramp (`gray-0…gray-950`), slightly warm. Surfaces: `bg` (gray-0), `bg-subtle` (gray-50), `border` (gray-200, 3:1 for input borders via `border-strong`) |
| Accent | One workspace accent → derived 11-step ramp. Uses: links, focus, selection, active-navigation marker (primary buttons use ink by default, §7.1.1). **Default: Procera brand red**, with role-specific shades verified for contrast in §7.1.1. Workspaces may override it (white-label) |
| Semantic | `success` green, `warning` amber (dark text on amber), `danger` red, `info` blue, each with text/bg/border variants all meeting the contrast rules |
| Data viz | 8-colour categorical palette, colour-blind safe (validated for deuteranopia, protanopia and tritanopia), plus a sequential ramp |
| Radius presets | `sharp` 0/2px · **`standard` 4px controls / 6px containers (default)** · `soft` 6/8px. Nothing above 8px except avatars |
| Elevation | `0` flat (default; borders) · `1` popovers/menus (small shadow) · `2` dialogs (shadow + scrim). No shadows on cards |
| Motion | 120ms (micro), 200ms (panels), ease-out; no bounce. `prefers-reduced-motion: reduce` → 0ms, except essential progress |

**Environment support (all P0):**
- Reflow with no horizontal scroll at **320 CSS px** (WCAG 1.4.10). Data tables are the exception allowed by WCAG and switch to cards.
- Usable at **200% zoom** and **400%** reflow.
- `prefers-color-scheme` (light/dark/auto), `prefers-reduced-motion`, and `prefers-contrast: more`.
- **`forced-colors: active`** (Windows High Contrast): system colours, borders kept visible, focus via `Highlight`, icons via `currentColor`, no information in background images.
- Print stylesheet for SOPs, policies and certificates.

#### 7.1.0 Units and fluid scale (normative, owner direction 2026-10-11)

- **rem everywhere.** Type, spacing, radii, control and layout sizes are in rem (token values in `web/src/ui/tokens.json`). px is allowed only for hairlines (≤ 2px borders, outlines, focus rings), shadow offsets and blurs, the 999px pill radius, and absolute floors written as `max(Npx, …rem)`.
- **Fluid root.** `html { font-size: clamp(100%, 87.5% + 0.15625vw, 112.5%) }`: 16px up to 1280px wide, about 17px at 1920px and 18px at 2560px and above. The whole UI scales with the screen. It never goes below 16px, and because it is in %, the user's browser font size still applies (WCAG 1.4.4).
- **Accessibility floors stay absolute.** `--target-min: max(24px, 1.5rem)` and `--target-comfortable: max(44px, 2.75rem)` (WCAG 2.5.8 and the touch target guideline) never depend on the root size.
- **Enforced.** `npm run check` runs `scripts/px-to-rem.mjs --check`; `npm run units` converts px left by new code (idempotent). Inline React styles use rem strings; lucide icons take `size="1rem"`.

### 7.1.1 Procera brand applied to the product UI (normative)

Source: `assets/brand/` (brand board and copy, 2026-10-11). Contrast ratios below were computed with the WCAG 2.x formula.

| Role token | Light theme | Dark theme | Contrast (verified) | Use |
|---|---|---|---|---|
| `brand` | `#EE4136` Flamingo Red | `#EE4136` | 3.86:1 on #FFF; 3.69 on #F8FAFC; 4.63 on #0F172A | Logo/symbol, focus ring, active-nav marker, selection, progress, large display text (≥ 24 px). **Never** body text. **Never** white normal-size text on it. |
| `accent-text` (links, inline actions) | `#C72D26` Deep Red | `#FF6B5A` Coral | 5.48 on #FFF, 5.24 on #F8FAFC / 6.38 on #0F172A, 5.23 on #1E293B | Links, text buttons, icons that carry meaning |
| `accent-strong` (filled accent button, when enabled) | `#C72D26` bg + white text; hover `#A8241E` | `#FF6B5A` bg + `#0F172A` text | white on #C72D26 5.48; on #A8241E 7.15 | Optional "brand button" style; off by default |
| `action-primary` (default primary button) | `#0F172A` Charcoal bg + white text | `#F8FAFC` bg + `#0F172A` text | 17.85 | Primary actions (Submit, Approve, Publish) |
| `text` | `#0F172A` | `#F8FAFC` | 17.85 / 17.06 | Body and headings |
| `text-secondary` | `#64748B` Slate (only on #FFF/#F8FAFC); `#475569` on tinted surfaces | `#94A3B8` | 4.76 / 4.55; 7.58 / 6.96 | Metadata, helper text |
| `bg` / `surface` | `#F8FAFC` / `#FFFFFF` | `#0F172A` / `#1E293B` | — | Page and panels |
| `danger` | `#B42318` (text, icon, outline; filled + white text 6.57) | `#FCA5A5` text | 6.57 / 9.41 | Destructive actions and errors, **always with an icon and words** |
| `success` / `warning` | `#067647` / `#B54708` | lightened equivalents | 5.69 / 5.43 | Status, always with an icon and text |

**Decisions**

1. **Primary buttons are ink (charcoal), not red.** Procera is an approvals product. A red "Approve" button would read as *danger/reject*, and the brand red fails contrast with white labels. Red is the brand signal: logo, focus, active state, selection. It is not the action colour. Workspaces can opt into `accent-strong` buttons, which use Deep Red with white text.
2. **Danger is visually distinct from the brand.** It uses a deeper crimson (`#B42318`), and every use has an icon, an explicit verb ("Delete policy") and a confirmation dialog for destructive operations. Reject in approvals is an **outlined neutral** button, never filled red.
3. **No gradients in product UI.** The logo's gradient is part of the mark and is allowed wherever the logo appears. Marketing art (the banner, the social card) may use the brand gradient. Application surfaces stay flat (§7.0.1).
4. **Typography:** the wordmark is a geometric sans. The UI font stays on the vetted open-licence list (§7.1). Inter or a similar geometric sans pairs best with the mark.
5. **Logo usage in the app:**
   - The app shell shows `procera-symbol` at 24 px with the "Procera" wordmark in `text` colour. On narrow screens it shows the symbol only.
   - Minimum symbol size is 16 px, and clear space around it is at least half the symbol's height.
   - When a workspace sets its own logo (white-label), Procera branding appears only as a small "Powered by Procera" mark in the footer and in certificates. Workspaces can hide that mark per the governance trademark rules.
6. **Production caveat:** the current files are raster extractions from a concept board. Vector (SVG) masters, an optically corrected symbol and a trademark search are tracked as BPMS-034 and OQ-01 before R1 ships.

### 7.1.2 Configurable but safe

**Workspace branding (Workspace Admin, Settings › Branding):**

| Setting | Options | Guardrail |
|---|---|---|
| Accent colour | Any colour (picker or hex) | The system derives the full ramp and **checks every use**: text on accent, accent on white/dark, and focus. If a step fails contrast, it **auto-adjusts lightness** to the nearest passing value, with a preview before/after. If no passing variant exists (e.g. pure yellow), it **rejects** with an explanation. |
| Logo | SVG/PNG, light and dark variants | Size and safe-area constrained; the logo never replaces the text product name for screen readers |
| Font | One of the vetted list | — |
| Density default | Comfortable / compact | Users can override |
| Corner radius preset | Sharp / standard / soft | — |
| Theme default | Light / dark / auto | Users can override |
| Email branding | Logo, accent, footer text | Same contrast checks |

**Personal preferences (every user, Profile › Display):**
- Text size 100 / 112.5 / 125 / 137.5 / 150% (scales the rem root; layouts are tested at each step).
- Density.
- Theme.
- Reduced motion (overrides OS).
- **High-contrast theme:** ≥ 7:1 text, 3px focus, stronger borders.
- Underline links always.
- Keyboard shortcuts on/off.

Preferences follow the user across workspaces. Branding stays per workspace.

### 7.2 Component inventory

App shell, nav, workspace switcher, command palette, DataGrid, filter builder, chips, saved-view menu, form controls (text, number, money, date/time with timezone, person picker, select, combobox, file dropzone), status chip, stepper, timeline, diff viewer, comment thread, block editor, step builder, flow canvas, properties panel, permission matrix, impact-preview modal, step-up auth dialog, signature dialog (with meaning and manifestation), banners (break-glass, test mode, suspended workspace), toasts, empty state, skeletons, charts (line, bar, distribution, funnel).

### 7.3 Libraries (licences to verify in SP-07)

| Need | Choice | Licence |
|---|---|---|
| Accessible primitives | **React Aria Components** (preferred, strongest a11y semantics) or Radix Primitives | Apache-2.0 / MIT |
| Grid | TanStack Table + TanStack Virtual | MIT |
| Step canvas | React Flow (xyflow) | MIT |
| BPMN view / import canvas (P2) | **diagram-js + bpmn-moddle with our own renderer**. Not bpmn-js, whose watermark clause blocks white-labelling. `bpmn-visualization` (Apache-2.0) for read-only views. | MIT / Apache-2.0 |
| Layout | elkjs (unmodified) | EPL-2.0 |
| Forms | RJSF renderer + our own builder | Apache-2.0 |
| Icons | Lucide | ISC |
| Charts | Visx or ECharts | MIT / Apache-2.0 |

## 8. Content design and terminology

- **Voice:** plain, specific, active. "Approve request", not "Submit decision". Errors say what happened and what to do. No blame, no codes up front (codes appear in details).
- **Glossary-locked terms** in UI copy: Request (instance), Step, Task, Decision, Owner, Process, SOP, Version, Case. "Instance", "token" and "IR" appear only in Advanced and admin ops views.
- **Terminology overrides** (FR-W04): a workspace may rename Request → Ticket and Case → Matter. Overrides apply to UI, emails and exports through the i18n layer as message parameters, never by string replacement.
- **i18n:**
  - ICU MessageFormat with no concatenation.
  - Dates and numbers via `Intl`, shown in the viewer's timezone with the workspace timezone on hover.
  - Money with currency.
  - RTL-ready layouts using logical CSS properties (`margin-inline-start`), mirrored icons where directional, and pseudo-localisation in CI.

## 9. Notifications UX

- **Actionable by default.** Only send what needs action or matters: assigned, mentioned, decision made on my request, SLA risk, SOP to acknowledge.
- **Channels:** in-app (P0), email (P0), email-actionable links (P1), web push (P1), chat (integration, later).
- Each notification carries a deep link to the authorised item, and permissions are revalidated on open. Email bodies never include restricted fields.
- **Grouping and digests:** a daily digest by default for non-urgent items, quiet hours, and a critical SLA override per workspace policy.
- **User preferences:** per category × channel matrix. Admins set the defaults.
- **Email-actionable approvals:** the signed single-use link lands on a confirm screen showing context, and the user taps `Approve`. Part 11 processes always require re-authentication.

## 10. Accessibility checklist (WCAG 2.2 AA)

| Item | Requirement |
|---|---|
| Keyboard | Everything operable. Visible focus (2 px accent plus offset). No keyboard traps. Skip links. Shortcuts can be remapped or turned off (2.1.4). |
| 2.4.11 Focus not obscured | Sticky headers and footers never cover the focused element (scroll-padding). |
| 2.5.7 Dragging | Every drag (designer, columns, directory) has a menu or keyboard alternative. |
| 2.5.8 Target size | ≥ 24×24 px targets, 44 px on touch layouts. |
| 3.3.7 Redundant entry | Forms prefill known data. Multi-step forms keep entered values. |
| 3.3.8 Accessible authentication | Paste and password managers allowed. Passkeys offered. No cognitive puzzles in step-up or signing. |
| 3.2.6 Consistent help | Help link in the same place on every screen. |
| Contrast | 4.5:1 text, 3:1 UI and graphics, enforced for workspace accents. |
| Screen readers | Landmarks, headings, labelled controls, live regions for async results, accessible names for canvas nodes, and a text alternative view for diagrams (the step list). |
| Zoom | Usable at 200% and 400% reflow (320 CSS px). |
| Motion | Respects reduced motion. No flashing. |
| Exports | Tagged PDF/UA for SOPs and signed records. |
| Process | axe-core in CI (no violations), manual screen-reader passes (NVDA, VoiceOver) per milestone, external audit before GA, VPAT/ACR at GA. |

## 11. UX quality metrics and research plan

| Metric | Target |
|---|---|
| SUS (requesters/approvers) | ≥ 80 |
| SUS (designers/admins) | ≥ 70 |
| Untrained task success: start request, approve, acknowledge SOP | ≥ 80% |
| Time to first decision from inbox | ≤ 3 interactions, median < 20 s |
| Template → published (owner) | ≤ 30 min |
| Admin finds and reassigns overdue work | ≤ 2 min, no help |
| Investigation: find all accesses to a field in 30 days | ≤ 5 min, no help |
| Accessibility defects at audit | 0 critical, 0 serious |

**Research cadence:**
- Formative tests with 5–8 participants per persona per round.
- Round 1: prototype, M1.
- Round 2: governance alpha, M3.
- Round 3: execution beta, M5.
- Round 4: pilot, M6, plus a diary study.
- Round 5: GA accessibility and admin console.

**Standard tasks:** locate an SOP, start a purchase request, approve in the inbox, set OOO, change a step owner, publish a revised process, install a template, create a custom role, find overdue work and reassign, export evidence, investigate a field access. Each round also tests narrow screens and 200% zoom, and there are no hover-only controls.

## 12. UX backlog

Milestone references point to [07-ROADMAP-AND-MILESTONES](07-ROADMAP-AND-MILESTONES.md).

| ID | Title | Milestone | Size |
|---|---|---|---|
| UX-001 | Design tokens, theming (accent with contrast enforcement), dark-ready | M1 | M |
| UX-002 | App shell, permission-shaped nav, workspace switcher | M1 | M |
| UX-003 | Command palette (⌘K) and global search page | M2 | M |
| UX-004 | Shared DataGrid: sort, filter builder, chips, columns, saved views, export, bulk preview, responsive cards | M2 | L |
| UX-005 | Empty/loading/error/no-permission state kit | M1 | S |
| UX-006 | Library and Process detail (incl. versions and semantic diff viewer) | M2 | L |
| UX-007 | SOP block editor, review mode, acknowledgement flow | M3 | L |
| UX-008 | Governance lifecycle chips, publish dialog, scheduled countdown | M3 | M |
| UX-009 | Designer: step builder + properties panel + validation problems panel | M4 | L |
| UX-010 | Designer: flow canvas view with keyboard and non-drag alternatives | M4 | L |
| UX-011 | Test run split view with act-as and trace | M4 | M |
| UX-012 | Form builder with classification and role preview | M4 | L |
| UX-013 | Template gallery and 4-step install wizard; upgrade 3-way diff (P1) | M3 / M8 | M |
| UX-014 | Start request with "what happens next" and undo window | M5 | M |
| UX-015 | Request detail with stepper, activity, history, files | M5 | M |
| UX-016 | My Work 3-pane inbox with keyboard triage, OOO, safe bulk approve | M5 | L |
| UX-017 | Admin Overview dashboard with drill-downs | M5 | M |
| UX-018 | Admin Members, Directory, Settings, Calendars | M2 | L |
| UX-019 | Roles editor: permission matrix, effect preview, step-up | M2 | M |
| UX-020 | Admin Requests (all), Workload, Process health, Ops monitor | M5 | L |
| UX-021 | Admin Files view, quota breakdown, quarantine tab | M5 | M |
| UX-022 | Audit explorer with timelines, diff view and chain badge | M3 | L |
| UX-023 | Investigations workspace with pin-to-investigation and signed export | M8 | M |
| UX-024 | Alert rules editor with preview, and alert inbox | M8 | M |
| UX-025 | System Admin console: workspaces, quotas, gallery management, health, system audit | M2 | L |
| UX-026 | Break-glass request/approval flow and banners | M8 | S |
| UX-027 | Cases board and case page with closure panel | M8 | L |
| UX-028 | Improve: standard reports and version compare | M6 | M |
| UX-029 | Notification centre, preferences matrix, email templates | M5 | M |
| UX-030 | Email-actionable approval confirm screen and web push (PWA) | M8 | M |
| UX-031 | Signature dialog (meaning, re-auth) and manifestation rendering | M8 | M |
| UX-032 | Mobile PWA pass: My Work, start, detail, acknowledge at 360 px | M6 | M |
| UX-033 | i18n pipeline, pseudo-localisation, RTL audit, terminology overrides | M6 | M |
| UX-034 | Accessibility: axe CI, SR passes per milestone, external audit, VPAT | M1→M10 | L |
| UX-035 | Usability research rounds 1–5 and SUS tracking | M1→M10 | M |
| UX-036 | Custom dashboard builder (P1) | M9 | L |
| UX-037 | Audience & scale: startup first-run wizard (3 questions → templates → live), module toggles, progressive promotion of power features | M3 | M |
| UX-038 | Policies: library, document-like read view, authoring fields, applicability, public link, publish → campaign preview | M3 | L |
| UX-039 | Acknowledgement & Signature campaign engine UX: employee Acknowledgements/Signatures sections in My Work; receipts; quiz | M5 | L |
| UX-040 | Campaign tracker: campaigns DataGrid, completion by department, remind/escalate/exempt, evidence export | M5 | M |
| UX-041 | Announcements: compose with audience targeting and reach count, schedule/expiry/pin, require-ack, reading-layout feed | M5 | M |
| UX-042 | Document signing: send flow (recipients, bulk, order), click-to-place field editor with keyboard positioning | M8 | L |
| UX-043 | Signing ceremony: mobile-first guided fields, adopt signature, Part 11 re-auth, decline, completion certificate view | M8 | L |
| UX-044 | Documents › Sent envelope tracking with per-recipient status, remind/void/resend | M8 | M |
| UX-045 | Visual language spec and anti-pattern review checklist in design review / PR template | M1 | S |
| UX-046 | Normative token package (type, spacing, targets, contrast, radius, elevation, motion) + CI lint banning raw values | M1 | M |
| UX-047 | Accent → accessible palette derivation with auto-adjust/reject and branding preview | M2 | M |
| UX-048 | Personal display preferences (text size 100–150%, density, theme, reduced motion, high contrast, underline links) | M3 | M |
| UX-049 | Environment conformance: 320px reflow, 200/400% zoom, forced-colors, prefers-contrast, print styles: automated visual tests | M3→M10 | M |
