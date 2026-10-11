-- BPMS-066 (PRV-04 policy part, docs/14 §4): retention classes honoured by jobs.
-- Drafts are measured from their last activity; operational delivery logs from creation.

ALTER TABLE artifact_versions ADD COLUMN last_activity_at timestamptz;
UPDATE artifact_versions SET last_activity_at = created_at;
-- New rows start active at creation; edits set it from the application clock.
CREATE FUNCTION procera_version_activity_default() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  NEW.last_activity_at := coalesce(NEW.last_activity_at, NEW.created_at);
  RETURN NEW;
END $$;
CREATE TRIGGER artifact_versions_activity_default BEFORE INSERT ON artifact_versions
  FOR EACH ROW EXECUTE FUNCTION procera_version_activity_default();
CREATE INDEX artifact_versions_draft_activity_ix ON artifact_versions (workspace_id, last_activity_at) WHERE lifecycle = 'draft';

CREATE TABLE retention_runs (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  ran_at timestamptz NOT NULL,
  purged jsonb NOT NULL,                          -- {"draft": n, "operational": {"emails": n, ...}, "notices": n}
  PRIMARY KEY (workspace_id, id)
);
SELECT procera_enable_workspace_rls('retention_runs');

-- Workspaces whose data the retention job may process (ids only): every live workspace.
GRANT SELECT (id, state) ON workspaces TO procera_discovery;
CREATE FUNCTION discovery.retention_workspaces(max_rows int DEFAULT 1000) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT w.id FROM workspaces w WHERE w.state IN ('active', 'suspended_ro', 'suspended_locked') ORDER BY w.id LIMIT max_rows
$$;
ALTER FUNCTION discovery.retention_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.retention_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.retention_workspaces(int) TO procera_app;
