-- 0015 search and audit explorer (BPMS-049, BPMS-064; docs/04 §6, §2.10; FR-G01, FR-T03/T04).
--
-- Search: Postgres full-text search plus pg_trgm for fuzzy, prefix and short queries.
--   * Artifacts keep `search_body` (text of the indexed version's content blocks) and a weighted
--     `tsv`, maintained by triggers: a BEFORE trigger on artifacts recomputes both, and changes to a
--     version's content or lifecycle touch the artifact row so the trigger runs (docs/04 §6).
--     Indexed version: the published one if any, else the newest (what `sop:view`/`policy:view`
--     holders can already read through the library API). Library content carries no classified
--     fields, so nothing restricted is indexed.
--   * Members, departments and groups are small and change through several paths (users are
--     global, memberships per workspace), so they are matched live with expression indexes:
--     nothing to keep in sync, never stale.
-- Audit explorer: per-member saved queries and an index for action-prefix filters.

-- Trusted extension since PG 13, so the migrator (CREATE on the database) can install it;
-- verified against deploy/sql/00-roles.sql roles. Hosts that restrict extensions can pre-create it there.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Every string `text` value anywhere in a content document, in document order, capped so the
-- tsvector stays far below its 1 MB limit whatever the content size.
CREATE FUNCTION procera_content_text(content jsonb) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT left(coalesce(string_agg(t #>> '{}', ' '), ''), 200000)
  FROM jsonb_path_query(content, 'strict $.**.text ? (@.type() == "string")') AS t
$$;

ALTER TABLE artifacts ADD COLUMN search_body text NOT NULL DEFAULT '';
ALTER TABLE artifacts ADD COLUMN tsv tsvector;

-- Title and key with the `simple` config (exact words, stop words kept: "IT onboarding"), title and
-- body with `english` (stemming). Queries OR both configs (procera-search).
CREATE FUNCTION procera_artifact_search_refresh() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  NEW.search_body := coalesce((
    SELECT procera_content_text(v.content) FROM artifact_versions v
    WHERE v.workspace_id = NEW.workspace_id AND v.artifact_id = NEW.id
    ORDER BY (v.lifecycle = 'published') DESC, v.version_no DESC LIMIT 1), '');
  NEW.tsv := setweight(to_tsvector('simple', NEW.title || ' ' || translate(NEW.key, '._-', '   ')), 'A')
          || setweight(to_tsvector('english', NEW.title), 'A')
          || setweight(to_tsvector('english', NEW.search_body), 'B');
  RETURN NEW;
END $$;
CREATE TRIGGER artifacts_search BEFORE INSERT OR UPDATE ON artifacts
  FOR EACH ROW EXECUTE FUNCTION procera_artifact_search_refresh();

-- A no-op update of the artifact re-runs the BEFORE trigger (revision is untouched, so ETags hold).
CREATE FUNCTION procera_artifact_version_search_touch() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  UPDATE artifacts SET search_body = search_body WHERE workspace_id = NEW.workspace_id AND id = NEW.artifact_id;
  RETURN NULL;
END $$;
CREATE TRIGGER artifact_versions_search AFTER INSERT OR UPDATE OF content, lifecycle ON artifact_versions
  FOR EACH ROW EXECUTE FUNCTION procera_artifact_version_search_touch();

-- Backfill. Forced RLS applies to the owner too, so lift FORCE for the duration of this
-- transaction only; the guard view (procera_rls_violations) sees it restored at commit.
ALTER TABLE artifacts NO FORCE ROW LEVEL SECURITY;
ALTER TABLE artifact_versions NO FORCE ROW LEVEL SECURITY;
UPDATE artifacts SET search_body = search_body;
ALTER TABLE artifacts FORCE ROW LEVEL SECURITY;
ALTER TABLE artifact_versions FORCE ROW LEVEL SECURITY;

CREATE INDEX artifacts_tsv ON artifacts USING gin (tsv);
CREATE INDEX artifacts_title_trgm ON artifacts USING gin (title gin_trgm_ops);

-- Live-matched kinds: the expressions here are exactly the ones procera-search queries.
CREATE INDEX users_display_name_trgm ON users USING gin (display_name gin_trgm_ops);
CREATE INDEX users_email_trgm ON users USING gin ((email::text) gin_trgm_ops);
CREATE INDEX memberships_title_trgm ON memberships USING gin (title gin_trgm_ops);
CREATE INDEX departments_name_trgm ON departments USING gin (name gin_trgm_ops);
CREATE INDEX departments_name_tsv ON departments USING gin (to_tsvector('simple', name));
CREATE INDEX groups_name_trgm ON groups USING gin (name gin_trgm_ops);
CREATE INDEX groups_search_tsv ON groups USING gin (to_tsvector('simple', name || ' ' || coalesce(description, '')));

-- Audit explorer: action-prefix filters ("member." …) without a category (FR-T03).
CREATE INDEX audit_events_action_prefix ON audit_events (workspace_id, action text_pattern_ops, occurred_at DESC);

-- Saved explorer queries, private to the member who saved them (FR-T03).
CREATE TABLE audit_saved_queries (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  membership_id uuid NOT NULL,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
  filters jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX audit_saved_queries_name ON audit_saved_queries (workspace_id, membership_id, lower(name));
SELECT procera_enable_workspace_rls('audit_saved_queries');
