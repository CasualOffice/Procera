-- 0010 outbox dispatcher (BPMS-029, docs/04 §2.11): retry with backoff and wake-ups.

ALTER TABLE outbox ADD COLUMN next_attempt_at timestamptz;   -- NULL = due now
ALTER TABLE outbox ADD COLUMN last_error text;
CREATE INDEX outbox_aggregate_unpublished ON outbox (workspace_id, aggregate_id, created_at) WHERE published_at IS NULL;

-- Only workspaces with events that are due (not backing off).
CREATE OR REPLACE FUNCTION discovery.outbox_pending_workspaces(max_rows int DEFAULT 100) RETURNS TABLE (workspace_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public AS $$
  SELECT DISTINCT o.workspace_id FROM outbox o
  WHERE o.published_at IS NULL AND (o.next_attempt_at IS NULL OR o.next_attempt_at <= now())
  LIMIT max_rows
$$;
GRANT SELECT (next_attempt_at) ON outbox TO procera_discovery;

-- Wake dispatchers when a transaction that wrote outbox rows commits (NOTIFY is delivered at commit).
CREATE FUNCTION procera_outbox_notify() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_notify('procera_outbox', NEW.workspace_id::text);
  RETURN NULL;
END $$;
CREATE TRIGGER outbox_notify AFTER INSERT ON outbox FOR EACH ROW EXECUTE FUNCTION procera_outbox_notify();
