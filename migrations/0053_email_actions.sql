-- 0053 (BPMS-145, FR-N03): email-actionable approvals. A plain approval step (nothing to fill in)
-- emails each candidate a single-use link that lands on a confirm screen; only the link's SHA-256 is
-- stored. It works once, until the step closes, and for at most seven days.

CREATE TABLE email_action_links (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  work_item_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  token_hash bytea NOT NULL,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, work_item_id) REFERENCES work_items (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE UNIQUE INDEX email_action_links_token ON email_action_links (token_hash);
SELECT procera_enable_workspace_rls('email_action_links');

-- The link names no workspace: find it by the token's hash.
GRANT SELECT (workspace_id, token_hash) ON email_action_links TO procera_discovery;
CREATE FUNCTION discovery.email_action_workspace(p_token_hash bytea) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT workspace_id FROM email_action_links WHERE token_hash = p_token_hash LIMIT 1
$$;
ALTER FUNCTION discovery.email_action_workspace(bytea) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.email_action_workspace(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.email_action_workspace(bytea) TO procera_app;
