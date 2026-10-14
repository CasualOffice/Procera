-- 0054 (BPMS-140, FR-P02, INT-001, docs/06 §3.3): OpenID Connect single sign-on per workspace.
-- A workspace names its identity provider (issuer, client, sealed secret), the email domains it
-- vouches for, whether SSO is required (owners keep a password as break-glass) and whether people
-- the provider vouches for may join on first sign-in. Sign-ins are authorization code + PKCE with a
-- one-time state and nonce; people are matched by issuer and subject afterwards.

CREATE TABLE sso_configs (
  workspace_id uuid NOT NULL PRIMARY KEY,
  issuer text NOT NULL CHECK (length(issuer) BETWEEN 8 AND 500),
  client_id text NOT NULL CHECK (length(client_id) BETWEEN 1 AND 300),
  client_secret_sealed bytea,
  domains text[] NOT NULL DEFAULT '{}',
  enabled boolean NOT NULL DEFAULT true,
  required boolean NOT NULL DEFAULT false,
  jit boolean NOT NULL DEFAULT false,
  updated_by uuid NOT NULL,
  updated_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  FOREIGN KEY (workspace_id, updated_by) REFERENCES memberships (workspace_id, id)
);
SELECT procera_enable_workspace_rls('sso_configs');

CREATE TABLE sso_states (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  state_hash bytea NOT NULL,
  code_verifier text NOT NULL,
  nonce text NOT NULL,
  return_to text,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, state_hash)
);
SELECT procera_enable_workspace_rls('sso_states');

CREATE TABLE sso_identities (
  workspace_id uuid NOT NULL,
  issuer text NOT NULL,
  subject text NOT NULL,
  user_id uuid NOT NULL REFERENCES users (id),
  email text,
  created_at timestamptz NOT NULL,
  last_login_at timestamptz,
  PRIMARY KEY (workspace_id, issuer, subject)
);
SELECT procera_enable_workspace_rls('sso_identities');

-- Before sign-in the workspace is unknown: which workspaces vouch for an email domain.
GRANT SELECT (workspace_id, domains, enabled) ON sso_configs TO procera_discovery;
CREATE FUNCTION discovery.sso_workspaces_for_domain(p_domain text) RETURNS TABLE (workspace_id uuid, slug text, name text)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT w.id, w.slug::text, w.name FROM sso_configs c JOIN workspaces w ON w.id = c.workspace_id
   WHERE c.enabled AND lower(p_domain) = ANY(c.domains) AND w.state NOT IN ('pending_deletion', 'deleted', 'suspended_locked')
   ORDER BY w.name LIMIT 20
$$;
ALTER FUNCTION discovery.sso_workspaces_for_domain(text) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.sso_workspaces_for_domain(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.sso_workspaces_for_domain(text) TO procera_app;
