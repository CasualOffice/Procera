-- 0045 (BPMS-103): a request can finish as `completed`, for request types that end in work done
-- rather than an approval (employee onboarding ends "Onboarded"). Expand-only: existing statuses stay.
ALTER TABLE requests DROP CONSTRAINT requests_status_check;
ALTER TABLE requests ADD CONSTRAINT requests_status_check
  CHECK (status IN ('in_progress', 'changes_requested', 'approved', 'rejected', 'cancelled', 'completed'));
