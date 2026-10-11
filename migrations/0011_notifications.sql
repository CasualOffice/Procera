-- 0011 notifications (BPMS-029 consumer, BPMS-048, FR-N01/N02, docs/04 §2.12).
-- In-app inbox per member, and an outbound email queue whose bodies are sealed (they can carry
-- invitation links). Both are workspace-isolated; the system workspace holds platform mail.

CREATE TABLE notifications (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  membership_id uuid NOT NULL,
  kind text NOT NULL,
  title text NOT NULL,
  body text NOT NULL DEFAULT '',
  link text,
  ref_type text,
  ref_id uuid,
  dedupe_key text NOT NULL,
  read_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, dedupe_key),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX notifications_inbox ON notifications (workspace_id, membership_id, created_at DESC);

CREATE TABLE email_messages (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  kind text NOT NULL,
  to_email citext NOT NULL,
  subject text NOT NULL,
  body_sealed bytea NOT NULL,                  -- {"text": …, "html": …} sealed with context email_messages:<id>
  dedupe_key text NOT NULL,
  state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'sent', 'failed')),
  attempts int NOT NULL DEFAULT 0,
  next_attempt_at timestamptz,
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  sent_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, dedupe_key)
);
CREATE INDEX email_messages_due ON email_messages (created_at) WHERE state = 'queued';

SELECT procera_enable_workspace_rls('notifications');
SELECT procera_enable_workspace_rls('email_messages');

CREATE FUNCTION discovery.email_pending_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT e.workspace_id FROM email_messages e
  WHERE e.state = 'queued' AND (e.next_attempt_at IS NULL OR e.next_attempt_at <= now())
  LIMIT max_rows
$$;
GRANT SELECT (workspace_id, state, next_attempt_at) ON email_messages TO procera_discovery;
ALTER FUNCTION discovery.email_pending_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.email_pending_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.email_pending_workspaces(int) TO procera_app;
