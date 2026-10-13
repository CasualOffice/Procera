-- 0048 (BPMS-084, ADR-0012): release bundles. Publishing a process artifact version compiles its IR
-- and writes an immutable, hashed manifest pinning the exact published version of everything it
-- references (forms, rules, calendars, SOP). Instances started from a published process name their
-- bundle, so a later change to a form or calendar never reaches a request already running.
-- Built-in request types are compiled at boot and keep `bundle_id` NULL (pinned by checksum).

CREATE TABLE release_bundles (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  process_version_id uuid NOT NULL,
  process_key text NOT NULL,                       -- the artifact key, as the engine names programs
  version_no int NOT NULL,
  manifest jsonb NOT NULL,                         -- {"forms": {...}, "rules": {...}, "calendars": {...}, "sop": ...}
  manifest_hash bytea NOT NULL,
  program_checksum text NOT NULL,                  -- the compiled program's canonical checksum
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, process_version_id),
  UNIQUE (workspace_id, process_key, program_checksum),
  FOREIGN KEY (workspace_id, artifact_id) REFERENCES artifacts (workspace_id, id),
  FOREIGN KEY (workspace_id, process_version_id) REFERENCES artifact_versions (workspace_id, id)
);
SELECT procera_enable_workspace_rls('release_bundles');
-- Bundles are immutable: no updates, ever (deletes only with the workspace).
CREATE FUNCTION procera_release_bundle_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'release bundles are immutable';
END $$;
CREATE TRIGGER release_bundles_immutable BEFORE UPDATE ON release_bundles
  FOR EACH ROW EXECUTE FUNCTION procera_release_bundle_immutable();

ALTER TABLE process_instances ADD COLUMN bundle_id uuid;
ALTER TABLE process_instances ADD CONSTRAINT process_instances_bundle_fk
  FOREIGN KEY (workspace_id, bundle_id) REFERENCES release_bundles (workspace_id, id);
