-- 0042 (BPMS-083, PR-08): the "why this happened" decision trace.
-- One row per decision node an advance passed through: the branch taken, the conditions checked
-- (expression source text and result) and the inputs they read with their values, except
-- restricted ones, which are never stored. The program build that decided is pinned on the row.
-- Written by the engine in the advance transaction; append-only like the stimulus log.

CREATE TABLE instance_decisions (
  workspace_id uuid NOT NULL,
  instance_id uuid NOT NULL,
  seq bigint NOT NULL,                            -- the instance revision of the advance
  ord int NOT NULL,                               -- order within the advance
  node_id text NOT NULL,
  branch text NOT NULL,                           -- the outcome taken (`default` when no condition held)
  facts jsonb NOT NULL,                           -- {checks: [{branch, expression, result}], inputs: {name: {classification, value?}}, rule?}
  process_version int NOT NULL,
  program_checksum text,
  decided_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, instance_id, seq, ord),
  FOREIGN KEY (workspace_id, instance_id) REFERENCES process_instances (workspace_id, id)
);
SELECT procera_enable_workspace_rls('instance_decisions');
CREATE TRIGGER instance_decisions_append_only BEFORE UPDATE OR DELETE ON instance_decisions
  FOR EACH ROW EXECUTE FUNCTION procera_append_only();

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'procera_app') THEN
    REVOKE UPDATE, DELETE, TRUNCATE ON instance_decisions FROM procera_app;
  END IF;
END $$;
