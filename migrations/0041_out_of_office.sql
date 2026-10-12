-- 0041 (BPMS-086): out of office and delegation (docs/12 §9, docs/04 §2.6).
-- A member who is away names a delegate for a date range (inclusive, UTC dates). While it is
-- active, new work items for them go to the delegate (the resolver substitutes and records it in
-- the work item's resolution snapshot), and the delegate may decide their existing open items.
-- One out-of-office per member; clearing it deletes the row (the audit trail keeps the history).

CREATE TABLE out_of_office (
  workspace_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  delegate_membership_id uuid NOT NULL,
  starts_on date NOT NULL,
  ends_on date NOT NULL,
  note text CHECK (note IS NULL OR length(note) <= 500),
  set_by uuid NOT NULL,                            -- membership: the member, or an admin/manager
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, membership_id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, delegate_membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, set_by) REFERENCES memberships (workspace_id, id),
  CHECK (ends_on >= starts_on),
  CHECK (delegate_membership_id <> membership_id)
);
CREATE INDEX out_of_office_delegate ON out_of_office (workspace_id, delegate_membership_id, ends_on);
SELECT procera_enable_workspace_rls('out_of_office');

-- A decision taken by a delegate records whom it was taken for ("X on behalf of Y").
ALTER TABLE work_items ADD COLUMN completed_on_behalf_of uuid;
ALTER TABLE work_items ADD CONSTRAINT work_items_on_behalf_of_fk
  FOREIGN KEY (workspace_id, completed_on_behalf_of) REFERENCES memberships (workspace_id, id);
