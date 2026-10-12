-- 0039 root key rotation (BPMS-030, docs/06 §4). `procera rotate-master-key` re-seals every value
-- sealed directly under the root key, workspace by workspace in one transaction. This discovery
-- function lists the workspaces that hold such values (ids only; the discovery role already reads
-- the workspace_id columns of these tables).
CREATE FUNCTION discovery.root_sealed_workspaces() RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT k.workspace_id FROM workspace_keys k
  UNION SELECT i.workspace_id FROM invitations i
  UNION SELECT m.workspace_id FROM email_messages m
$$;
ALTER FUNCTION discovery.root_sealed_workspaces() OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.root_sealed_workspaces() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.root_sealed_workspaces() TO procera_app;
