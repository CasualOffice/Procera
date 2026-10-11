# Information architecture: competitive analysis and redesign proposal (2026-10-12)

Status: proposal for owner review. Companion to [ia-audit.md](ia-audit.md), which records what each
persona sees today and the problems found. This document looks at how comparable products solve
the same problems and proposes a new information architecture (IA) for Procera.

**Method and evidence rules.** Every competitor claim below is from the vendor's own help or docs
pages (or, where marked, a customer-published user guide or third-party listing) fetched or searched
on 2026-10-12. Sources are numbered `[n]` and listed at the end. When something could not be checked
it is marked *not verified*, and we don't build on it. Products whose docs were thin (SweetProcess,
Rippling's in-app navigation) are covered only as far as the sources go.

---

## 1. Executive summary

1. **Split "what I must do" from "what I should know".** Asana says it plainly: "everything you
   need to do lives in your My tasks, while everything you need to know lives in your Inbox" [21].
   Linear (Inbox vs My issues) [17][18], Process Street (My Work) [9] and ServiceNow (My To-dos)
   [27] all keep the action list separate from the news feed. Procera has two inbox-like places
   (Home "For you" and Notifications) and neither is clearly the to-do list.
2. **The employee experience should be about one screen.** Trainual general users "only see the
   Home page left-hand navigation option" unless admins turn on more [12]. The PowerDMS homepage is
   "a snapshot of your assignments" with a To Do list of "all the items you are required to complete
   or sign" [1]. Procera shows employees 7 items.
3. **Admin is a separate place that you switch into, not a list in the everyday sidebar.** Lattice
   has "Switch to admin" [24]. Linear keeps an "Administration" section inside Settings that only
   admins see [19]. GitHub has an org **Settings** tab [33], Slack has "Tools & settings" →
   workspace or organisation settings [31], and Microsoft 365 and Google have whole separate admin
   centres [28][30]. None of them puts 11 admin destinations next to daily work.
4. **Group admin by the object being managed, and keep it shallow.** Google's admin menu "groups
   options based on commonly used services, such as Directory and Security" [30]. Microsoft 365
   uses Users / Teams and groups / Roles / Settings / Setup / Reports / Health [28]. Procera's 11
   items fold into about 4 groups: **People, Content and communication, Data and security,
   Workspace**.
5. **Show the simple view first and let experts expand it.** Microsoft 365 ships a *simplified view*
   ("top actions for you") and a *dashboard view*, with "Show all" in the nav [28]. Google lets
   admins **pin up to 5 pages** [30]. This is progressive disclosure as NN/g defines it: secondary
   options are deferred to a subsidiary screen [34].
6. **Navigation should follow role, with roles layered on a shared baseline.** Lattice: "the
   navigation panel will only include tools that an admin has enabled" [23]. Workday Home is made
   of cards "that will display based on your role", and managers get extra team sections [14][15].
   SharePoint audience-targets navigation links so users "see only the navigation links that are
   relevant to them" [7]. Procera filters by permission but doesn't shape by persona, and its roles
   are not additive (ia-audit problem 6).
7. **Show pending actions where people already look, with a count and a one-click action.**
   BambooHR puts an inbox badge on Home listing approvals [13]. Personio's Home widget shows the
   five most recent work items, which can be actioned in place [16]. Workday's "Awaiting your
   action" sorts by earliest due date [15]. ServiceNow offers Approve / Reject / Delegate as quick
   actions [27]. Guru's "My Tasks" ranks cards to verify by relevance [8].
8. **Directory: one hierarchy, many flat groups, and rules that keep both in sync.** Google: a user
   belongs to exactly one organisational unit (OU) and inherits its settings, while groups handle
   exceptions [35]. Rippling Supergroups and Entra dynamic groups compute membership from
   attributes such as department [38][37]. Notion and Slack groups exist so that "you only have to
   add them to the group" [40][39]. Procera already has the department tree, groups and managers
   APIs, but its only UI is CSV import. Creating and editing departments and groups in the UI is
   the biggest gap in the directory.
9. **Teach in place, not up front.** NN/g: empty states should show system status, explain what
   belongs here and give a direct path to the next action. In-context guidance at the moment of
   need works better than an upfront tutorial [36]. Microsoft 365's simplified view puts
   setup-aware "top actions" at the top [28]. Procera should give each role a "Get started"
   checklist and explain every empty page.
10. **Use plain names for pages.** The best-understood destinations are nouns for places (Home,
    Library, People) and plain verbs for actions (Acknowledge, Approve, Post). Product jargon goes
    into a "What's this?" line, not the page title. Microsoft and Google still use "Setup",
    "Health" and "Reports", but they explain each one inline [28].

**Extended-scope headlines** (sections 6–8):
- **Settings:** replace the single long page with 8 plain categories, each with a one-line
  description, search, and Advanced sections (Stripe [73], Microsoft [28]). Add "Configure" links
  from feature pages (Notion teamspace settings [40], Jira Service Management project settings
  [49]). Show department overrides with Google's Inherited / Overridden labels [74]. Hide the 11
  keys whose features don't exist yet (6.2).
- **Process landscape:** every service and workflow category has one actor inbox plus a requester
  portal (7.2). Procera's biggest near-term gaps are My Work (BPMS-105), the request portal
  (BPMS-103), forms (BPMS-101) and request approvals (BPMS-085/086). Three gaps are *not in the
  tracker*: directory UI, product onboarding and the settings redesign (7.3).
- **User management:** keep actions on the person page, grouped by intent. Make offboarding a
  "review → hand over → confirm" flow, as in Microsoft's 7-step guide [67] and Rippling's
  offboarding task lists [72] (8.3).

---

## 2. Pattern catalogue

Each pattern lists who uses it, what it looks like, why it lowers cognitive load, and sources.

### P1. Action inbox separate from the news feed ("to do" vs "to know")
- **Who:** Asana (My tasks vs Inbox) [20][21]; Linear (My issues vs Inbox, with a Priority tab that
  "separates notifications that need your attention from other updates") [17][18]; Process Street
  (My Work: tasks, approvals and workflow runs in one list) [9]; ServiceNow Employee Center (My
  To-dos with Open and Completed tasks; approvals in My Tasks / My Approvals) [27]; Personio (one
  Inbox "combining your notifications and tasks", with a Home widget) [16].
- **Looks like:** one list of items that need *me*, each with a type icon, a due date and an inline
  action. A separate feed for things to know about.
- **Why it helps:** people don't have to scan a mixed feed for obligations. "Is there anything I
  must do?" is answered by the count alone.

### P2. Due-date grouping and auto-prioritising
- **Who:** Process Street groups My Work by Overdue / Due today / this week / this month / later /
  no due date [9]. Asana's My tasks has Recently assigned / Today / Upcoming / Later, and moves
  tasks up automatically as they near their due date [20]. Workday sorts "Awaiting your action"
  by earliest due date [15]. Linear orders Assigned by focus and priority [18].
- **Why it helps:** the system orders the list, so the user doesn't have to.

### P3. The Home page is a role-shaped dashboard of cards
- **Who:** Workday (cards by role; "Awaiting your action" shows up to 4 items; announcements
  carousel; "Your Top Apps"; managers get "Team Highlights" and "Important Dates") [14][15];
  BambooHR (admin-chosen widgets; "What's Happening" lists approvals, e-signatures and onboarding
  tasks) [13]; Lattice ("a snapshot of where you are today": tasks, updates, 1:1s, quick actions)
  [22][23]; Viva Connections dashboard (audience-targeted cards) [6]; Microsoft 365 admin home
  (cards you can add, remove and rearrange) [28].
- **Why it helps:** Home answers "what's relevant to me today" without a trip through the menus.
  Different roles see different cards on the same page, and the URL stays the same.

### P4. Minimal navigation for consumers, more appears with permission
- **Who:** Trainual general users see only Home unless admins enable the directory, org chart or
  content [12]. Lattice shows only tools the admin enabled [23]. SharePoint audience-targets nav
  links [7]. In Notion you don't see private teamspaces unless you're a member [4].
- **Why it helps:** the person who only reads and acknowledges never meets authoring or admin
  concepts.

### P5. Admin is a separate mode or hub with its own grouped nav
- **Who:** Lattice "Switch to admin" opens an Admin page with sections: Directory, Settings
  (People, Company, Platform), Analytics, then one section per tool [24]. Linear: Settings has an
  "Administration" section only for admins and owners [19]. GitHub: org **Settings** tab; "Organization
  moderators only see moderation settings" [33]. Slack: "Tools & settings" → Workspace settings
  vs Organization settings (admin dashboard) [31][32]. Microsoft 365 and Google: separate admin
  centres [28][30].
- **Why it helps:** daily work and rare configuration are separated by context, not by scrolling.
  Note the risk Capterra reviewers raise about Rippling: when switching between admin and
  employee views, "it sometimes wasn't obvious which account they were in" [26]. So the mode must
  be clearly marked.

### P6. Admin grouped by object, shallow (two levels)
- **Who:** Microsoft 365 (Home, Users, Teams and groups, Roles, Resources, Billing, Support,
  Settings, Setup, Reports, Health, plus "Admin centers" for specialist depth) [28]. Google (groups
  such as Directory, Devices, Security, Billing, Reporting) [30]. Slack admin dashboard (Manage
  members; Settings and permissions; Configure access and security) [32].
- **Why it helps:** admins look for the *thing* (users, security, billing), not the feature.
  Specialist depth goes one level down (Microsoft's "specialist workspaces" such as Security,
  Exchange) [28].

### P7. Simplified vs full view; "Show all"; pinning
- **Who:** Microsoft 365 simplified view vs dashboard view, and "Show all" in the nav [28]. Google
  pins up to 5 pages [30]. Notion favourites "appear in your sidebar once you favorite your first
  page" [4][5]. Workday's "Top Apps" adapt to usage [14].
- **Why it helps:** the default is small and each person can add their frequent pages, so the
  product doesn't have to choose one layout for everyone.

### P8. Setup-aware "top actions" and getting-started lists
- **Who:** Microsoft 365 simplified view: "top actions for you… depending on what you've already
  set up" [28]. Linear Enterprise: "Member Onboarding" settings for Home view defaults and
  announcements [19]. Trainual assigns role-based training paths to new hires [11].
- **Why it helps:** a new owner always has one obvious next step, and the list shrinks as they
  complete it.

### P9. Explanatory empty states
- **Who / source:** NN/g's three guidelines (show status, teach what goes here, give a path to the
  key task) [36]. Process Street's My Work docs tell admins what to do so items appear ("assign
  tasks, approvals or workflow runs to users… so that they appear in each user's My Work") [10].
- **Why it helps:** the empty page is where people first learn a feature.

### P10. Approval and acknowledgement surfaced on the artefact *and* in the inbox
- **Who:** Confluence approvals sit in a panel under the page byline with Approve / Request
  changes, and the status shows on the page. The doc does not describe a central approvals inbox
  [3]. PowerDMS: the document drops off the To Do list and Inbox once signed, and "Compare to Last
  Revision I Signed" shows what changed before re-signing [1]. Guru: every card shows a verified or
  unverified status, timestamp and verifier, and verifiers get "My Tasks" [8]. BambooHR: inbox
  "Approvals" section [13].
- **Why it helps:** the person who must act finds the item from the inbox. The person who reads
  sees trust status on the page itself. Diff-before-re-ack keeps re-acknowledgement honest.

### P11. Groups as the unit of access and audience
- **Who:** Notion ("whenever a new member joins… add them to the group and they'll automatically
  be part of all the same teamspaces") [40]. Slack user groups with @handle and up to 100 default
  channels [39]. Google groups for exceptions, OUs for baseline policy [35]. Rippling Supergroups
  apply policies "so you never have to manually maintain who's attached to each policy" [38].
- **Why it helps:** admins manage a handful of groups instead of thousands of individual grants.

### P12. One picker for audiences and grants, with reach preview
- **Who:** SharePoint targets by SharePoint groups, distribution lists, security groups or "global
  audiences" [7]. Viva targets feed posts and cards by group [6]. Procera already specifies a "role
  change preview" ("this change gives 14 people…") in docs/12 §6.
- **Why it helps:** the same mental model ("who is this for?") applies everywhere: campaigns,
  announcements, collection visibility and roles.

### P13. Contextual help next to the thing
- **Who:** Microsoft 365 "Need help?" and "Help & support" on every page [28]. PowerDMS Help menu →
  Success Community [1]. NN/g: in-context guidance at the moment of need [36].
- **Why it helps:** terms like "retention" or "separation of duties" are explained where they
  appear.

### P14. Keyboard and command access for experts
- **Who:** Linear (`G` then `I` for Inbox, `J`/`K` to move, `H` to snooze) [17]. Google admin
  search [30]. Microsoft 365 search bar and "Show all" [28].
- **Why it helps:** experts can skip the nav entirely, so the nav can be designed for novices.

### Product-by-product notes (compact)

| Product | Nav depth and grouping | Persona or role views | Admin separation | Inbox / My work | First-run and help | Sources |
|---|---|---|---|---|---|---|
| PowerDMS | Left menu with "Find Anything" search and categories (Documents, Courses, Users) | Admin dashboard vs user homepage | Admin features in the same app; *details not verified* | Homepage To Do list of items to complete or sign; Inbox | Help menu → Success Community | [1][2] |
| Trainual | General users: Home only by default | Permission levels (general user, author/content creator, admin) | Admins enable directory, org chart, content | Assigned content on Home | Role-based training paths for new hires | [11][12] |
| Process Street | My Work hub across workflows | Admins, Builders, Users, Guests ("can view only their own tasks") | Builder vs user roles | My Work: tasks, approvals, workflow runs; due-date groups; snooze | Getting-started guidance for admins | [9][10] |
| SweetProcess | *Not verified* beyond: procedures, processes, policies; acknowledgement per procedure; submit to manager for approval | n/a | n/a | n/a | n/a | third-party summary only; not relied on |
| Confluence | Spaces → page tree | Space admins configure approvals | Space settings vs site admin | No central approvals list documented; act on the page | n/a | [3] |
| SharePoint / Viva Connections | Feed, Dashboard, Resources | Audience targeting of cards, posts and nav links | SharePoint admin centre | Dashboard cards to act from | Curated Resources links | [6][7] |
| Notion | Sidebar: Favourites, Teamspaces, Shared, Private | Private teamspaces hidden from non-members | "Settings & members" (Groups tab) | n/a | Favourites appear after first use | [4][5][40] |
| Guru | Cards with verification status | Verifiers / experts | n/a | My Tasks ranked by relevance | One-click verify | [8] |
| Workday | Home of role-based cards; apps | Manager-only sections | Separate admin tasks | "Awaiting your action" (top items, due-date sorted), My Tasks inbox | Timely suggestions | [14][15] |
| BambooHR | Employee tabs: Home, My Info, People, Files | Access levels; admin-chosen Home widgets | Settings area (for example Employee Fields) | Inbox with count and Approvals sections; "What's Happening" | n/a | [13][41] |
| Personio | Home widgets | Approver tasks appear only for approvers | n/a | Unified Inbox (tasks and notifications); Home widget shows 5 latest, actionable in place | n/a | [16] |
| Lattice | Sidebar per enabled tool | Employee / manager / admin guides | "Switch to admin" → Admin page | Home tasks | Per-persona navigation guides | [22][23][24] |
| Rippling | *In-app nav not verified* | Role-based permissions drive app and admin access | *Not verified*; reviewers note confusion switching views | n/a | n/a | [25][26] |
| Google Workspace Admin | Grouped side menu, pin up to 5 | Admin role privileges decide what is in the console | Separate console | n/a | Search | [29][30] |
| Microsoft 365 admin | ~12 groups plus specialist admin centres | "Tailored and customizable for each admin and role" | Separate centre | Home cards | Simplified view, top actions, Learn tab, Need help | [28] |
| Slack | Tools & settings menu | Workspace vs org admins | Admin dashboard | n/a | n/a | [31][32] |
| Linear | Inbox, My issues, teams; Settings | Admin-only "Administration" in Settings | Settings | Inbox (Priority tab), My issues tabs | Member Onboarding defaults (Enterprise) | [17][18][19] |
| GitHub org | Settings tab | Owners vs moderators see different settings | Settings tab | n/a | n/a | [33] |
| Asana | My tasks, Inbox | n/a | n/a | My tasks sections; Inbox for updates | n/a | [20][21] |
| ServiceNow Employee Center | Portal for employees; fulfiller workspaces for agents | Approvers see My Approvals | Separate workspaces | My To-dos: open and completed, urgency markers; quick Approve / Reject / Delegate | n/a | [27] |

---

## 3. Directory management patterns

Procera's backend already supports a department tree (sibling-unique names, no moving a node under
its own descendants), static groups, positions (behind a flag), manager links with cycle rejection,
and CSV import with plan and apply (docs/17, BPMS-023). Departments and groups are already
authorisation scopes and audience sources (docs/12 §2, §6). What's missing is the UI to create and
edit them.

### 3.1 How comparable products model structure

| Product | Hierarchy (one per person) | Flat / cross-cutting sets | Rule-based membership | Managers / reporting lines | How they plug into access and audiences | Sources |
|---|---|---|---|---|---|---|
| Google Workspace | **Organisational units**: top-level OU plus child OUs; "a user… belongs to only one organizational unit and inherits that organizational unit's settings"; children inherit unless overridden | **Groups** (name, email, description, access settings) for exceptions, email, Drive sharing; configuration and access groups | **Dynamic groups** from queries on standard attributes (including Department) or custom attributes (Enterprise plans) | User attributes (*manager UI not verified*) | OUs = baseline policy; groups = exceptions and sharing targets | [35][42][43] |
| Microsoft 365 / Entra | No OU tree in the M365 admin centre; Department is an attribute | Teams and groups: M365 groups, distribution, security groups, shared mailboxes | **Dynamic membership** rules such as `user.department -eq "Sales"`; "Direct Reports for" rule | Manager attribute on the user; drives org charts and approval routing (per a third-party guide) | Roles page assigns admin roles; groups gate resources | [28][37][44] |
| Rippling | Departments with optional parent (per third-party API listings); titles, work locations, teams | Teams | **Supergroups**: dynamic lists from attributes such as department, start date, location; "If an employee moves or changes roles, their policies update automatically" | Manager on employee record (*UI not verified*) | Supergroups drive permission profiles and policies | [25][38][45] |
| BambooHR | Department, Division, Location are **list fields** on the employee (Settings → Employee Fields), with history-aware edits ("Change for future but leave history alone") | n/a | n/a | "Reports to" on job info; org chart | Access levels; Home widgets | [41][46] |
| Slack | n/a | **User groups**: name plus @handle; up to 100 default channels; Enterprise can sync IdP groups | IdP-synced groups (Enterprise Grid) | n/a | Mentions, default channels, workspace access | [39] |
| Notion | n/a | **Groups** in Settings & members → Groups tab: shows teamspaces each group belongs to and member count; "+ Add members"; "Create teamspace from group" | n/a | n/a | Groups grant teamspace and page access | [40] |
| Linear | **Teams and sub-teams**, nested up to five levels; set the parent under Settings → Teams → Team hierarchy (admin); sub-teams inherit workflows and labels | n/a | n/a | n/a | Teams own workflows; settings inherit | [47] |

### 3.2 Patterns to copy

- **D1. Exactly one primary hierarchy per person.** Google OUs [35] and BambooHR's single
  Department field [41] both make "where does this person sit?" a single answer. Procera
  departments should work the same way: one department per member, a tree, and inheritance by
  subtree. Procera scopes already include "department and subtree" (docs/12 §5).
- **D2. Many flat groups for everything that crosses the tree.** Google uses groups for
  exceptions [35], Slack for @mentions and default channels [39], Notion for access [40]. Groups
  in Procera: "Fire wardens", "ISO auditors", "All contractors".
- **D3. Rule-based ("smart") membership so access follows HR changes.** Google dynamic groups
  [43], Entra dynamic rules [37], Rippling Supergroups [38]. Procera already allows role
  assignment to a "directory rule" (docs/12 §6 rule 3), so the UI should expose it as a first-class
  group type later (see open questions).
- **D4. Inheritance is visible.** Linear sub-teams inherit and show it [47]. Google children
  inherit unless overridden [35]. A Procera department page should show inherited grants and
  audiences, not just direct ones.
- **D5. "Used by" before delete.** Notion's Groups tab shows the teamspaces each group belongs to
  [40]. Procera should show which roles, campaign audiences, collection visibility and RACI
  entries reference a department or group before it can be renamed, moved or deleted.
- **D6. Bulk paths alongside single-item editing.** Google "Bulk update users" with a downloadable
  CSV template and a task-progress view [42]. Microsoft "Add multiple users" with a CSV [44].
  Procera's CSV import with a check step (plan/apply, per-row reasons) is already better than
  most, but it should be *one* path, not the only one.
- **D7. Names that match the shape.** Department = the hierarchy (one each). Group = flat and many.
  Avoid "Team" as a third concept. Keep "My team" to mean a manager's direct reports (Workday uses
  "Team Highlights" for direct reports [15]).
- **D8. History-safe renames.** BambooHR asks whether a list-value change should rewrite history
  [41]. Procera's audit chain and campaign snapshots make this important: renaming "Ops" to
  "Operations" must not rewrite evidence. The UI should say so.

---

## 4. Personas and jobs-to-be-done

Frequency: **D** daily, **W** weekly, **M** monthly, **Q** quarterly or rarer, **E** on event.

### 4.1 Core personas

| Persona | Top jobs (JTBD) | Freq. | What Home must show |
|---|---|---|---|
| **Employee (everyone)** | 1. Acknowledge a policy or SOP I've been asked to read. 2. Read the latest announcements and confirm the ones that need it. 3. Find the current version of a policy or SOP fast. 4. See what changed since I last acknowledged. | 1 E/W, 2 W, 3 W, 4 E | "Needs you" (acks due, overdue first); "News" (unread, must-confirm pinned); search box; recently viewed |
| **Author / owner** | 1. Draft or update an SOP or policy. 2. Send it for review and see where it's stuck. 3. Respond to review comments. 4. Re-certify documents due for periodic review. 5. Start from a template. | 1 W, 2 W, 3 W, 4 M/Q, 5 M | "My documents": drafts, in review (with who's blocking), due for review; "New" button |
| **Reviewer / approver** | 1. Review or approve a document assigned to me. 2. See what changed (diff) before approving. 3. Request changes with comments. 4. Delegate when away. | 1 W, 2 W, 3 W, 4 Q | "Reviews waiting for you", oldest first, with SLA; one-click open with diff |
| **Comms** | 1. Post an announcement to the right audience. 2. Require read or confirm receipts. 3. Check who hasn't read, then nudge. 4. Schedule or retire posts. | 1 W, 2 W, 3 W, 4 M | "Your posts": reach and read %; "Post" button |
| **HR / compliance campaign runner** | 1. Launch an acknowledgement campaign for a policy version. 2. Track completion and chase stragglers (reminders, manager escalation). 3. Export evidence for an audit. 4. Make sure new joiners get the policies. | 1 M, 2 W, 3 Q/E, 4 E | "Active campaigns" with % done and overdue counts; "Evidence ready to export" |
| **Workspace admin** | 1. Invite, deactivate and move people. 2. Maintain departments, groups and managers. 3. Grant roles and check who has what. 4. Adjust workspace settings. | 1 W, 2 M, 3 M, 4 Q | Setup checklist until done; then "People health" (no department, no manager, pending invites); recent admin changes |
| **Security / IT** | 1. Search the audit log for an event or person. 2. Export audit evidence. 3. Review retention and legal holds. 4. Rotate keys and check encryption status. 5. Review who holds admin and sensitive roles. | 1 W/E, 2 Q/E, 3 Q, 4 Q, 5 Q | "Security health": privileged role holders, last key rotation, failed sign-ins, retention jobs |
| **System admin (operator)** | 1. Create and suspend workspaces. 2. Watch health, quotas and job queues. 3. Handle incidents and maintenance mode. | 1 E, 2 D/W, 3 E | System console only (separate app area; never mixed with workspace nav) |

### 4.2 Leader and manager personas (extended scope)

| Persona | Top jobs (JTBD) | Freq. | Home cards | Nav they need |
|---|---|---|---|---|
| **HR / People team** | 1. Onboard new joiners: department, manager, onboarding acknowledgements. 2. Run policy acknowledgement campaigns. 3. Post HR announcements. 4. Keep the directory accurate (moves, leavers, manager changes). 5. Answer "has X acknowledged Y?" | 1 W, 2 M, 3 W, 4 W, 5 W | New joiners this week (with ack status); active campaigns; directory gaps (no manager or department); my posts | Home, Library, News (Manage), Campaigns, Admin → People |
| **CTO / Engineering head** | 1. Own incident-response and access-management SOPs. 2. Approve security policies. 3. See who has access to what (privileged roles, scoped grants) for engineering. 4. Get engineers to acknowledge security policies. | 1 M, 2 M, 3 Q, 4 Q | Reviews waiting; engineering documents due for review; ack completion for Engineering; "Who has access" summary | Home, Library (Engineering collection pinned), My documents, Campaigns (view), Admin → People → Access review (read) |
| **CFO / Finance head** | 1. Own finance policies (expenses, procurement, delegation of authority). 2. Approve changes to finance policies under separation of duties. 3. Pull audit evidence (who acknowledged, approval trail) for auditors. 4. Check that the delegation-of-authority matrix is current and acknowledged. | 1 Q, 2 M, 3 Q/E, 4 Q | Approvals waiting; finance documents due for review; evidence pack for Finance collection | Home, Library (Finance pinned), My documents, Evidence (export) |
| **People manager** | 1. See my team's overdue acknowledgements and nudge. 2. Act on escalations from campaigns. 3. Check onboarding of new reports. 4. Approve items routed by reporting line (later: requests). | 1 W, 2 E, 3 E, 4 W | "My team": overdue acks by person, escalations, new reports | Home, Library, News, My team |
| **Compliance officer** | 1. Approve controlled changes. 2. Run attestations and campaigns. 3. Export evidence and audit trails. 4. Monitor overdue reviews workspace-wide. | 1 M, 2 Q, 3 Q/E, 4 M | Overdue reviews (all); campaign health; evidence exports | Home, Library, Campaigns, Evidence, Audit (via Admin → Security) |

### 4.3 Suggested built-in role templates (conceptual)

Built on docs/12 §5 permissions. **Rule: every template is additive on top of `Member`.** This
fixes ia-audit problem 6. Leader templates are *presets with a scope prompt*: when granting, the
admin picks the collection or department.

| Template (display name) | Maps to existing (docs/12 §6) | Default scope | Bundles (conceptual) |
|---|---|---|---|
| **Member** | Member | workspace | View visible library; acknowledge; read news; own notifications |
| **Author** | Process Owner / Designer (SOP and policy subset) | collection | `sop`/`policy` create, edit_draft, submit; `template` browse, install |
| **Reviewer** | Reviewer / Approver | collection | `sop`/`policy` review, approve (SoD-enforced) |
| **Comms** | *new preset* | workspace or department | `announcement` create, publish, manage; audience view |
| **HR** | *new preset* | workspace | `member` view, invite, deactivate; `directory` manage; `campaign` manage; `announcement` create, publish; `sop`/`policy` view; evidence export for campaigns |
| **Manager** | Team Manager (derived from directory) | own reports / department | `work` view_team; campaign progress for own team; nudge; receive escalations |
| **Engineering lead** | *new preset* = Author + Reviewer on chosen collection, plus `role:view` and `directory:view` | collection | Draft and approve SOPs in scope; read who-has-access for their department |
| **Finance lead** | *new preset* = Author + Reviewer on chosen collection, plus `campaign:view` and scoped evidence export | collection | Own and approve finance policies; export acknowledgement and approval evidence for scope |
| **Compliance officer** | Compliance Officer | workspace | Approve controlled changes; `campaign` manage; `audit` view, export; `control`/`attestation` |
| **Security admin** | Security Admin | workspace | `audit` view, export, investigate; auth policy; API tokens; `file` manage_retention; encryption |
| **Workspace admin** | Workspace Admin | workspace | Everything but deleting the workspace (docs/12) |
| **Owner** | Workspace Owner | workspace | All, including delete and billing; people only |

---

## 5. Recommendations for Procera

### 5.1 Proposed information architecture

**Principles:** at most 5 items in everyday nav for any persona (7 for the heaviest). One **Admin**
entry for anything that configures the workspace. Work objects (campaigns, posts, documents) stay
in everyday nav, because the people who run them do so weekly. Settings move to Admin.

**Everyday sidebar.** Items appear only when the person holds the permission (P4):

| Item | Who sees it | What it is |
|---|---|---|
| **Home** | everyone | Role-shaped "Needs you" plus cards (5.3). Badge = count of items needing action |
| **Library** | everyone | All documents you can see (replaces Policies and Library). Default view for readers: "Applies to me". Authors get status filters. Templates appear as a filter and inside **New** |
| **News** | everyone | Tabs: **Feed** (everyone) · **Manage** (Comms, HR: drafts, scheduled, receipts) |
| **My documents** | Author, Owner, leads | Tabs: Drafts · In review · Due for review · Owned |
| **My team** | Managers (derived) | Team members' pending acknowledgements, escalations, new reports |
| **Campaigns** | HR, Compliance (`campaign:manage`/`view`) | List, create, track, evidence export |
| **Admin** (gear, bottom of sidebar) | anyone with an admin permission | Opens the Admin hub (separate nav, P5). Clearly labelled "Admin", with an "Exit admin" control (fixes the Rippling confusion risk [26]) |

Header (not sidebar): **New** button (Author / Comms / HR: document, from template, announcement,
campaign; only the types you can create), **Search** (command palette, P14), **Bell** (the
notification feed, P1: things to *know*), account menu (profile, preferences, security).

**Admin hub.** A landing page with health and setup, then four groups (two levels, P6):

| Group | Pages (tabs inside) |
|---|---|
| **Overview** | Setup checklist (until complete), health cards (people gaps, overdue reviews, security signals), recent admin changes |
| **People** | Members · Departments · Groups · Roles & access · Access review (5.4) |
| **Content and communication** | Collections and visibility · Templates (workspace templates) · Document and review rules · Announcement and campaign defaults |
| **Data and security** | Audit log · Files · Retention · Encryption · Sign-in and security |
| **Workspace** | General (name, branding, language and region) · Notifications defaults · Plan (owner) |

Security and IT can **pin** up to 3 Admin pages (for example Audit log) into their sidebar (P7,
Google's pin pattern [30]). That gives the security persona a 4–6 item nav without everyone
paying for it.

**System console** stays a separate area, reached from the account menu only by system operators
(docs/12). It never appears in workspace nav.

**Per-persona default sidebars (result):**

| Persona | Sidebar |
|---|---|
| Employee | Home · Library · News (3) |
| Reviewer | Home · Library · News (3); reviews arrive on Home |
| Author / owner | Home · Library · News · My documents (4) |
| Comms | Home · Library · News (Manage tab) (3) |
| Manager | Home · Library · News · My team (4) |
| HR | Home · Library · News · Campaigns · Admin (5) |
| Engineering / Finance lead | Home · Library · News · My documents · (My team) (4–5) |
| Compliance | Home · Library · News · Campaigns · Admin (5) |
| Security / IT | Home · Library · News · Admin + pinned Audit log (5) |
| Workspace admin / owner | Home · Library · News · My documents · Campaigns · Admin (6) |

### 5.2 Names (plain verbs and nouns)

| Current | Proposed | Why |
|---|---|---|
| Policies + Library | **Library** | One place for documents; filters do the rest |
| Announcements / Manage announcements | **News** (Feed / Manage) | Short noun; Manage is a tab |
| Notifications (sidebar) | Bell icon: **Notifications** | Feed of things to know (P1) |
| Home "For you" | **Needs you** (section on Home) | Says what it is: obligations |
| Campaigns | **Campaigns**, with the subtitle "Ask people to read and confirm a document" | Industry term; explain inline |
| Members, People & access, Roles & access | **People** (Members / Departments / Groups / Roles & access / Access review) | One noun for one area |
| Overview (admin) | **Admin → Overview** | Landing page of the hub |
| Retention | **Retention**, with "How long records are kept and when they're deleted" | Keep the term, add a "What's this?" |
| Encryption | **Encryption**, with "Keys that protect your data" | Same |
| Audit log | **Audit log**, with "Every action, tamper-evident" | Same |
| Templates (sidebar) | **New → From template**; Library filter "Templates"; Admin → Templates to manage | Templates are a starting point, not a destination |

Action verbs on buttons: **Acknowledge**, **Approve**, **Request changes**, **Send for review**,
**Publish**, **Post**, **Remind**, **Export evidence**, **Invite**, **Add department**,
**New group**.

### 5.3 Role-based Home: a "what needs me" inbox

Layout (same URL for everyone; cards vary by role, P3):

1. **Needs you** (always first). Merged, deduplicated actions, grouped Overdue / Due this week /
   Later (P2). Each row has a type chip (Acknowledge, Review, Approve, Confirm news, Campaign
   escalation) and an inline primary action where it's safe: "Acknowledge" opens the doc with the
   statement; "Review" opens the diff. Empty state: "You're all caught up" plus what will show up
   here.
2. **Role cards** (only if the role applies):
   - Author: *My documents in review* (who's blocking), *Due for re-certification*.
   - Reviewer: *Waiting for you* (count, oldest age).
   - Comms: *Your recent posts* (read %, Remind).
   - HR / Compliance: *Active campaigns* (% complete, overdue, Remind all), *New joiners*.
   - Manager: *My team* (overdue acks by person, escalations).
   - Leads: *Documents in your collection due for review*, *Ack completion for your department*.
   - Admin: *Setup checklist* until done, then *People health*.
   - Security: *Security signals* (privileged role changes, failed step-ups, key age).
3. **News**: the 3 latest unread items; must-confirm items are pinned until confirmed.
4. **Quick links**: recent and favourite documents (Notion-style favourites appear after first
   use [4]).

The bell holds informational notices only. Anything actionable is also in Needs you, and clearing
it there clears the bell (PowerDMS removes items from both lists once signed [1]).

### 5.4 The "People" area (Admin → People)

**Tabs:** Members · Departments · Groups · Roles & access · Access review.

**Members.** The existing DataGrid with saved views ("Pending invites", "No department", "No
manager", "Deactivated"). Row actions: Edit, Change department, Change manager, Grant role,
Deactivate. **Bulk actions** on selection: Set department, Set manager, Add to group, Grant role,
Resend invite. Header: **Invite** (single or list of emails) and **Import CSV** (the existing
plan/apply flow, moved here from Settings). The person page shows department, manager, groups,
roles (direct and inherited, with source) and documents they must acknowledge.

**Departments: creation and editing flows** (new UI over existing APIs):
- *Layout:* tree on the left (search, expand and collapse, member counts), detail panel on the
  right: Head, members, sub-departments, inherited and direct grants, audiences and campaigns
  using it ("Used by", D5).
- *Add department:* button above the tree or "+" on a node → dialog with **Name** (required,
  unique among siblings; validated as you type), **Parent** (defaults to the selected node; picker
  over the tree), **Head** (optional person picker; drives manager fallback per docs/17
  BPMS-023), **Code** (optional). Then **Save and add people**, which opens a people picker
  filtered to "No department" first.
- *Add people:* from the department panel ("Add members" → multi-select, showing each person's
  current department and a "will move from X" note), from Members bulk "Set department", or from
  the person page. One department per person (D1).
- *Rename:* inline. Shows "Past records and evidence keep the old name" (D8).
- *Move:* drag in the tree or "Move to…". Moves under its own descendants are blocked with an
  explanation (matches the API). Preview: "Moves 3 sub-departments and 41 people; 2 role grants
  scoped here will now also apply under Operations."
- *Delete:* only when empty (no members, no sub-departments); otherwise the dialog lists what to
  move first, with links. References in "Used by" must be resolved or acknowledged.
- *Empty state:* "Departments mirror your org chart. Each person belongs to one. Use them to scope
  roles and target campaigns." Actions: **Add department** · **Import from CSV**.

**Groups: creation and editing flows:**
- *Layout:* list with name, purpose, member count, owner, "Used by" count.
- *New group:* **Name** (required), **Purpose** (one line, shown in pickers, like Google's
  description field [42]), **Owners** (who may edit membership without `directory:manage`),
  **Members** (people now; later "whole department" and rule-based members, D3). Save → group page.
- *Edit membership:* "+ Add members" (multi-select; can paste a list of emails), remove via row
  action. Change preview when the group is used in roles: "Adding 3 people gives them Reviewer on
  Finance policies" (reuses docs/12 role change preview).
- *Delete:* blocked while referenced. Shows the "Used by" list (roles, campaigns, collection
  visibility, RACI).
- *Empty state:* "Groups are flexible sets of people across departments, for example Fire wardens
  or Auditors. Use them for access and audiences." **New group**.

**Roles & access.** Two views of the same grants: **By role** (templates and custom roles, their
permissions in plain language, holders) and **By person** (replaces the separate "People &
access" page). Grant flow: pick a role → who (person / department / group / rule, one picker,
P12) → scope (workspace / department / collection) → preview of reach → step-up → save.

**Access review / health.** Read-only for leads, actionable for admins:
- Privileged holders (Owner, Admin, Security admin), with last-active date.
- Grants to departed or deactivated people; grants not used in 90 days (*proposal; needs usage
  data*).
- People without a department or manager; departments without a head; groups without an owner.
- "Who has access to X": pick a collection or document → effective access with source.
- Later: periodic recertification campaigns (docs/12 CMP-04), reusing the campaign engine.

### 5.5 First-run guidance

- **Role-specific "Get started" checklist** on Home (P8), dismissible and re-openable from Help:
  - *Owner / admin:* Name and brand the workspace → Add departments (or import) → Invite people →
    Assign roles (Author, Reviewer, HR) → Add or import your first policy → Ask everyone to
    acknowledge it. Each step deep-links and ticks itself from real data.
  - *Author:* Create your first document (or start from a template) → Send it for review → See it
    published.
  - *Reviewer:* What a review is → Open your first review → How separation of duties works.
  - *HR / Compliance:* Launch your first campaign → Watch progress → Export evidence.
  - *Comms:* Post your first announcement → Require confirmation → See receipts.
  - *Employee:* a one-card welcome: "Procera is where you read company policies, confirm you've
    read them, and get announcements. Anything you need to do appears under Needs you."
- **Explanatory empty states** on every list page, using the NN/g triple: status, what goes
  here, next action [36]. Admin-dependent pages say who can fill them ("Your admin hasn't added
  departments yet").
- **Inline "What's this?"** next to jargon in page headers (Campaigns, Retention, Encryption,
  Audit log, Separation of duties, Scope). This is a one-line popover with a "Learn more" link
  (P13).
- **Demo content:** the existing seeded demo workspace stays available for evaluation, but it is
  never mixed into a real workspace.

### 5.6 Progressive disclosure rules

1. **Permission gates visibility.** No nav item, button or tab is shown without the permission to
   use it. No disabled "teaser" items in nav.
2. **Layered roles.** Every role includes Member; nav = union of what the person's roles enable.
3. **Admin lives behind one door.** Configuration pages exist only inside Admin. The only exception
   is up to 3 user-pinned Admin pages.
4. **Max 5 everyday items** for any built-in persona, 7 hard cap.
5. **Two levels deep at most** in Admin (group → page → tabs). Deeper detail opens in a side panel
   or detail page, not a third nav level.
6. **Advanced sections collapsed.** Rare settings (key rotation, legal hold, SoD policy tuning) sit
   under an "Advanced" disclosure inside their page, as NN/g's "defer secondary options" [34].
7. **Work object first, settings second.** Campaigns, posts and documents stay in everyday nav.
   Their *defaults* live in Admin → Content and communication.
8. **Feature-flagged modules stay out of sight** until they're turned on (workflow requests, forms
   and e-sign arrive later and slot in as "Requests" in everyday nav plus an Admin group; no
   placeholders before then).
9. **Counts only for obligations.** Badges appear only for things that need action (Needs you,
   reviews), never for informational feeds, so a badge always means "act".

### 5.7 Anti-patterns in the current design

- **Two inboxes** (Home "For you" and Notifications) with no clear to-do vs to-know split.
- **Near-duplicate destinations**: Members / People & access / Roles & access; Announcements /
  Manage announcements; Policies / Library.
- **A flat list of 11 admin items** mixing weekly work (Campaigns) with yearly settings
  (Encryption).
- **The same map for every job**: permission filtering without persona shaping. Authors have no
  "my drafts" place; reviewers have no queue.
- **Templates as a top-level destination** for people who never create anything.
- **Employees seeing an "Administration" section** (Audit log, for their own activity).
- **Non-additive roles**: a Reviewer loses Announcements.
- **Directory without a UI**: departments and groups can only be created by CSV import, hidden in
  Settings.
- **Jargon page titles** with no inline explanation.

---

## 6. Settings design

The owner's complaint: Settings is crowded and confusing. Today it has three tabs (ia-audit.md,
"Settings"):
- **Workspace** holds the whole catalogue on one page: 63 keys in 16 internal groups
  (`crates/settings/src/catalogue.rs`), shown as 11 label areas (`web/src/pages/settingsMeta.ts`).
- **Departments** holds per-department overrides.
- **Configuration profile** shows the starting values written at workspace creation (Starter,
  Standard, Enterprise, Regulated).

Some keys belong to features that don't exist yet.

### 6.1 How comparable products structure settings

| Pattern | Who does it, per their docs | Notes |
|---|---|---|
| **A categories index with short descriptions** | Microsoft 365 Settings → Org settings has three tabs: Services, Security & privacy, Organization profile (per a government configuration guide [76]). The admin-centre menu table gives each area a one-line "What it's for" [28]. Stripe splits settings into **Personal, Account, Product**, and each product's settings has a one-line summary ("Radar: Manage fraud protection…") [73]. Linear's Settings separates personal and team settings from an admin-only "Administration" section [19]. | Each category says what it controls, in one line. |
| **Search** | The Microsoft 365 admin centre says to "use the search bar to quickly find what you're looking for" [28]. Google's admin console has search [30]. Linear reaches admin views via Command+K [70]. | Search should cover labels, help text and synonyms. |
| **Settings next to the feature** | Notion: teamspace settings open from the "…" menu next to the teamspace, while workspace settings live in "Settings & members" [40]. Jira Service Management: approval-email settings live under the service project's **Project settings** [49]. Stripe has per-product settings pages (Billing, Radar, Tax) [73]. | Two doors to the same page: a central index and a "Configure" link on the feature. |
| **Basic vs advanced** | Microsoft 365 *simplified view* vs *dashboard view*, and "Show all" [28]. NN/g: defer secondary options [34]. | Rare controls collapse under "Advanced". |
| **Outcome-based labels and help** | Microsoft labels areas by job ("Users: Create and manage users… set their permission level or reset their passwords") [28]. Procera's `settingsMeta.ts` already moved toward outcomes ("Sign out after inactivity"). | Finish the job: every label says what happens, and the key name is never shown. |
| **Defaults and recommendations** | Google's **Security health** page shows each security setting's status per org unit and a "green checkbox to indicate a secure configuration", or recommendations when it isn't [75]. | Show a "Recommended" marker where the workspace differs from the recommended value. |
| **Per-org-unit overrides** | Google: each setting shows "**Inherited**" or "**Overridden**", with **Override / Save / Inherit** buttons per org unit [74]. Children inherit unless overridden [35]. | This is the clearest model for department overrides. |
| **Change history** | Google: admin changes appear in **Admin log events** and the audit and investigation tool [65]. Settings changes "can be audited in the Admin Console audit log" [75]. Linear's audit log tracks "settings changes including the IP and country of the actor" (Enterprise) [70]. | Every setting can show "Last changed by…" with a link to its history. |
| **Pinning and shortcuts** | Stripe "Shortcuts: pinned and most recently visited pages" [73]. Google pins up to 5 pages [30]. | Useful for the admin hub, but not needed inside Settings. |

GitHub's org Settings tab is role-filtered ("Organization moderators only see moderation settings")
[33]. Its internal sidebar grouping could not be verified from the fetched docs. Rippling's settings
structure could not be verified, so neither is relied on here.

### 6.2 Proposed Procera settings architecture

**Shape.** Admin → Workspace → **Settings** opens a **category index**: 8 cards, each with a
one-line description, a count of changed-from-recommended values, and a search box at the top.
Each category is its own page (not one long page). In each page, everyday settings come first and
an **Advanced** disclosure holds the rest. The Departments tab and the Configuration profile tab go
away: they are presented in context (6.4, 6.5).

| # | Category (label) | One-line description shown on the index |
|---|---|---|
| 1 | **General & branding** | Name, logo, colours, language, time zone and the words Procera uses |
| 2 | **Sign-in & security** | Passwords, two-step verification, session length and admin access logging |
| 3 | **People & directory** | Who can join, what happens when someone leaves, positions |
| 4 | **Documents & reviews** | Who must review before publishing, review cycles, numbering |
| 5 | **Acknowledgements & announcements** | When people must re-acknowledge, campaign reminders and escalation, announcement rules |
| 6 | **Notifications** | Which channels Procera uses, digests and quiet hours |
| 7 | **Data, retention & files** | Upload limits, malware scanning, and how long records are kept |
| 8 | **Advanced** | Rarely changed, expert options (only shown when there is something in it) |
| (9) | **Requests** *(hidden until the requests module ships)* | Request numbering, cancellation, bulk approval, stuck-request alerts |

**Where every existing key goes.** **B** = basic, **A** = under Advanced, **H** = hidden until the
feature exists, **D** = a department may override it (from `catalogue.rs`).

| Key | Category | Show | Feature-page entry point |
|---|---|---|---|
| `branding.logo`, `branding.accent`, `branding.theme_default`, `branding.email_sender_name` | General & branding | B | — |
| `locale.default` | General & branding | B, D | — |
| `locale.timezone`, `locale.week_start` | General & branding | B, D | — |
| `terminology.overrides` | General & branding | A | — |
| `auth.mfa.required_roles` | Sign-in & security | B | Roles & access (role page: "Requires two-step verification") |
| `auth.password.min_length`, `auth.password.breach_check` | Sign-in & security | B | — |
| `auth.session.idle_timeout` | Sign-in & security | B | — |
| `auth.session.absolute_timeout`, `auth.stepup.window`, `auth.mfa.allowed_methods` | Sign-in & security | A | — |
| `access.admin_content_access_log` | Sign-in & security | A | Audit log ("Admin content access logging: on") |
| `api.tokens.max_ttl` | Sign-in & security | A, **H** until an API-token screen ships (BPMS-144) | API tokens page |
| `auth.invite_only`, `auth.allowed_email_domains` | People & directory ("Who can join") | B | Members → Invite dialog footer |
| `directory.deactivation.reassign_strategy` | People & directory ("When someone leaves") | B | Deactivate dialog ("Change default") |
| `directory.positions.enabled` | People & directory | A | Departments page |
| `access.delegation.self_service_ooo`, `access.delegation.max_depth` | People & directory | **H** until delegation ships (BPMS-086) | Account → Out of office |
| `governance.review.required_for`, `governance.review.min_reviewers` | Documents & reviews | B, D (min reviewers) | Library → "Review rules" |
| `governance.review_cadence.default` | Documents & reviews | B, D | Document page → "Review cycle" |
| `governance.review_cadence.controlled`, `governance.verification.validity` | Documents & reviews | A, D | — |
| `governance.reason_for_change.required` | Documents & reviews | B | Publish dialog |
| `policies.numbering_pattern` | Documents & reviews | A | — |
| `governance.reack.policy`, `governance.reack.window` | Acknowledgements & announcements | B, D | Publish dialog ("Ask people to acknowledge again") |
| `policies.attest_on_join` | Acknowledgements & announcements | B, D | Campaigns → "New joiners" |
| `campaign.default_due`, `campaign.reminders.cadence`, `campaign.escalation.to_manager_after`, `campaign.ack_text` | Acknowledgements & announcements | B, D | Campaigns → "Campaign defaults" |
| `campaign.audience.snapshot` | Acknowledgements & announcements | A, D | New campaign → audience step |
| `announcements.enabled`, `announcements.approval_required`, `announcements.require_ack.default`, `announcements.expiry.default` | Acknowledgements & announcements | B, D | News → Manage → "Announcement settings" |
| `announcements.channels` | Acknowledgements & announcements | B; **H** the values push, webhook and chat until those channels exist | News → Manage |
| `notify.channels.enabled` | Notifications | B; **H** unavailable channel values | — |
| `notify.digest.schedule`, `notify.quiet_hours.enabled`, `notify.quiet_hours.start`, `notify.quiet_hours.end` | Notifications | B | Account → Notification preferences ("Workspace defaults") |
| `notify.digest.hour`, `notify.critical_override_quiet` | Notifications | A | — |
| `files.max_size`, `files.scan.required` | Data, retention & files | B | Files page → "File settings" |
| `files.exif_strip`, `files.orphan_purge_after` | Data, retention & files | A | Files page |
| `retention.governance_record.keep_for`, `retention.operational.keep_for` | Data, retention & files | B | Retention page (the page stays the main entry point) |
| `retention.draft.inactive_after`, `retention.draft.notice_before` | Data, retention & files | A | Retention page |
| `exec.request_id_pattern`, `exec.cancel.allowed_for_initiator`, `exec.bulk_approve.allowed`, `exec.loop.max_iterations`, `monitoring.stuck_instance_threshold` | Requests | **H** until requests ship (BPMS-103/108) | Requests admin |

The **Advanced** category (8) is only for options that don't belong to any one feature. With the
mapping above it starts empty and stays hidden. Each category keeps its own Advanced disclosure
instead.

**Also hide unavailable choices *inside* settings.** `auth.mfa.required_roles` currently offers
roles for features that don't exist (Case manager, Investigator, Analyst). The picker should list
only roles that exist in this workspace, built-in or custom.

### 6.3 Rules for labels, help and defaults

1. **Labels say the outcome:** "Sign people out after 30 minutes idle", not "Session idle timeout".
   The existing `settingsMeta.ts` labels are a good base. Add a label and help text to every key: 13 keys have no label at all today
   (`governance.review_cadence.*`, `governance.verification.validity`, `notify.digest.*`,
   `notify.quiet_hours.*`, `notify.critical_override_quiet`, `retention.*`). Many others have a
   label but no help.
2. **Each value shows its source**, using `LAYER` with plainer words:
   - "Default"
   - "Recommended for Regulated" (from the configuration profile)
   - "Changed by Olivia on 3 Oct" (workspace value), with a link to its audit history
   - "Required by your platform" (floor, shown with a lock and the reason)
3. **Recommended markers.** When a security or governance value is weaker than the
   profile's recommendation, show a quiet "Recommended: 2 reviewers" hint with a one-click
   "Use recommended". This follows Google's Security health pattern [75].
4. **Immediate (⚡) keys** keep the reason prompt and say "Applies to everyone right away".
   Step-up keys say "You'll be asked to confirm it's you".
5. **Search** across labels, help text, values and synonyms (MFA, 2FA, two-step; SSO; password;
   retention; GDPR). Results open the category with the setting highlighted.
6. **History.** A "History" link on every row opens the audit log filtered to that key. Changes
   are already audited.

### 6.4 Department overrides, presented for non-experts

Use Google's Inherited / Overridden model [74]:
- On a setting that departments may override (D in the table), show a chip under the control:
  "**Same for all departments**" or "**Different in 2 departments**". Clicking the chip opens a
  side panel listing departments with their values and "Use workspace value" (= Google's
  *Inherit*) per row, plus "Set a different value for a department…" (= *Override*).
- On the **department page** (Admin → People → Departments → *Finance*), add a **Settings** tab.
  It shows only overridable settings, each labelled "Inherited from workspace" or "Set for
  Finance", with **Override / Use workspace value** buttons. A sub-department shows "Inherited
  from Finance" when the parent overrides it.
- One sentence at the top: "Departments use the workspace settings unless you change them here.
  Sub-departments follow their parent."
- Never show security (⚡) keys here. This matches the existing rule that departments can't
  change workspace security (catalogue.rs comment on `department`).

### 6.5 Configuration profiles, presented for non-experts

- Rename it in the UI to **"Starting point"**: "This workspace started from the **Standard**
  settings. You can change any setting; we'll show where you differ from Standard's
  recommendations."
- Show it once, as a banner on the Settings index (with "Compare with Regulated" for workspaces
  considering stricter controls), not as a tab.
- The profile's values become the **Recommended** markers in 6.3. Profiles are written once at
  creation (catalogue.rs `profile()`), so the UI must not imply that changing the profile
  rewrites settings. If a "switch starting point" action is ever added, it must preview every
  change, like the role change preview.

### 6.6 Feature-page entry points (the second door)

| Feature page | Link label | Opens |
|---|---|---|
| Campaigns | Campaign defaults | Acknowledgements & announcements → Campaigns section |
| News → Manage | Announcement settings | Acknowledgements & announcements → Announcements section |
| Library | Review rules | Documents & reviews |
| Publish dialog | "Why am I asked this?" next to reason-for-change and re-ack | The relevant setting (read-only for non-admins) |
| Members → Invite | Who can join | People & directory |
| Deactivate dialog | Change default | People & directory → When someone leaves |
| Files | File settings | Data, retention & files |
| Retention | (main page) | Retention settings embedded |
| Audit log | Logging settings | Sign-in & security → Advanced |
| Account → Notification preferences | Workspace defaults (admins only) | Notifications |
| Department page | Settings tab | Department overrides (6.4) |

People without `workspace:settings.manage` see these links as read-only ("Set by your admin:
reminders after 3, 7 and 14 days"). That also answers "why did I get this reminder?".

---

## 7. Process-management feature landscape and gap analysis

Procera's next phase (workflow requests, forms, e-sign) puts it next to service-management and
workflow products. This section shows what those categories offer, to set expectations. The
"Procera today" column uses docs/17 and `trackers/README.md` (34 of 109 epics done on
2026-10-12).

### 7.1 Categories and what they offer

| Category | Products studied | Employee side | Fulfiller side | Sources |
|---|---|---|---|---|
| **ITSM / ESM suites** | ServiceNow (Employee Center, catalog), Jira Service Management, Freshservice | Portal with service catalogue. Request forms with variables. "My Requests" with status and estimated delivery (ServiceNow). Approvals listed in a section. Approve by email (Freshservice; JSM) or from Slack / Teams (JSM). | Queues by priority, request type or SLA status (JSM). SLA goals, calendars and start / pause / stop conditions (JSM). SLA policies per department or group (Freshservice). Approval groups and chains (Freshservice). Delegation (ServiceNow). | [27][51][52][48][49][50][58] |
| **Help desk** | Zendesk | Help Center (*employee-facing internal help desk features not verified in this research*) | SLA policies, **group SLA policies for internal teams**, OLA policies, SLA reporting dashboard. Views as saved ticket lists; macros; triggers and automations. | [59][60] |
| **No-code workflow** | Kissflow, Pipefy, Process Street, monday.com | Start form and **public form** (Pipefy). Start a new item and "My Items" for what I started (Kissflow). Process Street My Work holds tasks, approvals and **acknowledgments**. | My Tasks across processes; reassign, send back, reject (Kissflow). Late and expired card labels for SLAs (Pipefy). Approval tasks, stop tasks, conditional logic (Process Street). monday builds approvals from WorkForms intake, status columns and automations; *a native approval feature was not verified* (community threads say none). | [9][54][55][56][57] |
| **BPM engines** | Camunda, Flowable | Start processes; forms (Camunda Forms) | Tasklist: claim or unclaim, filters by assignee, candidate group, priority (Camunda). Flowable Work: "Work" app (cases and processes I'm involved in) and "Tasks" inbox (assigned to me or my groups); work items have owner, assignee, due date, priority; mobile app. | [61][62] |

### 7.2 Capability matrix

Cells name the products where the capability was verified, with sources. "n/v" = not verified in
this research (it may exist). Procera status: **Available** (Done in docs/17), **In progress**,
**Planned** (in tracker, not started), **Not in tracker**.

| Capability | ITSM / ESM | Help desk | No-code workflow | BPM engines | Procera today |
|---|---|---|---|---|---|
| Employee portal / service catalogue | ServiceNow [52], JSM [51], Freshservice (visibility by group) [58] | Zendesk Help Center (n/v detail) | Pipefy start/public form [55]; monday WorkForms [57] | Flowable "Work" start [62] | **Planned**: BPMS-103 |
| Request forms with dynamic fields | ServiceNow variables [52]; JSM request type sets the form [51] | n/v | Process Street conditional logic [56]; Pipefy forms [55] | Camunda Forms [61] | **Planned**: BPMS-101 |
| Track my requests | ServiceNow My Requests [52]; JSM portal [51]; Freshservice [58] | n/v | Kissflow My Items [54] | Flowable Work app [62] | **Planned**: BPMS-103 |
| One inbox for everything I must act on | ServiceNow My To-dos / My Approvals [27] | Views (agents) [60] | Kissflow My Tasks [54]; Process Street My Work [9] | Camunda Tasklist [61]; Flowable Tasks [62] | **Partial**: Home "For you" + Notifications. **Planned**: BPMS-105 My Work |
| Multi-step approvals | JSM [51]; Freshservice groups and chains [58]; ServiceNow [27] | n/v | Process Street approval tasks [56]; Kissflow [54]; monday via automations [57] | BPMN user tasks [61] | **Available** for documents (BPMS-044, SoD). **Planned** for requests (BPMS-085/086) |
| Approve from email | JSM [49]; Freshservice [58] | n/v | Pipefy email link [55] | n/v | **Planned**: BPMS-145 |
| Approve or raise requests from Slack / Teams | JSM [49][50] | n/v | n/v | n/v | **Planned**: BPMS-200/201 (chat cards) |
| Queues / saved views for fulfillers | JSM queues [51] | Zendesk views [60] | Process Street filters [9] | Camunda filters [61] | **Partial**: DataGrid saved views (BPMS-028). **Planned**: BPMS-108, BPMS-160 |
| Claim / assignment strategies | n/v | n/v | Kissflow reassign [54]; Process Street reassign [9] | Camunda claim [61] | **Planned**: BPMS-085 |
| SLAs (goals, calendars, pause) | JSM [48]; Freshservice per dept/group [58] | Zendesk SLA and group SLA [59] | Pipefy late/expired [55] | Due dates [62] | **In progress**: BPMS-083 (SLA clocks, calendars). Campaign due dates **Available** |
| Escalation and reminders | n/v | Triggers / automations [60] | n/v | n/v | **Available** for campaigns (reminder ladder, manager escalation, BPMS-046). Requests **Planned** (083/085) |
| Macros / canned responses | n/v | Zendesk macros [60] | n/v | n/v | **Planned**: BPMS-160 (cases) |
| Delegation / out of office | ServiceNow granular delegation [27] | n/v | n/v | Camunda delegation state filter [61] | **Planned**: BPMS-086 (settings keys exist) |
| Send back / request changes | n/v | n/v | Kissflow send back [54] | n/v | **Available** for documents (request changes, BPMS-044). Requests **Planned** (BPMS-086) |
| Visual process designer | n/v | n/v | Pipefy phases [55]; Process Street editor [56] | Camunda Modeler / BPMN [61] | **Planned**: BPMS-100 (IR compiler **In progress**: BPMS-080) |
| Template library for processes | n/v | n/v | n/v | n/v | **Planned**: BPMS-106 |
| SOP / policy library | JSM portal knowledge articles [51] | Help Center | n/v | n/v | **Available**: BPMS-041/045 (a differentiator) |
| Read-and-acknowledge with evidence | n/v | n/v | Process Street acknowledgments [56] | n/v | **Available**: BPMS-046, version- and hash-bound (a differentiator) |
| Announcements with receipts | n/v | n/v | n/v | n/v | **Available**: BPMS-047 |
| Tamper-evident audit trail | n/v | n/v | n/v | n/v | **Available**: BPMS-026/064 (a differentiator) |
| E-signature | ServiceNow + DocuSign from Employee Center [53] | n/v | n/v | n/v | **Planned**: BPMS-107 |
| Public / external forms | n/v | n/v | Pipefy public form [55] | n/v | **Planned**: BPMS-148 |
| Mobile | n/v | n/v | n/v | Flowable mobile app [62] | **Planned**: BPMS-109 (PWA) |
| Reports and SLA dashboards | n/v | Zendesk SLA dashboard [59] | n/v | n/v | **Partial**: overview dashboard (BPMS-063). **Planned**: BPMS-166 |
| Request numbering, on-behalf, recall | n/v | n/v | n/v | n/v | **Planned**: BPMS-104 |
| Email-to-request intake | n/v | n/v | n/v | n/v | **Not in tracker** (BPMS-145 covers reply-by-email for approvals only) |
| Suggest articles while raising a request (deflection) | JSM portal shows knowledge [51] (deflection itself n/v) | n/v | n/v | n/v | **Not in tracker** explicitly (fits BPMS-103) |
| Incident / problem / change / asset (ITSM) | JSM covers these [51] | — | — | — | **Not in tracker**. Recommend out of scope |

### 7.3 Prioritised gap list

Ordered by how strongly category buyers expect it, and by how much it reuses what Procera has.

| # | Gap | Why it matters | Tracker |
|---|---|---|---|
| 1 | **One "Needs you" / My Work inbox** covering acks, reviews, approvals, campaign escalations and later request tasks | Every category has one (7.2). It also fixes Procera's two-inbox confusion now, before requests exist | BPMS-105 (**Planned**). Recommend pulling a document-only slice forward as the Home "Needs you" (5.3) |
| 2 | **Directory UI** (create and edit departments and groups, set managers, bulk set) | Managers, escalations, audiences and scoped roles all depend on it. Today it is CSV-only | **Not in tracker as remaining work** (BPMS-023 is Done at API level). Add an epic |
| 3 | **Request portal + catalogue + track my requests** | The entry point for employees in every ITSM / ESM product | BPMS-103 (**Planned**) |
| 4 | **Forms** (dynamic fields, prefill) | Needed by requests and by templates | BPMS-101 (**Planned**) |
| 5 | **Work items, assignment, approvals for requests, delegation, send-back** | Fulfiller basics | BPMS-085, BPMS-086 (**Planned**) |
| 6 | **SLA clocks visible to fulfillers + breach escalation** | JSM, Zendesk, Freshservice and Pipefy all expose SLA state | BPMS-083 (**In progress**) |
| 7 | **Approve from email** | Removes the "log in to click approve" step | BPMS-145 (**Planned**) |
| 8 | **Templates before designer**: install a ready process from a gallery | Lower effort than a blank canvas for SMEs | BPMS-106 (**Planned**); designer BPMS-100 (**Planned**) |
| 9 | **Fulfiller queues / all work / workload** | Team leads need a team view | BPMS-108, BPMS-160 (**Planned**) |
| 10 | **Role-based onboarding (product "Get started")** | Owner feedback: needs explaining | **Not in tracker** (BPMS-168 is training paths, a different thing). Add |
| 11 | **Settings IA redesign** (section 6) | Owner feedback | **Not in tracker**. Add |
| 12 | **Admin member security actions** (send reset link, require password change, sign out everywhere, reset two-step verification) | Standard in Google, Microsoft and Entra (section 8) | Decided in docs/22 OQ-15 / A-39; in progress per owner; **no epic ID found**. Link it to an epic |
| 13 | **Access review / health page** | Leads ask "who has access to what?" | BPMS-163 (access reviews, **Planned**). The health page in 5.4 can ship earlier from existing data |
| 14 | E-sign v1, chat approvals, mobile PWA, reports | Category table stakes later | BPMS-107, BPMS-201, BPMS-109, BPMS-166 (**Planned**) |
| 15 | Email-to-request intake; article deflection on the portal | Common help-desk expectations (not verified here) | **Not in tracker**. Decide during BPMS-103 |

**Where Procera already beats the categories:** version-bound acknowledgements with evidence,
announcements with receipts, a tamper-evident audit chain, separation of duties on publishing,
and a governed SOP / policy library. The IA should make these visible on Home and in the Library,
not bury them in Admin.

---

## 8. User management patterns

### 8.1 How comparable products manage an individual user

| Capability | Google Workspace | Microsoft 365 / Entra | Rippling | Slack | Linear | Notion |
|---|---|---|---|---|---|---|
| **Profile with editable work fields** | User information → Employee information: ID, job title, employee type, **manager email, department, cost center**, building; custom attributes [64] | Contact information including job title, department, manager (per a third-party guide; Entra docs cover profile editing) [44] | n/v | n/v | n/v | n/v |
| **Roles and groups** | Admin roles; privileges set what the console shows [30]. Groups [42] | Roles page; Teams and groups [28] | Role-based permissions; Supergroups [25][38] | User groups [39] | "Change role…" from the member row [70] | Groups tab in Settings & members [40] |
| **Suspend / deactivate** | Suspend keeps data; delete retains data 20 days for transfer (third-party KB) [63] | Block sign-in; sign out of all sessions [67] | Schedule termination now or later [72] | Deactivate: removed from channels, signed out on all devices; messages kept [69] | "Suspend user…": lose all access immediately [70] | Remove; restore within 30 days restores pages, groups and teamspaces [71] |
| **Invitations** | n/v | n/v | n/v | n/v | Invite with a role [70] | n/v |
| **Reset sign-in / sessions** | Reset password; **reset sign-in cookies** signs the user out of browsers and mobile [63] | Sign out of all sessions [67]. Entra: **revoke sessions** [68] | n/v | Article "Reset all single sign on sessions" exists [69] | n/v | n/v |
| **Second-factor reset** | Admin generates 2SV backup codes [66] | Entra "require re-register MFA" [68] | n/v | n/v | n/v | n/v |
| **Offboarding with reassignment** | Transfer data on delete (third-party KB) [63] | 7-step guide: block, save mailbox, wipe device, forward mail, give OneDrive access, remove licence, delete [67] | Termination triggers task lists to people team and IT; revoke app access [72] | n/v | n/v | Transfer private pages from a deprovisioned user (Enterprise, within 30 days) [71] |
| **Bulk** | Bulk update users by CSV [42] | Add multiple users by CSV [44] | n/v | n/v | n/v | n/v |
| **Audit of changes** | Admin log events; per-user "check log events" [65] | n/v | n/v | n/v | Audit log (owners, Enterprise, 90 days) [70] | n/v |

Lessons:
- **Actions live on the person, grouped by intent.** Google groups them under the user's
  Security panel (sign-in cookies, 2SV codes) [63][66]. Microsoft puts them on the user's Account
  tab (block, sign out) [67] and Entra under Authentication methods [68].
- **Offboarding is a checklist, not a button.** Microsoft's 7-step guide [67], Rippling's
  task lists [72] and Notion's content transfer [71] all treat leaving as "secure, then hand
  over, then remove".
- **Reversible first.** Suspend before delete (Google), block before delete (Microsoft), restore
  within 30 days (Notion).

### 8.2 Procera today

From the owner's brief and docs/17 BPMS-021/023/024, docs/22 OQ-15 and A-39:
- **Invite** (UI). The invitation API also supports list, revoke and resend.
- **Deactivate** from the person page, with immediate sign-out.
- **Person page** with department, manager and location, **read-only**. The backend keeps a
  source for each field.
- **CSV directory import** with a check step.
- **Roles and grants** screen. Grants can be scoped and can expire.
- **Admin security actions in progress**: send reset link, require password change, sign out
  everywhere, reset two-step verification. Because accounts are global, these apply only when this
  is the member's only active workspace; otherwise a system administrator acts (A-39).
- **When someone leaves** setting: `directory.deactivation.reassign_strategy` (fallback owner,
  manager or process owner).
- Reactivating a deactivated member was not confirmed in the docs reviewed.

### 8.3 Recommended member profile and management design

**Person page** (Admin → People → Members → *person*; also reached from any avatar by admins):

- **Header:** name, email, status chip (Active / Invited / Deactivated), role chips, department,
  manager, last active.
- **Actions menu, grouped by intent:**
  - *Access:* Change roles · Add to group · Set department · Set manager
  - *Sign-in:* Send password reset link · Require password change · Sign out everywhere · Reset
    two-step verification. If the person belongs to other workspaces, these are disabled with:
    "This person also uses other workspaces. Ask a system administrator."
  - *Invitation* (if invited): Resend · Revoke
  - *Status:* Deactivate… (opens the offboarding flow) · Reactivate (if deactivated; *needs
    backend confirmation*)
  - Every action is step-up protected where docs/12 requires it, and audited.
- **Tabs:**
  - **Profile.** Editable title (or position), department, manager, location. Each field shows
    its source ("From CSV import, 2 Oct" / "Set by Olivia"); fields synced from SCIM later show a
    lock. Manager edits reject cycles, as the API does.
  - **Access.** Effective roles with scope and source (direct, via group, via department) and
    expiry. "Why can they…?" explains a permission. The grant flow is the one from 5.4.
  - **Groups.** Memberships, with add and remove.
  - **Obligations.** Pending and completed acknowledgements with evidence links; reviews assigned;
    documents owned; campaigns run; direct reports.
  - **Activity.** Audit timeline for this person as actor and as subject. The audit explorer
    already supports timelines.
  - **Security.** Two-step methods enrolled (types only, never secrets), active sessions count,
    last sign-in, recent failed sign-ins.

**Offboarding flow (Deactivate…).** A 3-step dialog, following Microsoft's "secure, hand over,
remove" order [67]:
1. **Review**: what they hold. Documents owned, open reviews, campaigns they run, groups they
   own, direct reports, active grants, pending acknowledgements (which close as "left
   organisation", never as "acknowledged").
2. **Hand over**: each item is pre-filled from `directory.deactivation.reassign_strategy`
   (manager / fallback owner / process owner) and can be changed per item. Direct reports get a
   new manager.
3. **Confirm**: signs out everywhere immediately, revokes tokens, keeps all evidence. Step-up.
   The result screen lists what was reassigned, with links.

**Members list (bulk).** Selection actions: Set department · Set manager · Add to group · Grant
role (with reach preview) · Resend invites · Deactivate (each with the review step) · Export.
Saved views: Pending invites · No department · No manager · Deactivated · Admins.

**Invitations.** "Invite" accepts one or many emails, a default role and an optional department.
The pending list shows expiry (7 days), with Resend and Revoke inline.

**Audit.** Every profile, role, group and status change already writes an audit event. The person
page's Activity tab and a "History" link next to each profile field make that visible.

---

## 9. Mapping: every current nav item → new home

| Current item (group) | New home | Notes |
|---|---|---|
| Home (Workspace) | **Home** | Rebuilt as Needs you plus role cards |
| Notifications (Workspace) | **Bell** in header; preferences under account menu | Actionable items also appear in Needs you |
| Policies (Workspace) | **Library**, default view "Applies to me" for readers | `/policies` redirects to Library with the filter |
| Library (Workspace) | **Library** | Status, type and collection filters; saved views |
| Announcements (Workspace) | **News → Feed** | |
| Templates (Workspace) | **New → From template**; **Library** "Templates" filter; **Admin → Content and communication → Templates** (manage) | Removed from sidebar |
| Overview (Administration) | **Admin → Overview** | Setup checklist plus health |
| Members (Administration) | **Admin → People → Members** | Import CSV moves here |
| Roles & access (Administration) | **Admin → People → Roles & access** (By role) | |
| People & access (Administration) | **Admin → People → Roles & access** (By person) | Merged |
| Manage announcements (Administration) | **News → Manage** | Visible to Comms and HR |
| Campaigns (Administration) | **Campaigns** (everyday sidebar for HR and Compliance) | Defaults in Admin → Content and communication |
| Settings (Administration) | **Admin → Workspace → Settings**: a category index with 8 categories (section 6.2), each also reachable from its feature page (6.6). The Departments tab becomes the **Settings tab on each department page** (6.4). The Configuration profile tab becomes a **"Starting point" banner** plus Recommended markers (6.5) | One long page today becomes a categorised index |
| Files (Administration) | **Admin → Data and security → Files** | |
| Retention (Administration) | **Admin → Data and security → Retention** | |
| Encryption (Administration) | **Admin → Data and security → Encryption** | |
| Audit log (Administration) | **Admin → Data and security → Audit log**; "My activity" for members under account menu | Pinnable for Security |
| *(new)* | **My documents**, **My team**, **Admin → People → Departments / Groups / Access review** | |
| System console | Unchanged: account menu, operators only | |

---

## 10. Risks and open questions

**Risks**
- **Hidden-feature risk.** Stricter permission gating can make features feel missing ("where did
  Templates go?"). Mitigations: redirects from old URLs, search finds Admin pages for those who
  can open them, and a one-time "What moved" note for existing users.
- **Mode confusion.** A separate Admin hub can leave users unsure which mode they're in (the
  concern raised by Rippling reviewers [26]). Mitigations: a distinct Admin header, a persistent
  "Exit admin", and breadcrumbs.
- **Over-personalisation.** Role-shaped Homes make support and documentation harder ("my Home
  doesn't look like the screenshot"). Mitigations: the same section order for everyone, with cards
  only added, and per-persona help pages (Lattice publishes separate employee, manager and admin
  navigation guides [22][23][24]).
- **Manager derivation accuracy.** "My team" and escalations depend on manager data. Without a
  directory UI the data will be stale, so the People area must ship before or with "My team".
- **E2E churn.** Moving 17 destinations will break many existing browser E2E selectors and URLs.
  Plan redirects and update tests in the same change (docs/24 rules).
- **Evidence integrity on renames and moves.** Department rename or move must not alter campaign
  audience snapshots or audit records. The UI copy promises this, so the backend must guarantee it
  (it does per snapshot design; verify with a test).

**Open questions**
1. Should Campaigns live in everyday nav (proposed) or inside Admin for workspaces where only
   admins run them? Option: show it in nav only when the person holds `campaign:manage`.
2. Should "My documents" and reviews merge into one "My work" page (Linear-style tabs [18]) or
   stay split (proposed: reviews on Home, documents in My documents)?
3. Leader templates (HR, Finance lead, Engineering lead, Comms): ship as built-in templates, or as
   "presets" in the grant dialog that expand to existing roles with a scope? The second keeps the
   role catalogue small.
4. Rule-based ("smart") groups: expose in v1 of the Groups UI, or keep to static groups plus role
   assignment by directory rule (docs/12 §6) until SCIM lands?
5. Should employees see a **People directory** (BambooHR and Trainual show the directory or org
   chart to employees when enabled [12][13])? Proposed: off by default, Admin toggle.
6. Should "Evidence" be its own everyday destination for Compliance and Finance, or stay as export
   actions on campaigns and documents plus the audit log?
7. Should the Home card set be admin-configurable (BambooHR lets HR choose widgets [13]; Microsoft
   lets admins add cards [28]) or fixed per role for v1 (proposed: fixed)?
8. Is "News" the right label, or does the owner prefer "Announcements" for clarity in regulated
   settings?
9. Settings: should Admin → Content and communication hold the document and campaign defaults
   (5.1), or should all keys live only in the Settings categories (6.2) with feature-page links?
   Proposed: Settings categories are the single home, and Admin groups link into them, so there is
   one source of truth.
10. Can a deactivated member be **reactivated** with their memberships, groups and grants
    restored (as Notion restores within 30 days [71])? It isn't confirmed in the docs reviewed.
11. Should the leaver flow close pending acknowledgements as "left organisation" automatically,
    or does compliance want them kept open for the evidence record?
12. Requests module scope: are email-to-request intake and article deflection in or out for
    BPMS-103 (7.3 #15)?

**Additional risks (sections 6–8)**
- **Hiding settings keys** for unbuilt features must not hide values that are already set
  (for example a profile that wrote `exec.bulk_approve.allowed`). Hidden keys keep their values
  and reappear when the module is enabled.
- **Global accounts.** Admin sign-in actions on a person who belongs to several workspaces must
  route to a system administrator (A-39). The UI has to explain this, or admins will think the
  feature is broken.
- **Competitor coverage gaps.** Several matrix cells are "n/v". Before using the matrix in sales
  material, verify those cells against current vendor docs.

---

## Sources

1. PowerDMS Basic User Guide (customer-published, Unified Fire Authority, 2024), homepage, To Do list, signing, "Compare to Last Revision I Signed", Help menu: https://unifiedfireut.gov/wp-content/uploads/Memo-24-071a-PowerDMS-Basic-User-Guide.pdf
2. PowerDMS Basic User Guide (customer-published, City of Detroit): https://devtraining.detroitmi.gov/sites/training.detroitmi/files/2020-09/PowerDMS%20Basic%20User%20Guide%281%29.pdf
3. Atlassian, "Request and manage approvals in Confluence": https://support.atlassian.com/confluence-cloud/docs/request-and-manage-approvals-in-confluence/
4. Notion, "Structure your sidebar for more focused work with teamspaces": https://www.notion.com/en-gb/help/guides/structure-sidebar-focused-work-teamspaces
5. Notion help (sidebar sections, favourites), same guide family: https://www.notion.com/help/guides/structure-sidebar-focused-work-teamspaces
6. Microsoft Learn, "Overview of Viva Connections" (feed, dashboard, resources; audience-targeted cards): https://learn.microsoft.com/en-us/viva/connections/viva-connections-overview
7. Microsoft Support, "Target navigation, news, files, links, and web parts to specific audiences": https://support.microsoft.com/office/target-navigation-news-files-links-and-web-parts-to-specific-audiences-33d84cb6-14ed-4e53-a426-74c38ea32293
8. Guru, Verification features (verifier, status, My Tasks): https://www.getguru.com/features/verification
9. Process Street help, "Inbox / My Work": https://www.process.st/help/docs/inbox/
10. Process Street help, "Getting started using My Work": https://www.process.st/help/docs/getting-started-using-my-work/
11. Axelor docs, Trainual connector (roles and subjects assignment for onboarding paths; third-party): https://docs.axelor.com/connect-help-center/docs/HTML/en/apps/hr-management/trainual.html
12. Trainual help, "General User Permission Level": https://help.trainual.com/en/articles/6370945-general-user-permission-level
13. BambooHR help: "BambooHR Home" https://bamboohr.screenstepslive.com/m/53917/l/587797-bamboohr-home ; "The inbox" https://bamboohr.screenstepslive.com/m/58956/l/747950-the-inbox ; "Employee Access Level View" https://bamboohr.screenstepslive.com/m/53910/l/587361-employee-access-level-view (content as returned by search; inbox page requires sign-in)
14. Willamette University, Workday navigation intro (customer-published): https://www.willamette.edu/dept/hr/pdf/workday/01-navigation/wd-navigation-introduction.pdf
15. Workday admin guide, Home page: https://doc.workday.com/admin-guide/en-us/manage-workday/user-experience/people-experience/home-page/rfo1603917895028.html
16. Personio: Community, "New in Personio: access tasks and notifications via the new Inbox" https://community.personio.com/product-updates/new-in-personio-access-tasks-and-notifications-via-the-new-inbox-636 ; blog, "Redesigned inbox" https://personio.com/blog/redesigned-inbox ; Approvals help section https://support.personio.de/hc/en-us/sections/360000006525-Approvals
17. Linear docs, Inbox: https://linear.app/docs/inbox
18. Linear docs, My issues: https://linear.app/docs/my-issues
19. Linear docs, Workspaces (Settings, Administration section): https://linear.app/docs/workspaces
20. Asana, "Four ways to master My Tasks": https://asana.com/resources/asana-tips-my-tasks
21. Asana help, "Stay informed with your Asana inbox": https://help.asana.com/s/article/stay-informed-with-your-asana-inbox
22. Lattice help, "Navigate Lattice for managers": https://help.lattice.com/en-us/articles/15178873-navigate-lattice-for-managers
23. Lattice help, "Navigate Lattice for employees": https://help.lattice.com/en-us/articles/15178750-navigate-lattice-for-employees
24. Lattice help, "Navigate Lattice for admins": https://help.lattice.com/en-us/articles/15178869-navigate-lattice-for-admins
25. Rippling, Permissions (role-based permissions): https://www.rippling.com/permissions ; blog, "Introducing role-based permissions": https://www.rippling.com/blog/introducing-role-based-permissions-get-complete-control-over-the-data-and-apps-that-your-team-can-manage
26. Capterra, Rippling reviews (user comment on admin and employee view confusion): https://www.capterra.com/p/172127/Rippling/reviews/?page=10
27. ServiceNow docs, "Use the My To-dos page": https://www.servicenow.com/docs/r/SogDT~k7ktsbe1ZYegnaLg/E4bnY~XOIPG~Hh09gxzQVQ
28. Microsoft Learn, "Microsoft 365 admin center – Overview" (menu, simplified vs dashboard view, Show all, cards, specialist admin centres): https://learn.microsoft.com/en-us/microsoft-365/admin/admin-overview/admin-center-overview?view=o365-worldwide
29. Google Workspace Updates, "Improved Admin console navigation" (2021): https://workspaceupdates.googleblog.com/2021/07/improved-admin-console-navigation.html
30. Google Workspace Admin Help, "Navigate the Admin console" (grouped menu, pin up to 5): https://knowledge.workspace.google.com/admin/getting-started/navigate-the-admin-console ; privileges determine console controls: https://knowledge.workspace.google.com/admin/users/administrator-privilege-definitions
31. Slack help, "Manage workspace settings" / Tools & settings: https://slack.com/help/articles/220403548
32. Slack help, "Guide to the Slack admin dashboard": https://slack.com/help/articles/115005594006-Guide-to-the-Slack-Admin-dashboard
33. GitHub docs, "Accessing your organization's settings": https://docs.github.com/en/organizations/collaborating-with-groups-in-organizations/accessing-your-organizations-settings
34. Nielsen Norman Group, "Progressive Disclosure" (video and article): https://www.nngroup.com/videos/progressive-disclosure/
35. Google Workspace Admin Help, "How the organizational structure works": https://knowledge.workspace.google.com/admin/users/advanced/how-the-organizational-structure-works
36. Nielsen Norman Group, "Empty States in Application Design: 3 Guidelines": https://www.nngroup.com/videos/empty-states-in-application-design-guidelines/
37. Microsoft Learn, "Manage rules for dynamic membership groups in Microsoft Entra ID": https://learn.microsoft.com/en-gb/entra/identity/users/groups-dynamic-membership
38. Rippling, Policies and Supergroups: https://www.rippling.com/platform/policies
39. Slack help, "Create and edit user groups": https://slack.com/help/articles/212906697-Create-and-edit-user-groups
40. Notion, "Grant the right level of access with teamspaces and groups": https://www.notion.com/help/guides/grant-access-teamspaces
41. BambooHR help, "Add, Edit, or Delete Options in a Drop-Down List" (Settings → Employee Fields): https://bamboohr.screenstepslive.com/m/53907/l/587324-add-edit-or-delete-options-in-a-dropdown-list
42. Google Workspace Admin Help, "Create a group in your organization": https://knowledge.workspace.google.com/admin/groups/create-a-group-in-your-organization ; "Add or update multiple users from a CSV file": https://knowledge.workspace.google.com/admin/users/add-or-update-multiple-users-from-a-csv-file
43. Google Workspace Admin Help, "Create membership queries for dynamic groups": https://knowledge.workspace.google.com/admin/groups/create-membership-queries-for-dynamic-groups
44. Microsoft 365 bulk user CSV ("Add multiple users"), via Office 365 IT Pros (third-party): https://office365itpros.com/2019/03/28/bulk-addition-office-365-accounts/ ; manager attribute in profile, Microsoft Entra docs: https://docs.azure.cn/en-us/active-directory/fundamentals/how-to-manage-user-profile-info
45. Apideck, Rippling departments API listing (parent department, hierarchy; third-party): https://www.apideck.com/integrations/rippling/departments
46. BambooHR, Employee records platform page (org chart, self-service): https://www.bamboohr.com/ca/platform/hr-data-and-reporting/employee-records
47. Linear docs, Sub-teams: https://linear.app/docs/sub-teams
48. Atlassian, Jira Service Management Cloud, "What are SLAs?" (goals, calendars, start/pause/stop conditions): https://support.atlassian.com/jira-service-management-cloud/docs/create-a-new-sla/
49. Atlassian, "Manage settings for approval by email and chat" (approve from email, Slack or Teams; set under Project settings): https://support.atlassian.com/jira-service-management-cloud/docs/manage-settings-for-approval-by-email
50. Atlassian, "Receive requests in Slack or Microsoft Teams": https://support.atlassian.com/jira-service-management-cloud/docs/receive-requests-in-slack-or-microsoft-teams/
51. Atlassian Community, "Explore core features of Jira Service Management" (portal, request types, queues, SLAs; ITSM scope): https://community.atlassian.com/learning/lesson/explore-jira-service-management-features ; Jira Service Management Server, "Administering service projects" (queues, approvals, SLAs): https://confluence.atlassian.com/servicemanagementserver0419/administering-service-projects-1087529675.html
52. ServiceNow docs, "Request a catalog item from Employee Center" (catalog item form, My Requests): https://www.servicenow.com/docs/r/8K~WUyyBmjUR0pLPxM8yeA/UnMd8hTunZq0DGmJ8V_RVg
53. ServiceNow docs, "Sign a document using DocuSign from Employee Center": https://www.servicenow.com/docs/r/12FXWPUPJJNiwgjoPO9yFQ/jM6m_t0kg1amBHN5xEnmFQ
54. Kissflow, "New user guide" (My Tasks, My Items, reassign, send back, reject; content as returned by search, page requires sign-in): https://docs.kissflow.com/article/uj1enkthc7-new-user-guide
55. Pipefy help, "How to create a pipe from scratch": https://help.pipefy.com/en/articles/6863686-how-to-create-a-pipe-from-scratch ; "How to create a form": https://help.pipefy.com/articles/614596-how-to-create-a-form ; Product overview (late/expired labels, public forms, email approvals as returned by search): https://pipefy.com/product-overview
56. Process Street: "Approvals" https://www.process.st/approvals/ ; "Conditional logic" https://www.process.st/help/docs/conditional-logic/ ; "Stop tasks" https://www.process.st/stop-tasks/ ; "My Work" (filters incl. Approvals and Acknowledgments) https://www.process.st/help/my-work/
57. monday.com, "Keep work moving through approvals" (WorkForms intake, automations): https://monday.com/ap/llm/wm-llm/approvals-tracking ; monday Community, "Help on setting up monday.com for approval process" (no native approval column, per community): https://community.monday.com/t/help-on-setting-up-monday-com-for-approval-process/2069
58. Freshservice: Support guide (catalog, SLA policies by department/group, approval groups and chains) https://support.freshservice.com/support/solutions/159916 ; Service catalog https://www.freshworks.com/freshservice/it-service-catalog-management/ ; "Ticket and change approval via email" https://support.freshservice.com/support/solutions/articles/50000004955-ticket-and-change-approval-via-email
59. Zendesk, "SLA resources" (SLA policies, group SLA policies for internal teams, OLA, SLA dashboard): https://support.zendesk.com/hc/en-us/articles/4408887228442-SLA-resources
60. Macha (third-party), "Zendesk business rules explained" (views, macros, triggers, automations): https://www.getmacha.com/blog/zendesk-business-rules-explained
61. Camunda docs, "Using Tasklist" (claim/unclaim, forms): https://docs.camunda.io/docs/components/tasklist/userguide/using-tasklist/ ; Camunda 7 Tasklist filters (assignee, candidate group, delegation state, priority): https://docs.camunda.org/manual/webapps/tasklist/filters/ ; BPMN user tasks: https://docs.camunda.io/docs/8.4/components/modeler/bpmn/user-tasks
62. Flowable, "Flowable Work" user guide (work items with owner, assignee, due date, priority; Work and Tasks apps): https://documentation.flowable.com/latest/user/work/ ; mobile app: https://documentation.flowable.com/latest/user/user-mobile-app
63. Google Workspace Admin Help, "Reset a user's password" (incl. reset sign-in cookies): https://knowledge.workspace.google.com/admin/users/reset-a-users-password ; "Sign a user out of a managed Google Account": https://knowledge.workspace.google.com/admin/devices/sign-a-user-out-of-a-managed-google-account ; K12MT (third-party), "Google Admin: Suspend users vs delete user": https://www.k12mt.com/kb/google-admin-suspend-users-vs-delete-user
64. Google Workspace Admin Help, "Add information to a user's directory profile": https://knowledge.workspace.google.com/admin/users/add-information-to-a-users-directory-profile
65. Google Workspace Admin Help, "Admin log events": https://knowledge.workspace.google.com/admin/reports/admin-log-events ; "Investigate user problems with log events": https://knowledge.workspace.google.com/admin/users/investigate-user-problems-with-log-events
66. Google Workspace Knowledge, "How to create an admin role to manage 2-Step Verification backup codes": https://knowledge.workspace.google.com/kb/how-to-create-an-admin-role-to-manage-2-step-verification-backup-codes-000006321
67. Microsoft Learn, "Remove a former employee – Overview" (7 steps): https://learn.microsoft.com/en-us/microsoft-365/admin/add-users/remove-former-employee ; Step 1 (sign out of all sessions, block sign-in): https://learn.microsoft.com/microsoft-365/admin/add-users/remove-former-employee-step-1
68. Microsoft Learn, "Manage user authentication methods for Microsoft Entra multifactor authentication" (require re-register MFA, revoke sessions): https://learn.microsoft.com/en-ie/Entra/Identity/Authentication/howto-mfa-userdevicesettings
69. Slack help, "Deactivate a member's account": https://slack.com/help/articles/204475027-Deactivate-a-members-account ; "Reset all single sign on sessions": https://slack.com/help/articles/206924507-Reset-all-single-sign-on-sessions
70. Linear docs, "Members and roles" (change role, suspend, view admins via Command+K): https://linear.app/docs/members-roles ; "Audit log": https://linear.app/docs/audit-log
71. Notion help, "Transfer content from a deprovisioned user" (and restore within 30 days): https://www.notion.com/help/transfer-content-deprovisioned-user
72. Rippling blog, "How Rippling runs IT: ensuring smooth offboarding": https://www.rippling.com/blog/how-rippling-runs-it-ensuring-smooth-offboarding
73. Stripe docs, "Web Dashboard" (settings: Personal, Account, Product; Shortcuts; Home widgets): https://docs.stripe.com/dashboard/basics
74. Google Workspace Admin Help, "Change service settings for different users" (Inherited / Overridden; Override / Inherit): https://knowledge.workspace.google.com/admin/users/advanced/change-service-settings-for-different-users
75. Google Workspace Admin Help, "Get started with the security health page": https://knowledge.workspace.google.com/admin/security/get-started-with-the-security-health-page
76. Australian Signals Directorate Blueprint, Microsoft 365 "Org settings" (Services, Security & privacy, Organization profile): https://blueprint.asd.gov.au/configuration/microsoft-365/settings/org-settings/
