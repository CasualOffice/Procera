-- BPMS-062 (FR-F06): when a file was last downloaded or previewed, for the Files view.
ALTER TABLE files ADD COLUMN last_accessed_at timestamptz;

-- BPMS-064: a background audit export holds its result file, so the orphan purge leaves it alone.
ALTER TABLE file_links DROP CONSTRAINT file_links_object_type_check;
ALTER TABLE file_links ADD CONSTRAINT file_links_object_type_check CHECK (object_type IN ('artifact_version', 'audit_export'));
