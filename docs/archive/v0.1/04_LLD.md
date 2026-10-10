# Low-Level Design (LLD)

## Core tables (PostgreSQL; conceptual DDL)
```sql
CREATE TABLE tenants (id uuid PRIMARY KEY, name text NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE processes (tenant_id uuid NOT NULL, id uuid NOT NULL, slug text NOT NULL, name text NOT NULL, owner_id uuid, status text NOT NULL, PRIMARY KEY (tenant_id,id), UNIQUE (tenant_id,slug));
CREATE TABLE process_versions (tenant_id uuid NOT NULL, id uuid NOT NULL, process_id uuid NOT NULL, version integer NOT NULL, ir jsonb NOT NULL, checksum text NOT NULL, published_at timestamptz, published_by uuid, PRIMARY KEY (tenant_id,id), UNIQUE (tenant_id,process_id,version), FOREIGN KEY (tenant_id,process_id) REFERENCES processes(tenant_id,id));
CREATE TABLE process_instances (tenant_id uuid NOT NULL, id uuid NOT NULL, definition_version_id uuid NOT NULL, state text NOT NULL, variables jsonb NOT NULL DEFAULT '{}'::jsonb, revision bigint NOT NULL DEFAULT 0, started_by uuid NOT NULL, started_at timestamptz NOT NULL DEFAULT now(), ended_at timestamptz, PRIMARY KEY (tenant_id,id), FOREIGN KEY (tenant_id,definition_version_id) REFERENCES process_versions(tenant_id,id));
CREATE TABLE execution_tokens (tenant_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, node_id text NOT NULL, state text NOT NULL, join_group text, revision bigint NOT NULL DEFAULT 0, PRIMARY KEY (tenant_id,id), FOREIGN KEY (tenant_id,instance_id) REFERENCES process_instances(tenant_id,id));
CREATE TABLE human_work (tenant_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, token_id uuid NOT NULL, assignee_id uuid, candidate_role text, state text NOT NULL, due_at timestamptz, decision jsonb, revision bigint NOT NULL DEFAULT 0, PRIMARY KEY (tenant_id,id), FOREIGN KEY (tenant_id,instance_id) REFERENCES process_instances(tenant_id,id));
CREATE TABLE timers (tenant_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, token_id uuid NOT NULL, due_at timestamptz NOT NULL, state text NOT NULL, lease_until timestamptz, PRIMARY KEY (tenant_id,id));
CREATE TABLE instance_events (tenant_id uuid NOT NULL, id uuid NOT NULL, instance_id uuid NOT NULL, seq bigint NOT NULL, event_type text NOT NULL, actor_id uuid, payload jsonb NOT NULL, happened_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id,id), UNIQUE (tenant_id,instance_id,seq));
CREATE TABLE outbox (tenant_id uuid NOT NULL, id uuid NOT NULL, aggregate_id uuid NOT NULL, event_type text NOT NULL, payload jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(), published_at timestamptz, attempts integer NOT NULL DEFAULT 0, PRIMARY KEY (tenant_id,id));
CREATE TABLE inbox_dedup (tenant_id uuid NOT NULL, consumer text NOT NULL, message_id text NOT NULL, processed_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id,consumer,message_id));
CREATE TABLE idempotency_keys (tenant_id uuid NOT NULL, actor_id uuid NOT NULL, operation text NOT NULL, key text NOT NULL, request_hash text NOT NULL, response jsonb, expires_at timestamptz NOT NULL, PRIMARY KEY (tenant_id,actor_id,operation,key));
```
Production migrations must add CHECK constraints, deletion/retention policy, indexes on `(tenant_id,state,due_at)`, `(tenant_id,instance_id,seq)`, audit partitions, tenant-safe foreign keys for every relation, and full RLS policies. Store secrets in dedicated encrypted secret store, not in IR JSON.

## Other entities
process_maps, process_links, process_owners, sop_documents, sop_versions, sop_reviews, control_mappings, evidence_files, attestation_campaigns, cases, case_activities, form_schemas, form_submissions, connector_configs, integration_attempts, policies, memberships, delegations, audit_log, improvement_initiatives, metric_snapshots.

## IR sample and validation
```json
{"schemaVersion":1,"id":"purchase","version":3,"nodes":[{"id":"start","kind":"start"},{"id":"approval","kind":"human.approval","assignment":{"role":"finance"},"timeout":"P2D"},{"id":"end","kind":"end"}],"edges":[{"from":"start","to":"approval"},{"from":"approval","to":"end","on":"approved"}]}
```
Compiler checks: exactly one allowed start, unique node IDs, all edge targets exist, reachable end or intentional wait, typed variable references, no unauthorized connector, bounded loops, parallel join topology, timer timezone/calendar semantics, all rejection paths handled. Store original canvas JSON separately from executable normalized IR and compiler version.

## Instance state machine
CREATED -> RUNNING -> WAITING -> RUNNING -> COMPLETED; RUNNING/WAITING -> SUSPENDED -> RUNNING; RUNNING/WAITING -> FAILED (retryable or terminal); nonterminal -> CANCELLED. Guard illegal transitions with revision CAS and transaction. Human work: OFFERED -> CLAIMED -> COMPLETED / REJECTED / CANCELLED / EXPIRED; reassignment requires authorization and audit.

## Transaction algorithm (human approval)
1. Authenticate actor; resolve tenant and policy context; authorize task and form fields.
2. Begin DB transaction; select work item and instance/token `FOR UPDATE` in deterministic order.
3. Validate current state, assignee, expected revision, decision and idempotency key.
4. Persist decision, update token/instance, append ordered event, enqueue outbox message.
5. Commit. Return stored response on retry with same key; reject same key with changed request hash.
6. Background executor claims next ready token and proceeds; external side effects only after commit.

## Parallel join semantics
Each fork records join_group + expected branch IDs. A join consumes only once after all required branches reach barrier; branch arrival and join release occur under instance-scoped lock. Define cancellation propagation and inclusive gateway semantics separately; do not implement inclusive joins in MVP without exhaustive tests.

## Timer execution
Persist UTC due_at; scheduler queries due timers using `FOR UPDATE SKIP LOCKED`, marks lease, emits timer-fired transition transactionally. On crash, lease expires and timer is reprocessed idempotently. Business-day SLA deadlines use tenant calendar version pinned at timer creation; store source timezone and rule.

## API sketch
`POST /v1/processes`, `GET /v1/processes`, `POST /v1/processes/{id}/versions`, `POST /v1/processes/{id}/publish`, `POST /v1/instances`, `GET /v1/instances/{id}`, `POST /v1/work/{id}/decisions`, `POST /v1/instances/{id}/cancel`, `GET /v1/instances/{id}/events`, `GET /v1/catalog`, `POST /v1/sops/{id}/review`, `POST /v1/cases`, `GET /v1/metrics/processes/{id}`. Use cursor pagination, ETags/revisions, tenant-scoped auth and `Idempotency-Key` on mutating commands.

## Domain events
`process.definition.published.v1`, `process.instance.started.v1`, `work.assigned.v1`, `work.decision.recorded.v1`, `process.instance.completed.v1`, `sop.review.due.v1`, `case.closed.v1`. Envelope includes event_id, tenant_id, aggregate_id, aggregate_revision, occurred_at, trace_id, schema_version, actor_ref; avoid sensitive form contents in broadly consumed events.

## Rust module layout
```text
crates/domain/{catalog,governance,workflow,case,policy}/
crates/application/{commands,queries,ports}/
crates/infra/{postgres,outbox,kafka,object_store,auth}/
crates/runtime/{ir,validator,interpreter,scheduler,adapter}/
apps/api/  apps/worker/  apps/scheduler/  apps/connector-runner/
web/{workspace,modeler,forms,admin}/
```
Use trait ports `ProcessRepository`, `RuntimeAdapter`, `PolicyEvaluator`, `EventPublisher`, `SecretProvider`, `Clock`. Deterministic runtime tests inject clock and IDs. Validate every connector response against schema and limits.
