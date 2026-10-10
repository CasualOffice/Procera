# 01 — Research, verified landscape and architecture decisions

Status: v0.2 · Re-verified **2026-10-11** against primary sources (GitHub API/LICENSE files,
release pages, vendor docs). Supersedes [v0.1](archive/v0.1/01_RESEARCH_AND_DECISIONS.md).
⚠ marks items still unverified.

## 1. Standards

| Standard | Role in Procera | Status we target |
|---|---|---|
| BPMN 2.0 (OMG) | Import/export of a documented subset; optional BPMN view | Subset + compatibility report (11-IR §10). No conformance claim. |
| DMN 1.x (OMG) / FEEL | Rule tables now; DMN import P2 | Table model = single-decision DMN subset |
| CMMN (OMG) | Inspiration for case management (stages, milestones, discretionary items) | Not implemented as a format |
| XES — IEEE 1849-2023 | Event-log export (P2) | Export only |
| OCEL 2.0 (RWTH community spec, 2023; not IEEE) | Object-centric event-log export (P2) | Export only |
| JSON Schema 2020-12 | Form data contract | Normative |
| RFC 9457 / 8785 / 9162 / 3161 | API errors / canonical JSON / Merkle logs / timestamps | Normative |
| Standard Webhooks | Outbound/inbound webhook signing | Normative |
| OWASP ASVS 5.0.0 (released 30 May 2025) | Security verification baseline | L2, L3 for auth/session/crypto |
| WCAG 2.2 AA, EN 301 549 | Accessibility | Normative |

## 2. Runtime engine landscape (verified)

| Option | Licence | Latest (as of 2026-10-11) | Fit | Verdict |
|---|---|---|---|---|
| **Own Rust interpreter on Postgres** | Ours (Apache-2.0) | — | Single binary + single Postgres. Data-driven definitions are native. Full control of human-task semantics. | **Primary spike** |
| **Temporal** + Rust SDK | Server MIT; Rust SDK MIT | Server v1.32.1; **Rust SDK 1.0.0 released 2026-09-04** (GA claim verified), 1.1.0 on 2026-10-06 | Battle-tested durability, timers, signals. Needs a generic "interpreter workflow"; extra cluster; history limits / continue-as-new. ⚠ Rust Worker Versioning support unverified. | **Spike as alternative substrate** |
| **Operaton** (Camunda 7 fork) | Apache-2.0, explicit no-open-core | **1.0 GA 2025-11-07**; current stable 2.1.5 (2026-09-18); 2.2.0-RC1 | Mature BPMN/DMN (FEEL-scala), Cockpit/Tasklist/Admin, C7-compatible REST, MCP server. JVM. | **Reference/benchmark + optional adapter** for C7 migrants |
| Flowable OSS | Apache-2.0 | 8.0.0 (2026-02-27) | Engines only (BPMN/CMMN/DMN). All UI apps removed since v7; design/work UIs commercial. JVM. | Not chosen. Possible later adapter. |
| Camunda 8 | Camunda License 1.0 (since 8.6): production needs a paid licence | 8.9.x | — | **Rejected** (licence) |
| Camunda 7 CE | — | EOL: 7.24 final (Oct 2025), repo archived | — | Rejected |
| Restate | BSL 1.1 (→ Apache after 4 yrs) | 1.7.x | Good primitives | Rejected for bundling (not OSI today) |
| DBOS Transact | MIT | Rust SDK 0.5 pre-release | Closest architecture (in-process on Postgres) | Watch; design reference |
| Apache KIE (jBPM/Drools) | Apache-2.0 | 10.2.0; still incubating, Kogito merged/archived | Heavy | Not chosen |
| Conductor OSS | Apache-2.0 | 3.32.x | JSON DSL with HUMAN tasks | Inspiration |
| n8n / Windmill / Inngest | SUL / AGPL / SSPL | — | — | **Rejected** (licence) |
| apalis / underway / pgmq | MIT / Apache-2.0 / PostgreSQL | 0.7 / 0.2 / 1.13 | Job/queue primitives | Optional building blocks; hand-rolled SKIP LOCKED tables are the default (04-LLD §2.5) |

**Rust BPMN engines:** none production-grade (all small/experimental).
`rust_bpmn_analyzer` (MIT) is a useful model-checker reference.
**Rust DMN/FEEL:** `dsntk` (MIT OR Apache-2.0, v0.3.0, active, small) is the only serious
option. ⚠ TCK pass rate unverified → spike SP-08.

## 3. UI building blocks (verified licences)

| Need | Option | Licence | Decision |
|---|---|---|---|
| BPMN modeler | bpmn-js / dmn-js / form-js | "bpmn.io licence" = MIT + **mandatory visible watermark**; white-label removal not permitted without a Camunda deal | **Do not use** in the product UI |
| BPMN canvas base | diagram-js + bpmn-moddle / dmn-moddle | MIT (no watermark clause) | **Use** for the optional BPMN view, with our own clean-room renderer/rules (no bpmn-js code copied) |
| Read-only BPMN viewer | bpmn-visualization (Process Analytics) | Apache-2.0 | Use for import previews and analytics overlays |
| Step/flow canvas | xyflow / React Flow | MIT (Pro = paid examples/support, library stays MIT) | **Use** for the default step-builder canvas |
| Auto-layout | elkjs (EPL-2.0, unmodified OK) / dagre (MIT) | — | elkjs, unmodified, as a separate chunk |
| Form renderer | RJSF (Apache-2.0) / JSON Forms (MIT) | — | **Own builder** over JSON Schema + RJSF-style renderer; formio.js builder (MIT) as fallback ⚠ re-verify per file |
| Form builder (commercial traps) | SurveyJS Creator (commercial), form-js (watermark), JSON Forms editor (archived) | — | Avoid |
| Data grid | TanStack Table + Virtual | MIT | Use (05-UX) |
| Accessible primitives | React Aria / Radix | Apache-2.0 / MIT | Decided in 05-UX |

## 4. Infrastructure components (licence traps found)

| Component | Finding | Decision |
|---|---|---|
| Object storage | **MinIO**: AGPL-3.0; community repo **archived 2026-04-25**, source-only, console stripped (2025) | Code to the **S3 API only**. Compose default **SeaweedFS** (Apache-2.0) or **RustFS** (Apache-2.0, 1.0 GA 2026-09-16, young); any S3/R2/GCS in production. Object Lock needed for WORM (AUD-04): verify per store. |
| Cache / rate limit | **Redis 8+**: RSAL/SSPL/AGPL tri-licence | **Valkey** (BSD-3) shipped as a standard component. Customers may use Redis ≤7.2, Redis 8 (unmodified, their choice) or a managed service. Never authoritative ([ADR-0019](adr/ADR-0019-valkey-redis-cache.md)) |
| Primary database | MariaDB lacks RLS, partial indexes, transactional DDL, jsonb/GIN, LISTEN/NOTIFY, ltree | **PostgreSQL 16+** ([ADR-0002](adr/ADR-0002-postgres-system-of-record.md)) |
| Broker | Kafka Apache-2.0; NATS Apache-2.0 (relicensing attempt settled 2025-05-01, stays CNCF); Redpanda BSL | **Postgres outbox by default**; NATS JetStream or Kafka adapters for scale ([ADR-0009](adr/ADR-0009-broker-optional.md)) |
| Search | ParadeDB pg_search AGPL; Meilisearch MIT+BUSL; Tantivy MIT | **Postgres FTS + pg_trgm**; optional embedded Tantivy |
| Authorization | Cedar Apache-2.0 (Rust, 4.13); OpenFGA/SpiceDB Apache (Go services); Oso OSS deprecated; Casbin-rs (ASF incubating) | **Cedar embedded** + RLS ([ADR-0011](adr/ADR-0011-authorization-engine.md)) |
| IdP (optional) | Keycloak Apache-2.0; Ory Apache-2.0; Zitadel **AGPL since v3**; Authentik MIT + enterprise dir | Built-in local auth (standalone). OIDC RP in Rust (`openidconnect`, MIT); SAML via IdP broker; **Keycloak** reference IdP |
| SAML in Rust | `samael` 0.0.22, pre-1.0, needs libxmlsec1 | Avoid in-process XML-DSig |
| SCIM | `scim_v2` types (young) | Own SCIM endpoints on those types |
| Secrets | OpenBao (MPL-2.0) / Vault (BSL) / cloud KMS | KeyProvider adapters; OpenBao reference (06) |
| Malware scan | ClamAV (GPL-2.0; separate daemon process, not linked) | Optional sidecar via clamd protocol |
| Backups | pgBackRest (MIT) | Reference (06) |
| Load testing | k6 (AGPL-3.0) | Dev/CI only, never shipped |
| Mining | pm4py **AGPL-3.0** | Not bundled; we export XES/OCEL |

**Licence policy** (enforced by `cargo-deny` + JS licence checker in CI): shipped artefacts
may contain only permissive licences (Apache-2.0, MIT, BSD, ISC, Zlib, MPL-2.0 unmodified,
EPL-2.0 unmodified as separate module). AGPL/GPL/SSPL/BSL/BUSL/SUL/Commons-Clause are
**blocked** from linked code; GPL tools may run as separate optional processes (ClamAV).

## 5. Framework alignment for templates

- APQC PCF: taxonomy usable royalty-free with attribution. ⚠ Exact current licence text is
  pending legal review.
- ITIL 4: generic practice names only; no copied text.
- COBIT and SCOR: licence restrictions, so **not used**.
- ISO standards: clause references only.

Detail is in [19-PROCESS-TEMPLATE-LIBRARY](19-PROCESS-TEMPLATE-LIBRARY.md).

## 6. Market findings (summary of [10](10-COMPETITIVE-ANALYSIS.md))

No product unifies governance (catalog/SOP/policy/attestation) and execution (no-code
workflows/cases) in OSS. The OSS BPM field emptied in 2024–2026:
- Camunda 7 CE reached EOL.
- Camunda 8 became source-available.
- Bonita community binaries stopped.
- ProcessMaker merged into Decisions.
- n8n is fair-code, not open source.

Text-to-process AI is table stakes. The differentiator is *governed* AI.

## 7. Architecture Decision Records

Full ADRs are in [`adr/`](adr/). Status after this research:

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/ADR-0001-governance-execution-separation.md) | Separate governance/catalog from execution; link by immutable version id | Accepted (pending G1) |
| [0002](adr/ADR-0002-postgres-system-of-record.md) | PostgreSQL 16+ is the system of record; MariaDB rejected (no RLS/partial indexes/jsonb) | Accepted |
| [0003](adr/ADR-0003-versioned-ir.md) | Canonical, versioned workflow IR ([11](11-WORKFLOW-IR-SPEC.md)) | Accepted |
| [0004](adr/ADR-0004-step-builder-first.md) | Step builder default; BPMN as optional view + import/export | Accepted |
| [0005](adr/ADR-0005-outbox-idempotency.md) | Transactional outbox/inbox + idempotency keys; at-least-once effects | Accepted |
| [0006](adr/ADR-0006-ai-human-gate.md) | AI outputs are drafts; never approve/sign/publish; no people-scoring | Accepted |
| [0007](adr/ADR-0007-modular-monolith.md) | Modular monolith + separate worker/scheduler processes | Accepted |
| [0008](adr/ADR-0008-workspace-tenancy.md) | **Workspace = tenant**; RLS + per-workspace keys; dedicated DB tier | Accepted (revised per product direction) |
| [0009](adr/ADR-0009-broker-optional.md) | Broker optional: Postgres outbox default; NATS/Kafka adapters | Accepted |
| [0010](adr/ADR-0010-expression-language.md) | Typed, total expression language: FEEL subset (dsntk) vs CEL, decided by SP-08 | Proposed |
| [0011](adr/ADR-0011-authorization-engine.md) | Cedar embedded, data-driven custom roles, fixed `forbid` invariants | Proposed (SP-09) |
| [0012](adr/ADR-0012-release-bundle.md) | Release bundle pins every dependent artifact version | Accepted |
| [0013](adr/ADR-0013-sop-content-model.md) | Structured block model for SOP/policy content; PDF/DOCX export | Accepted |
| [0014](adr/ADR-0014-licensing-and-contribution.md) | Apache-2.0, DCO, permissive-only dependency policy, trademark kept | Proposed (OQ-06) |
| [0015](adr/ADR-0015-runtime-selection.md) | Runtime: native interpreter primary; Temporal/Operaton spikes as fallback | Proposed (decided at G1 by SP-01…06) |
| [0016](adr/ADR-0016-standalone-first.md) | Standalone first: zero-integration P0; integrations via ports later | Accepted (product direction) |
| [0017](adr/ADR-0017-unified-campaign-engine.md) | One acknowledgement & signature campaign engine for SOPs, policies, announcements, documents, attestations | Accepted |
| [0018](adr/ADR-0018-ui-stack-and-canvas.md) | React + TS; React Flow canvas; diagram-js (MIT) for BPMN view; no bpmn-js | Accepted |
| [0019](adr/ADR-0019-valkey-redis-cache.md) | Valkey/Redis-protocol cache: standard, never authoritative | Accepted |

## 8. Mandatory validation spikes (before G1)

| Spike | Question | Pass condition |
|---|---|---|
| SP-01 | 10 000 concurrent waiting approvals; kill all workers | 0 lost transitions after restart |
| SP-02 | Double-approval race | Exactly one accepted transition |
| SP-03 | Parallel join, timer, cancellation, escalation semantics | Conformance traces match |
| SP-04 | Repeated webhook/connector deliveries | No duplicate effect |
| SP-05 | 30-day dormant instance after definition update | Completes on pinned version |
| SP-06 | BPMN round-trip of supported subset | Report lists every unsupported element |
| SP-07 | Full licence/SBOM review incl. UI, icons, fonts | No blocked licence in artefacts |
| SP-08 | Expression language: dsntk FEEL subset vs CEL (type-checking, budget, perf, error messages) | Chosen engine passes IR expression suite |
| SP-09 | Cedar + RLS + sqlx pool: custom-role compile, partial eval for list filters, `set_config` per tx | 0 leaks in randomized cross-workspace test; list P95 within budget |
| SP-10 | Audit chain at 10 M events: append contention, explorer queries | Append ≥ 2 000 ev/s/workspace with batching; explorer P95 < 2 s |

Details are in [15-TEST-STRATEGY](15-TEST-STRATEGY-AND-CONFORMANCE.md).
