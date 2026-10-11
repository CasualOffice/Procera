-- 0034 saved views shared with a role (BPMS-028, FR-V04 "shared (to roles)", docs/05 §4, A-35).
-- A view is personal, shared with everyone (`shared`), or shared with the members who hold one role
-- (`shared_role`, a role key of the same workspace), never both. Holding a role means an unexpired
-- grant of it at any scope, directly or through a group. Deleting the role makes its views personal.
ALTER TABLE saved_views ADD COLUMN shared_role text;
ALTER TABLE saved_views ADD CONSTRAINT saved_views_shared_role_fkey
  FOREIGN KEY (workspace_id, shared_role) REFERENCES roles (workspace_id, key) ON DELETE SET NULL (shared_role);
ALTER TABLE saved_views ADD CONSTRAINT saved_views_one_audience CHECK (NOT (shared AND shared_role IS NOT NULL));
CREATE INDEX saved_views_role_ix ON saved_views (workspace_id, list, shared_role) WHERE shared_role IS NOT NULL;
-- Forced RLS stays on (idempotent: re-asserts FORCE and the ws_isolation policy).
SELECT procera_enable_workspace_rls('saved_views');
