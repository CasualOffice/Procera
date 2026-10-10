# Contributing to Procera

Thanks for helping. Procera is **design-first**: the docs in `docs/` are the contract, and code follows them. The binding rules are in **[docs/24-ENGINEERING-RULES.md](docs/24-ENGINEERING-RULES.md)**. This page is the quick version.

## Before you start

1. Read [docs/00-INDEX.md](docs/00-INDEX.md), then the docs relevant to your change: PRD, LLD, IR spec, access model, security, UX.
2. Find or create the **tracker item** (`docs/17`, generated from `scripts/build_tracker.py` and the doc backlogs). New features go through the [catalogue change process](docs/21-FEATURE-CATALOGUE.md#17-change-process) first.
3. If the spec doesn't answer your question, open a discussion or add it to [docs/22](docs/22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md). Don't guess.

## Making a change

**The core team** commits directly to `main` (trunk-based, see [docs/24 §7](docs/24-ENGINEERING-RULES.md#7-git-and-commit-rules-trunk-based)). **External contributors** fork the repo and open a pull request, which is squash-merged.

1. Start from an up-to-date `main` (core team), or a fork branch (external contributors).
2. Update the docs first (or in the same PR) whenever behaviour, data, an API, security or UX changes. Architectural changes need an ADR.
3. Implement with tests:
   - unit tests;
   - integration tests against real Postgres;
   - **an E2E scenario** ([docs/23](docs/23-E2E-TEST-PLAN.md));
   - property tests for runtime, access and audit changes.
4. Update the tracker status and regenerate it:
   ```bash
   pip install openpyxl
   python3 scripts/build_tracker.py
   ```
5. Commit using Conventional Commits with **DCO sign-off**:
   ```bash
   git commit -s -m "feat(campaigns): new-joiner enrolment (BPMS-046, FR-AK03)"
   ```
6. Run the local gates (docs/24 §7.3). Then push to `main` (core team) or open a PR using the template (external contributors). Keep each change to about 400 lines or fewer, covering one tracker item.

## Review

- External PRs need 1 maintainer approval.
- Core-team commits to protected areas (runtime, access/authz, audit, database/RLS/migrations, crypto/auth) are marked `[protected]`. A second maintainer reviews them after they land. See `.github/CODEOWNERS`.
- Reviewers check against the docs, not personal taste.

## Using AI coding agents

Welcome, and in fact expected. Agents follow [CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) and [docs/24 §10](docs/24-ENGINEERING-RULES.md#10-rules-specific-to-ai-coding-agents). You are accountable for everything you submit. Don't add AI attribution lines to commits or PRs.

## Licensing

Contributions are accepted under [Apache-2.0](LICENSE) via the DCO. Dependencies must be permissively licensed ([ADR-0014](docs/adr/ADR-0014-licensing-and-contribution.md)).

## Conduct and security

- [Code of Conduct](CODE_OF_CONDUCT.md)
- Security issues go **privately** via [SECURITY.md](SECURITY.md).
