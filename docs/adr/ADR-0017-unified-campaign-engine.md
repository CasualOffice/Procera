# ADR-0017 — One acknowledgement & signature campaign engine

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
SOP/policy re-ack, attestations, announcements with ack, and document signing are the same pattern: subject + audience + action + deadline + reminders + evidence.

## Decision
Single `campaigns` model with actions read/acknowledge/quiz/sign; envelopes for signing; audience snapshots; new-joiner auto-enrolment; one employee queue and one admin tracker.

## Alternatives considered
Separate modules per feature (rejected: duplicated logic, inconsistent evidence).

## Consequences
Campaign engine is on the P0 critical path (M5–M6).
