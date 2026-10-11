-- BPMS-048 (FR-N04, docs/20 §2.9): per-member delivery preferences (digests, quiet hours), digest items
-- awaiting a digest email, and addresses suppressed after a permanent delivery failure (bounce).

CREATE TABLE notification_preferences (
  workspace_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  -- NULL = use the workspace setting (notify.digest.schedule / .hour, notify.quiet_hours.*).
  digest text CHECK (digest IN ('off', 'daily', 'weekly')),
  digest_hour int CHECK (digest_hour BETWEEN 0 AND 23),
  quiet_enabled boolean,
  quiet_start int CHECK (quiet_start BETWEEN 0 AND 23),
  quiet_end int CHECK (quiet_end BETWEEN 0 AND 23),
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, membership_id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE
);
SELECT procera_enable_workspace_rls('notification_preferences');

CREATE TABLE digest_items (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  membership_id uuid NOT NULL,
  kind text NOT NULL,
  title text NOT NULL,
  link text,
  dedupe_key text NOT NULL,
  created_at timestamptz NOT NULL,
  sent_at timestamptz,                              -- when it went out in a digest email
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, dedupe_key),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX digest_items_pending_ix ON digest_items (workspace_id, membership_id) WHERE sent_at IS NULL;
SELECT procera_enable_workspace_rls('digest_items');

-- Emails held until a time (quiet hours) keep state 'queued' with next_attempt_at in the future.

-- Global, like auth_throttle: an address that bounced permanently is not mailed again by any workspace.
CREATE TABLE email_suppressions (
  email citext PRIMARY KEY,
  reason text NOT NULL,
  created_at timestamptz NOT NULL
);

-- Workspaces with digest items not sent yet (ids only) for the worker.
CREATE FUNCTION discovery.digest_pending_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT d.workspace_id FROM digest_items d WHERE d.sent_at IS NULL LIMIT max_rows
$$;
GRANT SELECT (workspace_id, sent_at) ON digest_items TO procera_discovery;
ALTER FUNCTION discovery.digest_pending_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.digest_pending_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.digest_pending_workspaces(int) TO procera_app;
