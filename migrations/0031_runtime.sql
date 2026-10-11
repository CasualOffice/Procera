-- BPMS-083: runtime persistence (docs/04 §2.5, §2.12, §4.2-§4.4). Instances store the pure
-- interpreter's state; every accepted stimulus is appended to instance_events (replay log).
-- Effects are recorded once per command key; jobs are leased with fencing; timers fire once.
-- Release bundles (BPMS-084) will add the artifact/version/bundle references (expand step).

CREATE TABLE process_instances (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  display_id text NOT NULL,                       -- the instance id until display ids exist (A-26 (4))
  process_key text NOT NULL,
  process_version int NOT NULL,
  program_checksum text,                          -- placeholder for the release bundle reference (BPMS-084)
  status text NOT NULL CHECK (status IN ('created', 'running', 'suspended', 'completed', 'cancelled', 'failed')),
  waiting boolean NOT NULL DEFAULT false,         -- derived: every live token waits (docs/04 §4.1)
  is_test boolean NOT NULL DEFAULT false,
  revision bigint NOT NULL,                       -- = InstanceState.revision = seq of the last event
  state jsonb NOT NULL,                           -- procera_runtime::InstanceState
  started_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  ended_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, display_id)
);
CREATE INDEX process_instances_status ON process_instances (workspace_id, status, started_at DESC);
SELECT procera_enable_workspace_rls('process_instances');

-- Append-only stimulus log: replaying payloads in seq order reproduces `state` exactly.
CREATE TABLE instance_events (
  workspace_id uuid NOT NULL,
  instance_id uuid NOT NULL,
  seq bigint NOT NULL,
  event_type text NOT NULL,
  payload jsonb NOT NULL,                         -- procera_runtime::Stimulus
  happened_at timestamptz NOT NULL,               -- the stimulus time
  recorded_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, instance_id, seq),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id)
);
SELECT procera_enable_workspace_rls('instance_events');

CREATE FUNCTION procera_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION '% is append-only (% blocked)', TG_TABLE_NAME, TG_OP USING ERRCODE = 'insufficient_privilege';
END $$;
CREATE TRIGGER instance_events_append_only BEFORE UPDATE OR DELETE ON instance_events
  FOR EACH ROW EXECUTE FUNCTION procera_append_only();

-- Every interpreter command, recorded once per (key, command). Commands the engine applies itself
-- are 'applied'; commands owned by later items (work items, connectors, signals, subprocesses,
-- cases, incidents, notifications, SLA breach actions) wait as 'pending' (A-28).
CREATE TABLE instance_effects (
  workspace_id uuid NOT NULL,
  effect_key text NOT NULL,
  command text NOT NULL,
  instance_id uuid NOT NULL,
  payload jsonb NOT NULL,
  state text NOT NULL CHECK (state IN ('applied', 'pending', 'done')),
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, effect_key, command),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id)
);
CREATE INDEX instance_effects_pending ON instance_effects (workspace_id, command, created_at) WHERE state = 'pending';
SELECT procera_enable_workspace_rls('instance_effects');

CREATE TABLE jobs (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  kind text NOT NULL,                             -- 'advance' (more kinds arrive with their items)
  instance_id uuid,
  payload jsonb NOT NULL,
  run_at timestamptz NOT NULL,
  attempts int NOT NULL DEFAULT 0,
  max_attempts int NOT NULL DEFAULT 10,
  lease_owner text,
  lease_until timestamptz,
  fence bigint NOT NULL DEFAULT 0,
  last_error jsonb,
  state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'leased', 'done', 'failed', 'dead')),
  idempotency_key text NOT NULL,
  created_at timestamptz NOT NULL,
  finished_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, kind, idempotency_key),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id)
);
CREATE INDEX jobs_ready ON jobs (run_at) WHERE state = 'queued';
CREATE INDEX jobs_leased ON jobs (lease_until) WHERE state = 'leased';
SELECT procera_enable_workspace_rls('jobs');

-- Immutable calendar versions (FR-W05). `timezone` is a fixed UTC offset for now (A-27).
CREATE TABLE calendar_versions (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  calendar_ref text NOT NULL,                     -- the IR reference, e.g. calendar:in-blr@2026.2
  calendar_artifact_id uuid,                      -- library calendars (later item)
  version_label text NOT NULL,
  timezone text NOT NULL,
  working_hours jsonb NOT NULL,                   -- {"mon": [["09:00", "17:00"]], ...}
  holidays date[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, calendar_ref)
);
SELECT procera_enable_workspace_rls('calendar_versions');
CREATE TRIGGER calendar_versions_immutable BEFORE UPDATE ON calendar_versions
  FOR EACH ROW EXECUTE FUNCTION procera_append_only();

CREATE TABLE timers (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  timer_key text NOT NULL,                        -- the ScheduleTimer command key
  instance_id uuid,
  token_id bigint,
  node_id text,
  purpose text NOT NULL CHECK (purpose IN ('wait', 'boundary', 'signal_timeout', 'sla', 'escalation', 'publish', 'reminder', 'retention')),
  due_at timestamptz NOT NULL,
  calendar_version_id uuid,                       -- pinned at creation so due_at can be recomputed in an audit
  source_tz text,
  rule text,                                      -- how due_at was computed, e.g. "business P2D from 2026-10-16T10:00:00Z"
  state text NOT NULL DEFAULT 'scheduled' CHECK (state IN ('scheduled', 'fired', 'cancelled')),
  fired_at timestamptz,                           -- lateness = fired_at - due_at (SLO, docs/06)
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, timer_key),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id),
  FOREIGN KEY (workspace_id, calendar_version_id) REFERENCES calendar_versions (workspace_id, id)
);
CREATE INDEX timers_due ON timers (due_at) WHERE state = 'scheduled';
SELECT procera_enable_workspace_rls('timers');

CREATE TABLE sla_clocks (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  clock_key text NOT NULL,                        -- <instance>/sla/<sla id>/<revision at start>
  instance_id uuid NOT NULL,
  sla_key text NOT NULL,
  node_id text,
  target interval NOT NULL,
  target_text text NOT NULL,                      -- the IR duration
  business boolean NOT NULL,
  calendar_version_id uuid,
  breach jsonb NOT NULL DEFAULT '[]',             -- the SLA's breach actions
  started_at timestamptz NOT NULL,
  paused_total interval NOT NULL DEFAULT '0',
  paused_since timestamptz,
  due_at timestamptz NOT NULL,
  stopped_at timestamptz,
  breached_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, clock_key),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id),
  FOREIGN KEY (workspace_id, calendar_version_id) REFERENCES calendar_versions (workspace_id, id)
);
CREATE INDEX sla_clocks_open ON sla_clocks (workspace_id, instance_id) WHERE stopped_at IS NULL;
SELECT procera_enable_workspace_rls('sla_clocks');

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'procera_app') THEN
    REVOKE UPDATE, DELETE, TRUNCATE ON instance_events FROM procera_app;
    REVOKE UPDATE, TRUNCATE ON calendar_versions FROM procera_app;
  END IF;
END $$;

-- Workspaces with a due timer, a due job or an expired lease at `due_by` (ids only).
CREATE FUNCTION discovery.runtime_due_workspaces(due_by timestamptz, max_rows int DEFAULT 1000) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT w FROM (
    SELECT t.workspace_id AS w FROM timers t WHERE t.state = 'scheduled' AND t.due_at <= due_by
    UNION
    SELECT j.workspace_id FROM jobs j
     WHERE (j.state = 'queued' AND j.run_at <= due_by) OR (j.state = 'leased' AND j.lease_until < due_by)
  ) due LIMIT max_rows
$$;
GRANT SELECT (workspace_id, state, due_at) ON timers TO procera_discovery;
GRANT SELECT (workspace_id, state, run_at, lease_until) ON jobs TO procera_discovery;
ALTER FUNCTION discovery.runtime_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.runtime_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.runtime_due_workspaces(timestamptz, int) TO procera_app;
