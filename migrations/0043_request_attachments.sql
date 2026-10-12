-- BPMS-060 / BPMS-103: files attached to a request (form file fields and files decided with) are
-- linked to it, so retention treats them as part of the execution record and only people who may
-- see the request may download or preview them.
ALTER TABLE file_links DROP CONSTRAINT file_links_object_type_check;
ALTER TABLE file_links ADD CONSTRAINT file_links_object_type_check CHECK (object_type IN ('artifact_version', 'audit_export', 'request'));
