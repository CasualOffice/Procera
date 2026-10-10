-- 0009 role grants to groups (BPMS-024, docs/12 §5, docs/04 role_grants.subject_kind).
-- A grant's subject is exactly one of a membership or a static group.

ALTER TABLE role_grants ALTER COLUMN membership_id DROP NOT NULL;
ALTER TABLE role_grants ADD COLUMN group_id uuid;
ALTER TABLE role_grants ADD CONSTRAINT role_grants_group_fk FOREIGN KEY (workspace_id, group_id) REFERENCES groups (workspace_id, id) ON DELETE CASCADE;
ALTER TABLE role_grants ADD CONSTRAINT role_grants_one_subject CHECK ((membership_id IS NULL) <> (group_id IS NULL));
CREATE INDEX ON role_grants (workspace_id, group_id) WHERE group_id IS NOT NULL;
