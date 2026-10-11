# 04 — Low-Level Design (LLD)

Status: **Draft v0.2 for G1** · Owner: Tech lead · Supersedes [v0.1](archive/v0.1/04_LLD.md).
Inputs: [02-PRD](02-PRD.md), [03-HLD](03-HLD.md), [11-IR](11-WORKFLOW-IR-SPEC.md),
[12-AUTHZ](12-AUTHORIZATION-MODEL.md), [14-DATA-GOVERNANCE](14-DATA-GOVERNANCE-AND-COMPLIANCE.md).

Conventions in this doc:
- `workspace_id` is the tenant key everywhere.
- Every id is a UUIDv7 (time-ordered, index-friendly).
- Times are `timestamptz` in UTC.
- Money is `numeric(20,4)` plus an ISO 4217 code.
- All DDL is **normative in shape** (columns, keys, constraints). Final migrations live in `migrations/`, and that file wins if the two disagree.

---

## 1. Code layout (Rust workspace + web)

```text
crates/
  kernel/          ids, time (Clock trait), errors, Problem (RFC 9457), pagination, money
  domain/
    identity/      users, memberships, sessions, mfa, api tokens
    workspace/     workspaces, settings resolution, quotas, profiles
    directory/     departments, groups, positions, manager graph
    access/        permission catalogue, roles, grants, Cedar schema + policy compiler
    library/       processes, collections, artifacts (sop, policy, form, rule, calendar), lifecycle
    runtime/       ir model, compiler, validator, interpreter, scheduler port, adapter trait
    work/          work items, assignment resolution, delegation, sod
    campaigns/     acknowledgement & signature campaigns, envelopes, announcements
    cases/         cases, investigations, evidence pinning
    files/         file objects, scan pipeline, previews, retention
    audit/         audit events, hash chain, checkpoints, verifier
    insights/      projections, metrics, sla clocks
    notify/        notification intents, templates, digests
  application/     command & query handlers (use cases), transactions, authz enforcement
  infra/
    pg/            sqlx repositories, RLS session guard, migrations runner
    outbox/        writer, dispatcher (LISTEN/NOTIFY wakeups + polling), inbox dedup
    bus/           Bus trait: pg (default), nats, kafka
    objstore/      S3 client, signed URLs, envelope encryption
    kms/           KeyProvider: local-sealed, Vault/OpenBao Transit, AWS/GCP/Azure KMS
    mail/          SMTP, templates (MJML-compiled at build), bounce handling
    scan/          ClamAV (clamd) adapter, null adapter (dev only)
    convert/       sandboxed DOCX→PDF / preview worker client
    authn/         password (argon2id), totp, webauthn, oidc rp, saml-broker
    search/        pg FTS (default), tantivy (optional)
apps/
  api/             Axum HTTP server (public API + BFF)
  worker/          runtime executor, connector runner host, campaign sender, exports
  scheduler/       timers, SLA clocks, reminders, retention jobs (leased)
  admin-cli/       bootstrap, migrations, verify (`procera-verify`), backup hooks
web/               React + TypeScript (Vite), packages/{ui, grid, designer, forms, sign, app-workspace, app-system}
conformance/       IR fixtures + expected traces (runtime adapters)
migrations/        sqlx migrations (forward-only, expand/contract)
```

Dependency rule: `domain` depends only on `kernel`, `application` depends on `domain` plus ports, and `infra` implements the ports. `apps` wire everything together. This is enforced with `cargo-deny` bans and a workspace-level lint.

Key ports (traits): `Clock`, `IdGen`, `UnitOfWork`, `*Repository`, `Authorizer`, `OutboxWriter`, `Bus`, `ObjectStore`, `KeyProvider`, `Mailer`, `Scanner`, `Converter`, `RuntimeAdapter`, `ExpressionEngine`, `NotificationChannel`, `AiProvider`.

---

## 2. Data model

### 2.1 Platform & identity (system plane: no `workspace_id`, no RLS; system-role access only)

```sql
CREATE TABLE users (
  id uuid PRIMARY KEY, email citext NOT NULL UNIQUE, email_verified_at timestamptz,
  display_name text NOT NULL, locale text NOT NULL DEFAULT 'en', timezone text NOT NULL DEFAULT 'UTC',
  status text NOT NULL CHECK (status IN ('active','locked','disabled')),
  subject_ref uuid NOT NULL UNIQUE,               -- pseudonymous id used in immutable stores (14 §2)
  created_at timestamptz NOT NULL DEFAULT now());

CREATE TABLE user_credentials (                   -- one row per factor
  id uuid PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id),
  kind text NOT NULL CHECK (kind IN ('password','totp','webauthn','recovery_code')),
  secret_ref bytea NOT NULL,                      -- argon2id PHC string / encrypted TOTP seed / COSE key
  label text, created_at timestamptz NOT NULL DEFAULT now(), last_used_at timestamptz);
  -- webauthn rows (migration 0020, BPMS-022) add credential_id (globally unique), sign_count,
  -- transports, backup_eligible/backup_state, user_verified. Single-use ceremony challenges live in
  -- webauthn_challenges (user_id, purpose register|mfa|step_up, challenge, expires_at, used_at).

CREATE TABLE sessions (
  id_hash bytea PRIMARY KEY,                      -- SHA-256 of opaque session id; raw id only in cookie
  user_id uuid NOT NULL REFERENCES users(id), active_workspace_id uuid,
  plane text NOT NULL CHECK (plane IN ('workspace','system')),
  auth_time timestamptz NOT NULL, amr text[] NOT NULL, ip inet, user_agent text,
  idle_expires_at timestamptz NOT NULL, absolute_expires_at timestamptz NOT NULL, revoked_at timestamptz);

CREATE TABLE system_role_grants (user_id uuid REFERENCES users(id), role text NOT NULL
  CHECK (role IN ('system.owner','system.admin','system.operator','system.auditor')),
  granted_by uuid, granted_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (user_id, role));

CREATE TABLE workspaces (
  id uuid PRIMARY KEY, slug citext NOT NULL UNIQUE, name text NOT NULL,
  profile text NOT NULL CHECK (profile IN ('starter','standard','enterprise','regulated')),
  state text NOT NULL CHECK (state IN ('provisioning','active','suspended_ro','suspended_locked','pending_deletion','deleted')),
  isolation_tier text NOT NULL DEFAULT 'shared' CHECK (isolation_tier IN ('shared','dedicated_schema','dedicated_db')),
  db_route text,                                  -- for dedicated tiers
  kek_ref text NOT NULL,                          -- KMS key id / wrapped KEK
  region text NOT NULL DEFAULT 'default', deletion_due_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(), created_by uuid NOT NULL);

CREATE TABLE workspace_quotas (workspace_id uuid PRIMARY KEY REFERENCES workspaces(id),
  max_members int, max_active_instances int, max_storage_bytes bigint,
  api_rpm int, ai_tokens_month bigint, soft_ratio numeric(3,2) NOT NULL DEFAULT 0.8);

CREATE TABLE platform_settings (key text PRIMARY KEY, value jsonb NOT NULL, is_floor boolean NOT NULL DEFAULT false,
  updated_by uuid, updated_at timestamptz NOT NULL DEFAULT now());

CREATE TABLE break_glass_grants (id uuid PRIMARY KEY, workspace_id uuid NOT NULL REFERENCES workspaces(id),
  requested_by uuid NOT NULL, approved_by uuid, reason text NOT NULL, ticket_ref text,
  mode text NOT NULL DEFAULT 'read_only', starts_at timestamptz, expires_at timestamptz, revoked_at timestamptz);
```

### 2.2 Workspace-scoped tables: common pattern

Every workspace table follows the same pattern:
- It has `workspace_id uuid NOT NULL`.
- Its primary key is `(workspace_id, id)`.
- Every foreign key includes `workspace_id`, so a cross-workspace reference is impossible by construction.
- RLS is on, with `FORCE ROW LEVEL SECURITY`.

The common RLS policy:

```sql
ALTER TABLE <t> ENABLE ROW LEVEL SECURITY; ALTER TABLE <t> FORCE ROW LEVEL SECURITY;
CREATE POLICY ws_isolation ON <t>
  USING (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid)
  WITH CHECK (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid);
-- NULLIF: after a tx-local set_config commits, the GUC stays defined as '' on the pooled
-- connection; ''::uuid would error instead of yielding zero rows (SP-09 finding A-1).
```

Roles (bootstrap script [`deploy/sql/00-roles.sql`](../deploy/sql/00-roles.sql); migrations in [`migrations/`](../migrations)):
- `procera_migrator` owns the schema and runs migrations (`procera migrate`). Default privileges grant the app DML on everything it creates.
- `procera_discovery` (NOLOGIN, BYPASSRLS) owns only the `discovery.*` functions that tell background workers *which* workspaces have pending work. It has column-level `SELECT` on id and status columns only, so it can never read payloads. This replaces the earlier `procera_system` idea: FORCE RLS binds table owners, so cross-workspace discovery needs a dedicated, minimal bypass. Implemented in BPMS-025/029.
- `procera_app` runs workspace requests. It owns no tables, has no `BYPASSRLS`, and only has DML.
- The system plane runs as `procera_app` under the nil workspace id (`begin_system`). Cross-workspace jobs discover workspaces through `discovery.*` and then process each one under its own RLS context.
- `procera_migrator` owns the schema.
- Audit tables: the app role holds `INSERT`/`SELECT` but no `UPDATE`, `DELETE` or `TRUNCATE`, and an immutability trigger blocks everyone else (migration 0002).

### 2.3 Membership, directory, access

```sql
CREATE TABLE memberships (workspace_id uuid NOT NULL REFERENCES workspaces(id), id uuid NOT NULL,
  user_id uuid NOT NULL REFERENCES users(id), status text NOT NULL CHECK (status IN ('invited','active','deactivated')),
  title text, department_id uuid, position_id uuid, manager_membership_id uuid, location text,
  employment_type text, source jsonb NOT NULL DEFAULT '{}',   -- per-attribute source + synced_at
  joined_at timestamptz, deactivated_at timestamptz,
  PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, user_id));

CREATE TABLE departments (workspace_id uuid NOT NULL, id uuid NOT NULL, parent_id uuid, name text NOT NULL,
  path uuid[] NOT NULL,                             -- ancestors root→self (GIN, `@>`); as built in 0008, no ltree extension needed
  head_membership_id uuid, PRIMARY KEY (workspace_id, id));
CREATE INDEX ON departments USING gin (path);

CREATE TABLE groups (workspace_id uuid NOT NULL, id uuid NOT NULL, name text NOT NULL, kind text NOT NULL
  CHECK (kind IN ('static','rule','scim')), rule jsonb, PRIMARY KEY (workspace_id, id));
CREATE TABLE group_members (workspace_id uuid NOT NULL, group_id uuid NOT NULL, membership_id uuid NOT NULL,
  PRIMARY KEY (workspace_id, group_id, membership_id));

CREATE TABLE roles (workspace_id uuid NOT NULL, id uuid NOT NULL, key text NOT NULL, name text NOT NULL,
  builtin_template text, template_version int,        -- lineage when cloned from a built-in
  permissions text[] NOT NULL,                        -- subset of the permission catalogue for this product version
  default_scope_kind text NOT NULL, is_locked boolean NOT NULL DEFAULT false,
  revision bigint NOT NULL DEFAULT 0, PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, key));

CREATE TABLE role_grants (workspace_id uuid NOT NULL, id uuid NOT NULL, role_id uuid NOT NULL,
  subject_kind text NOT NULL CHECK (subject_kind IN ('membership','group','directory_rule')),
  subject_id uuid, subject_rule jsonb,
  scope_kind text NOT NULL CHECK (scope_kind IN ('workspace','department','collection','process','case')),
  scope_id uuid, expires_at timestamptz, granted_by uuid NOT NULL, reason text,
  PRIMARY KEY (workspace_id, id));

CREATE TABLE delegations (workspace_id uuid NOT NULL, id uuid NOT NULL, from_membership uuid NOT NULL,
  to_membership uuid NOT NULL, kind text NOT NULL CHECK (kind IN ('item','out_of_office')),
  work_item_id uuid, process_filter uuid[], starts_at timestamptz NOT NULL, ends_at timestamptz NOT NULL,
  created_by uuid NOT NULL, revoked_at timestamptz, PRIMARY KEY (workspace_id, id),
  CHECK (from_membership <> to_membership), CHECK (ends_at > starts_at));

CREATE TABLE workspace_settings (workspace_id uuid NOT NULL, scope_kind text NOT NULL, scope_id uuid,
  key text NOT NULL, value jsonb NOT NULL, revision bigint NOT NULL DEFAULT 0, updated_by uuid, updated_at timestamptz,
  PRIMARY KEY (workspace_id, scope_kind, COALESCE(scope_id,'00000000-0000-0000-0000-000000000000'::uuid), key));
```

The **effective-permission cache** works as follows:
- `access::compile(workspace)` turns roles and grants into a Cedar `PolicySet` plus entity data.
- The result is cached per workspace, keyed by `access_revision`. Any role, grant or directory change bumps `access_revision` through an outbox event.
- Directory-rule grants are materialised into `grant_members(workspace_id, grant_id, membership_id)` by an async job. Until the job finishes, an `access_stale` flag forces live evaluation.

### 2.4 Library: governed artifacts

All governed content shares one versioned-artifact structure. The kinds are: process, sop, policy, form, rule, calendar, document_template, and announcement-as-content.

```sql
CREATE TABLE collections (workspace_id uuid NOT NULL, id uuid NOT NULL, parent_id uuid, name text NOT NULL,
  path ltree NOT NULL, PRIMARY KEY (workspace_id, id));

CREATE TABLE artifacts (workspace_id uuid NOT NULL, id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('process','sop','policy','form','rule','calendar','document_template')),
  key text NOT NULL, title text NOT NULL, collection_id uuid, owner_membership_id uuid,
  criticality text NOT NULL DEFAULT 'standard' CHECK (criticality IN ('standard','controlled')),
  visibility jsonb NOT NULL DEFAULT '{"mode":"workspace"}', applicability jsonb,   -- policies: who it applies to
  review_cadence_days int, next_review_at timestamptz, last_reviewed_at timestamptz, -- BPMS-044 (0016): NULL cadence = workspace default
  review_overdue_at timestamptz, verified_at timestamptz, verified_by uuid, verification_expires_at timestamptz,
  template_ref text,                                                               -- 'tpl:fin.purchase-requisition@1.2.0'
  category text, restricted boolean NOT NULL DEFAULT false,                       -- BPMS-045 (0017); applicability = Audience jsonb
  current_published_version_id uuid, state text NOT NULL,                          -- denormalised for lists
  tsv tsvector, PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, kind, key));

CREATE TABLE change_requests (workspace_id uuid NOT NULL, id uuid NOT NULL, artifact_id uuid NOT NULL,  -- BPMS-044
  title text NOT NULL, body text NOT NULL, state text NOT NULL CHECK (state IN ('open','accepted','declined','implemented')),
  requested_by uuid NOT NULL, requested_at timestamptz NOT NULL, resolved_by uuid, resolved_at timestamptz,
  resolution text, version_id uuid, revision bigint NOT NULL, PRIMARY KEY (workspace_id, id));

CREATE TABLE artifact_versions (workspace_id uuid NOT NULL, id uuid NOT NULL, artifact_id uuid NOT NULL,
  version_no int NOT NULL, semver text,                -- major.minor for re-ack decisions
  lifecycle text NOT NULL CHECK (lifecycle IN ('draft','in_review','approved','scheduled','published','deprecated','retired')),
  content jsonb NOT NULL,                              -- canvas/source (process), blocks (sop/policy), schema (form)...
  compiled jsonb,                                      -- normalized IR / compiled rule table; NULL for docs
  compiler_version text, content_hash bytea,          -- SHA-256(JCS(content|compiled)) set at approval, immutable after
  change_reason text, effective_at timestamptz, created_by uuid NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
  submitted_by uuid, approved_at timestamptz, published_at timestamptz, published_by uuid,
  revision bigint NOT NULL DEFAULT 0,
  PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, artifact_id, version_no),
  FOREIGN KEY (workspace_id, artifact_id) REFERENCES artifacts(workspace_id, id));

-- immutability guard: no UPDATE of content/compiled/content_hash once lifecycle >= 'approved'
CREATE FUNCTION forbid_mutating_approved() RETURNS trigger AS $$
BEGIN IF OLD.lifecycle IN ('approved','scheduled','published','deprecated','retired')
   AND (NEW.content IS DISTINCT FROM OLD.content OR NEW.compiled IS DISTINCT FROM OLD.compiled
        OR NEW.content_hash IS DISTINCT FROM OLD.content_hash)
   THEN RAISE EXCEPTION 'artifact version % is immutable', OLD.id USING ERRCODE = 'P0001'; END IF; RETURN NEW; END $$ LANGUAGE plpgsql;

CREATE TABLE reviews (workspace_id uuid NOT NULL, id uuid NOT NULL, artifact_version_id uuid NOT NULL,
  reviewer_membership_id uuid NOT NULL, decision text CHECK (decision IN ('approve','reject','changes_requested')),
  comment text, signature_id uuid, decided_at timestamptz, PRIMARY KEY (workspace_id, id));

CREATE TABLE release_bundles (workspace_id uuid NOT NULL, id uuid NOT NULL, process_version_id uuid NOT NULL,
  manifest jsonb NOT NULL,                             -- {"forms":{"request":"<version_id>"},"rules":{...},"calendars":{...},"sop":...}
  manifest_hash bytea NOT NULL, PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, process_version_id));
```

**Artifact lifecycle transitions** (guard → effect):

| From → To | Who (permission) | Guards | Effects |
|---|---|---|---|
| draft → in_review | `*:submit_review` | Compiles with no errors (process). Required fields present. | Reviewers resolved. `review.requested` event. |
| in_review → draft | reviewer `changes_requested` / author withdraw | — | Comment thread kept. |
| in_review → approved | `*:approve` × N reviewers per policy | SoD: approver ≠ author (controlled ones always). N distinct approvers. Step-up for controlled. | `content_hash` frozen. Signatures linked (ESIG-02). |
| approved → scheduled | `*:publish` | `effective_at` > now | Timer created. |
| approved/scheduled → published | `*:publish` / timer | Bundle refs all published. Prior version exists → it moves to deprecated. | `release_bundle` written (process). `current_published_version_id` swapped. Campaign triggered if the major version changed (FR-PL03). Outbox `artifact.published.v1`. |
| published → deprecated | automatic on supersede, or `*:retire` | — | Running instances keep working (pinned). New starts are blocked if the artifact is retired. |
| deprecated → retired | `*:retire` | No active instances, or force with reason | Read-only. Superseded watermark on exports. |

### 2.5 Runtime

```sql
CREATE TABLE process_instances (workspace_id uuid NOT NULL, id uuid NOT NULL, display_id text NOT NULL,
  process_artifact_id uuid NOT NULL, process_version_id uuid NOT NULL, bundle_id uuid NOT NULL,
  state text NOT NULL CHECK (state IN ('created','running','suspended','completed','cancelled','failed')),
  is_test boolean NOT NULL DEFAULT false,               -- test runs (FR-E11) excluded from metrics/inboxes
  initiator_membership_id uuid NOT NULL, department_id uuid, parent_instance_id uuid, case_id uuid,
  title text, revision bigint NOT NULL DEFAULT 0, started_at timestamptz NOT NULL DEFAULT now(), ended_at timestamptz,
  sla_status text, due_at timestamptz, tsv tsvector,
  PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, display_id));
CREATE INDEX ON process_instances (workspace_id, state, started_at DESC);
CREATE INDEX ON process_instances (workspace_id, process_artifact_id, state);

CREATE TABLE instance_variables (workspace_id uuid NOT NULL, instance_id uuid NOT NULL, name text NOT NULL,
  classification text NOT NULL, value jsonb,            -- public/internal/confidential
  value_enc bytea, dek_id uuid,                         -- restricted: AES-256-GCM ciphertext, DEK wrapped by workspace KEK
  updated_seq bigint NOT NULL, PRIMARY KEY (workspace_id, instance_id, name),
  CHECK ((classification = 'restricted') = (value IS NULL AND value_enc IS NOT NULL)));

CREATE TABLE tokens (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, node_id text NOT NULL,
  state text NOT NULL CHECK (state IN ('ready','active','waiting','arrived','consumed','cancelled')),
  split_id uuid, branch_key text, loop_iter int NOT NULL DEFAULT 0, revision bigint NOT NULL DEFAULT 0,
  created_seq bigint NOT NULL, PRIMARY KEY (workspace_id, id));
CREATE INDEX ON tokens (workspace_id, instance_id) WHERE state IN ('ready','active','waiting','arrived');

CREATE TABLE splits (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, join_node_id text NOT NULL,
  expected_branches text[] NOT NULL, arrived_branches text[] NOT NULL DEFAULT '{}', released boolean NOT NULL DEFAULT false,
  PRIMARY KEY (workspace_id, id));

CREATE TABLE jobs (workspace_id uuid NOT NULL, id uuid NOT NULL, kind text NOT NULL,   -- 'advance','service_call','notify','export',...
  instance_id uuid, token_id uuid, payload jsonb NOT NULL, run_at timestamptz NOT NULL DEFAULT now(),
  attempts int NOT NULL DEFAULT 0, max_attempts int NOT NULL DEFAULT 10, lease_owner text, lease_until timestamptz,
  fence bigint NOT NULL DEFAULT 0, last_error jsonb, state text NOT NULL DEFAULT 'queued'
  CHECK (state IN ('queued','leased','done','failed','dead')), idempotency_key text,
  PRIMARY KEY (workspace_id, id), UNIQUE (workspace_id, kind, idempotency_key));
CREATE INDEX jobs_ready ON jobs (run_at) WHERE state = 'queued';

CREATE TABLE timers (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid, token_id uuid,
  purpose text NOT NULL,      -- 'wait','boundary','escalation','sla','publish','reminder','retention'
  due_at timestamptz NOT NULL, calendar_version_id uuid, source_tz text, rule text,
  state text NOT NULL DEFAULT 'scheduled' CHECK (state IN ('scheduled','leased','fired','cancelled')),
  lease_until timestamptz, fence bigint NOT NULL DEFAULT 0, PRIMARY KEY (workspace_id, id));
CREATE INDEX timers_due ON timers (due_at) WHERE state = 'scheduled';

CREATE TABLE instance_events (workspace_id uuid NOT NULL, instance_id uuid NOT NULL, seq bigint NOT NULL,
  event_type text NOT NULL, node_id text, token_id uuid, actor_subject_ref uuid, on_behalf_of uuid,
  payload jsonb NOT NULL, happened_at timestamptz NOT NULL, PRIMARY KEY (workspace_id, instance_id, seq))
  PARTITION BY RANGE (happened_at);   -- monthly partitions

CREATE TABLE incidents (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid, token_id uuid, job_id uuid,
  kind text NOT NULL, message text NOT NULL, details jsonb, state text NOT NULL DEFAULT 'open'
  CHECK (state IN ('open','resolved','ignored')), opened_at timestamptz NOT NULL DEFAULT now(),
  resolved_by uuid, resolution text, PRIMARY KEY (workspace_id, id));
CREATE UNIQUE INDEX one_open_incident ON incidents (workspace_id, token_id) WHERE state = 'open';
```

### 2.6 Work items

```sql
CREATE TABLE work_items (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, token_id uuid NOT NULL,
  node_id text NOT NULL, kind text NOT NULL CHECK (kind IN ('task','approval','sign','acknowledge')),
  state text NOT NULL CHECK (state IN ('offered','claimed','delegated','completed','cancelled','expired')),
  assignee_membership_id uuid, candidates jsonb NOT NULL,     -- resolved snapshot (users/groups) + directory snapshot ref
  acting_for_membership_id uuid,                              -- delegation / OOO
  outcome text, decision jsonb, form_version_id uuid, quorum_group uuid,
  due_at timestamptz, priority smallint NOT NULL DEFAULT 3, revision bigint NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now(), completed_at timestamptz, completed_by uuid, signature_id uuid,
  PRIMARY KEY (workspace_id, id));
CREATE INDEX wi_inbox ON work_items (workspace_id, assignee_membership_id, state, due_at) WHERE state IN ('offered','claimed','delegated');
CREATE TABLE work_item_candidates (workspace_id uuid NOT NULL, work_item_id uuid NOT NULL, membership_id uuid NOT NULL,
  PRIMARY KEY (workspace_id, work_item_id, membership_id));     -- materialised for inbox queries
```

Work item state machine (outcome is separate from state; this fixes v0.1 gap A2):

| From | Event | To | Guard |
|---|---|---|---|
| offered | claim | claimed | Actor ∈ candidates. SoD ok. |
| claimed | unclaim | offered | Actor = assignee. |
| offered/claimed | delegate | delegated | Delegate eligible (12 §9). |
| delegated | complete (by delegate) | completed | `acting_for` recorded. |
| offered/claimed | complete(outcome) | completed | Revision match. Form valid. SoD re-check. Step-up if required. |
| any open | token cancelled / instance cancelled | cancelled | — |
| any open | escalation `outcome`/deadline | expired | Process policy allows it. |

### 2.7 Campaigns: acknowledgements, signatures, announcements

```sql
CREATE TABLE campaigns (workspace_id uuid NOT NULL, id uuid NOT NULL,
  subject_kind text NOT NULL CHECK (subject_kind IN ('artifact_version','announcement','document','attestation')),
  subject_id uuid NOT NULL, subject_hash bytea NOT NULL,     -- binds evidence to exact content
  action text NOT NULL CHECK (action IN ('read','acknowledge','quiz','sign')),
  audience_rule jsonb NOT NULL, include_new_joiners boolean NOT NULL DEFAULT true,
  deadline_at timestamptz, reminder_schedule jsonb, escalation jsonb, signature_mode text,
  state text NOT NULL CHECK (state IN ('draft','scheduled','active','closed','cancelled')),
  created_by uuid NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (workspace_id, id));

CREATE TABLE campaign_recipients (workspace_id uuid NOT NULL, campaign_id uuid NOT NULL, membership_id uuid NOT NULL,
  state text NOT NULL CHECK (state IN ('pending','viewed','completed','declined','expired','excused')),
  viewed_at timestamptz, completed_at timestamptz, quiz_score numeric(5,2), envelope_id uuid,
  evidence jsonb,                                           -- auth method, ip, ua, version hash
  reminders_sent int NOT NULL DEFAULT 0, escalated_at timestamptz,
  PRIMARY KEY (workspace_id, campaign_id, membership_id));
CREATE INDEX ON campaign_recipients (workspace_id, membership_id, state);

CREATE TABLE announcements (workspace_id uuid NOT NULL, id uuid NOT NULL, title text NOT NULL, body jsonb NOT NULL,
  category text, urgent boolean NOT NULL DEFAULT false, audience_rule jsonb NOT NULL, audience_snapshot_count int,
  publish_at timestamptz, expires_at timestamptz, pinned boolean NOT NULL DEFAULT false, comments_enabled boolean,
  campaign_id uuid, state text NOT NULL, author_membership_id uuid NOT NULL, PRIMARY KEY (workspace_id, id));
-- As built (0014, BPMS-047, crate `procera-announcements`): `announcements` (title, `content` jsonb block document
-- as in the library, `content_hash` frozen at publication, `audience` = members::audience::Audience, priority
-- normal|critical, pinned, require_ack, publish_at, expires_at NOT NULL, state draft|pending_approval|scheduled|
-- published|archived, approval_required decided at submission, channels resolved when publication is requested,
-- decided_by/at/comment, published_by (NULL = scheduler), recipient_count, archived_*); a trigger freezes title,
-- content, audience and require_ack once out of draft and forbids deletes. `announcement_recipients` is the
-- audience snapshot taken at publication and holds the receipts (read_at; acked_at + ack_ip + ack_user_agent).
-- Approval: `announcements.approval_required` off | for_workspace_wide (audience.all) | always; author ≠ approver
-- when it applies. Expiry is derived (published and expires_at ≤ now → hidden from feeds, `expired: true`), not
-- a state change. Scheduled publication: `discovery.announcements_due_workspaces(as_of)` + scheduler/`__test/drain`.
-- Delivery: `announcement.published.v1` → notify consumer → in-app and/or email per channels (critical adds email),
-- deduplicated per announcement and recipient. Category, comments, reactions, quiz, deadline/reminders/manager
-- escalation and the campaign link (`campaign_id`) are not built yet.

CREATE TABLE envelopes (workspace_id uuid NOT NULL, id uuid NOT NULL, campaign_id uuid, instance_id uuid,
  source_file_id uuid NOT NULL, rendered_file_id uuid NOT NULL, rendered_hash bytea NOT NULL,
  order_mode text NOT NULL CHECK (order_mode IN ('sequential','parallel')),
  state text NOT NULL CHECK (state IN ('draft','sent','completed','declined','voided','expired')),
  completed_file_id uuid, certificate_file_id uuid, expires_at timestamptz, PRIMARY KEY (workspace_id, id));
CREATE TABLE envelope_signers (workspace_id uuid NOT NULL, envelope_id uuid NOT NULL, ord int NOT NULL,
  membership_id uuid, external_email citext, external_name text, role text NOT NULL DEFAULT 'signer',
  state text NOT NULL, signature_id uuid, PRIMARY KEY (workspace_id, envelope_id, ord));
CREATE TABLE envelope_fields (workspace_id uuid NOT NULL, envelope_id uuid NOT NULL, id uuid NOT NULL, signer_ord int NOT NULL,
  kind text NOT NULL, page int NOT NULL, rect numeric[4] NOT NULL, required boolean NOT NULL, value jsonb,
  PRIMARY KEY (workspace_id, envelope_id, id));

CREATE TABLE signatures (workspace_id uuid NOT NULL, id uuid NOT NULL, signer_subject_ref uuid NOT NULL,
  record_type text NOT NULL, record_id uuid NOT NULL, record_hash bytea NOT NULL, meaning text NOT NULL,
  reason text, mode text NOT NULL CHECK (mode IN ('simple','strong','part11')),
  auth_method text NOT NULL, auth_time timestamptz NOT NULL, ip inet, user_agent text,
  manifestation text NOT NULL, signed_at timestamptz NOT NULL, audit_seq bigint NOT NULL,
  PRIMARY KEY (workspace_id, id), CHECK (signed_at - auth_time <= interval '15 minutes'));
```

Recipient fan-out works as follows:
- When a campaign activates, the audience rule is resolved in batches of 1 000 in a job, and recipients are inserted with `ON CONFLICT DO NOTHING`.
- New joiners get applicable campaigns from a `membership.activated` consumer (FR-AK03).
- Reminders and escalations are timers with purpose `reminder`. The `reminders_sent` counter and a per-campaign idempotency key ensure each reminder is sent at most once per step.

**As built (0013, BPMS-046, crate `procera-campaigns`):** acknowledgement campaigns over published SOP/policy versions. Signatures, quizzes, announcements and attestations join later as further `subject_kind`/`action` values.
- `campaigns`: `subject_kind = 'artifact_version'` with `subject_version_id` (FK to `artifact_versions`) and `subject_hash` (the version's content hash at creation); `audience_rule` is the shared `procera_members::audience::Audience` (`all`, `departments` with sub-departments, `groups`, `roles`); `audience_snapshot` `at_launch | dynamic`; `ack_text`; `due_at` (explicit, or launch + `due_in_secs`); `reminder_schedule` (ascending seconds after assignment) and `escalation_after_secs` (NULL = off). States `draft → active → closed | cancelled`. `campaign.*` workspace settings (docs/20 §2.14) are **copied onto the campaign at creation**, so a later settings change never alters a running campaign.
- `campaign_recipients` (one row per member, `UNIQUE (campaign_id, membership_id)`, own `id` for the API): `source launch | joiner`, `state pending | completed`, `assigned_at`, `due_at`, and on completion `ack_version_id`, `ack_content_hash`, `ack_text`, `ack_ip`, `ack_user_agent`, `evidence {amr, auth_time}`; `reminders_sent`, `last_reminded_at`, `escalated_at`, `escalated_to`.
- Triggers freeze evidence: campaigns and assignments are never deleted; a launched campaign keeps its subject, audience and statement; an ended campaign never changes; a completed assignment never changes.
- Launch resolves the audience in the request transaction (batched `unnest` insert, `ON CONFLICT DO NOTHING`) and writes the in-app notification and queued email per assignee with dedupe key `campaign:{recipient}:assigned`. The 1 000-row job batching above is the target for very large audiences.
- Reminder offsets count from each member's `assigned_at` (so joiners get their own ladder). Escalation also counts from `assigned_at` (default 14 d, equal to the default due date) and happens once, to `Members::resolve_manager` (explicit manager, else nearest department head); with no manager it is recorded with `escalated_to = NULL`. A scheduler pass that finds several missed steps sends **one** reminder and records the latest step. Only active members are reminded. The scheduler (`serve --all`, every 5 s) and `/__test/drain` find work through `discovery.campaign_due_workspaces(as_of)` with `as_of` from the application clock. Reminder steps are audited per campaign per pass (`campaign.reminders_sent`), escalations per assignment (`campaign.escalated`), both as the system.
- Dynamic campaigns: the bus consumer `campaigns` on `membership.activated.v1` enrols a matching new member (`campaign.recipient_enrolled`, system) with a due date that keeps the campaign's window (`now + (due_at − launched_at)`). Joining a department after activation does not yet enrol (directory changes emit no outbox event yet).
- Acknowledging: own assignment only (anyone else's → not found), once (409 `already_acknowledged`), only while active (409); an optional `content_hash` from the client must match (409 `content_changed`). Closed and cancelled campaigns keep open assignments as the record of non-acknowledgement.
- Outbox: `campaign.activated.v1`, `campaign.recipient.completed.v1`, `campaign.closed.v1`, `campaign.cancelled.v1`.
- Overdue = `pending` and `due_at` < now (application clock). Notifications honour no channel preferences yet (`notify.channels.enabled` arrives with FR-N04).

The envelope signing protocol (FR-DS04/05):
1. `rendered_hash` is computed once at send time over the flattened PDF. Signers are shown that exact file.
2. At signing, after re-auth, the signer submits their field values with an `Idempotency-Key`. The server verifies the hash, the envelope state and the signer's turn (sequential mode), then writes the `signatures` row and the audit event.
3. On the last signer, a `finalize_envelope` job runs:
   - It stamps the field values and signature manifestations onto the PDF in the sandboxed convert worker.
   - It appends the certificate-of-completion pages.
   - It applies the platform PAdES signature (P1) and an optional TSA timestamp.
   - It stores `completed_file_id`, locks the file (FR-F05) and completes the campaign recipient or work item.

### 2.8 Cases & investigations

```sql
CREATE TABLE cases (workspace_id uuid NOT NULL, id uuid NOT NULL, display_id text NOT NULL,
  case_type text NOT NULL,                 -- template key, incl. 'investigation'
  title text NOT NULL, stage text NOT NULL, state text NOT NULL CHECK (state IN ('open','on_hold','closed')),
  owner_membership_id uuid, restricted boolean NOT NULL DEFAULT true, legal_hold boolean NOT NULL DEFAULT false,
  opened_at timestamptz NOT NULL DEFAULT now(), closed_at timestamptz, outcome text, findings jsonb,
  PRIMARY KEY (workspace_id, id));
CREATE TABLE case_members (workspace_id uuid NOT NULL, case_id uuid NOT NULL, membership_id uuid NOT NULL, case_role text NOT NULL,
  PRIMARY KEY (workspace_id, case_id, membership_id));
CREATE TABLE case_items (workspace_id uuid NOT NULL, id uuid NOT NULL, case_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('activity','milestone','note','evidence_file','pinned_audit','pinned_instance','pinned_file')),
  ref_id uuid, snapshot jsonb, snapshot_hash bytea,        -- pinned evidence is immutable
  state text, due_at timestamptz, created_by uuid NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id));
```

### 2.9 Files

```sql
CREATE TABLE files (workspace_id uuid NOT NULL, id uuid NOT NULL, object_key text NOT NULL,  -- ws/{ws}/{yyyy}/{mm}/{id}
  filename text NOT NULL, declared_type text, detected_type text, size_bytes bigint NOT NULL, sha256 bytea NOT NULL,
  dek_wrapped bytea NOT NULL, scan_state text NOT NULL CHECK (scan_state IN ('pending','clean','infected','error','skipped')),
  scan_engine text, scanned_at timestamptz, preview_file_id uuid,
  owner_kind text, owner_id uuid,                -- request/case/sop/envelope/announcement...
  uploaded_by uuid NOT NULL, uploaded_at timestamptz NOT NULL DEFAULT now(),
  retention_class text NOT NULL, retain_until timestamptz, legal_hold boolean NOT NULL DEFAULT false,
  locked boolean NOT NULL DEFAULT false,         -- referenced by published version / signed record
  version_of uuid, deleted_at timestamptz, last_accessed_at timestamptz,
  PRIMARY KEY (workspace_id, id));
CREATE INDEX ON files (workspace_id, owner_kind, owner_id);
CREATE INDEX ON files (workspace_id, uploaded_at DESC);
```

The upload pipeline:
1. `POST /v1/files/uploads` performs the authz and quota checks, then returns a presigned **multipart** upload (or tus-style chunk endpoints proxied by the API when the object store isn't reachable from the browser). Files are encrypted client→server via TLS. The server then encrypts at rest with a per-file DEK; for direct-to-S3 uploads, SSE-C with the per-file key is used.
2. `POST /v1/files/uploads/{id}/complete` sets `scan_state=pending` and enqueues a `scan` job.
3. The scan worker:
   - Streams the object and computes its SHA-256 (it must match the client-declared hash when one was sent).
   - Sniffs magic bytes and checks the result against the workspace allowlist.
   - Runs ClamAV `INSTREAM`.
   - Sets `clean` and enqueues `preview`, or sets `infected`, moves the object to a quarantine prefix, and raises an incident and an alert.
4. Preview runs in a sandboxed converter (no network, seccomp, CPU/time/memory limits) and produces a PDF/PNG preview. Active content is never rendered.
5. Download goes through `GET /v1/files/{id}/content`: authz check (the owning object's permission) → scan state must be `clean` → mint a 5-minute signed URL bound to the user and workspace → audit `file.downloaded` for confidential and above.

### 2.10 Audit

```sql
CREATE TABLE audit_events (workspace_id uuid NOT NULL,          -- NULL-equivalent '0000..' for system plane chain
  seq bigint NOT NULL, id uuid NOT NULL, occurred_at timestamptz NOT NULL,
  actor_subject_ref uuid, actor_kind text NOT NULL,             -- user/service/system/guest/break_glass
  on_behalf_of uuid, session_id_hash bytea, ip inet, user_agent text,
  category text NOT NULL,                                        -- auth/access/config/content/runtime/file/data_access/export/admin/system
  action text NOT NULL, object_type text, object_id uuid, result text NOT NULL CHECK (result IN ('allowed','denied','error')),
  reason text, policy_ids text[], before_hash bytea, after_hash bytea, diff jsonb,  -- diff redacts restricted values
  trace_id text, prev_hash bytea NOT NULL, hash bytea NOT NULL,
  PRIMARY KEY (workspace_id, seq)) PARTITION BY RANGE (occurred_at);
REVOKE UPDATE, DELETE, TRUNCATE ON audit_events FROM procera_app, procera_system;
CREATE INDEX ON audit_events (workspace_id, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, actor_subject_ref, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, object_type, object_id, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, category, action, occurred_at DESC);

CREATE TABLE audit_heads (workspace_id uuid PRIMARY KEY, last_seq bigint NOT NULL, last_hash bytea NOT NULL);
CREATE TABLE audit_checkpoints (workspace_id uuid NOT NULL, upto_seq bigint NOT NULL, merkle_root bytea NOT NULL,
  signature bytea NOT NULL, key_id text NOT NULL, tsa_token bytea, created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, upto_seq));
```

Chain append algorithm (in the same transaction as the business change, so it is atomic):
1. `SELECT last_seq, last_hash FROM audit_heads WHERE workspace_id=$1 FOR UPDATE`. This serialises appends per workspace. Sharding is not needed at reference load: measured by SP-10, with batching as the fallback.
2. `seq = last_seq+1`, `hash = SHA-256(last_hash ‖ JCS(event without hash))`.
3. Insert the event and update the head.

High-volume categories (`data_access` reads) use **batched appends**: events are buffered in an `audit_pending` table and a 1-second chain-sealer job takes over the head lock. This keeps hot read paths from contending, and the chain is still gap-free.

Explorer, as built (0015, BPMS-064; `procera_audit::explorer`, `/api/v1/audit/*`):
- **Queries** filter on time range (`from` inclusive, `to` exclusive), actor subject (or a membership id resolved to it), action prefix (`LIKE 'prefix%'`, served by the new `(workspace_id, action text_pattern_ops, occurred_at DESC)` index), category, object type/id and result. Keyset pagination on `seq`, newest first (`seq < cursor ORDER BY seq DESC`, per-partition PK scans merged); the cursor is the last `seq`. Actor names are resolved only for members of the workspace; other subjects stay pseudonymous.
- **Scope:** `audit:view` reads the whole workspace chain; `audit:view_own` adds `actor_subject_ref = caller`.
- **Timelines:** every event for one object, oldest first; `artifact` expands to its versions (the library records lifecycle events per `artifact_version`).
- **Saved queries:** `audit_saved_queries (workspace_id, id, membership_id, name, filters jsonb, created_at)`, forced RLS, unique `(workspace_id, membership_id, lower(name))`, at most 100 per member; `filters` uses the same strict shape as the query (unknown fields rejected). Saving and deleting append `audit.query_saved` / `audit.query_deleted` (category `admin`).
- **Exports:** synchronous CSV (RFC 4180; cells starting with `= + - @` are prefixed with `'`) or JSON lines, newest first, capped at 10 000 rows with a truncated flag; `audit:view` + `audit:export` + step-up. Each export appends `audit.exported` (category `export`, with format, row count, truncated flag and filters) in the same transaction. The object-store export job (04 §5) replaces the cap later.
- **Reads are not logged per query:** audit events carry no restricted values (diffs are redacted when written), and `data_access` events are defined for restricted/confidential reads (06 §7, 24 §3.5). Exports are always logged (FR-T01).

Checkpoints:
- Every 5 minutes or every 10 000 events, compute an RFC 9162-style Merkle root over `(last_checkpoint, upto_seq]` and sign it with the platform audit key (Ed25519 in KMS).
- Optionally request an RFC 3161 token for it.
- Optionally export the sealed segment to an Object-Lock bucket.

### 2.11 Outbox / inbox / idempotency

```sql
CREATE TABLE outbox (workspace_id uuid NOT NULL, id uuid NOT NULL, topic text NOT NULL, aggregate_id uuid NOT NULL,
  aggregate_revision bigint NOT NULL, payload jsonb NOT NULL, schema_version int NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(), published_at timestamptz, attempts int NOT NULL DEFAULT 0,
  PRIMARY KEY (workspace_id, id));
CREATE INDEX outbox_unpublished ON outbox (created_at) WHERE published_at IS NULL;
CREATE TABLE inbox_dedup (workspace_id uuid NOT NULL, consumer text NOT NULL, message_id uuid NOT NULL,
  processed_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (workspace_id, consumer, message_id));
CREATE TABLE idempotency_keys (workspace_id uuid NOT NULL, actor_id uuid NOT NULL, operation text NOT NULL, key text NOT NULL,
  request_hash bytea NOT NULL, status int, response jsonb, created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL, PRIMARY KEY (workspace_id, actor_id, operation, key));
```

The dispatcher:
- It loops `SELECT … FROM outbox WHERE published_at IS NULL ORDER BY created_at LIMIT 500 FOR UPDATE SKIP LOCKED`, delivers to the `Bus`, then sets `published_at`.
- A `pg_notify('outbox', '')` issued after commit wakes it. It also polls every 1 s, because NOTIFY can be lost.
- Ordering is per `aggregate_id`: the dispatcher partitions batches by aggregate and delivers each partition sequentially.
- Consumers insert into `inbox_dedup` in the same transaction as their effect.

### 2.12 Business calendars, SLA, notifications, settings history

```sql
CREATE TABLE calendar_versions (workspace_id uuid NOT NULL, id uuid NOT NULL, calendar_artifact_id uuid NOT NULL,
  version_label text NOT NULL, timezone text NOT NULL, working_hours jsonb NOT NULL,   -- per weekday ranges
  holidays date[] NOT NULL, PRIMARY KEY (workspace_id, id));
CREATE TABLE sla_clocks (workspace_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, sla_key text NOT NULL,
  node_id text, target interval NOT NULL, business boolean NOT NULL, calendar_version_id uuid,
  started_at timestamptz NOT NULL, paused_total interval NOT NULL DEFAULT '0', paused_since timestamptz,
  due_at timestamptz NOT NULL, stopped_at timestamptz, breached_at timestamptz, PRIMARY KEY (workspace_id, id));
-- As built (0011, BPMS-048): `notifications` (in-app inbox: membership, kind, title, body, link, ref, dedupe_key, read_at)
-- and `email_messages` (to, subject, body_sealed, dedupe_key, state queued|sent|failed, attempts, next_attempt_at).
-- Outbox consumers write them with inbox dedup; the worker sends due email over SMTP with backoff (2^n s, max 1 h, 12 tries).
-- `notification_intents` below remains the target once channels, digests and quiet hours (FR-N04) land.
CREATE TABLE notification_intents (workspace_id uuid NOT NULL, id uuid NOT NULL, membership_id uuid NOT NULL,
  kind text NOT NULL, ref_type text, ref_id uuid, channel text NOT NULL, priority smallint NOT NULL,
  digest_key text, state text NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), sent_at timestamptz,
  PRIMARY KEY (workspace_id, id));
CREATE TABLE saved_views (workspace_id uuid NOT NULL, id uuid NOT NULL, list_key text NOT NULL, name text NOT NULL,
  owner_membership_id uuid NOT NULL, shared_with jsonb, definition jsonb NOT NULL, is_default boolean NOT NULL DEFAULT false,
  PRIMARY KEY (workspace_id, id));
```

---

## 3. Request pipeline (every HTTP call)

```
TLS → reverse proxy (HSTS, size limits) → Axum
 1 request_id/trace_id (W3C traceparent)
 2 rate limit (per IP, per session, per workspace; token bucket in Valkey or in-proc fallback)
 3 authn: session cookie (__Host-sid) or Bearer access token → Principal{user, plane, workspace, amr, auth_time}
 4 CSRF: non-GET with cookie auth requires Origin ∈ allowlist AND X-CSRF header (double-submit token)
 5 workspace state gate: suspended_ro → reject writes 423; locked → 423 all
 6 handler: parse+validate DTO (serde + validator; unknown fields rejected)
 7 authz.check(principal, action, resource-with-attrs) → Allow{policy_ids} | Deny → 404/403 per disclosure
 8 UnitOfWork: BEGIN; set_config('app.workspace_id', ws, true); set_config('app.actor', subj, true)
 9 idempotency (mutations): lookup/insert key row; same hash → replay stored response; different → 409
10 domain command → state change + instance_events + outbox + audit append (same tx)
11 COMMIT → pg_notify wakeups
12 response: redaction serializer (field classifications) → ETag = revision
```

Errors use RFC 9457 `application/problem+json` with a stable `type` URI, plus `code`, `detail`, `trace_id` and optional `errors[]` (field-level). See [08-API-SPEC §3](08-API-SPEC.md).

---

## 4. Runtime algorithms (native interpreter)

### 4.1 Instance state machine

| From | To | Trigger | Guard |
|---|---|---|---|
| created | running | start committed | Bundle published. Initiator may start. |
| created | cancelled | start validation failed after create / admin | — |
| running | suspended | admin `suspend_resume` | Reason required |
| suspended | running | admin resume | — |
| running | completed | last token consumed at `end` | No open tokens |
| running | cancelled | user/admin cancel, or `end{terminate}` | Cancel rights |
| running | failed | Unrecoverable incident and process policy `on_incident=fail` | Default policy keeps it running with an open incident |
| failed | running | admin retry | Incident resolved |

An instance is "waiting" as a *derived* view (every live token is `waiting`), not a stored state. This removes v0.1's `RUNNING↔WAITING` churn.

### 4.2 Advance loop (`advance` job)

```text
advance(ws, instance_id, fence):
  tx:
    lock instance row FOR UPDATE (revision r)
    tokens = ready tokens of instance ORDER BY created_seq
    for each token (bounded: ≤ 200 steps per tx, else re-enqueue advance):
      node = bundle.ir.node(token.node_id)
      match node.kind:
        start/set/decision → evaluate expressions (budgeted) → move token along edge → append event
        parallel.split     → create split row; spawn child tokens per branch; consume token
        parallel.join      → mark token arrived; append branch_key to split.arrived (array_append, unique);
                             if arrived ⊇ expected and not released → released=true; spawn one token; consume arrivals
        human.* / human.sign / human.acknowledge → resolve assignment → insert work_item(s)/campaign/envelope → token waiting
        wait.timer         → compute due_at via calendar version → insert timer → token waiting
        wait.signal        → insert correlation subscription → token waiting (+ timeout timer)
        service.call/notify→ insert job(kind=service_call|notify, idempotency_key=eval(expr)) → token waiting (notify: move on)
        subprocess.call    → insert child instance (pinned version) + advance job for child → token waiting
        end                → consume token; if terminate → cancel siblings; if no live tokens → instance completed
    update instance revision r+1; insert outbox (instance.*), audit (runtime category, system actor acting for instance)
  commit
```

**Completion signals** come from sources such as a work-item completion, a timer firing, a service result or a correlated message. Each one is a command that:
- locks the instance,
- validates the token's state and revision,
- moves the token to `ready`,
- enqueues `advance` with `idempotency_key = signal_id`.

Duplicate signals hit the `jobs` unique key or fail the token state check, so they are no-ops.

### 4.3 Concurrency guarantees

- **One writer per instance:** every change to an instance takes `SELECT … FOR UPDATE` on `process_instances` first. Locks are always acquired in the order instance → token → work_item, which prevents deadlocks.
- **Double approval (T01):** two decisions race on the same work item. The second one blocks on the instance lock, then sees `state=completed`, and returns `409 conflict` (or the stored replay if it used the same idempotency key).
- **Leases with fencing:** jobs and timers are claimed with `UPDATE … SET lease_owner, lease_until=now()+30s, fence=fence+1 WHERE … FOR UPDATE SKIP LOCKED RETURNING fence`. The completing write checks `fence = $claimed`. A worker whose lease expired cannot overwrite the result of a newer claimant.
- **External side effects** are at-least-once. A connector receives an `idempotency_key` and must honour it. The result is recorded before the token advances, and a crash between the call and the record leads to a retry carrying the same key.

### 4.4 Timers & scheduler

- The scheduler loop runs every 1 s with jitter. It claims due timers (`due_at <= now() + 1s`) in batches of 500 using `SKIP LOCKED`.
- For each timer, in one transaction: set `fired`, apply its effect (complete the token / escalate / SLA breach / reminder / publish), and write outbox and audit.
- Business durations: `due = calendar.add(start, duration)`. It is computed from the calendar version pinned at creation and stored together with `source_tz` and `rule`, so it can be recomputed in an audit.
- Timer lateness (`now() - due_at` at fire) is exported as a histogram. The SLO is 99.99 % within 60 s (06).

### 4.5 Expression evaluation

- Source is compiled at publish into a typed AST stored in `compiled`.
- Evaluation has a budget of ≤ 10 000 steps and ≤ 1 MB of intermediate allocation. Exceeding the budget raises an incident `IR-R010`, never a panic.
- `now()` is bound to the triggering event's timestamp.
- Engine choice: SP-08 / [ADR-0010](adr/ADR-0010-expression-language.md).

### 4.6 Assignment resolution

`resolve(candidates, context)` runs these steps:
1. Expand users, groups, roles and directory rules against the current directory (or the last snapshot).
2. Apply SoD exclusions (`excludeInitiator`, `excludeActorsOf`).
3. Apply OOO substitutions (`delegations` where `kind=out_of_office` and now ∈ [starts, ends] and the process matches).
4. Apply the strategy: `claim` offers to all, `round_robin` uses a per-node cursor, `least_loaded` takes the min open items, `all` creates a quorum.
5. If the result is empty, use the fallback. If that is empty too, raise incident `NO_ASSIGNEE` and notify the process owner.

The resolution snapshot is stored on the work item.

---

## 5. Data-grid query engine (FR-V01–V09)

Every list endpoint accepts the same query model:

```json
GET /v1/requests?view=… or POST /v1/requests/query
{ "filter": { "and": [ {"field":"state","op":"in","value":["running"]},
                        {"field":"sla_status","op":"eq","value":"at_risk"},
                        {"or":[{"field":"department","op":"subtree","value":"d-fin"},{"field":"initiator","op":"eq","value":"me"}]} ] },
  "sort": [{"field":"due_at","dir":"asc"},{"field":"id","dir":"asc"}],
  "columns": ["display_id","title","process","current_step","assignee","due_at","sla_status"],
  "page": {"size": 50, "cursor": null}, "q": "laptop" }
```

- **List schema registry:** each list declares fields with `type`, `operators`, `sortable`, `filterable`, a SQL expression, the required permission and a classification. Unknown fields and operators are rejected with `400`. Sorts on columns without an index are allowed only below a row estimate, or return a warning header.
- **Compilation:** the filter AST is turned into a parameterised SQL `WHERE` using `sqlx::QueryBuilder`. Values are always bound, never concatenated. The authz scope predicate (from Cedar partial evaluation, or hand-compiled scope filters per list) is ANDed in, and RLS applies underneath that.
- **Keyset pagination:** the cursor is the base64 of the last row's sort tuple plus a filter hash. A tie-breaker on `id` is mandatory.
- **Counts:** exact `count(*)` with a `LIMIT 10001` probe. Above that, `pg_class`/EXPLAIN estimates, labelled `approximate`.
- **Export:** the same compiled query is streamed by an `export` job to CSV/XLSX in the object store. The redaction serializer is applied, the download is audited, and the link expires after 24 h.
- **Saved views** store the same JSON. The URL `?v=<base64url(json)>` carries an unsaved view.

---

## 6. Search

- `tsvector` columns are maintained by triggers on artifacts, instances, cases and announcements, with weighted title, body and tags, and `simple` plus language configs per workspace locale. `pg_trgm` handles fuzzy and short queries.
- Results are filtered by the same scope predicates as lists. Restricted variables are never indexed.
- An optional Tantivy index is fed from the outbox (one index per workspace) for large tenants.

**As built (0015, BPMS-049; crate `procera-search`, `GET /api/v1/search`):**
- **Artifacts:** `artifacts.search_body` (every string `text` value of the indexed version's content, capped at 200 000 characters) and `artifacts.tsv` = `simple`(title + key words, A) ‖ `english`(title, A) ‖ `english`(body, B). A `BEFORE INSERT OR UPDATE` trigger on `artifacts` recomputes both; an `AFTER INSERT OR UPDATE OF content, lifecycle` trigger on `artifact_versions` touches the artifact row (revision untouched, so ETags hold). The indexed version is the published one, else the newest: what `sop:view`/`policy:view` holders can already read through the library API. Triggers were chosen over write-path updates so every writer (API, scheduler, future imports) keeps the index current in the same transaction.
- **Members, departments, groups:** matched live, not copied. Users are global and change outside any workspace transaction, so a copied index could go stale; expression indexes (`gin_trgm_ops` on `users.display_name`, `users.email::text`, `memberships.title`, `departments.name`, `groups.name`; `to_tsvector('simple', …)` on departments and groups) keep it fast. Only active members are returned.
- **Query:** `websearch_to_tsquery('english', q) || websearch_to_tsquery('simple', q)`, OR `ILIKE '%q%'`, OR `q <% title` (word similarity, threshold 0.45 set transaction-locally), OR `q % title`. Rank = `ts_rank_cd` + `word_similarity(q, title)` + 0.5 when the title starts with q; one ranked list across types, at most 50. Snippets: `ts_headline` with control-character delimiters, then HTML-escaped and turned into `<mark>` (indexed text can never inject markup).
- **Permissions:** the handler asks the policy evaluator for each requested type first (`sop:view`, `policy:view`, `member:view`, `directory:view` for departments and groups) and passes only the allowed ones; forced RLS scopes every row. Nothing classified is indexed (library content has no classified fields today; when classified fields arrive they stay out of `search_body`).
- Trigram GIN indexes do not lead with `workspace_id` (that needs `btree_gin`); the RLS predicate filters after the index. Revisit with the Tantivy option for large tenants.

---

## 7. Settings resolution

`effective(ws, scope_chain, key) = clamp_to_floor(first_defined(process → collection/department → workspace → profile_default → platform_default), platform_floor)`

- The result is cached per `(ws, settings_revision)`.
- The admin UI shows an **explainer** listing each layer and its value.
- Settings JSON is validated against a schema per key; the catalogue is defined in [20](20-WORKSPACE-CONFIGURATION.md).

---

## 8. Key management (summary; protocols in 06)

```
Root key (KMS/HSM, or sealed local key for self-host)
 └── Workspace KEK (per workspace; rotated yearly or on demand; versioned)
      └── DEKs (per file, per restricted variable set, per subject identity row)
```

- Data is encrypted with AES-256-GCM, using a 96-bit random nonce and AAD = `workspace_id ‖ table ‖ row id ‖ column`.
- DEKs are wrapped by the KEK, and the wrapped DEK is stored with the data.
- **KEK rotation** rewraps DEKs in a background job; the data itself is not re-encrypted.
- **Workspace deletion** destroys the KEK, which crypto-shreds the workspace's data.
- **Subject erasure** destroys the DEK of the identity row.

---

## 9. Domain events (outbox topics, v1)

`workspace.{created,suspended,restored,deleted}` · `membership.{invited,activated,deactivated}` ·
`access.changed` · `artifact.{submitted,approved,published,deprecated,retired}` ·
`instance.{started,completed,cancelled,failed}` · `work.{offered,claimed,delegated,completed,expired}` ·
`sla.{started,paused,resumed,breached,stopped}` · `campaign.{activated,recipient.completed,closed}` ·
`envelope.{sent,viewed,signed,declined,completed}` · `announcement.published` ·
`file.{uploaded,clean,quarantined,deleted}` · `case.{opened,closed}` · `incident.{opened,resolved}`.

The envelope is defined in [08-API-SPEC §6](08-API-SPEC.md). Payloads carry ids and outcomes, never restricted values or form contents.

---

## 10. Performance budgets & indexes (verify in SP-01/SP-10)

| Path | Budget (P95) | Primary index |
|---|---|---|
| My Work inbox (50 rows) | 120 ms | `wi_inbox`, `work_item_candidates` |
| All requests (admin, filtered, 50 rows) | 250 ms | `(workspace_id, state, started_at)` + filter-specific |
| Decision commit | 150 ms DB time | PK locks |
| Advance step | 5 ms/step | PKs |
| Audit explorer (10 M events, indexed filter) | 2 s | audit composite indexes + monthly partitions |
| Campaign fan-out | 5 000 recipients/min | batched inserts |

---

## 11. Migrations & upgrades

- Migrations are forward-only, using **expand → migrate → contract** across at least one release, so rolling deploys work.
- Every release runs an N-1 → N upgrade test against a database holding live (waiting) instances (15-TEST).
- IR `schemaVersion` migrations are pure functions applied on read. Stored IR is never rewritten after publish.
- The audit chain format is versioned (`hash_v`), and the verifier supports all versions.
