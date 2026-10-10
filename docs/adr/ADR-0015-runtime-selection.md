# ADR-0015 — Runtime: native Rust interpreter primary

Status: **Proposed — decided at G1** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Verified landscape (01 §2): no production Rust BPMN engine; Temporal Rust SDK 1.0 GA (Sep 2026); Operaton 2.x Apache-2.0 JVM; Camunda 8 not usable.

## Decision
Primary: native interpreter on Postgres (04-LLD §4). Fallback spike: Temporal with a generic interpreter workflow. Operaton used as semantic benchmark and optional adapter for C7 migrants. Decide at G1 using SP-01…SP-06 results, ops cost and team skills.

## Alternatives considered
Operaton as core (JVM ops burden for self-hosters). Flowable OSS (headless, JVM).

## Consequences
Adapter seam (`RuntimeAdapter`) is a design boundary, not free portability.
