-- 0051 (BPMS-107 slice 3): bulk sending, reminders and expiry for e-signing.
-- A bulk draft (`envelopes.bulk`) has one placeholder signer ("each person", recorded as the sender)
-- whose fields are copied onto one envelope per audience member when it is sent; the copies share a
-- batch, and the draft stays as the batch's template (hidden from lists, never sent itself).
-- Reminders go to the signer whose turn it is every `reminder_days` (NULL = off), at most three
-- times; a sent envelope past `expires_at` expires (the scheduler and /__test/drain run both).

CREATE TABLE envelope_batches (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  template_id uuid NOT NULL,
  title text NOT NULL,
  audience jsonb NOT NULL,
  total int NOT NULL CHECK (total >= 1),
  created_by uuid NOT NULL,
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, template_id),
  FOREIGN KEY (workspace_id, template_id) REFERENCES envelopes (workspace_id, id),
  FOREIGN KEY (workspace_id, created_by) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('envelope_batches');

ALTER TABLE envelopes ADD COLUMN bulk boolean NOT NULL DEFAULT false;
ALTER TABLE envelopes ADD COLUMN batch_id uuid;
ALTER TABLE envelopes ADD COLUMN reminder_days int DEFAULT 3 CHECK (reminder_days IS NULL OR reminder_days BETWEEN 1 AND 30);
ALTER TABLE envelopes ADD CONSTRAINT envelopes_batch_fk FOREIGN KEY (workspace_id, batch_id) REFERENCES envelope_batches (workspace_id, id);
CREATE INDEX envelopes_by_batch ON envelopes (workspace_id, batch_id) WHERE batch_id IS NOT NULL;
CREATE INDEX envelopes_sent ON envelopes (workspace_id) WHERE state = 'sent';

ALTER TABLE envelope_recipients ADD COLUMN reminders_sent int NOT NULL DEFAULT 0;
ALTER TABLE envelope_recipients ADD COLUMN reminded_at timestamptz;

-- Workspaces with envelopes out for signature (the reminder and expiry pass looks at them).
GRANT SELECT (workspace_id, state) ON envelopes TO procera_discovery;
CREATE FUNCTION discovery.signing_due_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT workspace_id FROM envelopes WHERE state = 'sent' LIMIT max_rows
$$;
ALTER FUNCTION discovery.signing_due_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.signing_due_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.signing_due_workspaces(int) TO procera_app;
