-- BPMS-041 (FR-G01, docs/04 §2.4, docs/12 §7): library collections (a tree), per-collection
-- visibility, artifact owners and RACI assignments.

CREATE TABLE collections (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  parent_id uuid,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
  path uuid[] NOT NULL,                           -- ancestors root first, self last (like departments)
  visibility jsonb CHECK (visibility IS NULL OR jsonb_typeof(visibility) = 'object'), -- Audience; NULL = everyone
  created_at timestamptz NOT NULL,
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, parent_id) REFERENCES collections (workspace_id, id)
);
CREATE INDEX collections_path_ix ON collections USING gin (path);
SELECT procera_enable_workspace_rls('collections');

ALTER TABLE artifacts ADD COLUMN collection_id uuid;
ALTER TABLE artifacts ADD CONSTRAINT artifacts_collection_fk FOREIGN KEY (workspace_id, collection_id) REFERENCES collections (workspace_id, id);
CREATE INDEX artifacts_collection_ix ON artifacts (workspace_id, collection_id);

CREATE TABLE artifact_raci (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  role text NOT NULL CHECK (role IN ('responsible', 'accountable', 'consulted', 'informed')),
  membership_id uuid,
  group_id uuid,
  PRIMARY KEY (workspace_id, id),
  CHECK ((membership_id IS NULL) <> (group_id IS NULL)),
  FOREIGN KEY (workspace_id, artifact_id) REFERENCES artifacts (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, group_id) REFERENCES groups (workspace_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX artifact_raci_unique_ix ON artifact_raci (workspace_id, artifact_id, role, membership_id, group_id) NULLS NOT DISTINCT;
SELECT procera_enable_workspace_rls('artifact_raci');

-- Whether a membership may see a collection: every collection on its path that restricts visibility
-- must include the member. Runs as the caller (RLS applies).
CREATE FUNCTION procera_collection_visible(collection uuid, membership uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
  SELECT NOT EXISTS (
    SELECT 1 FROM collections c, collections x
    WHERE c.id = collection AND x.id = ANY(c.path) AND x.visibility IS NOT NULL
      AND NOT procera_audience_contains(x.visibility, membership)
  )
$$;
