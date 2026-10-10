# AGENTS.md

This file is for every AI coding agent: Codex, Cursor, Copilot, Gemini, Claude Code and others. They all follow the **same working contract**. It lives in **[CLAUDE.md](CLAUDE.md)** and the binding **[docs/24-ENGINEERING-RULES.md](docs/24-ENGINEERING-RULES.md)**. Read both in full before doing anything. In short:

1. **The specs are the source of truth.** Start at `docs/00-INDEX.md`. Implement only what a PRD or catalogue id describes (`docs/02`, `docs/21`). If the spec says nothing, add an entry to `docs/22` and stop. Don't invent behaviour.
2. **Commit directly to `main`.** There are no PRs. Each commit is one tracker item, or one complete slice of it, of roughly 400 changed lines or fewer (generated code excepted). The commit body names the tracker id, the spec section and the **E2E scenario id** (`docs/23`). Run the local gates first (`docs/24 §7.3`), and fix a red `main` before anything else.
3. **CI is the referee.** Never merge on red, and never weaken a test or gate to get green.
4. **Protected areas** need a `[protected]` commit subject and a second maintainer's review after the commit lands: `crates/domain/runtime`, `crates/domain/access`, `crates/domain/audit`, `crates/infra/pg` (RLS/migrations), `crates/infra/kms`, plus anything to do with crypto or auth.
5. **Invariants:**
   - workspace isolation: `workspace_id` on every row, forced RLS, `WsCtx` in every repository call
   - state change + events + outbox + audit commit in one transaction
   - `Idempotency-Key` and `If-Match` on mutating endpoints
   - Cedar authorization on every path
   - a deterministic runtime: injected Clock and IdGen, no I/O
6. **Licence gate.** Use permissive dependencies only. The traps are listed in CLAUDE.md.
7. **Database is PostgreSQL 16+** (ADR-0002). The cache is Valkey/Redis and is never authoritative (ADR-0019).
8. **Tracker:** update the item status, run `python3 scripts/build_tracker.py` and commit the regenerated outputs. Never hand-edit `docs/17` or `trackers/*`.
9. **No AI/assistant attribution** in commits or PRs. Commits are DCO signed-off (`git commit -s`).
