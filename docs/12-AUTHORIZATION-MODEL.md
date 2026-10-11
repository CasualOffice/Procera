# 12 — Tenancy, identity, roles and access control

Status: **Draft for G1 sign-off** · Owner: Security lead · Closes gaps B3, B4, B13, C3.

Product direction (2026-10-11):
- Multi-tenant and workspace-based.
- **The workspace is the tenant.**
- A **System Admin** layer manages every workspace.
- Roles are configurable per workspace.
- The product is standalone, so every identity feature works with no external IdP or integration.

## 1. Principles

- **Deny by default.** Every API handler, background transition, timer, notification deep link, search query and export calls the policy evaluator. No "internal" path skips it.
- **Workspace from credentials, never from input.** The active workspace comes from the verified session or token. Ids in the path or body are *looked up within* that workspace. A request can never name another workspace's object.
- **Hidden ≡ missing.** Return `404`, not `403`, for objects the caller may not know exist.
- **Workspace admins see and control everything in their workspace; System Admins do not.** By default the Workspace Owner/Admin has full visibility of all content in their workspace (requests, work, files, cases, audit), so they can monitor, investigate and intervene. Every read of `restricted`/`confidential` data by an admin outside their own participation is itself **access-logged** (`audit.data_access`) and visible in the audit explorer. Regulated workspaces can switch on **Separation mode** (§6.1), which removes content access from the admin role and requires an explicit, audited grant. System Admins manage workspaces but never read workspace content except via audited break-glass.
- **Explainable decisions.** The evaluator returns the matched policy ids. Sensitive actions store them in the audit log.
- **Configurable, but safe by construction.** Admins compose roles from a fixed permission catalogue. They cannot write arbitrary policy code. Some invariants (SoD, step-up, tenant isolation) cannot be configured away.

## 2. Tenancy model

```
Platform (one deployment)
 ├── System administration  ← System Admins: create/suspend workspaces, quotas, global template gallery, platform health
 ├── Global user accounts   ← one identity per person; can belong to many workspaces (Slack/Notion model)
 └── Workspaces  (TENANT = isolation boundary; tenant_id = workspace_id)
      ├── Members (user ↔ workspace membership, status, roles)
      ├── Directory: departments/teams (tree), groups, positions, manager links
      ├── Roles: built-in templates + custom roles (configurable)
      ├── Configuration: branding, terminology, calendars, numbering, policies, retention, auth rules
      ├── Content: process library, SOPs, forms, rules, instances, work, cases, evidence
      └── Keys: per-workspace data-encryption key hierarchy
```

- **Every** content and config row carries `workspace_id`, and every foreign key includes it.
- **Users are global, memberships are per workspace.** One person can be an Approver in "Acme HR" and an Admin in "Acme Finance". These are different workspaces with different roles, and no data crosses between them.
- A user's session has exactly one **active workspace**. Switching workspaces issues a new workspace-scoped token. There is no cross-workspace query path in the product API.
- **Inside a workspace**, departments/teams and **collections** (folders of the process library) are *authorization scopes*, not isolation boundaries. A role can be granted workspace-wide, for a department, for a collection, or for a single process.
- **Isolation tiers** (ADR-0008):

  | Tier | Isolation | When |
  |---|---|---|
  | `shared` (default) | `workspace_id` + Postgres RLS + per-workspace keys | Everyone |
  | `dedicated-schema` | Same, in a separate schema | P2 option |
  | `dedicated-database` | Separate database, connection routed by workspace | Regulated customers |

- **Workspace lifecycle** (System Admin): `provisioning → active → suspended (read-only or locked) → pending_deletion (grace period, default 30 d) → deleted (crypto-shred keys + purge)`. Export is available until deletion (PRV-08).
- **Quotas per workspace:** members, active instances, storage, API rate, AI usage. They are set by the System Admin and visible to the workspace admin.

## 3. Identity and authentication

Standalone first. No external system is needed, and enterprise identity layers on top later.

| Mechanism | Phase | Notes |
|---|---|---|
| Local accounts: email + password | P0 | Argon2id (m=19 MiB, t=2, p=1 minimum, per OWASP). Breached-password check against an offline k-anonymity list. Lockout and back-off. Verified email. |
| MFA: TOTP + WebAuthn/passkeys | P0 | A workspace can *require* MFA for all members or for certain roles (default: required for admins). Recovery codes. Passkeys are a second factor after the password and a step-up method; members add, name and remove them under Account › Security (removal needs step-up). See docs/22 A-19. |
| Passkey-only login | P1 | Phishing-resistant. Satisfies A11Y 3.3.8. |
| Magic-link login for guests | P1 | For external participants (FR-P08). Single-use, 15 min, bound to one work item. |
| OIDC SSO (any IdP; Keycloak is the reference) | P1 | Per workspace. Auth code + PKCE. Domain-verified auto-join. |
| SAML 2.0 | P1 | Via a brokering IdP, so there is no in-process XML-DSig (ADR-0011). |
| SCIM 2.0 provisioning | P1 | Per workspace. Users, groups, `manager` attribute. |
| API tokens / service accounts | P0 | Workspace-scoped. Created by a workspace admin. Scoped to permissions. Expiring. Hashed at rest. Cannot sign approvals. |
| Platform operator / System Admin login | P0 | Separate admin realm on its own route/subdomain. MFA **mandatory** (WebAuthn recommended). Optional IP allowlist. |

Sign-in (ADR-0020):
- **Access:** short-lived JWT access tokens (10 min, `Authorization: Bearer`). The access token carries `ws`, the active workspace; switching workspace issues a new token for the new workspace.
- **Refresh:** rotating refresh tokens in an `HttpOnly; SameSite=Strict` cookie (reuse revokes the sign-in).
- **Revocation:** per-request status and token-version checks make deactivation and "sign out everywhere" effective within seconds.

Full protocol details are in [06-SECURITY-AND-OPERATIONS §3](06-SECURITY-AND-OPERATIONS.md).

Deprovisioning (SEC-04) happens when a member is removed or deactivated, or deprovisioned via SCIM:
- Their sessions and tokens for that workspace are revoked immediately.
- Their open work items are re-resolved through each node's `fallback` assignee.
- Process owners are notified.

## 4. Two administration planes

### 4.1 System (platform) plane

| Role | Can | Cannot |
|---|---|---|
| `system.owner` | Everything in the system plane, including managing other system admins. Exists from install bootstrap. | Read workspace content without break-glass |
| `system.admin` | Create, suspend and delete workspaces. Set quotas and isolation tier. Assign the first workspace admin. Manage the global template gallery. Set platform settings: email/SMTP, storage, AI providers, auth policy floor. | Read workspace content without break-glass |
| `system.operator` | Health, metrics, job queues, incidents (metadata only), maintenance mode, backups | Change workspaces or roles, or read content |
| `system.auditor` | Read the system-plane audit log (workspace lifecycle, break-glass, admin actions) | Any change |

**Break-glass:**
1. A `system.admin` requests time-boxed access to a workspace, giving a reason and a ticket ref.
2. A workspace admin approves it, or a second system admin does if the workspace has none or is locked.
3. Access is read-only by default and expires after 4 h max.
4. Every request made during the window is audited and shown in the workspace's own audit log and admin banner.

Configurable platform-wide **policy floors** bind every workspace. Example: "MFA required for workspace admins", "min password length 12", "session max 12 h". A workspace can tighten these but never loosen them.

### 4.2 Workspace plane

Each workspace has its own member and role administration. Its built-in roles are templates: a workspace can use them as-is, clone and modify them, or create entirely new roles.

## 5. Permission catalogue (the building blocks of every role)

Permissions are `resource:action` strings, and the set is fixed per product version. Roles are named bundles of permissions plus a **scope**.

| Resource | Actions |
|---|---|
| `workspace` | `settings.view`, `settings.manage`, `branding.manage`, `billing.view`, `export`, `delete.request` |
| `member` | `view`, `invite`, `deactivate`, `manage_roles`, `security.manage` (send a reset link, require a password change, sign out everywhere, reset two-step verification; docs/06 §3.6.1) |
| `role` | `view`, `manage` (create/edit custom roles; cannot exceed own permissions) |
| `directory` | `view`, `manage` (departments, groups, positions, managers) |
| `collection` | `view`, `manage` (library folders) |
| `process` | `view`, `create`, `edit_draft`, `submit_review`, `review`, `approve`, `publish`, `retire`, `manage_owners`, `view_analytics`, `export` |
| `sop` | `view`, `create`, `edit_draft`, `review`, `approve`, `publish`, `acknowledge`, `export` |
| `policy` | `view`, `create`, `edit_draft`, `review`, `approve`, `publish`, `export` |
| `announcement` | `view`, `create`, `publish`, `manage` |
| `campaign` | `view`, `manage` (acknowledgement & signature campaigns) |
| `document` | `send_for_signature`, `view_all`, `void` |
| `form` / `rule` / `calendar` | `view`, `manage` |
| `template` | `browse`, `install`, `publish_workspace_template` |
| `request` (instance) | `start`, `view_own`, `view_all`, `view_detail`, `view_restricted`, `comment`, `cancel`, `suspend_resume`, `retry`, `migrate` |
| `work` | `view_assigned`, `claim`, `complete`, `delegate`, `reassign`, `view_team` |
| `case` | `create`, `view`, `edit`, `add_evidence`, `view_evidence`, `close`, `reopen` |
| `control` / `attestation` | `view`, `map`, `attest`, `manage_campaign` |
| `report` | `view`, `export`, `manage_dashboards` |
| `connector` | `view`, `configure`, `manage_secrets` (P1+) |
| `file` | `upload`, `view_own`, `view_all`, `download`, `delete`, `manage_retention`, `manage_quarantine` |
| `audit` | `view_own`, `view`, `export`, `investigate` (open/manage investigations) |
| `monitoring` | `view_dashboards`, `manage_dashboards`, `manage_alerts` |
| `api_token` | `manage` |
| `ai` | `use_assistant`, `manage_settings` |

**Scopes** are where a role grant applies: `workspace`, `department:<id>` (and subtree), `collection:<id>` (and subtree), or `process:<id>`. A user's effective permissions are the union of their grants, intersected with the scope of the target resource.

## 6. Built-in role templates (per workspace)

| Role (default name) | Default scope | Summary of grants | Notes |
|---|---|---|---|
| **Workspace Owner** | workspace | All workspace permissions, including deleting the workspace | At least one is required. Transfer needs step-up auth. |
| **Workspace Admin** | workspace | Everything except deleting the workspace: members, roles, directory, settings, branding, templates, connectors, retention, **all content (requests, files, cases, evidence, restricted fields)**, monitoring dashboards, audit explorer & investigations | Content reads beyond own participation are access-logged. In Separation mode, content permissions are removed and must be granted explicitly. Cannot bypass SoD: an admin still can't approve their own request or publish their own controlled change. |
| **Security Admin** | workspace | Audit view/export, auth policy, API tokens, break-glass approval | Optional separation from Admin |
| **Process Owner** | collection/process | Draft, submit, publish *standard* processes, manage owners, analytics, team work view | Accountable person on the catalogue entry |
| **Process Designer** (analyst) | collection | Create and edit drafts, test runs, forms, rules | No publish |
| **Reviewer / Approver (governance)** | collection | Review and approve drafts and SOPs | Cannot approve their own submissions (SoD) |
| **Compliance Officer** | workspace | Controls, attestations, evidence, approve *controlled* changes, audit export | |
| **Case Manager** | workspace/department | Create and manage cases, assign investigators | |
| **Investigator** | per case | Edit case, add/view evidence | Usually granted per case |
| **Team Manager** | department | View team requests and work, reassign within team, team analytics | Derived from directory where possible |
| **Analyst / Report Viewer** | collection | Dashboards and reports, exports (redacted) | |
| **Member** (default for everyone) | workspace | Browse visible library, start permitted processes, own requests, assigned work, acknowledge SOPs | |
| **Guest** (external) | per item | Act on one specific work item or form | Magic link, expiring (P1) |

Configuration rules:
1. Built-in templates are versioned by the product. Workspaces can clone a template into a **custom role**, then rename it, add or remove permissions, and set its default scope.
2. A custom role cannot include permissions the editor doesn't hold (no escalation). Granting `role:manage` and `member:manage_roles` itself requires step-up auth.
3. **Role assignment targets:** users, groups, or a *directory rule* (e.g. "all members of department Finance with position = Manager"). The rule form keeps access in sync with SCIM/HR data.
4. **Time-boxed grants** with an expiry date, e.g. an external auditor for 30 days.
5. **Role change preview:** before saving, the UI shows "this change gives 14 people `request:view_all` in Finance". The change is audited with a diff.

As built (BPMS-024, R1):
- **Who counts as affected.** The role change preview counts every active member who holds the role through a live grant, directly or through a group, at any scope. All of them gain every added permission and lose every removed one, within the scope of their own grant.
- **No-escalation applies in both directions.** Editing a custom role needs every permission in the old set and in the new set. Granting or revoking needs every permission in the role.
- **Owner-only permissions** (`workspace:delete.request`, `workspace:billing.view`) can't be put in a custom role. Duplicating the Owner role leaves them out.
- **Step-up.** Creating, editing, granting and revoking roles all need a recent step-up.
- **The owner role** goes to people only, never to groups.
6. **Access reviews** (CMP-04): periodic recertification campaigns where role holders' managers or owners confirm each grant.

### 6.1 Separation mode (opt-in, for regulated workspaces)

Off by default. When a Workspace Owner turns it on (step-up auth, audited, 24 h notice to all admins):
- `request:view_restricted`, `case:view_evidence`, `file:view_all` and `audit:export` are removed from Workspace Admin and moved to Security Admin / Compliance Officer.
- Admin access to content requires a time-boxed, reasoned self-grant that a second admin approves (workspace-level break-glass).
- It supports ISO 27001 A.5.3 (segregation of duties) and GxP expectations without making the default product awkward for SMEs.

## 7. Process-level (contextual) roles

Beyond workspace roles, each process defines **contextual roles** that resolve at run time and are referenced in the IR (11-IR §3.1):

- `initiator`, `initiator.manager`, `assignee`, `participants`, `process.owner`
- custom process roles, e.g. "Finance approver", that the owner maps to users, groups or directory rules. Template placeholders `{{role:…}}` map to these at install time (19-TEMPLATES).

Process visibility is set per catalogue entry: `everyone in workspace`, `departments…`, `groups…`, or `restricted list`. "Who can start" is set separately from "who can see the documentation".

## 8. Field-level and content policies

- Variable classification (11-IR §5) maps to the permission needed to read it:

  | Classification | Required |
  |---|---|
  | `public` / `internal` | `request:view_detail` |
  | `confidential` | `view_detail` plus being a participant, or an explicit grant |
  | `restricted` | `request:view_restricted` scoped to the process, plus being listed in the field's `readers` |

- Redaction is applied **in the API layer** before serialization, through one code path shared by API, exports, search and notifications (SEC-03). Restricted values are envelope-encrypted at rest.
- Redacted values serialize as `{"redacted": true, "reason": "classification"}`.

## 9. Delegation and substitution

| Mechanism | Set by | Scope | Rules |
|---|---|---|---|
| Per-item delegation | Assignee | One work item | Delegate must pass the node's SoD rules; delegator keeps visibility |
| Out-of-office substitution | User, or their manager/admin | Date range, optional process filter | New work routes to the substitute; moving existing work is optional; expires automatically |
| Admin reassignment | Process Owner, Team Manager (own department), Admin | Work items | Reason required; audited; both parties notified |

Hard invariants (SEC-08):
- Delegation never grants more than the delegator holds.
- Chain depth is 1 by default, up to 3 if the workspace enables it.
- No delegation to someone excluded by SoD.
- Expired delegation fails closed (T05).
- Delegated decisions record "X on behalf of Y".

## 10. Segregation of duties (non-negotiable invariants + configurable rules)

Always enforced:
- Author ≠ approver for publishing **controlled** processes and SOPs.
- No self-approval of one's own request when `excludeInitiator` is set (the default for approval nodes in templates).
- No user may approve two sequential levels where `excludeActorsOf` is set.

Configurable per workspace or criticality: whether standard processes need an independent reviewer, the minimum number of reviewers, and the required reviewer roles. SoD is checked at assignment and **re-checked at decision time**, because directory or delegation may have changed.

## 11. Step-up authentication

Recent re-authentication (default 5 min; ESIG-03 for Part 11 mode) is required for:
- Part 11 signatures
- publishing controlled versions
- audit/evidence export
- role and permission changes
- API token creation
- secret management
- workspace ownership transfer or deletion
- break-glass

It uses WebAuthn or password+TOTP, or OIDC `max_age=0` under SSO. `auth_time` is recorded. As built (BPMS-022): an account with MFA steps up with a TOTP code or a passkey (`amr` gains `otp` or `hwk`); an account without MFA uses its password. Account-level actions outside a workspace (removing a passkey) use the platform default window of 5 minutes.

## 12. Policy engine and enforcement

Engine: **Cedar** (Apache-2.0, Rust-native, schema-validated), embedded in-process ([ADR-0011](adr/ADR-0011-authorization-engine.md)):
- Custom roles compile to Cedar policies from the catalogue (data-driven, never hand-written by admins).
- Invariants (§10, §2) are fixed `forbid` policies that admin-created permits cannot override.
- Postgres RLS on `workspace_id` provides defence in depth.

```
HTTP → authn → SessionContext{user, workspace, memberships, auth_time, mfa}
     → handler → authz.check(principal, action, resource, context) → Allow{policy_ids} | Deny
     → repository: BEGIN; SELECT set_config('app.workspace_id', $ws, true); … COMMIT
Worker/timer → system principal "acting for" instance; re-checks assignment/SoD
Notification link → no capability in URL; full re-check on open
List/search → policy → SQL predicate (scope + visibility), then per-row check for residual conditions
System plane → separate route, separate principal type; cannot obtain workspace tokens except via break-glass grant
```

RLS rules:
- The app role is not the table owner and has no `BYPASSRLS`. `FORCE ROW LEVEL SECURITY` is set on all tenant tables.
- Policies use `NULLIF(current_setting('app.workspace_id', true), '')::uuid`, so a missing context means zero rows. The `NULLIF` matters: pooled connections keep the setting as `''` after a transaction-local set ([SP-09](spikes/SP-09-authz-rls-pool.md)).
- Views use `security_invoker`.

## 13. Required tests (subset of 15-TEST-STRATEGY)

- SEC-01…SEC-10.
- Generated role × action × scope matrix tests.
- Custom-role escalation attempts.
- Cross-workspace access with a valid token from workspace A against every endpoint: expect 404 and an audit entry.
- A missing RLS context returns zero rows.
- Break-glass is visible to the workspace.
- System Admin cannot read content without break-glass.
- Redaction parity.
- SoD re-check after the directory changes.
- An expired time-boxed grant is denied.
