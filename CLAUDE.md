# CLAUDE.md: working contract for AI agents in the Procera repository

**Procera** is an open-source (Apache-2.0), self-hostable, multi-tenant process-governance and workflow product. It covers policies, SOPs, approvals, e-signatures, announcements with acknowledgements, cases, investigations and audit.

The repository is in the **design phase**: `docs/` is the source of truth and there is no product code yet. Other tools read [AGENTS.md](AGENTS.md), which defers to this file.

## 1. Read before any task

1. `docs/24-ENGINEERING-RULES.md`: **binding rules**. Read this first, every session.
2. `docs/00-INDEX.md`: map of the pack and the owner's product direction.
3. `trackers/README.md` (what's active, what's next), the tracker item you're working on (`docs/17`), and every doc section it links.
4. For code: `docs/04-LLD.md`, `docs/11-WORKFLOW-IR-SPEC.md`, `docs/12-AUTHORIZATION-MODEL.md`, `docs/06-SECURITY-AND-OPERATIONS.md`, `docs/08-API-SPEC.md`, `docs/23-E2E-TEST-PLAN.md`.
5. `docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md`, so you know what is **not** decided.

## 2. Product direction (non-negotiable unless the product owner changes it)

- **Standalone first.** P0 and R1–R3 need only PostgreSQL + S3-compatible storage + SMTP (+ the Valkey cache). Integrations (SSO, chat platforms, HRIS, ERP…) go behind ports and ship later.
- **Workspace = tenant.** A System Admin plane manages all workspaces. Users are global; memberships are per workspace.
- **Configurable RBAC per workspace:**
  - a permission catalogue, built-in role templates, and custom roles with scopes;
  - invariants (isolation, SoD, step-up, audit) cannot be configured away.
- **Workspace admins see and control everything** in their workspace:
  - content reads are access-logged;
  - Separation mode is the opt-in exception;
  - system admins never read content without break-glass.
- **Complete audit trail and investigations.** Hash-chained. Every mutation and every restricted read is recorded.
- **Audience: startups → MNCs, every department.** Policies, employee e-signing and announcements with acks are core.
- **UX:**
  - industry-leading, easy yet powerful;
  - must **not** look AI-generated or templated (no gradients, glass, bento card grids or sparkles);
  - WCAG 2.2 AA tokens per `docs/05 §7`;
  - configurable branding with contrast enforcement.
- **Delivery:** phased usable releases P0 → R1 (Jan 2027) … R5 → GA (Oct 2027). Each release is gated by its E2E pack (`docs/07`, `docs/23`).
- **Stack:**
  - **PostgreSQL 16+ only**, not MariaDB (ADR-0002);
  - **Valkey/Redis is a cache only**, never authoritative (ADR-0019);
  - Rust (Axum, sqlx, Cedar), React + TypeScript (React Aria, TanStack, React Flow).
- **Brand:**
  - assets and copy live in `assets/brand/` (tagline: "Turn processes into progress."; pillars Design · Execute · Govern · Improve);
  - brand red `#EE4136` is for logo, focus and active states only. Primary buttons are charcoal, and the danger colour is distinct (`docs/05 §7.1.1`);
  - brand assets are not Apache-licensed.
- **No wild cards.** New features go through the change process in `docs/21 §17`. New uncertainties go into `docs/22`.

## 3. How to work (summary of docs/24)

- **Design first.** Behaviour, data, API, security and UX changes update the docs before or with the code. Architectural changes need an ADR. If the spec is silent, **stop and add a question to docs/22**. Never invent behaviour.
- **Commit directly to `main`.** The workflow is trunk-based and uses no PRs (owner decision). Each commit is one tracker item or one complete slice of it, around 400 changed lines or fewer. Run the local gates before pushing (`docs/24 §7.3`). Watch CI after each push, and fix a red `main` first. Mark protected-area commits `[protected]`.
- **Tracker:**
  - update the status in the `STATUS` map of `scripts/build_tracker.py` (core items), or in the doc's backlog table (domain items);
  - run `python3 scripts/build_tracker.py`;
  - commit the regenerated `trackers/README.md` (dashboard), `docs/17` and `trackers/*`;
  - never hand-edit generated files.
- **Tests with every feature:** unit + integration (real Postgres) + **an E2E scenario** (ids in `docs/23`). Runtime, access and audit changes also need property tests.
- **Never weaken** tests, lint, CI gates, RLS, authz checks or audit emission to get green.
- **Commits:** Conventional Commits, DCO sign-off (`git commit -s`), the tracker id in the message. **No AI/assistant attribution lines.** Commit and push to `main` when an item, or a complete slice of it, is done and green.
- **Honest claims.** Write "supports", "-ready" or "subset". Never write "compliant", "certified" or "conformant".

## 4. Engineering invariants (for when code exists)

- Every workspace table has `workspace_id`, composite PK/FKs and **forced RLS**. Repositories take `WsCtx`. Each transaction calls `set_config('app.workspace_id', $1, true)`. A CI sweep proves cross-workspace access returns 404.
- State change + domain events + outbox + audit append happen in **one transaction**. Side effects only after commit, with idempotency keys.
- Mutations require `Idempotency-Key`; updates require `If-Match`. Errors are RFC 9457. The OpenAPI document is generated from code.
- The Cedar evaluator runs on every handler, job, deep link, list and export. One redaction serializer is shared by API, export, search and notifications.
- The runtime is deterministic: injected `Clock`/`IdGen`, no I/O in the interpreter, budgeted expressions, conformance suite green.
- Test-only endpoints (`/__test/*`) exist only under the `e2e` cargo feature.
- **Protected areas** (runtime, access, audit, `infra/pg`, `infra/kms`, migrations, auth and crypto): commit subject `[protected]`, a second maintainer reviews it after it lands, and property or E2E proof is required.

## 5. Licence gate

Permissive dependencies only (ADR-0014). Never add the following to shipped code:
- AGPL, GPL (linked), SSPL, BSL/BUSL, SUL, Commons-Clause.

Known traps:
- MinIO, Redis 8 (bundled), ParadeDB, Zitadel, Redpanda, Documenso, pm4py.
- **bpmn-js/dmn-js/form-js** (watermark clause). Use diagram-js/bpmn-moddle (MIT) with our own renderer, and never copy bpmn-js code.
- k6 is fine as a dev-only tool.

## 6. Repository map

```text
docs/00–24 + adr/   design pack (24 = engineering rules)
docs/archive/v0.1/  original blueprint (historical; don't edit)
assets/brand/       logos, icons, favicons, banner, palette + brand guide
trackers/           generated tracker outputs
scripts/            build_tracker.py
.github/            PR/issue templates, CODEOWNERS
LICENSE NOTICE GOVERNANCE.md CONTRIBUTING.md SECURITY.md CODE_OF_CONDUCT.md MAINTAINERS.md
```

## 7. Commands

```bash
python3 scripts/build_tracker.py                      # regenerate docs/17 + trackers/*.xlsx/csv (needs: pip install openpyxl)
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo deny check                                      # licence + advisory gate
cargo run -p procera -- serve --all                   # http://127.0.0.1:8080/healthz
```
