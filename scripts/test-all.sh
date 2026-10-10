#!/usr/bin/env bash
# Complete local test suite — run before pushing a batch (docs/24 §7.3). Exits non-zero on any failure.
set -euo pipefail
cd "$(dirname "$0")/.."
API_PORT=${API_PORT:-18080}
WEB_PORT=14173
step() { printf '\n\033[1m▶ %s\033[0m\n' "$*"; }
cleanup() { [[ -n "${API_PID:-}" ]] && kill "$API_PID" 2>/dev/null || true; [[ -n "${WEB_PID:-}" ]] && kill "$WEB_PID" 2>/dev/null || true; }
trap cleanup EXIT

step "Rust: fmt"
cargo fmt --all -- --check
step "Rust: clippy (all features, and default features)"
cargo clippy --workspace --all-targets --all-features -q -- -D warnings
cargo clippy --workspace --all-targets -q -- -D warnings
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
step "E2E: start e2e API build and web preview"
cargo build -q -p procera --features e2e
PROCERA_BIND=127.0.0.1:$API_PORT target/debug/procera serve --all >/tmp/procera-test-all.log 2>&1 & API_PID=$!
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
