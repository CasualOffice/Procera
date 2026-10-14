-- 0055 (BPMS-222, docs/25 §3): work locations, holiday calendars and each member's HR record. A
-- location has an IANA time zone, a work week and a holiday calendar; leave day counting (BPMS-224)
-- and attendance (BPMS-227) read them. Existing free-text member locations become work locations;
-- `memberships.location` stays the location's name so audiences keep matching it.

CREATE TABLE holiday_calendars (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
  created_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id)
);
CREATE UNIQUE INDEX holiday_calendars_name ON holiday_calendars (workspace_id, lower(name));
SELECT procera_enable_workspace_rls('holiday_calendars');

CREATE TABLE holidays (
  workspace_id uuid NOT NULL,
  calendar_id uuid NOT NULL,
  date date NOT NULL,
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
  optional boolean NOT NULL DEFAULT false,         -- a restricted holiday people may choose to take
  PRIMARY KEY (workspace_id, calendar_id, date),
  FOREIGN KEY (workspace_id, calendar_id) REFERENCES holiday_calendars (workspace_id, id) ON DELETE CASCADE
);
SELECT procera_enable_workspace_rls('holidays');

CREATE TABLE work_locations (
  workspace_id uuid NOT NULL,
  id uuid NOT NULL,
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
  timezone text NOT NULL,                          -- IANA, checked against pg_timezone_names on save
  work_week smallint[] NOT NULL DEFAULT '{1,2,3,4,5}'
    CHECK (cardinality(work_week) BETWEEN 1 AND 7 AND work_week <@ '{1,2,3,4,5,6,7}'::smallint[]),
  day_hours numeric(4,2) NOT NULL DEFAULT 8 CHECK (day_hours > 0 AND day_hours <= 24),
  holiday_calendar_id uuid,
  archived_at timestamptz,
  created_at timestamptz NOT NULL,
  revision int NOT NULL DEFAULT 1,
  PRIMARY KEY (workspace_id, id),
  FOREIGN KEY (workspace_id, holiday_calendar_id) REFERENCES holiday_calendars (workspace_id, id)
);
CREATE UNIQUE INDEX work_locations_name ON work_locations (workspace_id, lower(name));
SELECT procera_enable_workspace_rls('work_locations');

CREATE TABLE membership_hr (
  workspace_id uuid NOT NULL,
  membership_id uuid NOT NULL,
  employee_no text CHECK (length(employee_no) <= 40),
  employment_type text CHECK (employment_type IN ('full_time', 'part_time', 'contract', 'intern')),
  work_location_id uuid,
  start_date date,
  probation_end date,
  updated_at timestamptz NOT NULL,
  PRIMARY KEY (workspace_id, membership_id),
  FOREIGN KEY (workspace_id, membership_id) REFERENCES memberships (workspace_id, id),
  FOREIGN KEY (workspace_id, work_location_id) REFERENCES work_locations (workspace_id, id)
);
CREATE UNIQUE INDEX membership_hr_employee_no ON membership_hr (workspace_id, lower(employee_no)) WHERE employee_no IS NOT NULL;
CREATE INDEX membership_hr_location ON membership_hr (workspace_id, work_location_id);
SELECT procera_enable_workspace_rls('membership_hr');

-- Free-text locations already on members become work locations (time zone UTC until an admin sets
-- it), and their members point at them. Runs as the migration owner, so RLS doesn't apply.
INSERT INTO work_locations (workspace_id, id, name, timezone, created_at)
SELECT workspace_id, gen_random_uuid(), min(btrim(location)), 'UTC', now()
  FROM memberships WHERE location IS NOT NULL AND btrim(location) <> ''
 GROUP BY workspace_id, lower(btrim(location));
INSERT INTO membership_hr (workspace_id, membership_id, work_location_id, start_date, updated_at)
SELECT m.workspace_id, m.id, l.id, m.joined_at::date, now()
  FROM memberships m JOIN work_locations l
    ON l.workspace_id = m.workspace_id AND lower(l.name) = lower(btrim(m.location));

-- A member's free-text location (directory edit, CSV import, later SCIM) picks the work location of
-- that name, creating it (UTC, Monday to Friday) the first time; clearing it clears the link.
-- Choosing a work location in the HR record writes its name here, so both always agree.
CREATE FUNCTION procera_sync_member_location() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE loc uuid;
BEGIN
  IF NEW.location IS NULL OR btrim(NEW.location) = '' THEN
    UPDATE membership_hr SET work_location_id = NULL, updated_at = now()
     WHERE workspace_id = NEW.workspace_id AND membership_id = NEW.id AND work_location_id IS NOT NULL;
    RETURN NEW;
  END IF;
  SELECT id INTO loc FROM work_locations WHERE workspace_id = NEW.workspace_id AND lower(name) = lower(btrim(NEW.location));
  IF loc IS NULL THEN
    loc := gen_random_uuid();
    INSERT INTO work_locations (workspace_id, id, name, timezone, created_at)
    VALUES (NEW.workspace_id, loc, btrim(NEW.location), 'UTC', now());
  END IF;
  INSERT INTO membership_hr (workspace_id, membership_id, work_location_id, start_date, updated_at)
  VALUES (NEW.workspace_id, NEW.id, loc, NEW.joined_at::date, now())
  ON CONFLICT (workspace_id, membership_id) DO UPDATE SET work_location_id = EXCLUDED.work_location_id, updated_at = now()
   WHERE membership_hr.work_location_id IS DISTINCT FROM EXCLUDED.work_location_id;
  RETURN NEW;
END $$;
CREATE TRIGGER memberships_location_sync AFTER INSERT OR UPDATE OF location ON memberships
  FOR EACH ROW EXECUTE FUNCTION procera_sync_member_location();
