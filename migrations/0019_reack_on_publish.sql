-- BPMS-045 (FR-PL03, FR-G03): re-acknowledgement on publish. Versions carry a major.minor number
-- decided when they are published (or scheduled) and whether going live asks the applicable audience
-- to acknowledge again (governance.reack.policy). The campaign that does so is marked `reack`; there
-- is at most one per version, which makes the publish path and the event consumer idempotent.

ALTER TABLE artifact_versions
  ADD COLUMN semver text CHECK (semver IS NULL OR semver ~ '^[1-9][0-9]{0,5}\.[0-9]{1,6}$'),
  ADD COLUMN reack_required boolean NOT NULL DEFAULT false,
  ADD COLUMN scheduled_by uuid;                  -- membership that scheduled it (the scheduler publishes as the system)

-- Versions published before this migration count as majors.
UPDATE artifact_versions SET semver = version_no || '.0'
WHERE semver IS NULL AND lifecycle IN ('scheduled', 'published', 'deprecated', 'retired');

ALTER TABLE campaigns
  ADD COLUMN origin text NOT NULL DEFAULT 'manual' CHECK (origin IN ('manual', 'reack'));
CREATE UNIQUE INDEX campaigns_one_reack_per_version ON campaigns (workspace_id, subject_version_id) WHERE origin = 'reack';
