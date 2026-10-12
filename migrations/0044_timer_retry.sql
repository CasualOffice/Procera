-- A reminder or escalation whose handler fails is retried with backoff, then marked failed with
-- its last error (docs/04 §4.4), instead of being dropped after one try.
ALTER TABLE timers
  ADD COLUMN attempts int NOT NULL DEFAULT 0,
  ADD COLUMN last_error text;
ALTER TABLE timers DROP CONSTRAINT timers_state_check;
ALTER TABLE timers ADD CONSTRAINT timers_state_check CHECK (state IN ('scheduled', 'fired', 'cancelled', 'failed'));
