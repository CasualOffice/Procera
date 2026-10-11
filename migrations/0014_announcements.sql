-- 0014 announcements (BPMS-047, FR-AN01–AN06, docs/04 §2.7, docs/20 §2.17).
-- An announcement is composed as a draft, optionally approved (separation of duties), then published
-- now or at `publish_at`. Publishing snapshots the audience into `announcement_recipients`, which
-- also hold read and acknowledgement receipts. Expiry hides it from feeds; nothing is deleted.

CREATE TABLE announcements (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  content jsonb NOT NULL,                       -- block document, same shape as library content
  content_hash bytea,                           -- SHA-256(JCS(content)), frozen at publication; acks cite it
  audience jsonb NOT NULL,                      -- members::audience::Audience
  priority text NOT NULL DEFAULT 'normal' CHECK (priority IN ('normal', 'critical')),
  pinned boolean NOT NULL DEFAULT false,
  require_ack boolean NOT NULL DEFAULT false,
  publish_at timestamptz,                       -- NULL = publish when approved / published
  expires_at timestamptz NOT NULL,
  state text NOT NULL DEFAULT 'draft' CHECK (state IN ('draft', 'pending_approval', 'scheduled', 'published', 'archived')),
  approval_required boolean NOT NULL DEFAULT false, -- decided when submitted
  channels text[] NOT NULL DEFAULT '{}',        -- delivery channels, resolved when publication is requested
  author_membership_id uuid NOT NULL,
  submitted_at timestamptz,
  decided_by uuid,                              -- approver or rejecter
  decided_at timestamptz,
  decision_comment text,
  published_by uuid,                            -- NULL when the scheduler published it
  published_at timestamptz,
  recipient_count int,
  archived_by uuid,
  archived_at timestamptz,
  archive_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  revision bigint NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, author_membership_id) REFERENCES memberships (workspace_id, id),
  CHECK (publish_at IS NULL OR expires_at > publish_at)
);
CREATE INDEX announcements_state ON announcements (workspace_id, state, publish_at);

-- Only drafts are edited: once submitted, scheduled or published, what recipients see (and acknowledge)
-- never changes, even with direct SQL; announcements are never deleted.
CREATE FUNCTION procera_forbid_mutating_announcement() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'announcements are archived, not deleted' USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state <> 'draft' AND (NEW.title IS DISTINCT FROM OLD.title OR NEW.content IS DISTINCT FROM OLD.content
      OR NEW.audience IS DISTINCT FROM OLD.audience OR NEW.require_ack IS DISTINCT FROM OLD.require_ack
      OR (OLD.content_hash IS NOT NULL AND NEW.content_hash IS DISTINCT FROM OLD.content_hash)) THEN
    RAISE EXCEPTION 'announcement % is not a draft and cannot change', OLD.id USING ERRCODE = 'P0001';
  END IF;
  IF OLD.state IN ('published', 'archived') AND NEW.state IN ('draft', 'pending_approval', 'scheduled') THEN
    RAISE EXCEPTION 'announcement % cannot return to %', OLD.id, NEW.state USING ERRCODE = 'P0001';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER announcements_immutable BEFORE UPDATE OR DELETE ON announcements
  FOR EACH ROW EXECUTE FUNCTION procera_forbid_mutating_announcement();

CREATE TABLE announcement_recipients (
  workspace_id uuid NOT NULL,
  announcement_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  read_at timestamptz,
  acked_at timestamptz,
  ack_ip text,
  ack_user_agent text,
  PRIMARY KEY (workspace_id, announcement_id, membership_id),
  FOREIGN KEY (workspace_id, announcement_id) REFERENCES announcements (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id)
);
CREATE INDEX announcement_recipients_member ON announcement_recipients (workspace_id, membership_id);

SELECT procera_enable_workspace_rls('announcements');
SELECT procera_enable_workspace_rls('announcement_recipients');

-- Scheduled announcements that are due, across workspaces (ids only) for the scheduler.
-- `as_of` comes from the application clock so tests can drive time.
CREATE FUNCTION discovery.announcements_due_workspaces(as_of timestamptz, max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT a.workspace_id FROM announcements a WHERE a.state = 'scheduled' AND a.publish_at <= as_of LIMIT max_rows
$$;
GRANT SELECT (workspace_id, state, publish_at) ON announcements TO procera_discovery;
ALTER FUNCTION discovery.announcements_due_workspaces(timestamptz, int) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.announcements_due_workspaces(timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.announcements_due_workspaces(timestamptz, int) TO procera_app;
