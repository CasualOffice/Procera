# 15 — Test strategy, conformance and quality gates

Status: **Draft for G1 sign-off** · Owner: QA/SDET lead + Runtime lead · Closes gap D8 and part of D6.

The top risks are R1 (execution correctness) and R3 (cross-workspace access), so most of the testing budget goes to them. Unit tests prove each piece works. The **conformance suite, model-based tests and the cross-workspace sweep** prove the product is correct, and those are the gates that block a release.

> **End-to-end tests** have their own plan, with the harness, test-only controls, 74 scenarios per release, CI and flake policy: [23-E2E-TEST-PLAN](23-E2E-TEST-PLAN.md). The milestone ids (M-codes) in this doc map to releases as described in [07 v0.3](07-ROADMAP-AND-MILESTONES.md).

## 1. Test pyramid and ownership

| Layer | Tooling | Scope | Owner | Runs |
|---|---|---|---|---|
| Unit | `cargo test`, Vitest | Pure domain logic, IR compiler, expression evaluator, redaction, policy compilation | Feature author | Every push |
| Property / model-based | `proptest`, state-machine models | Runtime state machines, compiler invariants, filter→SQL equivalence | Runtime lead | Every push (short), nightly (long) |
| Integration | `testcontainers` (Postgres 16/17, S3-compatible store, Valkey, NATS) | Repositories, RLS, outbox, scheduler, file pipeline, auth flows | Feature author | Every PR |
| Conformance | `crates/runtime/conformance` | IR fixtures → expected event traces, for every runtime adapter | Runtime lead | Every PR touching runtime; nightly for all adapters |
| Deterministic simulation | In-house harness (§5) | Whole runtime under injected faults, seeded | Runtime lead | Nightly (10 k seeds), PR (200 seeds) |
| Contract | Generated from route table + OpenAPI | Audit coverage, authz matrix, error model, pagination | Platform lead | Every PR |
| Security | Generated sweeps + targeted tests + DAST | SEC-01…SEC-30 | Security lead | Every PR (sweeps), weekly (DAST) |
| E2E UI | Playwright | Journeys J0–J11, keyboard-only flows | QA/SDET | Every PR (smoke), nightly (full) |
| Accessibility | axe-core in Playwright, manual audits | WCAG 2.2 AA | Design + QA | Every PR (axe), per milestone (manual) |
| Visual regression | Playwright screenshots (light/dark, 360/1280 px, pseudo-locale) | Design system and key screens | Frontend lead | Every PR |
| Performance / load | k6 (see §10), `criterion` microbenchmarks | Reference load, audit query, grid queries | Platform lead | Nightly + before each milestone gate |
| Chaos / recovery | Compose/K8s fault injection scripts | v0.1 recovery list (§8) | DevOps | Weekly + before G3/G4 |
| Upgrade / migration | N-1 → N harness | Schema + live instances | Platform lead | Every release candidate |

Rule: a bug fix ships with a test at the lowest layer that reproduces it. Runtime bugs also add a conformance fixture or a simulation seed.

## 2. Unit and integration testing conventions

- **Injected seams.** `Clock`, `IdGenerator`, `PolicyEvaluator`, `ObjectStore`, `Mailer`, `SecretProvider` and `RuntimeAdapter` are traits. Production wiring is the only place real implementations appear. Tests never sleep: time is advanced on a fake clock.
- **Real Postgres, never mocks**, for anything touching SQL. Testcontainers start one container per test binary. Each test runs in its own database created from a migrated template (`CREATE DATABASE … TEMPLATE`), which keeps tests parallel and isolated.
- The app connects as the **non-owner app role** in integration tests, exactly as in production, so RLS is exercised everywhere and not only in dedicated tests.
- Fixture builders (`WorkspaceBuilder`, `ProcessBuilder`, `InstanceBuilder`) create valid, policy-consistent data. No hand-written SQL inserts in tests.
- Snapshot tests (`insta`) cover compiler diagnostics, semantic diffs, canonical IR serialization and API problem-detail bodies.

## 3. Spikes as executable tests (Phase 0 / M0–M1)

The v0.1 spikes are kept and turned into permanent tests, so a spike that passes becomes a regression test.

| Spike | Question | Pass condition | Becomes |
|---|---|---|---|
| SP-01 | 10 000 concurrent waiting approval instances; restart all workers | 0 lost or duplicated transitions; all instances resumable | T11 (nightly) |
| SP-02 | Double-approval race | Exactly one accepted transition, loser gets 409 | T01 |
| SP-03 | Parallel join, timer, cancellation, escalation semantics | Matches conformance traces | Conformance groups `parallel`, `timer`, `cancel` |
| SP-04 | Repeated webhook deliveries / connector side effects | One effective downstream effect per idempotency key | T08, T21 |
| SP-05 | 30-day dormant instance; definition republished | Completes on pinned bundle | T03 |
| SP-06 | BPMN round-trip of supported elements | Supported elements round-trip; unsupported ones are reported by id | T22 |
| SP-07 | Full dependency licence / SBOM | No disallowed licence in shipped artefacts (`cargo-deny`, npm licence check) | CI gate G-LIC |
| **SP-08** | Expression language: FEEL subset (dsntk) vs CEL | Both evaluated on 300 shared golden expressions; choose on correctness, totality (budget enforcement), error quality, typing support, licence | ADR-0010; golden set kept as T23 |
| **SP-09** | RLS with pooled connections (sqlx + `set_config(…, true)`, PgBouncer transaction mode) | 0 cross-workspace rows under 64-way concurrent mixed-tenant load for 1 h; missing context returns 0 rows | T04, T24 |
| **SP-10** | Audit explorer at 10 M events per workspace (100 M total) | P95 < 2 s for indexed filter combinations; append throughput ≥ 2 000 events/s with hash chaining | T25 |

## 4. Runtime conformance suite

Location: `crates/runtime/conformance/`. It is the definition of runtime correctness and the R1 mitigation.

**Fixture format**

```
fixtures/<group>/<case>/
  process.ir.json        # normalized IR v1 (11-WORKFLOW-IR-SPEC)
  bundle.json            # forms, rules, calendars referenced
  script.yaml            # ordered stimuli: start, complete(work, outcome), fire_timer, signal, fail(job), cancel, crash_at(step), advance_clock
  expected.trace.jsonl   # expected domain events (types, node ids, outcomes, ordering constraints)
  expected.final.json    # final instance/token/work-item states
```

- Traces are compared **semantically**. Event ids and timestamps are normalized. Partial-order constraints are allowed for concurrent branches (`"after": [...]` rather than a total order).
- Every runtime adapter (the native interpreter, and any engine adapter that survives M1) implements a `ConformanceDriver` and must pass 100 % of the groups it claims to support. Unsupported groups must be declared and must be rejected at compile time for that adapter.
- Groups cover: `sequence`, `decision` (incl. default), `parallel` (structured split/join, nested), `loop` (bounded, iteration limit), `human` (claim, delegate, quorum one/all/n-of-m, SoD exclusion), `escalation`, `timer` (absolute, duration, business calendar, DST boundary), `signal` (correlation, timeout), `service` (ok/error/typed error, retry, idempotency key), `boundary` (interrupting, non-interrupting, cancel), `subprocess` (pinned), `terminate`, `cancel`, `suspend_resume`, `incident` (failure → incident → retry/skip), `version_pinning`, `crash` (crash injected at each transaction boundary).
- Target at M5 is ≥ 400 fixtures. Every runtime bug adds one.

**As built (BPMS-082):** the first 22 fixtures live in `conformance/runtime/<group>/<case>/` with `script.json` in place of `script.yaml` (A-23). The native interpreter is deterministic, so its traces are compared in total order. See [11 §15](11-WORKFLOW-IR-SPEC.md#15-as-built-bpms-082).

## 5. Model-based, property and formal testing

**Executable models (proptest state machines).** Simplified reference models of the instance state machine, the token set and the work-item lifecycle (04-LLD §4) are written as pure Rust. `proptest-state-machine` generates random command sequences (start, decide, delegate, cancel, timer fire, crash, retry, duplicate request) and runs them against both the model and the real runtime on Postgres, asserting after every step that:
- states match;
- no illegal transition was accepted;
- the revision increments by exactly 1 per accepted transition;
- every event sequence number is gap-free per instance;
- each idempotency key maps to exactly one response.

**Other properties**
- Compiler: any IR that compiles re-serializes canonically to identical bytes, and the checksum is stable.
- Migrations `vN→vN+1` are total on generated valid IR.
- Expressions: evaluation is deterministic, budget-bounded and never panics on generated inputs. The type-checker is sound with respect to the evaluator (well-typed expressions don't hit type errors at runtime).
- Policy → SQL partial evaluation is equivalent to per-row evaluation on random data (12-AUTHZ §12).
- Data-grid filter AST → SQL is equivalent to an in-memory reference filter (§9).
- Redaction parity: API, export, search and notification serializers produce the same visible field set.

**Protocol verification.** The parallel-join barrier and the timer lease/fencing protocol are the two places where concurrency bugs hide. Both are modelled in **Stateright** (Rust, model-checks the actual protocol code paths), with an optional TLA+ spec kept in `docs/specs/` if the team prefers it for review. The properties checked are:
- *join releases exactly once*;
- *no timer fires twice with effect*;
- *an expired lease never lets a stale worker commit* (fencing token monotonic).

## 6. Deterministic simulation harness

Inspired by FoundationDB-style simulation, it runs the runtime, scheduler, outbox publisher and a fake connector in one process:

- **Injected time and ids:** a seeded fake clock advances only when the simulator allows it, and ids come from a seeded generator.
- **Fault injection:** transaction aborts, serialization failures, connection drops, worker crash before/after commit, duplicate delivery, delayed delivery, reordering across instances, clock skew between workers, lease expiry mid-job.
- **Workload generator** built from conformance IRs and template fixtures.
- **Invariants checked continuously:**
  - no lost or duplicated transitions;
  - outbox eventually drains;
  - each idempotent side effect is applied at most once at the connector;
  - timers fire within lateness bounds;
  - no cross-workspace row ever visible.
- **Seed replay:** any failing seed is printed, rerunning with `SIM_SEED=<n>` reproduces it bit-for-bit, and failing seeds are committed to `sim/regressions/`.
- Budget: 200 seeds per PR, 10 000 nightly, 100 000 before G3.

## 7. Concurrency and race tests

These are real-Postgres, multi-connection tests using barriers to line up operations:
- double decision (T01);
- decide vs reassign;
- decide vs cancel;
- decide vs delegation expiry (T05);
- parallel branches completing simultaneously at a join (T12);
- timer fire vs manual completion (T13);
- publish vs concurrent draft edit (ETag 412);
- concurrent role change vs in-flight decision (SoD re-check, T14);
- concurrent file link vs retention deletion (T30).

Each runs at least 1 000 iterations nightly.

## 8. Chaos and recovery

The v0.1 list is kept and automated:
- kill the API during commit;
- kill a worker after the external call but before ack;
- duplicate broker message;
- DB failover (Patroni/CloudNativePG switchover);
- lost broker connectivity;
- outbox replay;
- clock skew;
- expired leases;
- missing assignee;
- revoked employee;
- rotated integration credential;
- corrupted attachment;
- restore from snapshot + WAL (T09);
- 30-day idle instance recovery (T03).

Each scenario has a script, an expected outcome and a measured recovery time. RPO/RTO are rehearsed for G4 (T09, T31).

## 9. Security testing

The test IDs SEC-01…SEC-30 are defined in [06-SECURITY-AND-OPERATIONS](06-SECURITY-AND-OPERATIONS.md). Mechanisms:

1. **Generated cross-workspace endpoint sweep.** Every route in the router is enumerated at test time. For each route, objects are created in workspace B and requested with a valid workspace-A token. Expected result: `404` (or `403` only on routes that never reveal existence), plus an `authz.denied` audit event. A route without a sweep entry fails the build, so new endpoints can't skip the check (SEC-01/02).
2. **Audit-coverage contract test.** Every mutating route (POST/PUT/PATCH/DELETE), and every read route tagged `sensitive_read`, is exercised. Exactly one audit event of the declared action type must be emitted, with actor, object, workspace, reason (if required) and trace id. Zero or two events both fail (FR-T01).
3. **Authorization matrix generation.** From the permission catalogue × built-in roles × scopes (12-AUTHZ §5–6), expected allow/deny tables are generated and asserted per action. Custom-role escalation attempts are generated as well: a role editor trying to add a permission they don't hold must be denied.
4. **RLS tests:** a missing `app.workspace_id` returns zero rows; the app role cannot `SET ROLE`, has no `BYPASSRLS`, and `FORCE RLS` is on for every tenant table (checked by a catalog query in CI).
5. **Targeted tests:**
   - field-level redaction (SEC-03);
   - revoked identity (SEC-04);
   - webhook replay (SEC-05);
   - SSRF to private, metadata and IPv6-mapped ranges and via DNS rebinding (SEC-06);
   - signed-URL expiry and workspace binding (SEC-10);
   - session fixation and rotation;
   - CSRF on cookie-auth routes;
   - refresh-token reuse detection;
   - step-up enforcement;
   - break-glass visibility;
   - Separation-mode enforcement;
   - EICAR quarantine;
   - zip-bomb and polyglot uploads;
   - SVG/HTML active-content preview blocked;
   - unsafe BPMN/script import rejected (SEC-09).
6. **Tooling:** `cargo-audit` and `cargo-deny`, npm audit, Semgrep, gitleaks, Trivy image scans, OWASP ZAP baseline DAST weekly against staging, and fuzzing (`cargo-fuzz`) of the IR parser, expression parser, BPMN importer and multipart upload parser.
7. **External pentest** before GA (G4) against OWASP ASVS 5.0 L2, with L3 for auth, session and crypto.

## 10. UI, accessibility and performance testing

**Playwright E2E.** One spec per PRD journey, J0–J11 (02-PRD §6), runs on Chromium, Firefox and WebKit nightly and on Chromium per PR. Every journey also has a **keyboard-only variant** with no pointer events, which closes T10 and A11Y 2.5.7 for the designer. A mobile-viewport (360 px) variant covers My Work, decisions and acknowledgements (FR-P07).

**Accessibility**
- axe-core runs on every page visited by E2E, and any new serious or critical violation blocks the PR.
- Manual screen-reader passes (NVDA + Firefox, VoiceOver + Safari) at M6 and M10.
- A WCAG 2.2 AA audit by an external party before GA, with the ACR/VPAT produced from it.

**Visual regression** covers design-system components and key screens in light and dark themes, at 360 and 1280 px, and in the pseudo-locale.

**Data-grid parity tests** (FR-V01–V09). For each list, generated filter/sort combinations are checked so that:
- the API result equals the in-memory reference;
- CSV/XLSX export equals the grid rows and columns, with identical redaction;
- the saved-view URL round-trips;
- unallowlisted fields are rejected with a 400 problem detail.

**Performance and load.** Uses **k6**, which is AGPL-3.0. It is a developer and CI tool only: it is never linked into or shipped with the product, so there are no distribution obligations. The reference load from 02-PRD §7 is 100 workspaces, 10 k waiting instances, 100 transitions/s burst and 1 M historical events. Scenarios:
- inbox browsing;
- decision bursts;
- request start;
- grid filtering on 1 M rows;
- audit explorer on 10 M events (T25);
- outbox lag;
- timer storms (10 k timers due in the same minute).

Results are stored per build. A > 10 % P95 regression against the baseline fails the nightly run and is reported to the PR author.

## 11. Upgrade and migration testing

- **N-1 → N harness:**
  1. Deploy release N-1.
  2. Load a fixture dataset: workspaces, published versions, waiting instances (human, timer, signal), open incidents, files, audit chain.
  3. Upgrade to N with the migration tool, with the app running (expand/contract migrations, ADR on zero-downtime).
  4. Assert that every waiting instance completes on its pinned bundle, the audit chain still verifies, files still resolve, and there are no RLS regressions.
- **Rollback rehearsal:** the contract phase is reversible until it is finalized.
- **IR schema migrations:** generated IR for v1 → v2 round-trip properties (§5).

## 12. Template fixture tests

Every template in the global gallery ([19-PROCESS-TEMPLATE-LIBRARY](19-PROCESS-TEMPLATE-LIBRARY.md)) ships `test-run` fixtures. Template publication (FR-S06) and every CI run require all of these to hold:
- the template compiles lint-clean against IR v1;
- install with default role mapping succeeds;
- every fixture path (happy path, rejection, escalation, timeout) test-runs green;
- forms pass axe;
- all locale bundles are complete.

## 13. Localisation tests

- A **pseudo-locale** (`en-XA`: accented, 40 % expanded, bracketed) and an RTL pseudo-locale (`ar-XB`) are run through E2E smoke and visual regression to catch truncation, concatenated strings and hard-coded text.
- An ICU MessageFormat lint checks plurals and select arguments.
- Missing keys fail CI.
- Date, time and number formatting is tested across time zones, including DST transitions for business-calendar timers.

## 14. CI gates

| Gate | Blocking on PR | Nightly / scheduled |
|---|---|---|
| G-BUILD | fmt, clippy `-D warnings`, tsc strict, eslint | — |
| G-UNIT | unit + short property tests | long property runs |
| G-INT | integration (testcontainers) | full matrix PG 16/17 |
| G-CONF | conformance (native adapter) | all adapters |
| G-SIM | 200 sim seeds | 10 000 seeds |
| G-SEC | cross-workspace sweep, audit coverage, authz matrix, RLS catalog check, Semgrep, gitleaks, cargo-deny | ZAP DAST, Trivy, fuzzing (1 h per target) |
| G-LIC | cargo-deny licences + npm licence check; SBOM generated (CycloneDX) | — |
| G-UI | Playwright smoke (Chromium), axe, visual regression | full browsers, keyboard-only, mobile |
| G-PERF | criterion microbench (no >10 % regression) | k6 reference load |
| G-MIG | migration lint (no destructive op without expand/contract) | N-1 → N harness |
| G-TPL | template fixtures | — |
| G-I18N | missing keys, ICU lint | pseudo-locale visual |

Flaky tests are quarantined within 24 h with an owner and a 7-day fix SLA. A quarantined test never counts as passing for a release gate.

## 15. Environments

| Env | Purpose | Data |
|---|---|---|
| Local (`docker compose up`) | Dev, all deps in containers | Seed fixtures |
| CI ephemeral | PR checks | Generated |
| `staging` | Nightly full suites, DAST, load | Synthetic (realistic volumes) |
| `pilot` | Internal pilot (M7) | Real internal, with backups and DR drills |
| `perf` | Dedicated, fixed hardware for comparable numbers | Reference dataset |

Production data is never copied into lower environments.

## 16. Quality metrics (reported per milestone)

| Metric | Target |
|---|---|
| Conformance pass rate (native) | 100 % of claimed groups |
| Mutation score on domain + runtime crates (`cargo-mutants`) | ≥ 70 % by M10 |
| Line coverage | Domain/runtime/authz ≥ 85 %; overall tracked, not gated |
| Escaped defects (found in pilot/prod) by severity | 0 critical, ≤ 2 high per milestone |
| Flaky test count | ≤ 5 quarantined at any time |
| Mean time to reproduce a sim failure | < 1 h (seed replay) |
| axe serious/critical violations | 0 |
| P95 latencies at reference load | Within 02-PRD §7 |

## 17. Test matrix (T01–T40)

Milestones follow [07-ROADMAP-AND-MILESTONES](07-ROADMAP-AND-MILESTONES.md): M0 Discovery · M1 Runtime ADR/G1 · M2 Foundations (auth/tenancy/audit) · M3 Library & SOP governance · M4 Admin console + files · M5 Runtime core · M6 Pilot surface (P0 pilot) · M7 Pilot live · M8 Enterprise identity & integrations framework · M9 Cases + investigations + attestations · M10 GA hardening · M11 GA · M12+ expansion.

| ID | Test | Category | Pass condition | Milestone |
|---|---|---|---|---|
| T01 | Duplicate approval (race) | Correctness | Only one transition commits; loser 409 | M5 |
| T02 | Worker killed after commit | Recovery | Outbox eventually delivers exactly once to the consumer (dedup) | M5 |
| T03 | 30-day wait then resume after republish | Durability | Completes on original pinned bundle | M5 |
| T04 | Workspace A reads workspace B | Security | Denied (404) and audited, every endpoint | M2 |
| T05 | Expired delegation used | Security | Decision denied; fails closed | M6 |
| T06 | Timer lease expiry | Recovery | Single effective firing | M5 |
| T07 | Published SOP edited | Governance | Published version immutable; edit creates new draft | M3 |
| T08 | Connector webhook replay | Security | No duplicate effect; replay rejected outside window | M8 |
| T09 | Restore database + objects | Operations | Recovery within RTO ≤ 2 h, RPO ≤ 15 min | M10 |
| T10 | Keyboard-only publish | Accessibility | Design → test run → publish without pointer | M6 |
| T11 | 10 k waiting instances, restart all workers (SP-01) | Durability | 0 lost/duplicated transitions | M5 |
| T12 | Parallel branches arrive at join simultaneously | Correctness | Join releases exactly once | M5 |
| T13 | Timer fires vs manual completion race | Correctness | Exactly one path wins; loser is no-op | M5 |
| T14 | Role/directory change during in-flight decision | Security | SoD re-checked at decision time | M6 |
| T15 | Idempotent request start (same key, same body / changed body) | Correctness | Same response / 409 | M5 |
| T16 | Cross-workspace signed file URL | Security | Rejected outside workspace and after expiry (SEC-10) | M4 |
| T17 | EICAR upload | Security | Quarantined; not downloadable without `file:manage_quarantine` | M4 |
| T18 | Audit coverage contract | Audit | Every mutating route emits exactly one audit event | M2 |
| T19 | Audit chain tamper (modify, delete, reorder) | Audit | Verifier detects each | M2 (chain) / M9 (verifier CLI) |
| T20 | Custom role escalation attempt | Security | Editor cannot grant permissions they lack | M2 |
| T21 | Duplicate service.call execution after crash | Correctness | Single downstream effect per idempotency key | M5 |
| T22 | BPMN import round-trip (SP-06) | Interop | Supported elements round-trip; unsupported ones reported by id | M6 |
| T23 | Expression golden set (SP-08) | Correctness | 100 % golden pass; budget enforced; no panics under fuzz | M1 |
| T24 | RLS under pooled mixed-tenant load (SP-09) | Security | 0 cross-workspace rows in 1 h soak | M1 |
| T25 | Audit explorer 10 M events (SP-10) | Performance | P95 < 2 s indexed filters; ≥ 2 000 appends/s | M4 |
| T26 | Data-grid filter/sort parity | Correctness | API = reference; export = grid incl. redaction | M4 |
| T27 | Admin bulk reassign 500 items | Admin | Per-item result + audit; impact preview count exact | M4 |
| T28 | Workspace suspend → restore | Admin | Writes 423 while suspended; no data loss | M2 |
| T29 | Workspace delete (after grace) | Privacy | Data purged, keys shredded, system record pseudonymised | M10 |
| T30 | File linked to published version: delete attempt | Governance | Refused with reason; retention/hold respected | M4 |
| T31 | DB failover during decision burst | Recovery | No lost accepted decisions; clients retry idempotently | M10 |
| T32 | Break-glass session | Security | Time-boxed, read-only, visible in workspace audit + banner | M8 |
| T33 | Separation mode on | Security | Admin loses content access; self-grant needs second admin | M8 |
| T34 | Part 11 signature without re-auth | Compliance | Rejected; with re-auth: manifestation + record hash stored | M9 |
| T35 | Investigation evidence export | Audit | Signed bundle verifies offline with `procera-verify` | M9 |
| T36 | Template gallery fixtures | Templates | Every Wave-1 template installs + test-runs green | M6 |
| T37 | N-1 → N upgrade with live instances | Operations | All waiting instances complete; chain verifies | M10 |
| T38 | Pseudo-locale + RTL screens | i18n | No truncation/hard-coded strings on key screens | M6 |
| T39 | SSRF via DNS rebinding / metadata IP | Security | Blocked at egress proxy (SEC-06) | M8 |
| T40 | Reference load (100 ws, 10 k waiting, 100 tps burst) | Performance | P95 read < 300 ms, transition < 500 ms | M10 |
