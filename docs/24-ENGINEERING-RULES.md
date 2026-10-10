# 24 — Engineering rules (normative)

Status: **Binding** · Owner: Tech lead · Applies to every contributor, human or AI agent.

These rules are how Procera stays consistent with its design. **MUST** and **MUST NOT** are hard rules: CI or review rejects violations. **SHOULD** is expected unless a reason is written in the PR. To change a rule, open a PR to this file and get maintainer approval ([GOVERNANCE](../GOVERNANCE.md)).

---

## 1. Design-first: the system design is the contract

1.1 The documents in `docs/` are the **source of truth**. The order of authority when sources disagree is:

| Rank | Source | Notes |
|---|---|---|
| 1 | Accepted ADRs (`docs/adr/`) | Highest authority |
| 2 | LLD (04) and IR spec (11) | |
| 3 | Access model (12), security (06), API (08) | |
| 4 | HLD (03) | |
| 5 | PRD (02) and feature catalogue (21) | The catalogue wins on priority |
| 6 | UX (05) | |
| 7 | Code | Lowest. Code that disagrees with a doc is a bug in one of them, and the PR MUST say which. |

1.2 Every change that **alters behaviour, data shape, an API, a security property or a UX pattern** MUST update the relevant doc **in the same PR, or in an earlier one**. Behaviour that is not in the docs MUST NOT be merged.

1.3 Every PR MUST link:
- a tracker id (`BPMS-…` or a domain id: `TPL/UX/INT/CFG/AI`)
- the requirement ids it implements (`FR-…`, `FX-…`, `ESIG-…`, …)
- the doc sections it follows

1.4 If the spec is silent or ambiguous, **stop**. Add an entry to [22](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md) and ask. Never invent product behaviour. (This rule applies with extra force to AI agents.)

1.5 An architectural change needs a **new ADR**, or one that supersedes an old one. The change is architectural if it adds a store or broker, changes the tenancy, authz or runtime model, adds a process type, touches the crypto or audit design, or adds a new external dependency class. ADRs are never deleted. They are marked `Superseded by ADR-NNNN`.

1.6 New features enter through the catalogue change process ([21 §17](21-FEATURE-CATALOGUE.md#17-change-process)) before any code is written.

1.7 Requirement ids, scenario ids and test ids are **stable**. New ids are appended, and old ids are never renumbered or reused.

## 2. Tracker rules

2.1 The tracker is **generated**:
- **Sources:** the `CORE` list in [`scripts/build_tracker.py`](../scripts/build_tracker.py), plus the backlog tables inside docs 06, 13, 15, 16, 19, 20, 22 and 23.
- **Outputs:** [17](17-EXECUTION-TRACKER.md), `trackers/Delivery_Tracker.xlsx` and `trackers/*.csv`. These MUST NOT be hand-edited.

2.2 Each PR that starts, finishes, blocks or re-scopes an item MUST, in the same PR:
- update the item's **Status** (`Not started · In progress · Blocked · In review · Done · Deferred`), and its release if that moves;
- run `python3 scripts/build_tracker.py`;
- commit the regenerated outputs.

CI checks that the outputs are up to date: it regenerates them and fails on diff.

2.3 An item may be marked `Done` only when:
- its acceptance criterion is met;
- its E2E scenario is green ([23](23-E2E-TEST-PLAN.md));
- the docs are updated;
- and it is merged to `main`.

2.4 `Blocked` MUST name the blocker (an item id or OQ id) in the Depends/Notes column. `Deferred` MUST name the target release and the reason.

2.5 New work discovered during implementation is added as a new tracker row in the same PR. It is not done silently.

2.6 Releases (P0, R1…R5, GA, POST) come from [07](07-ROADMAP-AND-MILESTONES.md). Moving an item between releases needs product-owner approval, recorded in the PR.

2.7 Weekly: the tech lead reviews `Blocked` items and the burndown sheet. Per release: the exit checklist in 07 §2, the register (22) and the risks are re-scored.

## 3. Architecture & code rules

3.1 **Module boundaries** follow [04 §1](04-LLD.md):
- `domain` depends only on `kernel`.
- `application` depends on `domain` and ports.
- `infra` implements the ports.
- `apps` wire everything together.

A context MUST NOT write another context's tables. Cross-context reactions go through application services or outbox events.

3.2 **Tenancy:**
- Every workspace-scoped table MUST have `workspace_id`, a composite primary key, composite foreign keys including `workspace_id`, and **forced RLS**.
- Every repository function MUST take `WsCtx`.
- Every transaction MUST call `set_config('app.workspace_id', $1, true)`.
- Workspace ids MUST come from the authenticated context, never from request input.

3.3 **Transactions:** a state change, its domain events, the outbox row and the audit append MUST commit in **one** transaction. External side effects (email, HTTP, webhooks) MUST happen only after commit and MUST carry an idempotency key.

3.4 **Authorization:** every handler, job, deep link, list, search and export MUST call the policy evaluator (Cedar). Hand-rolled permission checks are forbidden. Redaction MUST use the single shared serializer.

3.5 **Audit:** every mutating endpoint MUST emit exactly one audit event, and every read of `restricted`/`confidential` data by a non-participant MUST emit a `data_access` event. The audit-coverage contract test enforces both.

3.6 **Runtime determinism:**
- The interpreter MUST NOT perform I/O, read the wall clock or use randomness. Use the injected `Clock` and `IdGen`.
- Expressions MUST be budgeted.
- Any change to the IR requires a spec update (11), a `schemaVersion` bump with a migration function, and conformance fixtures.

3.7 **APIs** follow [08](08-API-SPEC.md):
- `/v1` evolves additively only.
- Mutating commands require `Idempotency-Key`, and updates require `If-Match`.
- Errors are RFC 9457.
- Lists use cursors.
- Unknown fields are rejected.

The OpenAPI document is generated from code and MUST be regenerated in the PR.

3.8 **Database:**
- PostgreSQL 16+ only (ADR-0002).
- Migrations are forward-only, **expand → migrate → contract** across at least one release, and live in `migrations/`.
- Every migration MUST be reversible in effect (a contract step is only allowed after a release ships the expand step).
- New queries on hot paths MUST include an `EXPLAIN` note in the PR.
- No SQL string concatenation. Use bound parameters only (`sqlx` macros or `QueryBuilder`).

3.9 **Cache** (Valkey/Redis, ADR-0019) is advisory. Code MUST work with the cache disabled. Jobs, timers, the outbox, idempotency and audit MUST NOT live in the cache.

3.10 **Errors and panics:** no `unwrap()`/`expect()` in non-test code, except proven-unreachable cases with a comment. Errors map to typed problem codes. Logs MUST NOT contain secrets, tokens, restricted values or form contents.

3.11 **Style:**
- `cargo fmt` and `clippy -D warnings`.
- TypeScript `strict`, ESLint and Prettier.
- Names follow the [glossary](18-GLOSSARY.md) code names.
- Comments explain *why*, not what.

3.12 **Dependencies:** each new dependency MUST be justified in the PR and pass the licence gate. Permissive licences only (ADR-0014). Blocked: AGPL, GPL (linked), SSPL, BSL/BUSL, SUL, Commons-Clause, and bpmn.io-watermark packages. Pin versions, and keep lockfiles committed.

## 4. Testing rules

4.1 Every feature PR MUST include:
- unit tests;
- integration tests against real Postgres (testcontainers) for any data access;
- **an E2E scenario added or extended** in `e2e/packs/<release>/`, with its id from [23](23-E2E-TEST-PLAN.md) referenced in the PR.

4.2 Runtime, access and audit changes additionally need property/model-based tests ([15 §5](15-TEST-STRATEGY-AND-CONFORMANCE.md)), and the conformance suite MUST stay at 100 %.

4.3 E2E tests:
- use role- or label-based locators;
- never use fixed sleeps (use `__test/drain` and `__test/clock`);
- seed through the API;
- include the two-workspace isolation sweep.

4.4 Tests MUST NOT be skipped, weakened or deleted to make CI green. A flaky test goes to the quarantine lane with an owner and a 5-day fix window ([23 §7](23-E2E-TEST-PLAN.md#7-flake-policy)).

4.5 Coverage is not a target in itself. However, untested branches in `runtime/`, `access/`, `audit/` and `infra/pg` block review.

## 5. Security rules

5.1 Follow [06](06-SECURITY-AND-OPERATIONS.md) protocols exactly:
- TLS
- session cookies
- CSRF
- OIDC with PKCE
- Argon2id
- WebAuthn/TOTP
- token formats
- the key hierarchy
- Standard Webhooks
- the SSRF egress proxy

Do not invent cryptography or token formats.

5.2 Secrets live only in the configured KeyProvider or secret store. They MUST NOT be in code, IR, logs, fixtures (except generated test keys) or the cache.

5.3 Any PR touching auth, sessions, crypto, RLS, the Cedar compiler, audit, file serving or connectors gets the `security-review` label and a security reviewer.

5.4 Test-only endpoints (`/__test/*`) MUST be behind the `e2e` cargo feature. The release pipeline verifies they are absent.

5.5 Vulnerabilities are handled through [SECURITY.md](../SECURITY.md) and never through public issues.

## 6. UX & accessibility rules

6.1 UI MUST follow the design system and tokens in [05 §7](05-UX-AND-DESIGN.md):
- WCAG 2.2 AA;
- body text ≥ 16 px;
- targets ≥ 24×24 px;
- contrast ≥ 4.5:1 for text and ≥ 3:1 for UI;
- visible focus;
- reflow at 320 px.

The **no-AI-look anti-pattern list** is binding: no gradients, glassmorphism, card-grid "bento" layouts or sparkle decorations.

6.2 Every list uses the shared DataGrid. Every destructive or bulk action shows an impact preview. Every screen defines its empty, loading, error and no-permission states.

6.3 Use only accessible primitives (React Aria). Every interaction has a keyboard path, including the designer canvas and signature-field placement.

6.4 UI copy uses glossary terms through i18n keys. No hard-coded strings.

## 7. Git, PR and commit rules

7.1 `main` is always releasable. Work happens on short-lived branches named `<type>/<tracker-id>-<slug>` (e.g. `feat/BPMS-046-campaign-engine`).

7.2 Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `perf:`, `security:`) and reference the tracker id. Every commit MUST be **DCO signed-off** (`git commit -s`).

7.3 PRs:
- one tracker item each;
- ≤ ~400 changed lines (excluding generated files);
- use the PR template;
- all CI gates green;
- at least 1 maintainer approval, or **2** for protected areas: `crates/domain/{runtime,access,audit}`, `crates/infra/{pg,kms}`, migrations, auth and crypto, enforced via CODEOWNERS.

7.4 Squash-merge to `main`. No force-push to `main`. Release tags are signed.

7.5 Commits and PRs MUST NOT include AI or assistant attribution lines (`Co-Authored-By` for AI tools, "Generated with…" footers). The human who opens the PR is accountable for its contents.

## 8. Documentation rules

8.1 Doc numbering is stable (`NN-TITLE.md`). New docs take the next number and are added to [00-INDEX](00-INDEX.md).

8.2 Every doc starts with `Status · Owner`. Docs that cite external facts carry an as-of date and source links. Unverified claims are marked ⚠ and tracked in 22.

8.3 Claims must be honest. Never write "compliant", "certified", "BPMN-conformant" or "legally binding". Use "supports", "-ready" or "subset" ([14](14-DATA-GOVERNANCE-AND-COMPLIANCE.md)).

8.4 `docs/archive/` is historical and is not edited, except for legal or brand scrubs approved by maintainers.

## 9. Release rules

9.1 A release ships only when every item in its exit checklist ([07 §2](07-ROADMAP-AND-MILESTONES.md)) and every always-on gate is green:
- its E2E pack and all earlier packs;
- the isolation sweep;
- audit coverage;
- a11y;
- the licence/SBOM check;
- the upgrade test from the previous release;
- 0 open critical or high security defects.

9.2 Artefacts are signed (cosign), with an SBOM (CycloneDX) and provenance. Release notes include the E2E report and the upgrade guide.

9.3 Versions: R1 = `0.1.0` … R5 = `0.5.0`, GA = `1.0.0`. Semantic versioning applies from GA.

## 10. Rules specific to AI coding agents

10.1 Read `CLAUDE.md`/`AGENTS.md` and this file before starting. Re-read the doc sections linked from the tracker item.

10.2 Work on one tracker item per session or branch. Don't expand scope. Don't refactor outside the item without a separate tracker row.

10.3 When unsure, stop and write the question into 22. Never guess product behaviour, security behaviour or legal wording.

10.4 Never disable, skip or weaken tests, lint rules, CI gates, RLS, authz checks or audit emission. Never add dependencies that fail the licence gate.

10.5 Show evidence in the PR: commands run, test output and E2E ids. Report failures honestly.

10.6 Never commit secrets, real personal data or customer data. Fixtures are synthetic.
