-- 0038 (BPMS-046, BPMS-023): when a member leaves the organisation (deactivated), their open
-- assignments in active campaigns close as `left`, never as acknowledged. A `left` assignment is
-- frozen evidence like a completed one and drops out of the progress denominators.

ALTER TABLE campaign_recipients ADD COLUMN left_at timestamptz;
ALTER TABLE campaign_recipients DROP CONSTRAINT campaign_recipients_state_check;
ALTER TABLE campaign_recipients ADD CONSTRAINT campaign_recipients_state_check
  CHECK (state IN ('pending', 'completed', 'left'));
ALTER TABLE campaign_recipients ADD CONSTRAINT campaign_recipients_left_check
  CHECK (state <> 'left' OR left_at IS NOT NULL);

CREATE OR REPLACE FUNCTION procera_campaign_recipient_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'campaign assignments are evidence and cannot be deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state = 'completed' THEN
    RAISE EXCEPTION 'acknowledgement % is recorded and cannot change', OLD.id USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state = 'left' THEN
    RAISE EXCEPTION 'assignment % closed when the member left and cannot change', OLD.id USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;
