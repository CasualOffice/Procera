# ADR-0012 — Release bundle pins every dependency of a definition

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Forms, rules, calendars, connector configs and SOPs referenced by a process can change under running instances (gap B6).

## Decision
On publish, write an immutable, hashed manifest of exact versions of every referenced artifact. Instances reference the bundle, not just the process version.

## Alternatives considered
Pin only the process version (rejected: drift).

## Consequences
Publishing a form requires republishing processes that should adopt it; UI offers 'dependents to update'.
