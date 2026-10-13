-- 0052 (BPMS-143, docs/13 §2.5, WH-01..04): outgoing webhooks. An endpoint's signing secret is sealed
-- under the workspace key (a roll keeps the previous one for 24 hours); deliveries are queued by the
-- outbox bus, one per endpoint and event, and sent by the scheduler with retries.

CREATE TABLE webhook_endpoints (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  url text NOT NULL CHECK (length(url) BETWEEN 8 AND 2000),
  description text CHECK (length(description) <= 200),
  events text[] NOT NULL CHECK (cardinality(events) >= 1),
  enabled boolean NOT NULL DEFAULT true,
  disabled_reason text,
  secret_sealed bytea NOT NULL,
  secret_prev_sealed bytea,
  secret_prev_until timestamptz,
  abandoned_streak int NOT NULL DEFAULT 0,
  created_by uuid NOT NULL,
  created_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, created_by) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('webhook_endpoints');

CREATE TABLE webhook_deliveries (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  endpoint_id uuid NOT NULL,
  event_id uuid NOT NULL,                       -- the outbox event (the webhook-id header)
  event_type text NOT NULL,
  payload jsonb NOT NULL,
  state text NOT NULL CHECK (state IN ('pending', 'delivered', 'failed')),
  attempts int NOT NULL DEFAULT 0,
  next_attempt_at timestamptz,
  last_status int,
  last_error text,
  last_duration_ms int,
  response_excerpt text,
  created_at timestamptz NOT NULL,
  delivered_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, endpoint_id, event_id),
  FOREIGN KEY (workspace_id, endpoint_id) REFERENCES webhook_endpoints (workspace_id, id)
);
SELECT procera_enable_workspace_rls('webhook_deliveries');
CREATE INDEX webhook_deliveries_due ON webhook_deliveries (workspace_id, next_attempt_at) WHERE state = 'pending';
CREATE INDEX webhook_deliveries_by_endpoint ON webhook_deliveries (workspace_id, endpoint_id, created_at DESC);

-- Workspaces with deliveries due (the scheduler's delivery pass).
GRANT SELECT (workspace_id, state, next_attempt_at) ON webhook_deliveries TO procera_discovery;
CREATE FUNCTION discovery.webhook_due_workspaces(as_of timestamptz, max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT workspace_id FROM webhook_deliveries WHERE state = 'pending' AND next_attempt_at <= as_of LIMIT max_rows
$$;
ALTER FUNCTION discovery.webhook_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.webhook_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.webhook_due_workspaces(timestamptz, int) TO procera_app;
