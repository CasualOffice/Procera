# ADR-0005 — Transactional outbox/inbox and idempotency keys

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Dual writes (DB + broker/email/webhook) lose or duplicate effects on crash.

## Decision
State change + events + outbox + audit commit atomically. Dispatcher publishes after commit; consumers dedupe via inbox table. All mutating commands accept `Idempotency-Key`. External effects are at-least-once with idempotency keys; never claim exactly-once.

## Alternatives considered
2PC/XA (rejected). Fire-and-forget after commit (rejected: loss).

## Consequences
Connector contracts must honour idempotency keys; outbox lag is a monitored SLI.
