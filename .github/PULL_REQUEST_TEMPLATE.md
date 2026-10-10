## What & why

<!-- One tracker item per PR. -->
- Tracker id: `BPMS-___` / `UX-___` / `CFG-___` …
- Requirements: `FR-___`, `FX-___` …
- Spec sections followed: `docs/__ §__`

## Checklist (docs/24-ENGINEERING-RULES.md)

- [ ] Docs updated first or in this PR (behaviour, data, API, security or UX change); ADR added if architectural
- [ ] Tests: unit + integration (real Postgres) + **E2E scenario** `E2E-R_-__` added/extended
- [ ] Runtime/access/audit change → property/model tests; conformance 100 %
- [ ] Workspace isolation respected (`workspace_id`, RLS, `WsCtx`); authz via Cedar; audit event emitted
- [ ] No secrets, PII or restricted values in logs/fixtures; new deps pass the licence gate
- [ ] UI: design tokens, WCAG 2.2 AA, keyboard path, empty/loading/error/no-permission states, no AI-look patterns
- [ ] Tracker status updated and `python3 scripts/build_tracker.py` run
- [ ] Commits are DCO signed-off; no AI attribution lines

## Evidence

<!-- Commands run, test output, E2E ids, screenshots for UI. -->
