-- 0001 foundation (BPMS-025): RLS helper and guard view. Runs as procera_migrator.
-- Rule (docs/24 §3.2): every table with a workspace_id column gets FORCE ROW LEVEL SECURITY and the
-- ws_isolation policy via procera_enable_workspace_rls(). The NULLIF form is required: pooled
-- connections keep app.workspace_id as '' after a tx-local set_config (SP-09 finding A-1).

CREATE FUNCTION procera_current_workspace() RETURNS uuid
  LANGUAGE sql STABLE AS $$ SELECT NULLIF(current_setting('app.workspace_id', true), '')::uuid $$;

CREATE FUNCTION procera_enable_workspace_rls(t regclass) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
  EXECUTE format('ALTER TABLE %s ENABLE ROW LEVEL SECURITY', t);
  EXECUTE format('ALTER TABLE %s FORCE ROW LEVEL SECURITY', t);
  EXECUTE format('DROP POLICY IF EXISTS ws_isolation ON %s', t);
  EXECUTE format('CREATE POLICY ws_isolation ON %s USING (workspace_id = procera_current_workspace()) WITH CHECK (workspace_id = procera_current_workspace())', t);
END $$;

-- Guard: lists workspace-scoped tables that are NOT correctly protected. Must always be empty.
-- Checked by the test suite and at startup (docs/03 §7 layer 3).
CREATE VIEW procera_rls_violations AS
SELECT c.oid::regclass::text AS table_name,
       c.relrowsecurity AS rls_enabled,
       c.relforcerowsecurity AS rls_forced,
       EXISTS (SELECT 1 FROM pg_policy p WHERE p.polrelid = c.oid AND p.polname = 'ws_isolation') AS has_policy
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
WHERE c.relkind IN ('r', 'p')
  AND EXISTS (SELECT 1 FROM pg_attribute a WHERE a.attrelid = c.oid AND a.attname = 'workspace_id' AND NOT a.attisdropped)
  AND NOT (c.relrowsecurity AND c.relforcerowsecurity
           AND EXISTS (SELECT 1 FROM pg_policy p WHERE p.polrelid = c.oid AND p.polname = 'ws_isolation'));
