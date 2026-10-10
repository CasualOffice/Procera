# ADR-0003 — Canonical, versioned workflow IR

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Multiple authoring surfaces (step builder, BPMN import, templates, AI) and possibly multiple runtimes need one contract.

## Decision
Adopt IR v1 as specified in 11-WORKFLOW-IR-SPEC: JCS-canonical JSON, checksummed, integer `schemaVersion` with pure migrations, compiler diagnostics with stable codes, conformance suite.

## Alternatives considered
Execute BPMN XML directly (rejected: unbounded semantics, scripts). Code-defined workflows (rejected: not no-code).

## Consequences
Every feature that touches execution must extend the IR spec first. Conformance suite gates adapters.
