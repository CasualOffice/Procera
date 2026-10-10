# ADR-0010 — Typed, total expression language

Status: **Recommended: CEL with Procera extensions (SP-08, 2026-10-11); final acceptance at G1** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Conditions, assignments, rule tables, SLAs and form visibility need a safe expression language; v0.1 chose none.

## Decision
Choose between (a) FEEL subset via `dsntk` (MIT/Apache, aligns with DMN) and (b) CEL (non-Turing-complete, mature spec). Criteria: static typing against IR variables, evaluation budgets, determinism, error-message quality, perf, licence, TCK coverage. The same engine must run in the browser (WASM) for form logic and test runs.

## Alternatives considered
JSONLogic (weak typing), JavaScript sandbox (rejected: security), custom DSL (rejected: cost).

## Consequences
Whichever wins, IR stores `expr:<lang>:` source + typed AST; switching later requires a migration.

## SP-08 result (2026-10-11)

See [docs/spikes/SP-08](../spikes/SP-08-expression-language.md). **CEL** is recommended:
- it is non-Turing-complete and bounded by the input data;
- it builds to WASM;
- it is faster.

It needs a decimal/money extension, typed marshalling, a Procera stdlib (aggregates, business time), our own static type-checker and evaluation guards.

**FEEL (dsntk) is rejected for runtime use**, for three reasons:
- iteration is quadratic with no budget (50k items took 74 s);
- it fails to build for wasm32 (Intel DFP C dependency);
- its silent-null semantics.

It remains a candidate for the *offline* DMN importer (P2).
