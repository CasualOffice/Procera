#!/usr/bin/env bash
# Complete local test suite — run before pushing a batch (docs/24 §7.3). Exits non-zero on any failure.
set -euo pipefail
cd "$(dirname "$0")/.."
API_PORT=${API_PORT:-18080}
WEB_PORT=14173
step() { printf '\n\033[1m▶ %s\033[0m\n' "$*"; }
cleanup() {
  for p in "${API_PID:-}" "${WEB_PID:-}" "${CACHE_PID:-}"; do [[ -n "$p" ]] && kill "$p" 2>/dev/null || true; done
  if [[ -n "${PGTMP:-}" ]]; then "$PGBIN/pg_ctl" -D "$PGTMP" stop -m fast >/dev/null 2>&1 || true; rm -rf "$PGTMP"; fi
}
trap cleanup EXIT

step "Rust: fmt"
cargo fmt --all -- --check
step "Rust: clippy (all features, and default features)"
cargo clippy --workspace --all-targets --all-features -q -- -D warnings
cargo clippy --workspace --all-targets -q -- -D warnings
step "Cache server for integration tests (Valkey/Redis if installed)"
if [[ -z "${PROCERA_TEST_CACHE_URL:-}" ]]; then
  if SRV=$(command -v valkey-server || command -v redis-server); then
    "$SRV" --port 16391 --save "" --appendonly no >/dev/null 2>&1 & CACHE_PID=$!
    until (exec 3<>/dev/tcp/127.0.0.1/16391) 2>/dev/null; do sleep 0.2; done
    export PROCERA_TEST_CACHE_URL=redis://127.0.0.1:16391; echo "using $SRV on :16391"
  else echo "no valkey/redis installed: remote cache tests will be skipped"; fi
fi
step "PostgreSQL for integration tests (throwaway cluster if PROCERA_TEST_DATABASE_URL is unset)"
if [[ -z "${PROCERA_TEST_DATABASE_URL:-}" ]]; then
  if command -v initdb >/dev/null || [[ -x /opt/homebrew/opt/postgresql@16/bin/initdb ]]; then
    PGBIN=$(dirname "$(command -v initdb || echo /opt/homebrew/opt/postgresql@16/bin/initdb)")
    PGTMP=$(mktemp -d /tmp/procera-pg.XXXX)
    "$PGBIN/initdb" -D "$PGTMP" -U postgres --auth=trust -E UTF8 >/dev/null
    "$PGBIN/pg_ctl" -D "$PGTMP" -o "-p 16432 -c unix_socket_directories='' -c listen_addresses=127.0.0.1" -l "$PGTMP/log" start >/dev/null
    until "$PGBIN/pg_isready" -h 127.0.0.1 -p 16432 -q; do sleep 0.3; done
    export PROCERA_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:16432/postgres; echo "throwaway Postgres on :16432"
  else echo "no PostgreSQL found: database tests will be skipped"; fi
fi
step "Rust: tests (all features, and default features)"
cargo test --workspace --all-features -q 2>&1 | grep -E "test result|FAILED|panicked" || true
cargo test --workspace --all-features -q >/dev/null
cargo test --workspace -q >/dev/null
step "Licence gates (Rust product, Rust spikes, JS)"
cargo deny check 2>&1 | tail -1
cargo deny --manifest-path spikes/Cargo.toml --config deny.toml check licenses 2>&1 | tail -1
node scripts/js-licence-gate.mjs
step "Spikes compile"
cargo check -q --manifest-path spikes/Cargo.toml --workspace
step "Tracker regeneration + links"
python3 scripts/build_tracker.py >/dev/null
python3 - <<'PY'
import re, pathlib, sys
bad = []
for f in pathlib.Path('.').rglob('*.md'):
    if any(x in f.parts for x in ('archive', 'node_modules', 'source', 'target')): continue
    for m in re.finditer(r'\]\(([^)#\s]+)(#[^)]*)?\)', f.read_text()):
        t = m.group(1)
        if not t.startswith(('http', 'mailto')) and not (f.parent / t).exists(): bad.append(f"{f}: {t}")
print("links ok" if not bad else "\n".join(bad)); sys.exit(1 if bad else 0)
PY
step "Compose stack definition"
docker compose -f deploy/compose.e2e.yml config -q && echo "compose ok"
step "Web: tokens, contrast, types, build"
(cd web && npm run check --silent && npm run build --silent >/dev/null && echo "web build ok")
step "E2E: start e2e API build (database-backed when Postgres is available) and web preview"
cargo build -q -p procera --features e2e
API_ENV=()
if [[ -n "${PROCERA_TEST_DATABASE_URL:-}" ]] && command -v psql >/dev/null; then
  E2E_DB="procera_e2e_$(date +%s)"
  SUPER_BASE="${PROCERA_TEST_DATABASE_URL%/*}"
  psql "$PROCERA_TEST_DATABASE_URL" -qc "CREATE DATABASE $E2E_DB" >/dev/null
  psql "$SUPER_BASE/$E2E_DB" -q -f deploy/sql/00-roles.sql >/dev/null 2>&1
  HOSTPART="${SUPER_BASE#*@}"
  target/debug/procera migrate --migrator-url "postgres://procera_migrator@$HOSTPART/$E2E_DB" 2>/dev/null
  API_ENV=(PROCERA_DATABASE_URL="postgres://procera_app@$HOSTPART/$E2E_DB" PROCERA_MASTER_KEY="$(target/debug/procera keygen 2>/dev/null)" PROCERA_INSECURE_COOKIES=true PROCERA_LOGIN_RATE_PER_MIN=10000)
  echo "e2e database $E2E_DB"
fi
env "${API_ENV[@]}" PROCERA_BIND=127.0.0.1:$API_PORT target/debug/procera serve --all >/tmp/procera-test-all.log 2>&1 & API_PID=$!
(cd web && npx vite preview --port $WEB_PORT --strictPort --host 127.0.0.1 >/tmp/web-test-all.log 2>&1) & WEB_PID=$!
until curl -fs -o /dev/null "http://127.0.0.1:$API_PORT/healthz"; do sleep 0.2; done
until curl -fs -o /dev/null "http://127.0.0.1:$WEB_PORT/"; do sleep 0.2; done
step "E2E: no-sleeps lint, API smoke pack, accessibility pack"
(cd e2e && npm run --silent lint:no-sleeps && PROCERA_BASE_URL="http://127.0.0.1:$API_PORT" npx playwright test --project api --project chromium-desktop --reporter=line)
step "Release binary has no /__test routes"
cargo build -q --release -p procera
PROCERA_BIND=127.0.0.1:18089 target/release/procera serve --all >/dev/null 2>&1 & REL=$!
until curl -fs -o /dev/null http://127.0.0.1:18089/healthz; do sleep 0.2; done
code=$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18089/__test/clock); kill $REL
[[ "$code" == "404" ]] && echo "release /__test/clock -> 404"
printf '\n\033[1;32m✔ all checks passed\033[0m\n'
