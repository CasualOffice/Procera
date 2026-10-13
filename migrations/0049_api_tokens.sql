-- 0049 (BPMS-144, docs/06 §3.5, docs/12 §4): personal API tokens. A token acts as the member who
-- created it, narrowed to the permissions chosen for it; it is bound to one workspace, expires
-- (at most a year), can be revoked, and never decides, acknowledges or signs. Only an HMAC of the
-- secret under a server-held key is stored; `lookup` is the public part of the token
-- (`prc_live_<lookup>_<secret>`), unique across workspaces so a request can find its workspace.

CREATE TABLE api_tokens (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  lookup text NOT NULL,
  membership_id uuid NOT NULL,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
  secret_mac bytea NOT NULL,
  last4 text NOT NULL,
  permissions text[] NOT NULL CHECK (cardinality(permissions) >= 1),
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
  last_used_at timestamptz,
  last_used_ip text,
  revoked_at timestamptz,
  revoked_by uuid,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, revoked_by) REFERENCES memberships (workspace_id, id)
);
CREATE UNIQUE INDEX api_tokens_lookup ON api_tokens (lookup);
CREATE INDEX api_tokens_member ON api_tokens (workspace_id, membership_id);
SELECT procera_enable_workspace_rls('api_tokens');

-- Token lookup across workspaces (the request names no workspace until its token is found).
GRANT SELECT (workspace_id, lookup) ON api_tokens TO procera_discovery;
CREATE FUNCTION discovery.api_token_workspace(p_lookup text) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT workspace_id FROM api_tokens WHERE lookup = p_lookup LIMIT 1
$$;
ALTER FUNCTION discovery.api_token_workspace(text) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.api_token_workspace(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.api_token_workspace(text) TO procera_app;
