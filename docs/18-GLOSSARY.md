# 18 — Glossary

Use these terms consistently in code, UI copy and docs. UI copy can be renamed per workspace (terminology overrides, FR-W04); code names never change.

| Term | Meaning | Code name |
|---|---|---|
| **Platform** | One deployment of Procera | — |
| **System plane** | Platform administration above all workspaces | `system` |
| **System Admin** | Platform-level administrator; manages workspaces, never reads their content without break-glass | `system.admin` |
| **Workspace** | The tenant. Isolation boundary for data, roles, settings and keys | `workspace` (`workspace_id`) |
| **Member** | A user's membership in a workspace | `membership` |
| **User** | A global identity that can belong to many workspaces | `user` |
| **Department** | Node in the workspace's org tree; an authorization scope | `department` |
| **Group** | Static or rule-based set of members | `group` |
| **Position** | Job position used for approval authority (DoA) | `position` |
| **Role** | Named bundle of permissions with a default scope; built-in template or custom | `role` |
| **Grant** | Assignment of a role to a subject within a scope, optionally time-boxed | `role_grant` |
| **Scope** | Where a grant applies: workspace, department, collection, process, case | `scope_kind` |
| **Separation mode** | Opt-in mode that removes content access from workspace admins | — |
| **Break-glass** | Time-boxed, approved, audited emergency access | `break_glass_grant` |
| **Profile** | Configuration preset chosen at workspace creation (Starter/Standard/Enterprise/Regulated) | `profile` |
| **Policy floor** | Platform minimum a workspace may tighten but not loosen | `is_floor` |
| **Artifact** | Any governed, versioned item: process, SOP, policy, form, rule table, calendar, document template | `artifact` |
| **Artifact version** | One immutable-once-approved revision of an artifact | `artifact_version` |
| **Lifecycle** | Draft → In review → Approved → Scheduled → Published → Deprecated → Retired | `lifecycle` |
| **Process (model)** | Catalog entry describing how work is done (documentation and design) | `artifact(kind=process)` |
| **Definition** | A *published* process version, compiled to IR | `artifact_version` + `compiled` |
| **IR** | Canonical workflow intermediate representation (11) | `ir` |
| **Release bundle** | Immutable manifest pinning every artifact version a definition depends on | `release_bundle` |
| **Request / Instance** | One running execution of a definition. UI calls it "Request" | `process_instance` |
| **Token** | Execution pointer inside an instance | `token` |
| **Step / Node** | An element of a process (task, approval, decision, wait, …) | `node` |
| **Work item** | A unit of human work: task, approval, sign, acknowledge | `work_item` |
| **Outcome** | The result of a completed work item (approved, rejected, …). Distinct from state | `outcome` |
| **Delegation** | Hand-over of one item or of all new work during out-of-office | `delegation` |
| **SoD** | Segregation of duties | — |
| **DoA** | Delegation of authority. Approval limits by position and amount | `approval_matrix` |
| **SLA** | Service-level target with business-time clocks | `sla_clock` |
| **Business calendar** | Working hours plus holidays (versioned) used for business durations | `calendar_version` |
| **Incident** | An operational failure in an instance needing attention (retry/skip/cancel) | `incident` |
| **Test run** | Execution of a draft with no real side effects | `is_test` |
| **SOP** | Standard operating procedure. Governed documentation of how to perform work | `artifact(kind=sop)` |
| **Policy** | Governed statement of rules with an applicability scope | `artifact(kind=policy)` |
| **Campaign** | Subject + audience + required action (read/ack/quiz/sign) + deadline + reminders | `campaign` |
| **Acknowledgement** | A recipient's recorded confirmation of a specific version | `campaign_recipient` |
| **Announcement** | Targeted communication, optionally requiring acknowledgement | `announcement` |
| **Envelope** | A document sent for signature to one or more recipients | `envelope` |
| **Certificate of completion** | Evidence pages appended to a signed document | — |
| **Case** | Flexible, evidence-centred work container (incident, HR case, CAPA) | `case` |
| **Investigation** | A case type for audit/forensic inquiries with pinned evidence | `case(case_type=investigation)` |
| **Pinned evidence** | Immutable, hashed snapshot of an audit event, file or request attached to a case | `case_item(kind=pinned_*)` |
| **Legal hold** | Flag that suspends retention, disposition and erasure | `legal_hold` |
| **Retention class** | Policy group that determines how long records are kept | `retention_class` |
| **Audit event** | Hash-chained accountability record | `audit_event` |
| **Checkpoint** | Signed Merkle root over a range of audit events | `audit_checkpoint` |
| **Data access event** | Audit record of reading restricted/confidential data | `category=data_access` |
| **Classification** | public / internal / confidential / restricted | `classification` |
| **Template** | Pre-designed, standards-aligned process package in the gallery | `template` |
| **Master data table** | Workspace reference data (cost centres, vendors) used in forms and rules | `data_table` |
| **DataGrid** | The shared table component with sort/filter/saved views/export | `grid` |
| **Saved view** | Stored grid query (filters, sort, columns) | `saved_view` |
| **Outbox** | Transactional table of events to publish after commit | `outbox` |
| **Connector** | Out-of-process adapter to an external system (P1+) | `connector` |
| **Guest** | External participant acting on one item via magic link | `guest` |
