#!/bin/sh
# Runs once, on the first start of an empty Postgres volume, right after 10-roles.sql
# (deploy/sql/00-roles.sql) has created the roles. Sets their passwords from the env file.
set -eu
: "${PROCERA_MIGRATOR_DB_PASSWORD:?set PROCERA_MIGRATOR_DB_PASSWORD in deploy/.env}"
: "${PROCERA_APP_DB_PASSWORD:?set PROCERA_APP_DB_PASSWORD in deploy/.env}"
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v migrator_pw="$PROCERA_MIGRATOR_DB_PASSWORD" -v app_pw="$PROCERA_APP_DB_PASSWORD" <<'SQL'
ALTER ROLE procera_migrator PASSWORD :'migrator_pw';
ALTER ROLE procera_app PASSWORD :'app_pw';
SQL
