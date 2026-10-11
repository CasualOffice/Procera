-- BPMS-028 (FR-V01–V09, docs/05 §4): saved DataGrid views per list. Personal by default; a shared
-- view is visible to everyone in the workspace who can open the list. One default per member per list.
CREATE TABLE saved_views (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  owner_membership_id uuid NOT NULL,
  list text NOT NULL CHECK (list ~ '^[a-z][a-z0-9_.-]{1,62}$'),
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  state jsonb NOT NULL CHECK (jsonb_typeof(state) = 'object'),   -- {q, sort, filters, columns, density}
  shared boolean NOT NULL DEFAULT false,
  is_default boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, owner_membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX saved_views_list_ix ON saved_views (workspace_id, list);
CREATE UNIQUE INDEX saved_views_one_default ON saved_views (workspace_id, owner_membership_id, list) WHERE is_default;
SELECT procera_enable_workspace_rls('saved_views');
