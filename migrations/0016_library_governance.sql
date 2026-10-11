-- BPMS-044 (FR-G05/G07, FX-G01/G02): review cadence and recertification, overdue flags, page
-- verification with expiry, and change requests against governed artifacts.

ALTER TABLE artifacts
  ADD COLUMN review_cadence_days int CHECK (review_cadence_days BETWEEN 1 AND 3650), -- NULL = workspace default
  ADD COLUMN next_review_at timestamptz,          -- set on publish and on recertification
  ADD COLUMN last_reviewed_at timestamptz,
  ADD COLUMN review_overdue_at timestamptz,       -- set by the worker when next_review_at passes; cleared on recertify/publish
  ADD COLUMN verified_at timestamptz,
  ADD COLUMN verified_by uuid,
  ADD COLUMN verification_expires_at timestamptz,
  ADD CONSTRAINT artifacts_verification_ck CHECK ((verified_at IS NULL) = (verification_expires_at IS NULL));

CREATE INDEX artifacts_review_due_ix ON artifacts (next_review_at) WHERE review_overdue_at IS NULL AND next_review_at IS NOT NULL;

CREATE TABLE change_requests (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  body text NOT NULL DEFAULT '' CHECK (length(body) <= 10000),
  state text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'accepted', 'declined', 'implemented')),
  requested_by uuid NOT NULL,                     -- membership id
  requested_at timestamptz NOT NULL,
  resolved_by uuid,
  resolved_at timestamptz,
  resolution text,
  version_id uuid,                                -- the version that implements it (when accepted/implemented)
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, artifact_id) REFERENCES artifacts (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, version_id) REFERENCES artifact_versions (workspace_id, id),
  CHECK ((state = 'open') = (resolved_at IS NULL))
);
CREATE INDEX change_requests_artifact_ix ON change_requests (workspace_id, artifact_id, state);
SELECT procera_enable_workspace_rls('change_requests');

-- Workspaces with artifacts whose review date has passed and are not flagged yet (worker).
CREATE FUNCTION discovery.library_review_due_workspaces(as_of timestamptz, max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT a.workspace_id FROM artifacts a
  WHERE a.review_overdue_at IS NULL AND a.next_review_at <= as_of AND a.current_published_version_id IS NOT NULL
  LIMIT max_rows
$$;
GRANT SELECT (workspace_id, review_overdue_at, next_review_at, current_published_version_id) ON artifacts TO procera_discovery;
ALTER FUNCTION discovery.library_review_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.library_review_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.library_review_due_workspaces(timestamptz, int) TO procera_app;
