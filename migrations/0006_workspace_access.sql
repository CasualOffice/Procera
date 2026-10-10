-- 0006 workspace plane: memberships, invitations, roles, grants (BPMS-023/024, docs/12 §2, §5-6, docs/04 §2.3).
-- All tables are workspace-isolated (workspace_id + forced RLS).

CREATE TABLE memberships (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  user_id uuid NOT NULL REFERENCES users(id),
  status text NOT NULL CHECK (status IN ('active','deactivated')),
  title text,
  department_id uuid,
  manager_membership_id uuid,
  location text,
  joined_at timestamptz NOT NULL DEFAULT now(),
  deactivated_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, user_id)
);

CREATE TABLE invitations (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  email citext NOT NULL,
  role_keys text[] NOT NULL,                    -- built-in or custom role keys granted on acceptance (workspace scope)
  token_hash bytea NOT NULL UNIQUE,
  token_sealed bytea NOT NULL,                  -- for the mailer (BPMS-048); sealed with context invitations:<id>
  invited_by uuid REFERENCES users(id),         -- NULL = system plane (owner invite)
  expires_at timestamptz NOT NULL,
  accepted_at timestamptz,
  accepted_by uuid REFERENCES users(id),
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id)
);
CREATE INDEX ON invitations (workspace_id, email) WHERE accepted_at IS NULL AND revoked_at IS NULL;

CREATE TABLE roles (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  key text NOT NULL CHECK (key ~ '^[a-z][a-z0-9_.-]{1,63}$'),
  name text NOT NULL,
  description text,
  builtin_template text,                        -- set for built-in roles and their clones (lineage)
  is_builtin boolean NOT NULL DEFAULT false,    -- built-ins are versioned by the product; edit by cloning
  permissions text[] NOT NULL,
  revision bigint NOT NULL DEFAULT 1,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, key)
);

CREATE TABLE role_grants (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  role_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  scope_kind text NOT NULL CHECK (scope_kind IN ('workspace','department','collection','process','case')),
  scope_id uuid,                                -- NULL for workspace scope
  expires_at timestamptz,
  granted_by uuid,                              -- membership id (NULL = system/bootstrap)
  reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, role_id) REFERENCES roles (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE,
  CHECK ((scope_kind = 'workspace') = (scope_id IS NULL))
);
CREATE INDEX ON role_grants (workspace_id, membership_id);

-- Bumped on any role/grant/membership change; compiled Cedar policy sets are cached per (workspace, revision).
CREATE TABLE access_revisions (
  workspace_id uuid PRIMARY KEY,
  revision bigint NOT NULL DEFAULT 1
);

SELECT procera_enable_workspace_rls('memberships');
SELECT procera_enable_workspace_rls('invitations');
SELECT procera_enable_workspace_rls('roles');
SELECT procera_enable_workspace_rls('role_grants');
SELECT procera_enable_workspace_rls('access_revisions');

-- Workspace discovery for a signed-in user (workspace switcher): ids + names of workspaces where the
-- user has an active membership. SECURITY DEFINER under the discovery role; returns nothing else.
GRANT SELECT (workspace_id, user_id, status) ON memberships TO procera_discovery;
GRANT SELECT (id, slug, name, state) ON workspaces TO procera_discovery;
CREATE FUNCTION discovery.user_workspaces(p_user uuid) RETURNS TABLE (workspace_id uuid, slug text, name text, state text)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT w.id, w.slug::text, w.name, w.state FROM memberships m JOIN workspaces w ON w.id = m.workspace_id
  WHERE m.user_id = p_user AND m.status = 'active' AND w.state NOT IN ('pending_deletion','deleted')
  ORDER BY w.name
$$;
ALTER FUNCTION discovery.user_workspaces(uuid) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.user_workspaces(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.user_workspaces(uuid) TO procera_app;

-- Invitation lookup by token hash across workspaces (the accepting user doesn't know the workspace yet).
GRANT SELECT (workspace_id, id, token_hash, accepted_at, revoked_at, expires_at) ON invitations TO procera_discovery;
CREATE FUNCTION discovery.invitation_workspace(p_token_hash bytea) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT workspace_id FROM invitations WHERE token_hash = p_token_hash LIMIT 1
$$;
ALTER FUNCTION discovery.invitation_workspace(bytea) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.invitation_workspace(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.invitation_workspace(bytea) TO procera_app;
