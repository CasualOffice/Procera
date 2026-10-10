-- 0008 directory (BPMS-023, FR-W02, docs/04 §2.3): department tree, static groups, positions,
-- manager links and per-field source attribution. Subtrees use a uuid[] ancestor path (root first,
-- self last) instead of ltree, so no extension is required.

CREATE TABLE departments (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  parent_id uuid,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
  path uuid[] NOT NULL,
  head_membership_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, parent_id) REFERENCES departments (workspace_id, id),
  FOREIGN KEY (workspace_id, head_membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE UNIQUE INDEX departments_sibling_name ON departments (workspace_id, coalesce(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));
CREATE INDEX departments_path ON departments USING gin (path);

CREATE TABLE groups (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
  description text,
  kind text NOT NULL DEFAULT 'static' CHECK (kind IN ('static', 'rule', 'scim')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id)
);
CREATE UNIQUE INDEX groups_name ON groups (workspace_id, lower(name));

CREATE TABLE group_members (
  workspace_id uuid NOT NULL,
  group_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  added_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, group_id, membership_id),
  FOREIGN KEY (workspace_id, group_id) REFERENCES groups (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);

CREATE TABLE positions (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
  department_id uuid,
  holder_membership_id uuid,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, department_id) REFERENCES departments (workspace_id, id),
  FOREIGN KEY (workspace_id, holder_membership_id) REFERENCES memberships (workspace_id, id)
);

-- Which source last set each directory field (manual, csv; scim/hris later): {"department": "csv", ...}.
ALTER TABLE memberships ADD COLUMN field_sources jsonb NOT NULL DEFAULT '{}';
ALTER TABLE memberships ADD CONSTRAINT memberships_department_fk FOREIGN KEY (workspace_id, department_id) REFERENCES departments (workspace_id, id);
ALTER TABLE memberships ADD CONSTRAINT memberships_manager_fk FOREIGN KEY (workspace_id, manager_membership_id) REFERENCES memberships (workspace_id, id);
ALTER TABLE memberships ADD CONSTRAINT memberships_manager_not_self CHECK (manager_membership_id IS DISTINCT FROM id);

SELECT procera_enable_workspace_rls('departments');
SELECT procera_enable_workspace_rls('groups');
SELECT procera_enable_workspace_rls('group_members');
SELECT procera_enable_workspace_rls('positions');
