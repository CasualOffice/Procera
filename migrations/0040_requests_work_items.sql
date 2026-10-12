-- 0040 (BPMS-085, BPMS-103): requests and work items (docs/04 §2.6).
-- `requests` is the listing view of a process instance started from a request type: who asked,
-- its display id (`LR-2026-000123`, a per-workspace sequence per prefix and year), a title and the
-- request status derived after every stimulus. Work items are the human steps the runtime waits
-- for; candidates are resolved once, at activation, and materialised for the inbox.

CREATE TABLE request_sequences (
  workspace_id uuid NOT NULL,
  prefix text NOT NULL,
  year int NOT NULL,
  last bigint NOT NULL,
  PRIMARY KEY (workspace_id, prefix, year)
);
SELECT procera_enable_workspace_rls('request_sequences');

CREATE TABLE requests (
  workspace_id uuid NOT NULL,
  instance_id uuid NOT NULL,
  display_id text NOT NULL,
  process_key text NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
  initiator_membership_id uuid NOT NULL,
  status text NOT NULL CHECK (status IN ('in_progress', 'changes_requested', 'approved', 'rejected', 'cancelled')),
  current_step text,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  ended_at timestamptz,
  PRIMARY KEY (workspace_id, instance_id),
  UNIQUE (workspace_id, display_id),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id),
  FOREIGN KEY (workspace_id, initiator_membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE INDEX requests_mine ON requests (workspace_id, initiator_membership_id, created_at DESC);
CREATE INDEX requests_all ON requests (workspace_id, status, created_at DESC);
SELECT procera_enable_workspace_rls('requests');

CREATE TABLE work_items (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  work_key text NOT NULL,                          -- the runtime activation key (<instance>/<node>/<n>)
  instance_id uuid NOT NULL,
  token_id bigint NOT NULL,
  node_id text NOT NULL,
  name text NOT NULL,                              -- the step name shown to people
  kind text NOT NULL CHECK (kind IN ('task', 'approval', 'sign', 'acknowledge')),
  state text NOT NULL CHECK (state IN ('offered', 'claimed', 'completed', 'cancelled')),
  outcomes text[] NOT NULL,
  form text,
  resolution jsonb NOT NULL,                       -- snapshot: candidate sources, exclusions, fallback
  unassigned boolean NOT NULL DEFAULT false,       -- nobody resolved: visible to admins, never lost
  exclude_initiator boolean NOT NULL DEFAULT false,
  exclude_actors_of text[] NOT NULL DEFAULT '{}',
  outcome text,
  decision jsonb,                                  -- {comment, fields}
  due_at timestamptz,
  revision bigint NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL,
  completed_at timestamptz,
  completed_by uuid,                               -- membership
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, work_key),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id),
  FOREIGN KEY (workspace_id, completed_by) REFERENCES memberships (workspace_id, id),
  CHECK (state <> 'completed' OR (outcome IS NOT NULL AND completed_at IS NOT NULL))
);
CREATE INDEX work_items_instance ON work_items (workspace_id, instance_id, created_at);
CREATE INDEX work_items_open ON work_items (workspace_id, state, due_at) WHERE state IN ('offered', 'claimed');
SELECT procera_enable_workspace_rls('work_items');

CREATE TABLE work_item_candidates (
  workspace_id uuid NOT NULL,
  work_item_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  PRIMARY KEY (workspace_id, work_item_id, membership_id),
  FOREIGN KEY (workspace_id, work_item_id) REFERENCES work_items (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE INDEX work_item_candidates_inbox ON work_item_candidates (workspace_id, membership_id);
SELECT procera_enable_workspace_rls('work_item_candidates');
