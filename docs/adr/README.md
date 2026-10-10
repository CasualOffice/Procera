# Architecture Decision Records

Format: Context · Decision · Alternatives · Consequences. New ADRs append; superseded ADRs stay with status `Superseded by ADR-NNNN`.

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](ADR-0001-governance-execution-separation.md) | Separate governance/catalog from execution | Accepted (pending G1) |
| [ADR-0002](ADR-0002-postgres-system-of-record.md) | PostgreSQL 16+ is the only system of record (not MariaDB/MySQL) | Accepted |
| [ADR-0003](ADR-0003-versioned-ir.md) | Canonical, versioned workflow IR | Accepted |
| [ADR-0004](ADR-0004-step-builder-first.md) | Step builder is the default authoring UX; BPMN is optional | Accepted |
| [ADR-0005](ADR-0005-outbox-idempotency.md) | Transactional outbox/inbox and idempotency keys | Accepted |
| [ADR-0006](ADR-0006-ai-human-gate.md) | AI outputs are drafts behind a human gate | Accepted |
| [ADR-0007](ADR-0007-modular-monolith.md) | Modular monolith with separate worker/scheduler processes | Accepted |
| [ADR-0008](ADR-0008-workspace-tenancy.md) | Workspace is the tenant; layered isolation | Accepted (revised per product direction 2026-10-11) |
| [ADR-0009](ADR-0009-broker-optional.md) | Message broker is optional | Accepted |
| [ADR-0010](ADR-0010-expression-language.md) | Typed, total expression language | Proposed — decided by SP-08 |
| [ADR-0011](ADR-0011-authorization-engine.md) | Cedar embedded for authorization | Proposed — validated by SP-09 |
| [ADR-0012](ADR-0012-release-bundle.md) | Release bundle pins every dependency of a definition | Accepted |
| [ADR-0013](ADR-0013-sop-content-model.md) | Structured block model for SOP/policy content | Accepted |
| [ADR-0014](ADR-0014-licensing-and-contribution.md) | Apache-2.0, DCO, permissive-only dependencies | Proposed (OQ-06) |
| [ADR-0015](ADR-0015-runtime-selection.md) | Runtime: native Rust interpreter primary | Proposed — decided at G1 |
| [ADR-0016](ADR-0016-standalone-first.md) | Standalone first; integrations later via ports | Accepted (product direction 2026-10-11) |
| [ADR-0017](ADR-0017-unified-campaign-engine.md) | One acknowledgement & signature campaign engine | Accepted |
| [ADR-0018](ADR-0018-ui-stack-and-canvas.md) | UI stack and canvas libraries | Accepted |
| [ADR-0019](ADR-0019-valkey-redis-cache.md) | Redis-protocol cache (Valkey default) as a standard, non-authoritative component | Accepted |
