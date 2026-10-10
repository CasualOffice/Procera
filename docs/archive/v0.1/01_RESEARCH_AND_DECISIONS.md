# Research, comparison and architecture decisions

## Primary sources checked (2026-10-09)
- OMG BPMN 2.0 specification: https://www.omg.org/spec/BPMN/2.0/
- OMG DMN: https://www.omg.org/dmn/
- OMG CMMN: https://www.omg.org/cmmn/index.htm
- Flowable OSS overview and docs: https://www.flowable.com/open-source ; https://www.flowable.com/open-source/docs/
- Operaton project and docs: https://github.com/operaton/operaton ; https://docs.operaton.org/docs/documentation/
- Temporal Rust SDK GA announcement: https://temporal.io/blog/build-durable-applications-rust-temporal-rust-sdk-now-generally-available
- Temporal durable execution: https://docs.temporal.io/temporal

## Findings
BPMN models structured processes, DMN decisions, CMMN adaptive cases. These standards are complementary, not interchangeable. Flowable OSS advertises Apache-2.0 BPMN/CMMN/DMN Java engines; product-grade design experiences can be separately licensed. Operaton is an Apache-2.0 continuation of Camunda 7 with engine, REST and operational apps. Temporal offers durable workflow execution and its Rust SDK is GA as of September 2026, but is not a BPMN modeling or governance system.

## Build/buy comparison (provisional; verify exact artifact licenses before adoption)
| Option | Strength | Cost / constraint | Recommendation |
|---|---|---|---|
| Custom Rust engine | Tight control, minimal deployment | High correctness risk; long-tail BPMN semantics | Limit to carefully specified subset |
| Operaton adapter | BPMN maturity, Apache-2.0 engine | JVM runtime; UI customization; separate governance | Prototype for BPMN-heavy customers |
| Flowable OSS adapter | BPMN + CMMN + DMN | JVM; OSS UI and commercial feature boundaries | Prototype for mixed workflows/cases |
| Temporal runtime adapter | Durable execution, Rust SDK | Runtime code/version semantics, not native BPMN | Prototype for automation-heavy workflows |

## Decisions (proposed, pending benchmarks)
ADR-001: Separate governance/catalog from execution; link by immutable version ID.
ADR-002: Use PostgreSQL as system of record; Kafka for asynchronous integration, not transaction commit.
ADR-003: Define a canonical typed process intermediate representation (IR), versioned with migration tools.
ADR-004: Start with user-friendly step builder; provide optional BPMN mode and import/export with explicit compatibility reporting.
ADR-005: Use durable outbox/inbox and per-tenant authorization checks on every API, worker and callback.
ADR-006: Treat AI suggestions as drafts, never publish or execute autonomously without authorization.
ADR-007: Build modular monolith first; separate workers and integration executors for independent scaling.
ADR-008: Tenant isolation at data and policy layers; dedicated tenant DB option for regulated customers.

## Validation spikes (mandatory before engine commitment)
SP-01: 10,000 concurrent waiting approval instances; restart all workers; prove no lost transitions.
SP-02: Double-approval race; assert exactly one accepted state transition.
SP-03: Parallel gateway join, timer, cancellation and escalation semantics.
SP-04: Repeated webhook deliveries and downstream side effects; verify idempotency.
SP-05: 30-day dormant process; definition updated; original instance completes on pinned version.
SP-06: BPMN round-trip import/export of supported elements with unsupported-element warnings.
SP-07: Verify full dependency license/SBOM; specifically check modeler UI, icons, fonts, SDK and connectors.

## Research gaps
No primary-source verification yet of all candidate component release versions, licensing of individual UI packages, production performance, pricing or long-term maintainer health. Do not treat estimates or compatibility claims as benchmark results. Perform repository-level due diligence before dependency lock-in.
