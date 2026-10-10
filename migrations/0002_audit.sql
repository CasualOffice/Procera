-- 0002 audit chain (BPMS-026, docs/04 §2.10, docs/14 AUD-01/07, SP-10).
-- The system plane uses the nil UUID as its workspace_id (its own chain).

CREATE TABLE audit_events (
  workspace_id uuid NOT NULL,
  seq bigint NOT NULL,
  id uuid NOT NULL,
  occurred_at timestamptz NOT NULL,
  actor_subject_ref uuid,
  actor_kind text NOT NULL CHECK (actor_kind IN ('user','service','system','guest','break_glass')),
  on_behalf_of uuid,
  session_id_hash bytea,
  ip inet,
  user_agent text,
  category text NOT NULL CHECK (category IN ('auth','access','config','content','runtime','file','data_access','export','admin','system')),
  action text NOT NULL,
  object_type text,
  object_id uuid,
  result text NOT NULL CHECK (result IN ('allowed','denied','error')),
  reason text,
  policy_ids text[] NOT NULL DEFAULT '{}',
  diff jsonb,
  trace_id text,
  hash_v smallint NOT NULL DEFAULT 1,
  prev_hash bytea NOT NULL,
  hash bytea NOT NULL,
  PRIMARY KEY (workspace_id, seq, occurred_at)
) PARTITION BY RANGE (occurred_at);

CREATE INDEX ON audit_events (workspace_id, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, actor_subject_ref, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, object_type, object_id, occurred_at DESC);
CREATE INDEX ON audit_events (workspace_id, category, action, occurred_at DESC);

CREATE TABLE audit_heads (workspace_id uuid PRIMARY KEY, last_seq bigint NOT NULL, last_hash bytea NOT NULL);

-- High-volume categories (data_access) are buffered and sealed in batches (SP-10). LOGGED on purpose.
CREATE TABLE audit_pending (
  id bigserial PRIMARY KEY,
  workspace_id uuid NOT NULL,
  event jsonb NOT NULL,
  enqueued_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON audit_pending (workspace_id, id);

SELECT procera_enable_workspace_rls('audit_events');
SELECT procera_enable_workspace_rls('audit_heads');
SELECT procera_enable_workspace_rls('audit_pending');

-- Append-only, enforced twice: no UPDATE/DELETE privilege for the app, and a trigger that blocks
-- everyone else (AUD-07). Retention drops whole partitions via a privileged job, never row deletes.
CREATE FUNCTION procera_audit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'audit_events is append-only (% blocked)', TG_OP USING ERRCODE = 'insufficient_privilege';
END $$;
CREATE TRIGGER audit_events_immutable BEFORE UPDATE OR DELETE ON audit_events
  FOR EACH ROW EXECUTE FUNCTION procera_audit_immutable();

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'procera_app') THEN
    REVOKE UPDATE, DELETE, TRUNCATE ON audit_events FROM procera_app;
  END IF;
END $$;

-- Monthly partitions: created ahead of time by the scheduler (and at startup), each with forced RLS.
CREATE FUNCTION procera_ensure_audit_partitions(months_ahead int DEFAULT 2) RETURNS int
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$
DECLARE m date; name text; created int := 0;
BEGIN
  FOR i IN 0..months_ahead LOOP
    m := (date_trunc('month', now()) + make_interval(months => i))::date;
    name := format('audit_events_%s', to_char(m, 'YYYY_MM'));
    IF to_regclass(name) IS NULL THEN
      EXECUTE format('CREATE TABLE %I PARTITION OF audit_events FOR VALUES FROM (%L) TO (%L)', name, m, (m + interval '1 month')::date);
      PERFORM procera_enable_workspace_rls(name::regclass);
      EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION procera_audit_immutable()', name || '_immutable', name);
      IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'procera_app') THEN
        EXECUTE format('REVOKE ALL ON %I FROM procera_app', name); -- partitions are reached only through the parent
      END IF;
      created := created + 1;
    END IF;
  END LOOP;
  RETURN created;
END $$;
SELECT procera_ensure_audit_partitions(2);
