# ADR-0010 — Typed, total expression language

Status: **Proposed — decided by SP-08** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Conditions, assignments, rule tables, SLAs and form visibility need a safe expression language; v0.1 chose none.

## Decision
Choose between (a) FEEL subset via `dsntk` (MIT/Apache, aligns with DMN) and (b) CEL (non-Turing-complete, mature spec). Criteria: static typing against IR variables, evaluation budgets, determinism, error-message quality, perf, licence, TCK coverage. The same engine must run in the browser (WASM) for form logic and test runs.

## Alternatives considered
JSONLogic (weak typing), JavaScript sandbox (rejected: security), custom DSL (rejected: cost).

## Consequences
Whichever wins, IR stores `expr:<lang>:` source + typed AST; switching later requires a migration.
