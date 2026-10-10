# ADR-0006 — AI outputs are drafts behind a human gate

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
AI drafting is table stakes, but autonomous action and people-scoring create legal (EU AI Act Annex III §4(b)) and trust risk.

## Decision
AI may draft processes, SOPs, policies, forms, summaries and translations. Drafts enter the normal review/SoD lifecycle. AI never approves, signs, publishes, routes by individual behaviour or scores people. Provenance logged; per-feature workspace kill switch; off by default; provider-agnostic incl. local models.

## Alternatives considered
Autonomous agents (deferred to P2 as governed agent steps).

## Consequences
See 16-AI-ASSISTANCE. Keeps the product outside high-risk classification by design.
