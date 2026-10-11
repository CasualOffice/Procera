-- 0013 campaigns (BPMS-046, FR-AK01–04, docs/04 §2.7): acknowledgement campaigns over published
-- library versions. One assignment row per member; completed assignments are frozen evidence.

CREATE TABLE campaigns (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  -- Announcements, documents and attestations join as further subject kinds with their items.
  subject_kind text NOT NULL DEFAULT 'artifact_version' CHECK (subject_kind IN ('artifact_version')),
  subject_version_id uuid NOT NULL,
  subject_hash bytea NOT NULL,                   -- the version's content hash: binds evidence to exact content
  action text NOT NULL DEFAULT 'acknowledge' CHECK (action IN ('acknowledge')),
  audience_rule jsonb NOT NULL,                  -- procera_members::audience::Audience
  audience_snapshot text NOT NULL CHECK (audience_snapshot IN ('at_launch', 'dynamic')),
  ack_text text NOT NULL CHECK (length(ack_text) BETWEEN 1 AND 500),
  due_at timestamptz,                            -- explicit due date; set at launch when absent
  due_in_secs bigint NOT NULL CHECK (due_in_secs > 0),         -- campaign.default_due at creation
  reminder_schedule jsonb NOT NULL DEFAULT '[]', -- seconds after assignment, ascending (campaign.reminders.cadence)
  escalation_after_secs bigint CHECK (escalation_after_secs > 0), -- NULL = off (campaign.escalation.to_manager_after)
  state text NOT NULL DEFAULT 'draft' CHECK (state IN ('draft', 'active', 'closed', 'cancelled')),
  created_by uuid NOT NULL,
  created_at timestamptz NOT NULL,
  launched_at timestamptz,
  launched_by uuid,
  ended_at timestamptz,
  ended_by uuid,
  end_reason text,
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, subject_version_id) REFERENCES artifact_versions (workspace_id, id),
  FOREIGN KEY (workspace_id, created_by) REFERENCES memberships (workspace_id, id),
  CHECK (state = 'draft' OR (launched_at IS NOT NULL AND due_at IS NOT NULL) OR state = 'cancelled')
);
CREATE INDEX ON campaigns (workspace_id, state);

CREATE TABLE campaign_recipients (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  campaign_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  source text NOT NULL CHECK (source IN ('launch', 'joiner')),
  state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'completed')),
  assigned_at timestamptz NOT NULL,
  due_at timestamptz NOT NULL,
  completed_at timestamptz,
  ack_version_id uuid,
  ack_content_hash bytea,
  ack_text text,
  ack_ip inet,
  ack_user_agent text,
  evidence jsonb,                                -- how the member was authenticated (amr, auth_time)
  reminders_sent int NOT NULL DEFAULT 0,         -- steps of the reminder ladder already sent
  last_reminded_at timestamptz,
  escalated_at timestamptz,
  escalated_to uuid,
  PRIMARY KEY (workspace_id, id),
  UNIQUE (workspace_id, campaign_id, membership_id),
  FOREIGN KEY (workspace_id, campaign_id) REFERENCES campaigns (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, ack_version_id) REFERENCES artifact_versions (workspace_id, id),
  CHECK (state <> 'completed' OR (completed_at IS NOT NULL AND ack_version_id IS NOT NULL AND ack_content_hash IS NOT NULL))
);
CREATE INDEX ON campaign_recipients (workspace_id, membership_id, state);
CREATE INDEX ON campaign_recipients (workspace_id, campaign_id, state);

SELECT procera_enable_workspace_rls('campaigns');
SELECT procera_enable_workspace_rls('campaign_recipients');

-- Evidence integrity: a launched campaign keeps its subject, audience and statement; an ended one
-- never changes again. Assignments are never deleted, and a completed one never changes.
CREATE FUNCTION procera_campaign_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'campaigns are evidence and cannot be deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state IN ('closed', 'cancelled') THEN
    RAISE EXCEPTION 'campaign % has ended and is frozen', OLD.id USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state <> 'draft' AND (NEW.subject_version_id IS DISTINCT FROM OLD.subject_version_id
      OR NEW.subject_hash IS DISTINCT FROM OLD.subject_hash OR NEW.audience_rule IS DISTINCT FROM OLD.audience_rule
      OR NEW.ack_text IS DISTINCT FROM OLD.ack_text OR NEW.audience_snapshot IS DISTINCT FROM OLD.audience_snapshot) THEN
    RAISE EXCEPTION 'campaign % is launched; its subject, audience and statement are fixed', OLD.id USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER campaigns_frozen BEFORE UPDATE OR DELETE ON campaigns
  FOR EACH ROW EXECUTE FUNCTION procera_campaign_frozen();

CREATE FUNCTION procera_campaign_recipient_frozen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'campaign assignments are evidence and cannot be deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state = 'completed' THEN
    RAISE EXCEPTION 'acknowledgement % is recorded and cannot change', OLD.id USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER campaign_recipients_frozen BEFORE UPDATE OR DELETE ON campaign_recipients
  FOR EACH ROW EXECUTE FUNCTION procera_campaign_recipient_frozen();

-- Workspaces with a reminder step or an escalation due (ids only) for the scheduler.
-- `as_of` comes from the application clock so tests can drive time.
CREATE FUNCTION discovery.campaign_due_workspaces(as_of timestamptz, max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT r.workspace_id
  FROM campaign_recipients r
  JOIN campaigns c ON c.workspace_id = r.workspace_id AND c.id = r.campaign_id
  JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id AND m.status = 'active'
  WHERE c.state = 'active' AND r.state = 'pending'
    AND ((r.reminders_sent < jsonb_array_length(c.reminder_schedule)
          AND r.assigned_at + make_interval(secs => (c.reminder_schedule ->> r.reminders_sent)::double precision) <= as_of)
      OR (c.escalation_after_secs IS NOT NULL AND r.escalated_at IS NULL
          AND r.assigned_at + make_interval(secs => c.escalation_after_secs::double precision) <= as_of))
  LIMIT max_rows
$$;
GRANT SELECT (workspace_id, id, state, reminder_schedule, escalation_after_secs) ON campaigns TO procera_discovery;
GRANT SELECT (workspace_id, campaign_id, membership_id, state, assigned_at, reminders_sent, escalated_at) ON campaign_recipients TO procera_discovery;
GRANT SELECT (id) ON memberships TO procera_discovery;
ALTER FUNCTION discovery.campaign_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.campaign_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.campaign_due_workspaces(timestamptz, int) TO procera_app;
