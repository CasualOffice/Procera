# Governance

Procera is an open-source project licensed under Apache-2.0 and stewarded by **CasualOffice**. This document explains:
- who makes decisions, and how;
- how to become a maintainer;
- how releases, security and the trademark are handled.

## 1. Principles

- **Design in the open.** Product and architecture decisions are written down as numbered docs and ADRs in `docs/` before code ([engineering rules](docs/24-ENGINEERING-RULES.md)).
- **Users' trust first.** Workspace isolation, auditability, security and accessibility outrank feature velocity.
- **Genuinely open.** The core stays Apache-2.0. No feature needed to run Procera in production is withheld from the open-source edition ([ADR-0014](docs/adr/ADR-0014-licensing-and-contribution.md)).
- **Merit and accountability.** Roles are earned through sustained, quality contributions. Whoever merges is accountable for what lands, including AI-assisted work.

## 2. Roles

| Role | Who | Rights and responsibilities |
|---|---|---|
| **Contributor** | Anyone who opens an issue, discussion or PR | Follows the [Code of Conduct](CODE_OF_CONDUCT.md), [CONTRIBUTING](CONTRIBUTING.md) and DCO sign-off |
| **Reviewer** | Contributors with repeated high-quality reviews in an area | Can approve PRs in their area. Approval counts toward the required reviews outside protected areas. |
| **Maintainer** | Listed in [`MAINTAINERS.md`](MAINTAINERS.md) | Merge rights. Triage. Release duties. Owners of areas in `CODEOWNERS`. Uphold the engineering rules. |
| **Area owner** | Maintainers named for protected areas (runtime, access, audit, data/RLS, crypto/auth, UX system) | Required reviewers for their area (two approvals for protected areas) |
| **Product owner** | Appointed by CasualOffice | Final say on product scope, priorities, release contents and the roadmap ([07](docs/07-ROADMAP-AND-MILESTONES.md)) |
| **Technical lead** | Appointed by maintainers, with steward confirmation | Final say on architecture when consensus fails. Owns the ADR process and merge sequencing. |
| **Security response team** | 2+ maintainers | Handles private vulnerability reports ([SECURITY.md](SECURITY.md)) |

AI coding agents are tools, not contributors. A human contributor opens every PR, signs it off and is accountable for it.

## 3. Decision making

1. **Lazy consensus.** Most decisions are made in PRs and issues. If no maintainer objects within a reasonable window (normally 3 working days for docs and features, 5 for ADRs), the change can merge once it has the required approvals.
2. **ADRs** decide architecture ([`docs/adr/`](docs/adr/README.md)). An ADR moves `Proposed → Accepted` with approval from the technical lead and at least one other maintainer. A contested ADR goes to a maintainer vote.
3. **Scope** goes through the feature catalogue change process ([21 §17](docs/21-FEATURE-CATALOGUE.md)). The product owner decides priority and release.
4. **Votes** are used only when consensus fails. A simple majority of maintainers decides, with a quorum of half the maintainers. The technical lead breaks ties on architecture, and the product owner breaks ties on scope.
5. **Engineering rules** ([docs/24](docs/24-ENGINEERING-RULES.md)) change by PR, with approval from 2 maintainers including the technical lead.
6. **Governance changes** need a two-thirds maintainer majority and steward (CasualOffice) consent.

## 4. Becoming a maintainer

- **Nomination.** An existing maintainer nominates a candidate after ≥ 3 months of sustained contributions: code, reviews, docs or tests. The candidate must have shown good judgement against the engineering rules.
- **Approval.** No maintainer objects within 7 days, or a two-thirds vote passes.
- **Inactivity.** Maintainers inactive for 6 months move to *emeritus*. They can return on request.
- **Removal.** Possible for Code of Conduct violations or sustained disregard of the rules, by a two-thirds vote.

## 5. Releases

- Release contents and gates follow [07](docs/07-ROADMAP-AND-MILESTONES.md) and [24 §9](docs/24-ENGINEERING-RULES.md#9-release-rules).
- A release manager (a rotating maintainer) cuts the release, signs the artefacts and publishes the SBOM, the E2E report and the upgrade guide.
- After GA there is an LTS line every 12 months, with 18 months of security fixes.

## 6. Security

Vulnerabilities are reported privately ([SECURITY.md](SECURITY.md)). The security response team coordinates fixes and advisories, with 90-day coordinated disclosure by default.

## 7. Trademark

"Procera", its logo and the brand assets in `assets/brand/` are trademarks of CasualOffice. They are not licensed under Apache-2.0. Brand usage rules are in [`assets/brand/README.md`](assets/brand/README.md).
- Forks are welcome under Apache-2.0, but a fork MUST use a different name and logo if it is distributed or offered as a service.
- Unmodified builds of official releases, and descriptive use ("works with Procera", "based on Procera"), are allowed.
- Questions go to the maintainers.

## 8. Contributions and licensing

- Contributions are accepted under Apache-2.0 through the **Developer Certificate of Origin** (DCO 1.1). Every commit carries `Signed-off-by:`. There is no CLA.
- Dependencies must pass the permissive-licence gate ([ADR-0014](docs/adr/ADR-0014-licensing-and-contribution.md)).

## 9. Code of Conduct

Everyone in project spaces follows the [Code of Conduct](CODE_OF_CONDUCT.md). Enforcement is handled by maintainers not involved in the incident.
