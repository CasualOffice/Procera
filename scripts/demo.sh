#!/usr/bin/env bash
# Local demo: throwaway Postgres + the e2e build of procera + the web app (http://127.0.0.1:$WEB_PORT).
# Seeds the "Acme" workspace and prints sign-in details. Ctrl-C stops everything.
set -euo pipefail
cd "$(dirname "$0")/.."
PGBIN=$(dirname "$(command -v initdb || echo /opt/homebrew/opt/postgresql@16/bin/initdb)")
OFF=${DEMO_OFFSET:-0}; API_PORT=$((18500 + OFF)); WEB_PORT=$((5173 + OFF))
PGTMP=$(mktemp -d /tmp/procera-demo.XXXX); PORT=$((16599 + OFF))
cleanup() { kill "${API_PID:-}" "${WEB_PID:-}" "${TICK_PID:-}" 2>/dev/null || true; "$PGBIN/pg_ctl" -D "$PGTMP" stop -m fast >/dev/null 2>&1 || true; rm -rf "$PGTMP"; }
trap cleanup EXIT
"$PGBIN/initdb" -D "$PGTMP" -U postgres --auth=trust -E UTF8 >/dev/null
"$PGBIN/pg_ctl" -D "$PGTMP" -o "-p $PORT -c unix_socket_directories='' -c listen_addresses=127.0.0.1" -l "$PGTMP/log" start >/dev/null
until "$PGBIN/pg_isready" -h 127.0.0.1 -p $PORT -q; do sleep 0.3; done
psql "postgres://postgres@127.0.0.1:$PORT/postgres" -q -f deploy/sql/00-roles.sql >/dev/null
cargo build -q -p procera --features e2e
target/debug/procera migrate --migrator-url "postgres://procera_migrator@127.0.0.1:$PORT/postgres" 2>/dev/null
PROCERA_DATABASE_URL="postgres://procera_app@127.0.0.1:$PORT/postgres" PROCERA_MASTER_KEY="$(target/debug/procera keygen 2>/dev/null)" \
  PROCERA_INSECURE_COOKIES=true PROCERA_PUBLIC_URL=http://127.0.0.1:$WEB_PORT PROCERA_BIND=127.0.0.1:$API_PORT \
  target/debug/procera serve --all >/tmp/procera-demo-$OFF.log 2>&1 & API_PID=$!
until curl -fs -o /dev/null http://127.0.0.1:$API_PORT/healthz; do sleep 0.2; done
# The e2e build has a test clock that stands still; keep it in step with real time for the demo.
(while sleep 15; do curl -fs -o /dev/null -X POST -H 'content-type: application/json' -d '{"advance":"PT15S"}' http://127.0.0.1:$API_PORT/__test/clock || exit 0; done) & TICK_PID=$!
API=http://127.0.0.1:$API_PORT node scripts/demo-seed.mjs | tee /tmp/procera-demo-credentials-$OFF.json
(cd web && PROCERA_API_URL=http://127.0.0.1:$API_PORT exec ./node_modules/.bin/vite --port $WEB_PORT --strictPort --host 127.0.0.1 >/tmp/procera-demo-web-$OFF.log 2>&1) & WEB_PID=$!
until curl -fs -o /dev/null http://127.0.0.1:$WEB_PORT/; do sleep 0.2; done
echo; echo "Procera demo: http://127.0.0.1:$WEB_PORT  (design gallery: /design). Sign in as olivia@acme.test / correct horse battery staple (no code needed)."
wait "$API_PID"
