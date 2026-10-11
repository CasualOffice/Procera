-- 0012 library (BPMS-040, FR-G04/G05, docs/04 §2.4): governed artifacts, immutable versions, reviews.

CREATE TABLE artifacts (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('process', 'sop', 'policy', 'form', 'rule', 'calendar', 'document_template')),
  key text NOT NULL CHECK (key ~ '^[a-z0-9][a-z0-9._-]{1,63}$'),
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  owner_membership_id uuid,
  criticality text NOT NULL DEFAULT 'standard' CHECK (criticality IN ('standard', 'controlled')),
  current_published_version_id uuid,
  state text NOT NULL DEFAULT 'draft',          -- lifecycle of the newest version (denormalised for lists)
  created_by uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, kind, key),
  FOREIGN KEY (workspace_id, owner_membership_id) REFERENCES memberships (workspace_id, id)
);

CREATE TABLE artifact_versions (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  version_no int NOT NULL,
  lifecycle text NOT NULL CHECK (lifecycle IN ('draft', 'in_review', 'approved', 'scheduled', 'published', 'deprecated', 'retired')),
  content jsonb NOT NULL,
  content_hash bytea,                            -- SHA-256(JCS(content)), frozen at approval
  change_reason text,
  effective_at timestamptz,
  created_by uuid NOT NULL,                      -- membership id of the author
  created_at timestamptz NOT NULL DEFAULT now(),
  submitted_at timestamptz,
  review_round int NOT NULL DEFAULT 0,          -- incremented on each submission; reviews belong to one round
  approved_at timestamptz,
  published_at timestamptz,
  published_by uuid,
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, artifact_id, version_no),
  FOREIGN KEY (workspace_id, artifact_id) REFERENCES artifacts (workspace_id, id)
);
-- At most one open (draft or in review) version per artifact, and one published.
CREATE UNIQUE INDEX artifact_versions_one_open ON artifact_versions (workspace_id, artifact_id) WHERE lifecycle IN ('draft', 'in_review');
CREATE UNIQUE INDEX artifact_versions_one_published ON artifact_versions (workspace_id, artifact_id) WHERE lifecycle = 'published';

-- Immutability (FR-G05, T07): approved and later versions never change content or hash, and never go back.
CREATE FUNCTION procera_forbid_mutating_approved() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.lifecycle IN ('approved', 'scheduled', 'published', 'deprecated', 'retired') THEN
    IF NEW.content IS DISTINCT FROM OLD.content OR NEW.content_hash IS DISTINCT FROM OLD.content_hash THEN
      RAISE EXCEPTION 'artifact version % is immutable', OLD.id USING ERRCODE = 'P0001';
    END IF;
    IF NEW.lifecycle IN ('draft', 'in_review') THEN
      RAISE EXCEPTION 'artifact version % cannot return to %', OLD.id, NEW.lifecycle USING ERRCODE = 'P0001';
    END IF;
  END IF;
  IF TG_OP = 'DELETE' AND OLD.lifecycle <> 'draft' THEN
    RAISE EXCEPTION 'only drafts can be deleted' USING ERRCODE = 'P0001';
  END IF;
  RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END $$;
CREATE TRIGGER artifact_versions_immutable BEFORE UPDATE OR DELETE ON artifact_versions
  FOR EACH ROW EXECUTE FUNCTION procera_forbid_mutating_approved();

CREATE TABLE reviews (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  artifact_version_id uuid NOT NULL,
  reviewer_membership_id uuid NOT NULL,
  review_round int NOT NULL,
  decision text NOT NULL CHECK (decision IN ('approve', 'reject', 'changes_requested')),
  comment text,
  decided_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, artifact_version_id) REFERENCES artifact_versions (workspace_id, id)
);
CREATE INDEX ON reviews (workspace_id, artifact_version_id);

SELECT procera_enable_workspace_rls('artifacts');
SELECT procera_enable_workspace_rls('artifact_versions');
SELECT procera_enable_workspace_rls('reviews');

-- Scheduled publications that are due, across workspaces (ids only) for the worker.
-- `as_of` comes from the application clock so tests can drive time.
CREATE FUNCTION discovery.library_due_workspaces(as_of timestamptz, max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT v.workspace_id FROM artifact_versions v WHERE v.lifecycle = 'scheduled' AND v.effective_at <= as_of LIMIT max_rows
$$;
GRANT SELECT (workspace_id, lifecycle, effective_at) ON artifact_versions TO procera_discovery;
ALTER FUNCTION discovery.library_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.library_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.library_due_workspaces(timestamptz, int) TO procera_app;
