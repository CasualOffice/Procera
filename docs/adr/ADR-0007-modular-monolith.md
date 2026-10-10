# ADR-0007 — Modular monolith with separate worker/scheduler processes

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Small team, self-host simplicity, transactional boundaries spanning contexts.

## Decision
One codebase/image with strict module boundaries; `api`, `worker`, `scheduler`, `converter` run as separate processes from the same image (or all-in-one for trial). Split services only on measured need.

## Alternatives considered
Microservices from day one (rejected: ops cost, distributed transactions).

## Consequences
Enforce boundaries via crate visibility and schema-ownership lint.
