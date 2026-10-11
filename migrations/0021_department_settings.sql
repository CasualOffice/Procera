-- 0021 department overrides (BPMS-027, docs/20 §1 layer 4, §5): a value per department for keys the
-- catalogue marks department-scopable. Overrides go with their department; their history stays in
-- settings_revisions, which now records the department of a `department` revision.

CREATE TABLE department_settings (
  workspace_id uuid NOT NULL,
  department_id uuid NOT NULL,
  key text NOT NULL,
  value jsonb NOT NULL,                          -- validated against the catalogue before write
  revision bigint NOT NULL,                      -- keeps counting across clear and set (If-Match)
  updated_by uuid NOT NULL REFERENCES users(id),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, department_id, key),
  FOREIGN KEY (workspace_id, department_id) REFERENCES departments (workspace_id, id) ON DELETE CASCADE
);

SELECT procera_enable_workspace_rls('department_settings');

-- Expand only: a nullable column and a wider CHECK; older code keeps working.
ALTER TABLE settings_revisions ADD COLUMN scope_id uuid;
ALTER TABLE settings_revisions DROP CONSTRAINT settings_revisions_layer_check;
ALTER TABLE settings_revisions ADD CONSTRAINT settings_revisions_layer_check
  CHECK (layer IN ('platform', 'platform_floor', 'workspace', 'profile', 'department'));
CREATE INDEX ON settings_revisions (workspace_id, scope_id, key) WHERE scope_id IS NOT NULL;
