-- 0003 transactional outbox, consumer inbox, idempotency keys (BPMS-029, docs/04 §2.11, ADR-0005).

CREATE TABLE outbox (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  topic text NOT NULL,
  aggregate_type text NOT NULL,
  aggregate_id uuid NOT NULL,
  aggregate_revision bigint NOT NULL,
  schema_version int NOT NULL DEFAULT 1,
  payload jsonb NOT NULL,
  trace_id text,
  created_at timestamptz NOT NULL DEFAULT now(),
  published_at timestamptz,
  attempts int NOT NULL DEFAULT 0,
  PRIMARY KEY (workspace_id, id)
);
CREATE INDEX outbox_unpublished ON outbox (created_at) WHERE published_at IS NULL;

CREATE TABLE inbox_dedup (
  workspace_id uuid NOT NULL,
  consumer text NOT NULL,
  message_id uuid NOT NULL,
  processed_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, consumer, message_id)
);

CREATE TABLE idempotency_keys (
  workspace_id uuid NOT NULL,
  actor_id uuid NOT NULL,
  operation text NOT NULL,
  key text NOT NULL CHECK (length(key) BETWEEN 8 AND 200),
  request_hash bytea NOT NULL,
  status int,
  response jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, actor_id, operation, key)
);
CREATE INDEX ON idempotency_keys (expires_at);

SELECT procera_enable_workspace_rls('outbox');
SELECT procera_enable_workspace_rls('inbox_dedup');
SELECT procera_enable_workspace_rls('idempotency_keys');

-- Cross-workspace discovery without bypassing RLS: background dispatchers learn *which* workspaces
-- have pending work (ids only, never payloads), then process each one under its own RLS context.
CREATE FUNCTION discovery.outbox_pending_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT o.workspace_id FROM outbox o WHERE o.published_at IS NULL LIMIT max_rows
$$;

-- Ownership + least privilege for discovery (FORCE RLS binds owners, so the definer must bypass it).
GRANT SELECT (workspace_id, published_at) ON outbox TO procera_discovery;
ALTER FUNCTION discovery.outbox_pending_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.outbox_pending_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.outbox_pending_workspaces(int) TO procera_app;
