-- Procera database roles (docs/04 §2.2). Run ONCE per database by a DBA / Helm pre-install hook,
-- as a superuser or a role with CREATEROLE. Migrations then run as procera_migrator.
--   procera_migrator : owns the schema; runs migrations; never used by the app at runtime.
--   procera_app      : the runtime role. Not an owner, NOBYPASSRLS, DML only (forced RLS applies).
--   procera_discovery: NOLOGIN, BYPASSRLS; owns only id-returning discovery functions.
-- Passwords are set out of band (e.g. ALTER ROLE … PASSWORD from a secret store).
-- Roles are cluster-wide; tolerate concurrent bootstrap (parallel hooks/tests) instead of racing.
DO $$
BEGIN
  BEGIN
    CREATE ROLE procera_migrator LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEROLE;
  EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
  END;
  BEGIN
    CREATE ROLE procera_app LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEROLE NOCREATEDB;
  EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
  END;
  -- Owns ONLY the cross-workspace discovery functions (SECURITY DEFINER). NOLOGIN; BYPASSRLS is
  -- needed because FORCE RLS applies to table owners too. It is granted column-level SELECT on
  -- id/status columns only, so it can never read payloads (docs/04 §2.2).
  BEGIN
    CREATE ROLE procera_discovery NOLOGIN NOSUPERUSER BYPASSRLS;
  EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
  END;
END $$;
-- Discovery functions live in their own schema, owned by the discovery role. The migrator creates
-- them there and hands ownership over; the app may only EXECUTE them.
GRANT procera_discovery TO procera_migrator;
CREATE SCHEMA IF NOT EXISTS discovery AUTHORIZATION procera_discovery;
GRANT USAGE, CREATE ON SCHEMA discovery TO procera_migrator;
GRANT USAGE ON SCHEMA discovery TO procera_app;
GRANT USAGE ON SCHEMA public TO procera_discovery;

-- Let the migrator create objects in the target database's public schema and own them.
DO $$ BEGIN EXECUTE format('GRANT CONNECT, CREATE ON DATABASE %I TO procera_migrator', current_database()); END $$;
DO $$ BEGIN EXECUTE format('GRANT CONNECT ON DATABASE %I TO procera_app', current_database()); END $$;
ALTER SCHEMA public OWNER TO procera_migrator;
GRANT USAGE ON SCHEMA public TO procera_app;

-- Anything the migrator creates is usable (DML only) by the app role.
ALTER DEFAULT PRIVILEGES FOR ROLE procera_migrator IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO procera_app;
ALTER DEFAULT PRIVILEGES FOR ROLE procera_migrator IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO procera_app;
ALTER DEFAULT PRIVILEGES FOR ROLE procera_migrator IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO procera_app;
-- Extensions need superuser on most hosts; create them here so migrations don't.
CREATE EXTENSION IF NOT EXISTS citext;
CREATE EXTENSION IF NOT EXISTS ltree;
