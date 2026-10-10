# Security policy

## Reporting a vulnerability

**Do not open public issues for security problems.** Report privately through one of these channels:
- **GitHub private vulnerability reporting** on this repository (*Security → Report a vulnerability*). This is the preferred channel.
- Email **security@procera.dev** (placeholder until the domain is registered; use GitHub reporting meanwhile).

Please include the affected version or commit, reproduction steps, impact, and any proof of concept. Please don't access other people's data and don't degrade service. Test only against your own deployment.

## What to expect

| Step | Target |
|---|---|
| Acknowledgement | within 3 working days |
| Triage and severity (CVSS v4) | within 7 working days |
| Fix or mitigation for critical/high | as fast as possible, normally ≤ 30 days |
| Coordinated disclosure | 90 days by default, or earlier once a fix ships. GitHub Security Advisory + CVE where applicable. |

We credit reporters who want credit.

## Supported versions

Before 1.0, only the latest pre-release (R-series) receives fixes. After GA, the latest minor and the current LTS line receive security fixes ([GOVERNANCE](GOVERNANCE.md#5-releases)).

## Scope

In scope is everything in this repository:
- workspace isolation
- authentication, sessions, MFA and step-up
- authorization and role escalation
- audit-trail tampering
- file handling and malware quarantine bypass
- signed URLs, webhooks, connectors and SSRF
- e-signature integrity
- test-only endpoints present in release builds

Out of scope: denial of service via volumetric traffic, findings in unmodified third-party services, social engineering, and missing hardening that has no demonstrated impact.

Our security design is documented in [docs/06-SECURITY-AND-OPERATIONS.md](docs/06-SECURITY-AND-OPERATIONS.md).
