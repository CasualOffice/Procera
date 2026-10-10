# ADR-0001 — Separate governance/catalog from execution

Status: **Accepted (pending G1)** · Date: 2026-10-11 · Deciders: Architect, Product, Security lead

## Context
Process documentation (SOPs, policies, owners, reviews) and process execution (instances, work) have different lifecycles, owners and scaling profiles. v0.1 established that model ≠ definition ≠ instance ≠ SOP ≠ case.

## Decision
Library & Governance owns versioned artifacts; Runtime owns instances. They link only through immutable `artifact_version_id` / `release_bundle_id`. Publishing is the single governed act that changes how work runs.

## Alternatives considered
Single 'process object' that is edited in place (rejected: breaks audit and running instances). Separate products (rejected: loses the core wedge).

## Consequences
Clear audit story and pinning. Requires the release bundle (ADR-0012) and semantic diff across artifact kinds.
