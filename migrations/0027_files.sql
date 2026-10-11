-- BPMS-060/062 (FR-F01–F03, F05–F07): files with resumable upload, hash, sniffed type, malware scan
-- state and quarantine; links to the objects that use them; per-file DEK wrapped by the workspace KEK.
CREATE TABLE files (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK (length(name) BETWEEN 1 AND 255),
  declared_type text,
  sniffed_type text,
  size bigint NOT NULL CHECK (size >= 0),
  received bigint NOT NULL DEFAULT 0 CHECK (received >= 0 AND received <= size),
  chunks int NOT NULL DEFAULT 0,
  sha256 bytea,
  state text NOT NULL DEFAULT 'uploading' CHECK (state IN ('uploading', 'scanning', 'clean', 'quarantined', 'rejected', 'deleted')),
  scan_result text,
  wrapped_dek bytea NOT NULL,                     -- envelope of the per-file DEK (workspace KEK)
  storage_key text NOT NULL,                      -- ws/<workspace>/<file>
  uploaded_by uuid NOT NULL,                      -- membership id
  created_at timestamptz NOT NULL,
  completed_at timestamptz,
  deleted_at timestamptz,
  PRIMARY KEY (workspace_id, id)
);
CREATE INDEX files_state_ix ON files (workspace_id, state);
SELECT procera_enable_workspace_rls('files');

CREATE TABLE file_links (
  workspace_id uuid NOT NULL,
  file_id uuid NOT NULL,
  object_type text NOT NULL CHECK (object_type IN ('artifact_version')),
  object_id uuid NOT NULL,
  linked_by uuid NOT NULL,
  linked_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, file_id, object_type, object_id),
  FOREIGN KEY (workspace_id, file_id) REFERENCES files (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX file_links_object_ix ON file_links (workspace_id, object_type, object_id);
SELECT procera_enable_workspace_rls('file_links');
