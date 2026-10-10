# ADR-0011 — Cedar embedded for authorization

Status: **Proposed — validated by SP-09** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Need configurable custom roles per workspace, scopes, field-level rules, and non-overridable invariants, evaluated in-process with low latency.

## Decision
Embed `cedar-policy` (Apache-2.0). Roles/grants compile to Cedar policies from the permission catalogue (no admin-authored policy text). Invariants are fixed `forbid` policies. List filtering via partial evaluation or hand-compiled scope predicates. Postgres RLS remains for tenant isolation.

## Alternatives considered
OpenFGA/SpiceDB (separate services; consider later for heavy ReBAC). Casbin (weaker modelling). Hand-rolled checks (rejected: unanalysable).

## Consequences
Policy compile cache keyed by access revision; generated role×action×scope tests.
