-- 0033 announcement acknowledgements run on the campaign engine (BPMS-046/047, FR-AN04, FR-AK01
-- "one engine", docs/04 §2.7). An announcement that requires acknowledgement gets a campaign with
-- `subject_kind = 'announcement'` when it goes live: its recipients are the assignments (a fixed
-- snapshot), and the campaign's reminder ladder and one-time manager escalation apply. Existing
-- campaigns keep `artifact_version` subjects and stay valid.

-- Announcements: an optional acknowledgement deadline, chosen while composing (A-32).
ALTER TABLE announcements ADD COLUMN ack_by timestamptz;
ALTER TABLE announcements ADD CONSTRAINT announcements_ack_by_check
  CHECK (ack_by IS NULL OR (require_ack AND ack_by <= expires_at));

-- Campaign subjects: exactly one of a library version or an announcement.
ALTER TABLE campaigns ALTER COLUMN subject_version_id DROP NOT NULL;
ALTER TABLE campaigns ADD COLUMN subject_announcement_id uuid;
ALTER TABLE campaigns ADD CONSTRAINT campaigns_subject_announcement_fkey
  FOREIGN KEY (workspace_id, subject_announcement_id) REFERENCES announcements (workspace_id, id);
ALTER TABLE campaigns DROP CONSTRAINT campaigns_subject_kind_check;
ALTER TABLE campaigns ADD CONSTRAINT campaigns_subject_kind_check
  CHECK (subject_kind IN ('artifact_version', 'announcement'));
ALTER TABLE campaigns ADD CONSTRAINT campaigns_subject_check CHECK (
  (subject_kind = 'artifact_version' AND subject_version_id IS NOT NULL AND subject_announcement_id IS NULL)
  OR (subject_kind = 'announcement' AND subject_announcement_id IS NOT NULL AND subject_version_id IS NULL
      AND audience_snapshot = 'at_launch'));  -- recipients are the announcement's snapshot: no joiners
ALTER TABLE campaigns DROP CONSTRAINT campaigns_origin_check;
ALTER TABLE campaigns ADD CONSTRAINT campaigns_origin_check
  CHECK (origin IN ('manual', 'reack', 'announcement'));
-- One campaign per announcement, which makes the publish path and the event consumer idempotent.
CREATE UNIQUE INDEX campaigns_one_per_announcement ON campaigns (workspace_id, subject_announcement_id)
  WHERE subject_announcement_id IS NOT NULL;

-- Assignments: a completed one cites the exact subject it acknowledged.
ALTER TABLE campaign_recipients ADD COLUMN ack_announcement_id uuid;
ALTER TABLE campaign_recipients ADD CONSTRAINT campaign_recipients_ack_announcement_fkey
  FOREIGN KEY (workspace_id, ack_announcement_id) REFERENCES announcements (workspace_id, id);
ALTER TABLE campaign_recipients DROP CONSTRAINT campaign_recipients_check;
ALTER TABLE campaign_recipients ADD CONSTRAINT campaign_recipients_check CHECK (
  state <> 'completed' OR (completed_at IS NOT NULL AND ack_content_hash IS NOT NULL
    AND (ack_version_id IS NULL) <> (ack_announcement_id IS NULL)));

-- Launched campaigns also keep their announcement subject.
CREATE OR REPLACE FUNCTION procera_campaign_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'campaigns are evidence and cannot be deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state IN ('closed', 'cancelled') THEN
    RAISE EXCEPTION 'campaign % has ended and is frozen', OLD.id USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state <> 'draft' AND (NEW.subject_version_id IS DISTINCT FROM OLD.subject_version_id
      OR NEW.subject_announcement_id IS DISTINCT FROM OLD.subject_announcement_id
      OR NEW.subject_kind IS DISTINCT FROM OLD.subject_kind
      OR NEW.subject_hash IS DISTINCT FROM OLD.subject_hash OR NEW.audience_rule IS DISTINCT FROM OLD.audience_rule
      OR NEW.ack_text IS DISTINCT FROM OLD.ack_text OR NEW.audience_snapshot IS DISTINCT FROM OLD.audience_snapshot) THEN
    RAISE EXCEPTION 'campaign % is launched; its subject, audience and statement are fixed', OLD.id USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;

-- The deadline is frozen with the rest of what recipients acknowledge once out of draft.
CREATE OR REPLACE FUNCTION procera_forbid_mutating_announcement() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'announcements are archived, not deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state <> 'draft' AND (NEW.title IS DISTINCT FROM OLD.title OR NEW.content IS DISTINCT FROM OLD.content
      OR NEW.audience IS DISTINCT FROM OLD.audience OR NEW.require_ack IS DISTINCT FROM OLD.require_ack
      OR NEW.ack_by IS DISTINCT FROM OLD.ack_by
      OR (OLD.content_hash IS NOT NULL AND NEW.content_hash IS DISTINCT FROM OLD.content_hash)) THEN
    RAISE EXCEPTION 'announcement % is not a draft and cannot change', OLD.id USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state IN ('published', 'archived') AND NEW.state IN ('draft', 'pending_approval', 'scheduled') THEN
    RAISE EXCEPTION 'announcement % cannot return to %', OLD.id, NEW.state USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;
