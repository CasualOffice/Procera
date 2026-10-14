-- 0057 (BPMS-224, docs/25 §4, §7): leave requests. A leave request is a request (process instance)
-- of `hr.leave-request`; this row keeps what it asks for in HR's terms (type, dates, halves, the days
-- it counts and why each date counts) and its state, which follows the request: pending while it is
-- decided (the days are held), then approved (taken), rejected or cancelled (released).

CREATE TABLE leave_requests (
  workspace_id uuid NOT NULL,
  request_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  leave_type_id uuid NOT NULL,
  leave_year date NOT NULL,
  start_date date NOT NULL,
  start_half text NOT NULL DEFAULT 'full' CHECK (start_half IN ('full', 'second_half')),
  end_date date NOT NULL,
  end_half text NOT NULL DEFAULT 'full' CHECK (end_half IN ('full', 'first_half')),
  halves int NOT NULL CHECK (halves >= 0),
  detail jsonb NOT NULL,                           -- the per-date breakdown: [{date, why, halves}]
  state text NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'cancelled')),
  created_at timestamptz NOT NULL,
  decided_at timestamptz,
  PRIMARY KEY (workspace_id, request_id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, leave_type_id) REFERENCES leave_types (workspace_id, id),
  CHECK (end_date >= start_date)
);
CREATE INDEX leave_requests_member ON leave_requests (workspace_id, membership_id, start_date);
CREATE INDEX leave_requests_dates ON leave_requests (workspace_id, start_date, end_date) WHERE state IN ('pending', 'approved');
SELECT procera_enable_workspace_rls('leave_requests');
