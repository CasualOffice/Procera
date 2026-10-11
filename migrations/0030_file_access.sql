-- BPMS-062 (FR-F06): when a file was last downloaded or previewed, for the Files view.
ALTER TABLE files ADD COLUMN last_accessed_at timestamptz;
