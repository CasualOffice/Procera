# 14 — Data governance, privacy and compliance readiness

Status: Draft · Owner: Security lead + Product · Closes gaps B10, B14 and part of D5.
Researched 2026-10-11. **Unverified items are flagged ⚠.**

> Positioning rule: Procera **supports** customers' compliance. It is never "certified"
> or "compliant" by itself. The customer validates their use, as Part 11 §11.100(c) and
> SOC 2 shared responsibility expect. Marketing copy must say "Part 11-ready" or
> "supports ISO 27001 evidence", never more.

## 1. Requirement catalogue (NFR IDs)

These IDs are referenced from the PRD, the tracker and the test strategy.

### 1.1 Electronic signatures and records (21 CFR Part 11, EU GMP Annex 11, eIDAS)

| ID | Requirement | Phase |
|---|---|---|
| ESIG-01 | **Manifestation (§11.50):** the signer's printed name, date/time with timezone, and the meaning of the signature (authored / reviewed / approved / responsible). Shown in UI, print and PDF exports. | P1 |
| ESIG-02 | **Linking (§11.70):** the signature object stores a hash of the canonical record version. Signatures cannot be copied to another record or version. Any post-sign edit forces a new version and invalidates prior signatures for the new one. | P0 (data model) / P1 (UI) |
| ESIG-03 | **Re-authentication (§11.200):** at least two components. The first signing in a session needs all components; subsequent ones need at least one secret component. Under SSO, force OIDC `prompt=login`/`max_age=0` or a WebAuthn assertion. No "remember me" for signing. | P1 |
| ESIG-04 | **Unique, non-reusable identities (§11.100/§11.300):** user ids are never reassigned. Lockout and alert on repeated failures. Revocation procedures exist. | P0 |
| ESIG-05 | **Audit trail (§11.10(e)):** computer-generated and timestamped. Records who/when/what plus old→new values. Never obscures prior entries. Retained at least as long as the record. Exportable in human-readable form. | P0 |
| ESIG-06 | **Validation pack:** URS/FS traceability matrix, IQ/OQ scripts, release notes and controls mapping, shipped per release for regulated customers. | P1/GA |
| ESIG-07 | **Annex 11:** reason-for-change on GxP records, periodic audit-trail review workflow, backup/archival readability over time. | P1 |
| ESIG-08 | **Design to the 2025 Annex 11 draft:** audit trail cannot be disabled, plus Annex 22 (AI in GMP). ⚠ Final adoption was planned for mid-2026 and is unconfirmed; re-check EudraLex Vol 4. | P1 |
| ESIG-09 | **eIDAS:** native SES/AES, with a pluggable QES path via a QTSP remote-signing API (CSC API; PAdES for exported PDFs). EUDI Wallet signing as an optional method once Member States ship (~late 2026). | P2 |
| ESIG-10 | Ship a Part 11 / Annex 11 controls mapping matrix (product vs customer responsibility). | GA |

### 1.2 Audit trail tamper-evidence

| ID | Requirement | Phase |
|---|---|---|
| AUD-01 | Per-tenant hash chain: `hash_n = SHA-256(hash_{n-1} ‖ JCS(event_n))` (RFC 8785 canonical JSON). | P0 |
| AUD-02 | Periodic Merkle checkpoints (every N events or 5 min), with the root signed by Ed25519 from KMS/HSM. Inclusion/consistency proofs in RFC 9162 style. | P1 |
| AUD-03 | Optional RFC 3161 timestamp of each checkpoint root from a configurable TSA (a qualified TSA for EU evidential weight). Token stored. | P1 |
| AUD-04 | Optional export of sealed segments and checkpoints to S3 Object Lock in **compliance** mode with per-tenant retention. Governance mode is not WORM-grade. | P1 |
| AUD-05 | Offline verifier CLI (`procera-verify`) that recomputes chains, proofs and timestamps without trusting the vendor. | P1 |
| AUD-06 | NTP-disciplined clocks; record time source. | P0 |
| AUD-07 | DB-level append-only: `REVOKE UPDATE, DELETE` on `audit_log` from the app role, a trigger guard, and no admin delete path. Crypto strengthens the controls but does not replace them. | P0 |

### 1.3 Compliance features customers use (SOC 2 TSC 2017/2022, ISO 27001:2022, ISO 9001)

| ID | Requirement | Maps to |
|---|---|---|
| CMP-01 | Evidence export per control (CSV/PDF + signed manifest) tagged with TSC and Annex A ids. | SOC2 CC2–CC9; ISO A.5.x |
| CMP-02 | SoD rules (requester ≠ approver, author ≠ publisher); overrides logged with reason. | CC8.1, A.5.3, A.8.32 |
| CMP-03 | Recurring attestation campaigns with completion evidence. | CC2.2, A.6.3 |
| CMP-04 | Access-review campaigns, for example quarterly membership recertification of roles. | CC6.2–6.3, A.5.18 |
| CMP-05 | Incident case template with evidence chain-of-custody. | CC7.3–7.5, A.5.24–5.28 |
| QMS-01 | Document identification: title, id, version, author, date, format; documented review and approval. | ISO 9001 §7.5.2 |
| QMS-02 | Controlled distribution and access. Superseded/obsolete versions are watermarked and blocked from execution use. Retention/disposition. External-origin documents marked. | §7.5.3 |
| QMS-03 | Periodic review dates, read-and-understood acknowledgements, change requests with impact assessment. | §7.5, §6.3 |

### 1.4 Privacy (GDPR, India DPDP, EU Data Act)

| ID | Requirement | Phase |
|---|---|---|
| PRV-01 | DSAR tooling: access, export, rectification, erasure and restriction, with a 1-month SLA tracker. It can itself run as a shipped process template. | P1 |
| PRV-02 | **Erasure vs immutable audit:** see §2. | P0 (design) |
| PRV-03 | Legal hold at case, record or custodian level. Holds suspend retention, disposition and erasure jobs. Release only by an authorised role; audited. | P1 |
| PRV-04 | Retention engine per record class, with disposition certificates. | P1 |
| PRV-05 | Residency: the tenant is pinned to a region for data, backups, indexes, logs and AI inference. Subprocessor list. | P1 (hosted) |
| PRV-06 | Ship templates for an Art. 30 RoPA export, an Art. 28 DPA, an Art. 33 breach workflow (72 h clock) and a DPIA. | P1 |
| PRV-07 | **India DPDP Act 2023 + Rules (notified 13 Nov 2025):** notice/consent records, erasure when purpose served, grievance redressal, safeguards, logs retained ≥ 1 year, breach notice. Consent-manager provisions ~Nov 2026; substantive obligations 13 May 2027. | P1 |
| PRV-08 | **EU Data Act switching** (from 12 Sep 2025; egress fees banned from 12 Jan 2027): full tenant export in open formats (JSON + BPMN XML + original files + audit chain) and a documented switching process. Good for the OSS story anyway. | P1 |

### 1.5 AI governance (EU AI Act, as amended by the AI Omnibus)

See [16-AI-ASSISTANCE](16-AI-ASSISTANCE.md). Key dates:
- Prohibitions and AI literacy: 2 Feb 2025.
- Art. 50 transparency: 2 Aug 2026. ⚠ Confirm the Omnibus didn't move it.
- Annex III high-risk: **2 Dec 2027**.

Employment-related task allocation based on individual behaviour or traits is Annex III §4(b), which makes it **high-risk**. Rule: Procera AI never scores people or allocates work based on behaviour or performance profiles (AI-01).

### 1.6 Accessibility

| ID | Requirement |
|---|---|
| A11Y-01 | WCAG 2.2 AA. Pay special attention to 2.5.7 Dragging (the designer needs keyboard/menu placement), 3.3.8 Accessible Authentication (signing re-auth allows paste/password managers; WebAuthn offered), 2.5.8 Target size, 2.4.11 Focus not obscured, 3.3.7 Redundant entry. |
| A11Y-02 | European Accessibility Act (in force 28 Jun 2025): pure employee tools are mostly out of direct scope, but **public-facing forms** (external intake, guest portals) and self-serve plans are in. Build to EN 301 549 regardless. |
| A11Y-03 | Publish an ACR/VPAT 2.5 INT edition at GA. |
| A11Y-04 | Tagged PDF/UA exports for SOPs and signed records. |

### 1.7 Webhooks and outbound connectors

| ID | Requirement |
|---|---|
| WH-01..04 | Implement the **Standard Webhooks** spec. Headers `webhook-id`, `webhook-timestamp`, `webhook-signature`; signed content `id.timestamp.body`. HMAC-SHA256 `v1` (secret `whsec_…`) and Ed25519 `v1a`. Multiple signatures allow rotation. 5-min tolerance. Retries with jitter over ~24 h. `410` disables; honour `Retry-After`. Auto-disable plus owner notice. Delivery log and replay. |
| WH-05 | Inbound webhooks verified the same way, with per-connector secrets. |
| SSRF-01..07 | All outbound traffic goes through an egress proxy in an isolated network (Smokescreen pattern). Resolve then block private, loopback, link-local, CGNAT, metadata, ULA and IPv4-mapped ranges. Connect to the validated IP (defeats DNS rebinding). Redirects off or re-validated per hop. http/https only with a port allowlist. Response size and time caps. Self-hosted admins may allowlist internal CIDRs explicitly (audited, default deny). Secrets never echoed or logged. |

## 2. Erasure vs immutability: the design

Conflict: GDPR Art. 17 and DPDP erasure, versus an append-only, hash-chained audit trail and signed GxP records that must be kept.

1. **Pseudonymous subject ids in immutable stores.** `audit_log`, `instance_events` and signatures reference `subject_ref` (a random UUID), never names or emails.
2. **Identity vault.** A mutable `subject_identities` table maps `subject_ref → {name, email, …}`. Each row is encrypted with a per-subject data key.
3. **Erasure = crypto-shredding** of that key, plus tombstoning the identity row. The chains stay valid because they hash only pseudonymous ids. Free-text fields that may contain personal data (comments, form values) are stored outside the chain. They are referenced by hash and can be redacted, leaving a "redacted under DSAR #…" marker.
4. **Exemptions.** Where retention is legally required (signed GxP records, SOX, legal hold), apply Art. 18 **restriction** instead of erasure. Document the Art. 17(3)(b)/(e) basis. The DSAR workflow records the decision.
5. Signature manifestations on exported PDFs are snapshots, so they are subject to the record's retention class and are not re-rendered after erasure.

## 3. Electronic signature data model (summary)

```text
signatures(tenant_id, id, subject_ref, record_type, record_id, record_version,
           record_hash, meaning, reason, auth_method, auth_time, idp_acr,
           signed_at, manifestation_text, prev_signature_id, audit_seq)
```
- `record_hash` is computed over the JCS canonical record version.
- `auth_time` must be no more than `signed_at − 0`, within the re-auth window.
- `part11` mode forbids service identities and requires ESIG-03.

The full DDL is in [04-LLD](04-LLD.md).

## 4. Retention classes (defaults; tenant-configurable)

| Class | Examples | Default retention | Deletion |
|---|---|---|---|
| `governance_record` | Published process/SOP versions, approvals, signatures | 10 years after retirement | Disposition workflow with certificate |
| `execution_record` | Instances, decisions, events | 7 years after completion | Batch purge; audit keeps pseudonymous refs |
| `case_file` | Investigations and evidence | Per case type (default 7 years) | Legal hold aware |
| `operational` | Incidents, delivery logs, metrics | 400 days | Automatic |
| `audit` | Audit log and checkpoints | Longest of the above for linked records; ≥ 1 year (DPDP) | Never by app role |
| `draft` | Unpublished drafts, AI suggestions not accepted | 180 days inactive | Automatic, with notice |

## 5. What the vendor itself needs (hosted offering)

- **SOC 2 Type II:** Security, Availability, Confidentiality.
- **ISO 27001:2022.** The transition from 2013 ended 31 Oct 2025.
- **Optional:** ISO 27701 / 27017 / 27018.
- **Self-hosted builds:** hardening guide, CycloneDX SBOM, Sigstore-signed releases, and a published shared-responsibility matrix (VND-04).

## Sources

- 21 CFR 11: <https://www.ecfr.gov/current/title-21/chapter-I/subchapter-A/part-11>
- Annex 11 drafts (Jul 2025): <https://www.gmp-compliance.org/gmp-news/drafts-of-eu-gmp-guideline-annex-11-annex-22-and-chapter-4-released-for-comment>
- eIDAS 2.0: <https://eur-lex.europa.eu/eli/reg/2024/1183/oj>
- RFC 3161: <https://www.rfc-editor.org/rfc/rfc3161>
- RFC 9162: <https://www.rfc-editor.org/rfc/rfc9162>
- S3 Object Lock: <https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html>
- TSC 2017 (rev. 2022): <https://www.aicpa-cima.com/resources/download/2017-trust-services-criteria-with-revised-points-of-focus-2022>
- EDPB pseudonymisation 01/2025: <https://www.edpb.europa.eu/our-work-tools/documents/public-consultations/2025/guidelines-012025-pseudonymisation_en>
- DPDP Rules: <https://www.tcsa.in/resources/dpdp-rules-2025-implementation-roadmap>
- EU Data Act switching: <https://www.cooley.com/news/insight/2025/2025-09-08-paas-iaas-or-saas-be-aware-new-switching-rules-will-become-applicable-in-eu>
- AI Omnibus: <https://www.whitecase.com/insight-alert/eu-ai-omnibus-enters-force-amending-ai-act>
- WCAG 2.2: <https://www.w3.org/TR/WCAG22/>
- EAA B2B scope: <https://karlgroves.com/the-european-accessibility-act-and-b2b-software-what-internal-platforms-must-comply-with/>
- Standard Webhooks: <https://www.standardwebhooks.com>
- OWASP SSRF: <https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html>
