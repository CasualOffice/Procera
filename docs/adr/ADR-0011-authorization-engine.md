# ADR-0011 — Cedar embedded for authorization

Status: **Validated by SP-09 (2026-10-11); acceptance at G1** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Need configurable custom roles per workspace, scopes, field-level rules, and non-overridable invariants, evaluated in-process with low latency.

## Decision
Embed `cedar-policy` (Apache-2.0). Roles/grants compile to Cedar policies from the permission catalogue (no admin-authored policy text). Invariants are fixed `forbid` policies. List filtering via partial evaluation or hand-compiled scope predicates. Postgres RLS remains for tenant isolation.

## Alternatives considered
OpenFGA/SpiceDB (separate services; consider later for heavy ReBAC). Casbin (weaker modelling). Hand-rolled checks (rejected: unanalysable).

## Consequences
Policy compile cache keyed by access revision; generated role×action×scope tests.

## SP-09 result (2026-10-11)

See [docs/spikes/SP-09](../spikes/SP-09-authz-rls-pool.md).
- **Cedar:** all scope, invariant and isolation cases are correct. Decisions take 5.6 µs (P50) and 7.3 µs (P99). Partial evaluation yields residuals that translate to SQL (ltree subtree). A hand-compiled fallback is still required because the feature is experimental.
- **RLS through the sqlx pool:** 0 leaks in 5 000 concurrent randomized transactions. List overhead is about 0.2 ms.
- **Corrections:**
  - `NULLIF(current_setting(...), '')` in policies;
  - pipelined `set_config`;
  - explicit policy ids for explainable audit.
