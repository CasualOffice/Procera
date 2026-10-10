# ADR-0009 — Message broker is optional

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
v0.1 mandated Kafka, which is heavy for trials and SMEs.

## Decision
Default bus = Postgres outbox + in-process dispatcher with LISTEN/NOTIFY wakeups and polling. `Bus` trait adapters for NATS JetStream (Apache-2.0) and Kafka for large deployments and external consumers. Redpanda not bundled (BSL).

## Alternatives considered
Kafka mandatory (rejected). pgmq extension (optional; requires extension install).

## Consequences
Throughput ceiling of the PG bus measured in SP-01; adapters must pass the same dedup/ordering tests.
