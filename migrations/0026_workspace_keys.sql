-- BPMS-030 (docs/04 §8, docs/06 §4): versioned workspace KEKs, wrapped by the root key. The DEKs of
-- envelope-encrypted values carry the KEK version they are wrapped with; rotation rewraps them.
CREATE TABLE workspace_keys (
  workspace_id uuid NOT NULL,
  version int NOT NULL CHECK (version >= 1),
  wrapped bytea,                                  -- root-sealed KEK; NULL once destroyed (crypto-shred)
  state text NOT NULL CHECK (state IN ('active', 'retired', 'destroyed')),
  created_at timestamptz NOT NULL,
  retired_at timestamptz,
  PRIMARY KEY (workspace_id, version),
  CHECK ((state = 'destroyed') = (wrapped IS NULL))
);
CREATE UNIQUE INDEX workspace_keys_one_active ON workspace_keys (workspace_id) WHERE state = 'active';
SELECT procera_enable_workspace_rls('workspace_keys');

-- Workspaces with values still wrapped by a retired KEK (for the rewrap job).
CREATE FUNCTION discovery.rewrap_pending_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT k.workspace_id FROM workspace_keys k WHERE k.state = 'retired' LIMIT max_rows
$$;
GRANT SELECT (workspace_id, state) ON workspace_keys TO procera_discovery;
ALTER FUNCTION discovery.rewrap_pending_workspaces(int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.rewrap_pending_workspaces(int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.rewrap_pending_workspaces(int) TO procera_app;
