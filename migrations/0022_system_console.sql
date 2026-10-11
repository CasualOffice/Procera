-- 0022 system console (BPMS-065; FR-S02, S04, S08, S09; docs/05 §3.20, docs/20 §3).
-- The console shows workspace metadata, counts and platform health only. It never reads workspace
-- content (docs/12 §1): cross-workspace numbers come from SECURITY DEFINER discovery functions that
-- return counts and timestamps, never rows.

-- 1. Platform quota defaults, one bundle per configuration profile (docs/20 §3 "Quotas & plans").
-- A new workspace copies its profile's bundle into workspace_quotas; later changes to a default
-- never rewrite existing workspaces. NULL means unlimited.
CREATE TABLE platform_quota_defaults (
  profile text PRIMARY KEY CHECK (profile IN ('starter','standard','enterprise','regulated')),
  max_members int CHECK (max_members > 0),
  max_active_instances int CHECK (max_active_instances > 0),
  max_storage_bytes bigint CHECK (max_storage_bytes > 0),
  api_rpm int CHECK (api_rpm > 0),
  ai_tokens_month bigint CHECK (ai_tokens_month >= 0),
  revision bigint NOT NULL DEFAULT 1,
  updated_by uuid REFERENCES users(id),
  updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO platform_quota_defaults (profile, max_members, max_active_instances, max_storage_bytes, api_rpm, ai_tokens_month) VALUES
  ('starter',    100,  5000,   10737418240,  600,   0),
  ('standard',   2000, 100000, 214748364800, 3000,  0),
  ('enterprise', NULL, NULL,   NULL,         10000, 0),
  ('regulated',  NULL, NULL,   NULL,         10000, 0);

-- 2. Usage per workspace: active members, open invitations and the time of the last event on the
-- workspace's audit chain. `p_workspace` NULL = every workspace that is not deleted.
GRANT SELECT (workspace_id, occurred_at) ON audit_events TO procera_discovery;
CREATE FUNCTION discovery.workspace_usage(as_of timestamptz, p_workspace uuid DEFAULT NULL)
RETURNS TABLE (workspace_id uuid, members bigint, invited bigint, last_activity_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT w.id,
    (SELECT count(*) FROM memberships m WHERE m.workspace_id = w.id AND m.status = 'active'),
    (SELECT count(*) FROM invitations i WHERE i.workspace_id = w.id AND i.accepted_at IS NULL
       AND i.revoked_at IS NULL AND i.expires_at > as_of),
    (SELECT max(e.occurred_at) FROM audit_events e WHERE e.workspace_id = w.id)
  FROM workspaces w
  WHERE w.state <> 'deleted' AND (p_workspace IS NULL OR w.id = p_workspace)
$$;
ALTER FUNCTION discovery.workspace_usage(timestamptz, uuid) OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.workspace_usage(timestamptz, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.workspace_usage(timestamptz, uuid) TO procera_app;

-- 3. Platform queues for the health page (FR-S08): totals and the oldest pending timestamp only.
GRANT SELECT (created_at, attempts) ON outbox TO procera_discovery;
GRANT SELECT (created_at) ON email_messages TO procera_discovery;
CREATE FUNCTION discovery.platform_queues()
RETURNS TABLE (outbox_pending bigint, outbox_retrying bigint, outbox_oldest_at timestamptz,
               mail_queued bigint, mail_failed bigint, mail_oldest_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT
    (SELECT count(*) FROM outbox o WHERE o.published_at IS NULL),
    (SELECT count(*) FROM outbox o WHERE o.published_at IS NULL AND o.attempts > 0),
    (SELECT min(o.created_at) FROM outbox o WHERE o.published_at IS NULL),
    (SELECT count(*) FROM email_messages e WHERE e.state = 'queued'),
    (SELECT count(*) FROM email_messages e WHERE e.state = 'failed'),
    (SELECT min(e.created_at) FROM email_messages e WHERE e.state = 'queued')
$$;
ALTER FUNCTION discovery.platform_queues() OWNER TO procera_discovery;
REVOKE ALL ON FUNCTION discovery.platform_queues() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION discovery.platform_queues() TO procera_app;
