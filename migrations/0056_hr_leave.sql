-- 0056 (BPMS-223, docs/25 §3): leave types, leave policies with a rule per type, and the leave
-- ledger. Amounts are whole half-days (`halves`), so 1.5 days is 3 and nothing is ever a float. A
-- balance is the sum of a person's ledger for a type and leave year; the ledger is append-only.

CREATE TABLE leave_types (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  key text NOT NULL CHECK (key ~ '^[a-z][a-z0-9_]{1,39}$'),
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
  colour text NOT NULL DEFAULT 'blue' CHECK (colour IN ('gray', 'brown', 'orange', 'yellow', 'green', 'blue', 'purple', 'pink', 'red')),
  paid boolean NOT NULL DEFAULT true,
  limited boolean NOT NULL DEFAULT true,           -- false: no balance (e.g. unpaid leave), only what's taken
  sensitive boolean NOT NULL DEFAULT false,        -- colleagues never see the type (docs/25 §6)
  half_days boolean NOT NULL DEFAULT true,
  needs_attachment_after_days smallint CHECK (needs_attachment_after_days BETWEEN 1 AND 365),
  min_notice_days smallint NOT NULL DEFAULT 0 CHECK (min_notice_days BETWEEN 0 AND 365),
  max_consecutive_days smallint CHECK (max_consecutive_days BETWEEN 1 AND 366),
  negative_limit_halves int NOT NULL DEFAULT 0 CHECK (negative_limit_halves BETWEEN 0 AND 730),
  counts_non_working_days boolean NOT NULL DEFAULT false,
  optional_holidays_only boolean NOT NULL DEFAULT false,
  archived_at timestamptz,
  created_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id)
);
CREATE UNIQUE INDEX leave_types_key ON leave_types (workspace_id, key);
CREATE UNIQUE INDEX leave_types_name ON leave_types (workspace_id, lower(name));
SELECT procera_enable_workspace_rls('leave_types');

CREATE TABLE leave_policies (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
  audience jsonb NOT NULL,
  priority int NOT NULL,                           -- lower wins when several audiences include someone
  year_start_month smallint NOT NULL DEFAULT 1 CHECK (year_start_month BETWEEN 1 AND 12),
  archived_at timestamptz,
  created_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id)
);
CREATE UNIQUE INDEX leave_policies_name ON leave_policies (workspace_id, lower(name));
SELECT procera_enable_workspace_rls('leave_policies');

CREATE TABLE leave_policy_rules (
  workspace_id uuid NOT NULL,
  policy_id uuid NOT NULL,
  leave_type_id uuid NOT NULL,
  yearly_halves int NOT NULL CHECK (yearly_halves BETWEEN 0 AND 730),
  accrual text NOT NULL CHECK (accrual IN ('upfront', 'monthly')),
  prorate boolean NOT NULL DEFAULT true,
  probation_waits boolean NOT NULL DEFAULT false,
  carry_over_max_halves int NOT NULL DEFAULT 0 CHECK (carry_over_max_halves BETWEEN 0 AND 730),
  carry_over_expiry_months smallint CHECK (carry_over_expiry_months BETWEEN 1 AND 12),
  PRIMARY KEY (workspace_id, policy_id, leave_type_id),
  FOREIGN KEY (workspace_id, policy_id) REFERENCES leave_policies (workspace_id, id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id, leave_type_id) REFERENCES leave_types (workspace_id, id)
);
SELECT procera_enable_workspace_rls('leave_policy_rules');

ALTER TABLE membership_hr ADD COLUMN leave_policy_id uuid;
ALTER TABLE membership_hr ADD CONSTRAINT membership_hr_policy_fk
  FOREIGN KEY (workspace_id, leave_policy_id) REFERENCES leave_policies (workspace_id, id);

CREATE TABLE leave_ledger (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  membership_id uuid NOT NULL,
  leave_type_id uuid NOT NULL,
  leave_year date NOT NULL,                        -- the first day of the leave year
  kind text NOT NULL CHECK (kind IN ('grant', 'accrual', 'hold', 'release', 'taken', 'cancelled', 'adjustment', 'carry_over', 'expiry')),
  halves int NOT NULL CHECK (halves <> 0),
  period text,                                     -- idempotency key of system entries ('grant:2026-01-01', 'accrual:2026-01-01:3')
  request_id uuid,
  reason text CHECK (length(reason) <= 500),
  created_by uuid,                                 -- the admin for adjustments; NULL = Procera
  created_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, leave_type_id) REFERENCES leave_types (workspace_id, id),
  CHECK (kind <> 'adjustment' OR (reason IS NOT NULL AND created_by IS NOT NULL))
);
CREATE UNIQUE INDEX leave_ledger_period ON leave_ledger (workspace_id, membership_id, leave_type_id, period) WHERE period IS NOT NULL;
CREATE INDEX leave_ledger_balance ON leave_ledger (workspace_id, membership_id, leave_type_id, leave_year);
SELECT procera_enable_workspace_rls('leave_ledger');

CREATE FUNCTION procera_leave_ledger_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'leave_ledger is append-only: add a correcting entry instead';
END $$;
CREATE TRIGGER leave_ledger_append_only BEFORE UPDATE OR DELETE ON leave_ledger
  FOR EACH ROW EXECUTE FUNCTION procera_leave_ledger_append_only();

-- New permissions (docs/12 §5, docs/25 §6) for the built-in roles of workspaces created before they
-- existed: owners and admins get all of them, members can request time off, and a new built-in
-- "HR Administrator" role appears. New workspaces get these from the templates.
DO $$
DECLARE w uuid;
BEGIN
  FOR w IN SELECT id FROM workspaces LOOP
    PERFORM set_config('app.workspace_id', w::text, true);
    UPDATE roles SET permissions = permissions || ARRAY(
             SELECT p FROM unnest(ARRAY['time_off:request', 'time_off:view_all', 'time_off:manage_policies', 'time_off:adjust_balances']) p
              WHERE NOT (p = ANY (permissions))),
           revision = revision + 1
     WHERE is_builtin AND key IN ('workspace.owner', 'workspace.admin');
    UPDATE roles SET permissions = array_append(permissions, 'time_off:request'), revision = revision + 1
     WHERE is_builtin AND key = 'member' AND NOT ('time_off:request' = ANY (permissions));
    INSERT INTO roles (workspace_id, id, key, name, description, builtin_template, is_builtin, permissions)
    VALUES (w, gen_random_uuid(), 'hr.admin', 'HR Administrator',
            'Time off, holidays, locations and people records: policies, balances and adjustments.', 'hr.admin', true,
            ARRAY['member:view', 'directory:view', 'directory:manage', 'calendar:view', 'calendar:manage',
                  'time_off:request', 'time_off:view_all', 'time_off:manage_policies', 'time_off:adjust_balances'])
    ON CONFLICT (workspace_id, key) DO NOTHING;
    INSERT INTO access_revisions (workspace_id, revision) VALUES (w, 1)
      ON CONFLICT (workspace_id) DO UPDATE SET revision = access_revisions.revision + 1;
  END LOOP;
  PERFORM set_config('app.workspace_id', '', true);
END $$;

-- The leave policy that applies to a member: the one HR assigned, else the first (by priority) whose
-- audience includes them. Archived policies never apply.
CREATE FUNCTION procera_leave_policy_of(membership uuid) RETURNS uuid LANGUAGE sql STABLE AS $$
  SELECT coalesce(
    (SELECT h.leave_policy_id FROM membership_hr h
       JOIN leave_policies p ON p.workspace_id = h.workspace_id AND p.id = h.leave_policy_id AND p.archived_at IS NULL
      WHERE h.membership_id = membership),
    (SELECT p.id FROM leave_policies p
      WHERE p.archived_at IS NULL AND procera_audience_contains(p.audience, membership)
      ORDER BY p.priority, p.created_at LIMIT 1))
$$;
