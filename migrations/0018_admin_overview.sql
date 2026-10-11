-- BPMS-063 (FR-A01/A06): failed sign-ins of the current workspace's members, for the admin overview
-- and the people & access monitor. Sign-ins are audited on the system chain (no workspace yet), which
-- workspace sessions cannot read; this function returns per-member counts only, for the workspace
-- the transaction is scoped to (app.workspace_id), never event details. The range is inclusive of `until` (usually "now").
GRANT SELECT (workspace_id, occurred_at, actor_subject_ref, category, action) ON audit_events TO procera_discovery;
GRANT SELECT (id, subject_ref) ON users TO procera_discovery;

CREATE FUNCTION discovery.member_login_failures(since timestamptz, until timestamptz) RETURNS TABLE (subject_ref uuid, failures bigint)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT e.actor_subject_ref, count(*)
  FROM audit_events e
  JOIN users u ON u.subject_ref = e.actor_subject_ref
  JOIN memberships m ON m.user_id = u.id AND m.workspace_id = nullif(current_setting('app.workspace_id', true), '')::uuid
  WHERE e.workspace_id = '00000000-0000-0000-0000-000000000000'
    AND e.category = 'auth' AND e.action = 'login.failed'
    AND e.occurred_at >= since AND e.occurred_at <= until
  GROUP BY e.actor_subject_ref
$$;
ALTER FUNCTION discovery.member_login_failures(timestamptz, timestamptz) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.member_login_failures(timestamptz, timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.member_login_failures(timestamptz, timestamptz) TO procera_app;
