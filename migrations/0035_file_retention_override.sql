-- E2E-R1-12 (FR-F06/F09): an admin can set a file's retention class explicitly (bulk, with a reason);
-- otherwise it follows what holds the file (docs/14 §4, A-37).
ALTER TABLE files ADD COLUMN retention_class_override text
  CHECK (retention_class_override IS NULL OR retention_class_override IN
    ('governance_record', 'execution_record', 'case_file', 'operational', 'audit', 'draft'));
