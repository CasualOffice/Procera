-- BPMS-064: background audit exports (no 10 000-row cap). The result is stored as a file of the
-- requester (encrypted, signed-link download) and they are notified.
CREATE TABLE audit_export_jobs (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  requested_by uuid NOT NULL,                     -- membership id
  subject_ref uuid NOT NULL,                      -- for the audit record
  own_only boolean NOT NULL,                      -- audit:view_own requesters export their own events only
  filters jsonb NOT NULL,
  format text NOT NULL CHECK (format IN ('csv', 'jsonl')),
  state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'done', 'failed')),
  rows bigint,
  file_id uuid,
  error text,
  created_at timestamptz NOT NULL,
  finished_at timestamptz,
  PRIMARY KEY (workspace_id, id)
);
SELECT procera_enable_workspace_rls('audit_export_jobs');

CREATE FUNCTION discovery.audit_export_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT j.workspace_id FROM audit_export_jobs j WHERE j.state = 'queued' LIMIT max_rows
$$;
GRANT SELECT (workspace_id, state) ON audit_export_jobs TO procera_discovery;
ALTER FUNCTION discovery.audit_export_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.audit_export_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.audit_export_workspaces(int) TO procera_app;
