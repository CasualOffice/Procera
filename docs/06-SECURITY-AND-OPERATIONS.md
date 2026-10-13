# 06 — Security protocols, reliability and operations

Status: **Draft for G1 sign-off** · Owner: Security lead (+ Platform lead for §9–12) ·
Supersedes [v0.1](archive/v0.1/06_SECURITY_OPERATIONS.md) (its threat model, controls, SLOs,
recovery tests and SEC-01…10 are kept and extended).

This chapter is **normative**: *MUST* / *MUST NOT* are release-blocking, *SHOULD* needs a
recorded exception in the ADR register to skip. Compliance-driven requirement IDs
(ESIG, AUD, PRV, WH, SSRF, A11Y, CMP, QMS) live in [14](14-DATA-GOVERNANCE-AND-COMPLIANCE.md)
and are referenced, not repeated. Access-control semantics live in [12](12-AUTHORIZATION-MODEL.md).

## 1. Objectives and baseline

| Objective | Measure |
|---|---|
| No cross-workspace data exposure | 0 findings in SEC suite, pentest and bug bounty; RLS + app-layer checks both tested |
| Every privileged or content-revealing action is attributable | 100 % audit coverage contract test (FR-T01) |
| Compromise of one component does not yield all workspaces' data | Per-workspace KEKs, sandboxed connector/conversion workers, egress proxy |
| Customers can verify integrity without trusting us | Hash-chained audit + offline verifier (AUD-01…05), signed releases + SBOM |

**Standards baseline**

- **OWASP ASVS 5.0.0** (released 30 May 2025): **Level 2** for the whole product; **Level 3**
  for authentication, session management, cryptography, authorization and file handling
  chapters. An ASVS checklist is maintained in `docs/security/asvs-5.0.md` with each
  requirement → test or rationale.
- **OWASP API Security Top 10 (2023)**: each item mapped to a SEC test (BOLA → SEC-01/02,
  broken auth → SEC-11…15, BOPLA → SEC-03, resource consumption → SEC-24/27, BFLA → SEC-21,
  SSRF → SEC-06/25, misconfiguration → SEC-28, inventory → OpenAPI diff gate, unsafe
  consumption → connector schema validation).
- **CIS Benchmarks**: Kubernetes, Docker, PostgreSQL 16 — hardening guide references them;
  Helm chart passes `kube-bench`/`kubescape` defaults with documented exceptions.
- **Supply chain**: **SLSA Build Level 3** provenance for release artefacts by GA (L2 at
  pilot); OpenSSF Scorecard ≥ 7.
- **NIST SP 800-63B-4** for authenticator rules (passwords, MFA, re-auth).

## 2. Threat model

Assets: employee/personal data in variables and files; restricted fields; confidential
approvals and signatures; compliance evidence and investigations; audit trail; integration
secrets; workspace keys; platform admin credentials; release artefacts.

Trust boundaries: **B1** browser/guest ↔ edge · **B2** API ↔ Postgres/object store ·
**B3** API ↔ workers (runtime, scheduler, conversion, scan) · **B4** connector workers ↔
external systems (via egress proxy) · **B5** workspace ↔ workspace · **B6** system plane ↔
workspace plane · **B7** workspace admin ↔ workspace content · **B8** product ↔ AI provider ·
**B9** CI/CD ↔ release artefacts.

| # | Boundary | STRIDE | Threat | Mitigations (MUST) | Tests |
|---|---|---|---|---|---|
| T-01 | B5 | I, E | BOLA/IDOR: valid user of workspace A reads/acts on B's objects by id | Workspace from token only; all repos take `WorkspaceCtx`; composite `(workspace_id,id)` keys; RLS `FORCE`; 404 on miss | SEC-01, 02, 31 |
| T-02 | B5 | I | Missing tenant predicate in a new query | RLS fails closed (`current_setting(...,true)` → 0 rows); CI lint (§5.9) | SEC-32 |
| T-03 | B1 | S | Credential stuffing / phishing | Argon2id, breached-password check, lockout+backoff, MFA (required for admins), passkeys | SEC-11, 12 |
| T-04 | B1 | S, E | Session fixation/hijack, CSRF | `__Host-` cookie, rotation on login/privilege change, Origin + token CSRF defence, SameSite | SEC-11, 13 |
| T-05 | B1 | S | Refresh/API token theft and replay | Rotation with reuse detection (family revoke), hashed tokens, short TTLs, optional DPoP (P2) | SEC-14 |
| T-06 | B1 | T, E | Magic-link / email-action link replay or prefetch mutation | Single-use, 15 min, bound to user+item, confirm page, POST-only mutation | SEC-15, 30 |
| T-07 | B1 | T, E | Malicious file upload (malware, polyglot, XSS via SVG/HTML, zip bomb, XXE in OOXML) | Pipeline §5.6; quarantine until clean; no inline active content; sandboxed conversion; decompression limits | SEC-19, 20, 26 |
| T-08 | B1 | T | Stored/DOM XSS via SOP content, comments, form labels | Structured content model, contextual encoding, sanitizer allowlist, strict CSP + Trusted Types | SEC-22 |
| T-09 | B4 | I, E | SSRF through user-configured connector / webhook URL | Egress proxy, resolve-then-block, pinned IP, no redirects (SSRF-01…07) | SEC-06, 25 |
| T-10 | B4 | S, R | Forged/replayed inbound webhook/callback | Standard Webhooks signature, timestamp window, `webhook-id` dedup (WH-01…05) | SEC-05 |
| T-11 | B3 | T, D | Compromised connector/conversion worker pivots | Separate processes/pods, no DB credentials beyond scoped queue, seccomp, read-only FS, network policy | SEC-28 |
| T-12 | B7 | I, R | Workspace admin insider browses restricted data | Full visibility by design **with access-logging** (`audit.data_access`), anomaly rules, Separation mode option | SEC-23, 33 |
| T-13 | B6 | I, E | System admin reads tenant content | No content permissions in system plane; break-glass with workspace approval + visible banner + audit | SEC-21, 34 |
| T-14 | B7 | E | Custom-role escalation (grant self more) | No-escalation rule, step-up for role changes, Cedar `forbid` invariants | SEC-08, 21 |
| T-15 | B2 | T, R | Audit tampering by DBA/app bug | `REVOKE UPDATE/DELETE`, trigger guard, hash chain, signed checkpoints, RFC 3161, WORM export | SEC-35 |
| T-16 | B3 | D | Expression/rule evaluation DoS, deep JSON, huge forms | Evaluation step/memory budgets, request size limits, depth limits | SEC-27 |
| T-17 | B1 | D | Abuse of API/search/export | Rate limits per IP/user/workspace/token; async exports with quotas | SEC-24 |
| T-18 | B8 | I, T | Prompt injection through SOP/form content steering AI tools; data leakage to provider | AI is draft-only, no tool can publish/approve; workspace data isolation in prompts; provider allowlist; local-model option; redact `restricted` from prompts | SEC-36 |
| T-19 | B9 | T | Compromised dependency or build | Pinned lockfiles, cargo-deny/audit, review of new deps, SLSA provenance, cosign signatures, licence gate | SEC-37 |
| T-20 | B2 | I | Backup theft | Encrypted backups (separate key), restricted bucket, Object Lock option | Restore drill |
| T-21 | B1 | I | Signed download URL leaked/shared | ≤ 5 min TTL, bound to user+workspace, re-authorized server-side via redirect endpoint | SEC-10, 18 |
| T-22 | B5 | I | Cross-workspace leakage via caches, search index, notifications | Cache keys prefixed by workspace; index docs carry workspace_id filtered in query; notification rendering under recipient's policy | SEC-31 |

## 3. Protocols (precise)

### 3.1 Transport
- TLS **1.3** preferred; TLS 1.2 minimum with only `ECDHE-ECDSA/RSA-AES128/256-GCM-SHA256/384`
  and `ECDHE-*-CHACHA20-POLY1305`. No TLS ≤ 1.1, no CBC, no RSA key exchange.
- HSTS `max-age=63072000; includeSubDomains` (preload optional for hosted). OCSP stapling.
- Internal traffic SHOULD use mTLS in Kubernetes (service mesh or native certs) — **MUST**
  for hosted multi-node deployments; optional for single-node Compose.
- Postgres connections: `sslmode=verify-full` when not on loopback/unix socket.

### 3.2 Browser sign-in (JWT, ADR-0020)
- **Access token:** a JWT (HS256, algorithm pinned), **10 min**, sent as `Authorization: Bearer`, held in memory by the web client (never in localStorage). Claims: `sub, sid, plane, ws, amr, auth_time, tv, typ, iat, exp, jti`; `ws` is the authoritative workspace.
- **Refresh token:** an opaque 256-bit value in the cookie `__Host-procera_refresh` (`Secure; HttpOnly; Path=/; SameSite=Strict`), stored as SHA-256. It is **rotated on every use**, and reuse revokes the family (SEC-14). Sliding expiry is 12 h; absolute expiry is 30 d (configurable within floors).
- **CSRF:** Bearer-authenticated endpoints are not CSRF-exposed. The cookie endpoints `/auth/refresh` and `/auth/logout` require `SameSite=Strict` **and** `X-Requested-With: procera`; missing → 403 `csrf`.
- **Revocation:** logout revokes the family. "Sign out everywhere" and deactivation revoke all families and bump `users.token_version`, which each request checks through a ≤ 30 s cache (SEC-04).
- **MFA step:** after the password, a 5-minute `typ=mfa` token is valid only for `/auth/mfa`.
- The system plane uses a separate host (e.g. `admin.<domain>`) and its own tokens (`plane=system`).

### 3.3 OIDC (relying party; Keycloak is the reference IdP)
- Authorization Code + **PKCE S256**; `state` (CSRF) and `nonce` (replay) MUST be
  validated; exact redirect URI match; `iss` and `aud` checks; ID token signature
  verification against cached JWKS with rotation; clock skew ≤ 60 s.
- Step-up: `max_age=0` / `prompt=login` and `acr_values` for signatures (ESIG-03); result
  recorded from `auth_time`/`acr`/`amr`.
- SAML is accepted only via a brokering IdP (no in-process XML signature validation).
- Per-workspace IdP configuration; domain verification (DNS TXT) before auto-join.

### 3.4 API access tokens
- The same JWT format as §3.2 (HS256 today, ADR-0020; EdDSA with a JWKS endpoint if an external resource server ever needs to verify tokens). **TTL ≤ 15 min**, audience `procera-api`. The workspace claim is authoritative.
- Refresh token: opaque 256-bit, TTL 12 h sliding / 30 d absolute (configurable),
  **rotated on every use**; reuse of an old token revokes the whole family and the
  session and emits `security.token_reuse` (SEC-14).
- **DPoP** (RFC 9449) sender-constrained tokens: P2, for high-assurance API clients.

### 3.5 Personal / service API tokens
- Format `prc_<env>_<id8>_<secret>` where `secret` = 32 random bytes base62; prefix enables
  secret scanning (register pattern with GitHub secret scanning).
- Stored as `HMAC-SHA256(pepper, secret)`; pepper held in the key service, not the DB.
  Only last 4 chars shown after creation.
- Mandatory expiry (max 1 year, default 90 d), scoped to permissions ⊆ creator's, bound to
  one workspace; last-used time/IP tracked; revocable; service tokens cannot sign.

### 3.6 Passwords and MFA
- **Argon2id**, m = 64 MiB, t = 3, p = 1 (minimum m = 19 MiB, t = 2 on constrained
  hardware), 16-byte salt, parameters stored with hash; rehash on login when parameters
  change. Min length 12 (platform floor configurable ≥ 8 per NIST), max 128, no composition
  rules, breached-password check against an offline HIBP k-anonymity list.
- Lockout: exponential backoff after 5 failures per account and per IP; alert on
  distributed attempts.
- **TOTP** RFC 6238 (SHA-1, 6 digits, 30 s, ±1 step), secrets encrypted; replay of the same
  code within its window rejected. **WebAuthn Level 3** / passkeys with user verification
  required for admin roles where possible. 10 single-use recovery codes (hashed).
- Passkeys as built (BPMS-022, docs/22 A-19): RP ID = public URL host (`PROCERA_WEBAUTHN_RP_ID`
  overrides), exact-origin check, single-use 32-byte challenges (5 min, bound to user and
  ceremony), user presence required and user verification preferred, attestation `none`,
  ES256/EdDSA/RS256 verified with `ring`, signature counters must increase (clone detection),
  public keys stored as COSE in `user_credentials`. Every add, removal, sign-in and step-up is
  audited in the system chain (`mfa.webauthn_added`, `mfa.webauthn_removed`, `login.succeeded`,
  `mfa.failed`, `session.stepped_up`, `session.step_up_failed`).
- MFA required: all system-plane users (always); workspace owners/admins (default on);
  any role a workspace marks.
- First-run bootstrap as built (FR-S01, BPMS-020/065, E2E-R1-01). There are no default
  credentials. While no `system.owner` exists, the server prints a one-time token (24 h) to its log
  at start-up. The web app's `/setup` page, reachable signed out, takes that token (or `?token=`,
  removed from the address bar once read), work email, name and password, and calls
  `POST /sysapi/v1/bootstrap`. An invalid, expired or used token is marked on the token field; once
  an owner exists, the page says Procera is already set up and links to sign-in. The token is spent
  before the account is created, so the page checks the password policy it can (length, no email
  name) first; a rejection after that tells the operator to restart for a new token. The new owner
  is signed in, enrols TOTP in the system console's MFA gate, signs in again with the code and
  reaches the console.

### 3.6.1 Account recovery and admin account controls (decided 2026-10-12, OQ-15, A-39)

Self-service, signed-in and admin paths. Every path keeps two-step verification unless an admin
explicitly resets it, so a mailbox alone never takes over an account.

- **Forgot password (self-service).** `POST /auth/password/forgot {email}` always answers 202 with
  the same body and similar timing, whether or not the account exists (no enumeration). Rate-limited
  per IP and per email (3 per email per hour). For an active account it creates a reset token
  (32 random bytes, stored only as SHA-256, single use, 30 minutes; a newer request voids older
  ones) and emails a link to `/reset-password?token=…` (the page reads the token and removes it
  from the address bar). These are system emails, not tied to a workspace.
- **Reset with the link.** `POST /auth/password/reset {token, password}`: the password policy above
  applies; on success the password is replaced, **every session and refresh token is revoked**, a
  "your password was changed" email is sent, and `password.reset` is audited (system chain). Two-step
  verification is unchanged: the next sign-in still asks for it.
- **Change password (signed in).** Account › Security: `POST /me/password {current_password,
  new_password}`. The current password is checked (failures count toward lockout); other sessions are
  revoked, the current one stays; email notice; `password.changed` audited.
- **Admin controls** (workspace plane, permission `member:security.manage`, owners and admins by
  default; each needs a recent sign-in (step-up) and a reason, and is audited in the workspace chain
  and the system chain):
  - **Send a reset link**: the same email as self-service, triggered for the member.
  - **Require a password change at next sign-in**: sign-in then returns
    `{status: "password_change_required", change_token}` (short-lived, single use) instead of a session;
    `POST /auth/password/expired {change_token, password}` sets the new one and continues to two-step
    verification as usual.
  - **Sign out everywhere**: revokes all of the member's sessions and refresh tokens.
  - **Reset two-step verification** (lost authenticator): removes TOTP, passkeys and recovery codes; the
    member enrols again at next sign-in where their role requires it.
  Accounts are global and can belong to several workspaces. **Require password change, sign out
  everywhere and reset two-step verification** are allowed only when this workspace is the member's
  only active workspace; otherwise the page explains that a system administrator must do it (the
  system console offers the same actions). Sending a reset link is always allowed (the member still
  needs their mailbox and second factor). Admins never see or set a member's password.

*As built (BPMS-021, OQ-15):*
- **Data.** Migration 0036: `password_resets` (system scope like `users`; `token_hash` SHA-256 only,
  `expires_at`, `used_at`, `voided` for links replaced by a newer one, `requested_by` NULL for
  self-service or the admin's subject, `requester_ip`) and `users.must_change_password`.
- **Email.** Recovery emails go to the system workspace's queue (`email_messages` under the nil
  workspace id, which 0011 reserved for platform mail): forced RLS keeps them out of every tenant,
  bodies are sealed under the system workspace's own KEK, and the worker's `send_due` delivers them
  like any other. Templates `password_reset`, `password_changed`, `account_security_changed`.
  Addresses suppressed after a bounce are skipped.
- **Forgot.** The answer is always 202 with the same body; the email is queued in the background so
  timing doesn't depend on the account. Limits: the sign-in per-IP budget, and 3 self-service links
  per account per hour (further requests are audited and send nothing). An admin's link doesn't count
  against that budget.
- **Required change.** Sign-in answers `password_change_required` before the MFA step, with a
  10-minute `pwchange` token bound to the token version; setting the password bumps the version, so
  the token works once. `POST /auth/password/expired` then revokes every session and continues as a
  password sign-in (MFA next when enrolled).
- **Change.** `POST /me/password` revokes the other refresh families, bumps the token version and
  returns a re-signed access token for the current session. A wrong current password records a
  sign-in failure for the account and IP keys.
- **Admin.** `POST /members/{id}/security/{reset-link|require-password-change|sign-out-everywhere|reset-mfa}`
  with `{reason}`; `member:security.manage` on the member (owner and admin templates; 0036 backfills
  existing workspaces), step-up, an active membership. "Only active workspace" means no other active
  membership in a live workspace **and no system role** (a system account is always outside the
  workspace's reach); the answer is 409 `other_workspaces`. Reset two-step verification removes TOTP,
  passkeys and recovery codes; it keeps the password and sessions.
- **Audit.** System chain (actor = the person or the admin, object = the account's subject):
  `password.reset_requested`, `password.reset`, `password.changed`, `password.change_failed`,
  `password.change_required`, `login.password_change_required`, `sessions.revoked_all`, `mfa.reset`.
  Workspace chain (with the reason): `member.password_reset_link_sent`,
  `member.password_change_required`, `member.signed_out_everywhere`, `member.mfa_reset`.
- **Not yet built:** the system console's versions of these actions (for accounts with several
  workspaces) arrive with the system console's user management.

### 3.7 Magic links (guests) and email-action links
- 256-bit random token, stored hashed, **single-use**, **15 min** TTL, bound to
  `(user or guest identity, workspace, work_item, action set)`.
- Link opens a **confirm page**; mutation happens only on an explicit POST with CSRF token.
  GET never mutates (defeats mail-scanner prefetch, SEC-30).
- For Part 11 processes email links only open the item; signing requires full re-auth.
- Consumed/expired tokens show a safe error and offer sign-in.

### 3.8 Webhooks
Outbound and inbound follow **Standard Webhooks** (WH-01…05): `webhook-id`,
`webhook-timestamp`, `webhook-signature` (`v1` HMAC-SHA256 or `v1a` Ed25519), 5 min
tolerance, dedup on id, multiple signatures for rotation.

### 3.8a Email approval links (BPMS-145)
A plain approval step emails each candidate a 256-bit single-use token (only its SHA-256 stored,
found through a discovery function). `GET` only reads the confirm page's data; `POST` decides once as
that member through the normal decision path (candidacy, SoD, one decision wins), spending every link
of the step in the same transaction. Links expire at the step's due date or after 7 days. Workspaces
can turn them off (`notify.email_actions.enabled`). See A-49 for the forwarding trade-off.

### 3.9 Signed object URLs
Clients never receive raw bucket URLs persistently. `GET /v1/files/{id}/content` authorizes
the user, logs access if required, then 302-redirects to a presigned URL with **TTL ≤ 5 min**,
`response-content-disposition: attachment` for non-previewable types, and object key
prefixed `ws/{workspace_id}/`. Preview content is served from a separate sandbox origin.

## 4. Cryptography and key management

**Algorithms** (MUST; anything else needs an ADR):
- Symmetric: AES-256-GCM (96-bit random nonce, per-DEK message limit 2³²) or
  XChaCha20-Poly1305 (preferred for random nonces at volume).
- Signatures: Ed25519 (audit checkpoints, release signing via cosign keyless/KMS, webhook
  `v1a`). Hashing: SHA-256 (records, chains), BLAKE3 permitted internally for dedup only.
- KDF: HKDF-SHA256. MAC: HMAC-SHA256. Random: OS CSPRNG (`getrandom`).
- Libraries: RustCrypto / `ring` / `aws-lc-rs`; no custom primitives. Post-quantum readiness:
  keep algorithm ids in ciphertext headers (crypto-agility).

**Key hierarchy**
```
Root key (KMS / HSM / Vault-OpenBao Transit; self-host fallback: file key sealed with age/passphrase)
 └── Platform KEK  (system plane secrets, pepper for tokens)
 └── Workspace KEK (one per workspace; held in key service, never in DB plaintext)
      └── DEKs (per object class/record: restricted variables, files, evidence, secrets, identity vault per subject)
```
- DEKs wrapped by workspace KEK and stored alongside ciphertext (`key_id`, `alg`, `wrapped_dek`).
- **Rotation**: KEKs rotate yearly (configurable) and on incident; rotation re-wraps DEKs
  (cheap), no bulk re-encryption needed. DEK rotation on re-write.
- **Crypto-shredding**: deleting a workspace destroys its KEK → all its DEKs unrecoverable
  (FR-S02); subject erasure destroys the per-subject DEK (PRV-02). Backups age out within
  the backup retention window; documented in the deletion certificate.
- **Secrets storage** for connector credentials/SMTP/AI keys: adapter interface with
  backends **OpenBao** (MPL-2.0, Linux Foundation fork of Vault), HashiCorp Vault (BSL — customer-supplied only, not bundled),
  AWS/GCP/Azure KMS + secret managers, and built-in encrypted table (envelope-encrypted
  with workspace KEK) as default for small installs.
- Secrets MUST NOT appear in IR, release bundles, events, logs, traces, error messages,
  exports or config-as-code (FR-W12). Log scrubbing filter with tests.

**Files as built (BPMS-060).** Chunks are AES-256-GCM under a per-file DEK (AAD `files:<id>:<chunk>`); the DEK is an envelope under the workspace KEK, so rotation rewraps it and shredding destroys it. Storage keys are `ws/<workspace>/<file>/<chunk>` in the local store (`PROCERA_FILES_DIR`; an S3-compatible backend is next). Malware scanning uses clamd over TCP (`PROCERA_CLAMD_ADDR`, `INSTREAM`); without it, a built-in scanner detects only the EICAR test file and the server logs a warning. Types are sniffed from content; executables, scripts, HTML and SVG are always rejected. Download links are HMAC tokens valid 5 minutes, bound to file, workspace and member, re-authorized per request and served with `nosniff` and `CSP: sandbox`.

**Previews as built (BPMS-061, FR-F04 P0 part).** Preview links are a separate token purpose (`file-preview`, its own HMAC context, so a download token never verifies as a preview token and the other way round), valid 2 minutes, bound to file, workspace and member and re-authorized per request exactly like downloads (active member, `file:download`, can see the file). Only clean files whose recorded type **and** a fresh sniff of the stored bytes are PNG, JPEG, GIF, WebP, PDF, DOCX or XLSX (whose zip central directory must name `word/document.xml` or `xl/workbook.xml`) or CSV (plain text named `*.csv`) are served for preview; anything else is 415, and SVG and HTML can never qualify (they are rejected at upload and are not on the allow-list). The `Content-Type` comes from the allow-list, never from stored metadata. Response headers: `Content-Disposition: inline`, `Content-Security-Policy: sandbox; default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'`, `X-Content-Type-Options: nosniff`, `Cross-Origin-Resource-Policy: same-origin`, `Referrer-Policy: no-referrer`, `Cache-Control: private, no-store`. Every preview is audited `file.previewed`. With `files.exif_strip` on (the default), JPEG APP1 (Exif, XMP), APP13 (IPTC) and comment segments are removed from the preview bytes by a dependency-free segment walker that copies the image data unchanged and refuses malformed files; the EXIF orientation tag goes with them, so sideways phone photos preview unrotated.

## 5. Application security controls

1. **Input validation**: typed DTOs (serde with `deny_unknown_fields`), JSON Schema for forms,
   max body 1 MB default (uploads separate), JSON depth ≤ 32, string length caps, unicode
   normalisation (NFC) for identifiers, reject control chars in names.
2. **Output encoding**: React auto-escaping; no `dangerouslySetInnerHTML` except through the
   sanitizer component (allowlist, DOMPurify with Trusted Types); emails rendered with
   escaping templates; CSV export neutralises formula injection (`=`,`+`,`-`,`@` prefix).
3. **CSP** (MUST, enforced in all environments including dev — a sibling-repo lesson:
   dev without CSP masked a prod-only breakage of an inline theme bootstrap):
   `default-src 'self'; script-src 'self' 'sha256-<bootstrap>'; style-src 'self' 'sha256-…';
   img-src 'self' data: blob:; connect-src 'self'; frame-src <preview-origin>;
   frame-ancestors 'none' (or embed allowlist); object-src 'none'; base-uri 'none';
   form-action 'self'; require-trusted-types-for 'script'`. Inline scripts only by sha256
   hash generated at build time; CSP violations reported to `/csp-report` (SEC-22).
4. **Other headers**: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`,
   `Permissions-Policy` minimal, `Cross-Origin-Opener-Policy: same-origin`,
   `Cross-Origin-Resource-Policy: same-site`.
5. **CORS**: none by default; explicit per-origin allowlist for embedding hosts (P2), no
   wildcard with credentials.
6. **Rate limiting** (token bucket; Valkey-backed when multi-node, in-memory single node):
   per IP (auth endpoints 10/min), per user (300/min), per workspace (quota FR-S04), per
   token; `429` + `Retry-After`; separate budgets for exports, search, AI.
7. **File upload pipeline** (FR-F01…F05):
   `receive (size cap, chunked) → hash → magic-byte sniff vs allowlist → store in quarantine prefix
   → ClamAV scan (clamd as separate GPL process via socket — not linked) → (optional CDR / image re-encode,
   EXIF strip) → mark clean → move to live prefix`. Archives: max ratio 100:1, max entries
   10 k, max depth 3. OOXML/ODF parsed with XXE disabled. Office→PDF conversion in a
   sandboxed worker (no network, seccomp, CPU/mem/time limits, ephemeral FS). Previews:
   images re-encoded, PDFs rendered client-side in a sandboxed iframe on a separate origin,
   SVG/HTML never served inline (`Content-Disposition: attachment`).
   *R1 as built (BPMS-061) is a subset of this design:*
   - Previews are served from the API origin, not a separate sandbox origin. The response's
     `CSP: sandbox` puts the document in an opaque origin with no script, forms, popups or
     navigation, which is the isolation the separate origin was for. A dedicated preview origin
     stays the target once deployments have a second host name.
   - Images are served as stored (JPEG metadata stripped), not re-encoded: re-encoding needs an
     image codec, and none is in the dependency set yet.
   - PDFs are shown by the browser's own viewer in a frame, not rendered client-side. The frame
     has **no** `sandbox` attribute: Chromium will not start its PDF viewer in any sandboxed frame
     (it shows a blocked-page icon even with `sandbox=""`), while the response-level CSP sandbox
     leaves the viewer working. Images use a plain `<img>`, which never runs script.
   - *Office preview spike note (P1):* convert DOCX/XLSX/PPTX to PDF in the sandboxed converter
     (the `convert/` worker in docs/04, no network, seccomp, CPU/memory/time limits) and preview the PDF through
     the same `file-preview` route; candidates and their licences are evaluated in SP-07 before
     any code.
8. **SSRF**: egress proxy for all connector/webhook/URL-fetch traffic (SSRF-01…07); the API
   process itself has no general outbound internet route.
9. **Multi-tenant query guard**: (a) RLS on every tenant table with `FORCE`; (b) repository
   functions require `&WorkspaceCtx`; (c) CI lint (custom `sqlx` query scanner) fails the
   build if a query touching a tenant table lacks `workspace_id` predicate or an explicit
   `// tenant-exempt: <reason>` reviewed by security CODEOWNERS; (d) tests run as the
   non-owner app role.
10. **Deserialization & parsing**: no polymorphic deserialization; BPMN XML import with
    entity expansion disabled, size ≤ 5 MB, element count ≤ 20 k.
11. **Expression sandbox**: step budget (default 10 k ops), memory budget (1 MB), max
    output size, no I/O functions; budget exhaustion → incident, never a hang (SEC-27).
12. **Error handling**: RFC 9457 problem details without stack traces or SQL; correlation id
    returned; detailed error only in logs.
13. **Authorization everywhere**: Cedar check per request + per row residuals (12 §12);
    generated route-table test ensures every handler declares an action (SEC-21).

## 6. Data protection

| Classification | Examples | At rest | In transit/display | Logging |
|---|---|---|---|---|
| `public` | Published catalog titles | Disk/DB encryption | Normal | Allowed |
| `internal` | Most process data | Disk/DB encryption | Normal | IDs only |
| `confidential` | HR cases, approvals comments | + access policy | Participants/grants | IDs only; access logged for admins |
| `restricted` | Salary, health, national id, secrets | **Field-level envelope encryption** | Redacted unless granted; never in events, notifications, search, AI prompts | Never; every read access-logged |

- Backups encrypted with a dedicated backup key (not the workspace KEKs) and stored in a
  separate account/bucket; restore requires two-person approval in hosted.
- Data residency (PRV-05): region pinning covers DB, object store, backups, search indexes,
  logs and AI inference endpoints; dedicated-database tier for strict cases.
- Test/staging never uses production data; synthetic fixtures only.

## 7. Audit, detection and admin access logging

- **Security event catalogue** (subset; all land in the workspace or system audit log):
  `auth.login.{success,failure}`, `auth.mfa.{enrolled,removed,failed}`, `auth.session.revoked`,
  `security.token_reuse`, `security.lockout`, `member.{invited,deactivated,role_changed}`,
  `role.{created,updated,deleted}`, `settings.changed`, `separation_mode.changed`,
  `breakglass.{requested,approved,used,expired}`, `audit.data_access` (restricted/confidential
  read by non-participant, incl. field names, never values), `file.{uploaded,quarantined,downloaded,deleted}`,
  `export.{requested,downloaded}`, `api_token.{created,used_first,revoked}`, `connector.secret.changed`,
  `authz.denied` (sampled for noise, always for cross-workspace attempts).
- Tamper-evidence per AUD-01…07; offline verifier (AUD-05).
- **Anomaly rules** (FR-T07), rule-based and explainable: mass download (> N files / 10 min),
  off-hours privileged change, repeated cross-workspace 404s from one token, MFA disabled on
  admin, new admin + export within 1 h, dormant admin reactivation, SoD override attempts.
- **SIEM export** (FR-T08): syslog/CEF, JSON over HTTPS, OTel logs; at-least-once with
  sequence numbers so gaps are detectable.
- System Admins see system-plane events only; workspace sees break-glass events about itself.

## 8. Secure SDLC and supply chain

| Practice | Requirement |
|---|---|
| Threat modelling | Every design doc touching a boundary adds a STRIDE table delta; security review sign-off at G-gates |
| SAST | `clippy -D warnings` (incl. `clippy::unwrap_used` in non-test), `cargo-deny` (advisories, bans, licences, sources), `cargo-audit`, Semgrep (Rust + TS rules), `eslint-plugin-security`, `tsc --strict` |
| Licence gate | `cargo-deny` + `license-checker`: **deny AGPL, SSPL, BSL/BUSL, Commons Clause, SUL, proprietary** in shipped artefacts; MPL/EPL allowed unmodified with notice; NOTICE file generated |
| DAST | OWASP ZAP baseline on every PR preview env; full active scan weekly |
| Fuzzing | `cargo-fuzz` targets: IR parser, expression parser, BPMN importer, form-schema validator, file sniffing |
| Secrets | Gitleaks pre-commit + CI; GitHub push protection; token prefix pattern registered |
| SBOM | CycloneDX for Rust, npm and container images per release |
| Signing & provenance | Container images and binaries signed with **Sigstore cosign**; SLSA provenance via GitHub OIDC builders; verification documented for customers |
| Reproducibility | Goal: reproducible builds for the server binary (pinned toolchain via `rust-toolchain.toml`, `--locked`, `SOURCE_DATE_EPOCH`) |
| Dependency hygiene | Renovate with grouped updates, 7-day cool-off for new major versions, mandatory review for new direct dependencies |
| Pentest | Independent pentest before pilot (scoped) and before GA (full, incl. multi-tenant) and annually |
| Disclosure | `SECURITY.md` with security@ contact, PGP/age key, 90-day coordinated disclosure, GitHub private advisories; bug bounty after GA |
| Patch SLAs | Critical 72 h, High 7 d, Medium 30 d for supported versions (latest minor + previous) |

## 9. Reliability and SLOs

| SLI | SLO (pilot → GA hosted) | Error budget / month |
|---|---|---|
| API availability (non-5xx, excluding 429) | 99.9 % → 99.95 % | 43 min → 21.6 min |
| P95 read latency (reference load) | < 300 ms | 5 % of requests may exceed |
| P95 state-transition latency (excl. connectors) | < 500 ms | — |
| Timer firing | 99.99 % fired within 60 s of due at nominal load | — |
| Outbox delivery lag | P99 < 10 s | — |
| Email notification enqueue → handed to SMTP | P95 < 60 s | — |
| Audit write durability | 100 % (same transaction as action) | 0 |

Policy: when the monthly budget is exhausted, feature releases pause in favour of
reliability work. **Reference load** for capacity tests: 100 workspaces, 10 k members,
10 k waiting instances, 100 transitions/s burst, 1 M historical events, 10 M audit rows,
500 GB files. These are benchmark targets, not proven capacity claims.

## 10. Observability

- **Metrics** (Prometheus/OTel): request rate/error/latency by route; transition rate and
  failures; runnable-token backlog; timer lateness histogram; outbox lag and attempts;
  connector retries and DLQ size; email bounces; auth failures and lockouts; authz denies
  (by reason); DB pool saturation, lock waits, replica lag; scan queue depth and quarantine
  count; storage per workspace; publication failures; SOP review overdue; quota utilisation.
- **Traces**: W3C trace context end-to-end (browser → API → worker → connector); spans tagged
  with `workspace_id` (hashed in hosted shared telemetry), `instance_id`, never PII/values.
- **Logs**: structured JSON; PII/secret scrubbing layer; correlation id; retention 30 d
  (operational class).
- **Dashboards** for `system.operator`: platform overview, runtime health, queues & timers,
  database, storage & scanning, security events, per-workspace usage. Workspace-scoped
  equivalents are product features (FR-A01…A05).
- **Alerts**: sustained backlog, timer drift > 60 s, outbox lag > 1 min, DLQ spikes, replica
  lag, error-budget burn rate (fast 2 %/1 h, slow 5 %/6 h), cert expiry < 14 d, backup failure.

## 11. Backup, DR and incident response

- **Postgres**: continuous WAL archiving + base backups with **pgBackRest** (MIT) or
  CloudNativePG/Barman on Kubernetes; PITR window 14 d (configurable); encrypted; one
  off-site copy. **RPO ≤ 15 min, RTO ≤ 2 h**.
- **Object store**: versioning on, replication to a second site/bucket; optional Object Lock
  for evidence/audit exports (AUD-04). DB and object backups correlated by timestamp so a
  restore yields consistent references (orphan/missing-object reconciliation job).
- **Keys**: root/KEK backup procedure (sealed, split knowledge for hosted); loss of keys =
  loss of data, documented prominently for self-hosters.
- **Restore drills** quarterly (hosted) with measured RTO; self-host guide includes a drill script.
- **Severity matrix**

| Sev | Definition | Response | Customer comms |
|---|---|---|---|
| SEV-1 | Cross-workspace exposure, data loss, full outage, active exploitation | Page immediately, IC assigned, 15 min ack | Affected workspace owners within 24 h (72 h regulatory clocks tracked: GDPR Art. 33, DPDP) |
| SEV-2 | Major feature down, timers/transitions delayed > 15 min, security vuln with no exploitation | 30 min ack | Status page |
| SEV-3 | Degradation, single-workspace issue | Next business day | Ticket |

- Runbooks: credential leak, token reuse storm, key compromise (KEK rotation + re-wrap),
  ransomware/backup restore, runaway workflow (suspend process/version), connector abuse
  (disable connector, egress block), mass-quarantine event, audit chain verification failure.
- Post-incident review within 5 business days; blameless; tracked actions.

## 12. Deployment hardening

- Containers: distroless/`scratch` base for Rust binaries, **non-root** UID, read-only root
  FS, no shell, `seccomp: RuntimeDefault`, `capabilities: drop [ALL]`, `allowPrivilegeEscalation: false`.
- Kubernetes: NetworkPolicies default-deny; API may reach Postgres, object store, Valkey,
  key service, broker; only connector workers reach the egress proxy; conversion/scan
  workers have no egress. PodSecurity `restricted`. Secrets via external secrets operator
  / CSI driver. PodDisruptionBudgets and anti-affinity for API/worker.
- Helm values expose: isolation tier, external Postgres/S3/Valkey/NATS/Kafka endpoints,
  IdP, key service, egress proxy, resource limits, `airgap: true` (no outbound calls,
  bundled ClamAV signatures updated from an internal mirror, local AI model optional).
- Compose (trial): single host, still non-root, CSP on, TLS via bundled reverse proxy with
  self-signed or ACME; banner "evaluation deployment" until hardening checklist completed.
- Default dependencies (licence-checked): PostgreSQL 16+, S3 API (SeaweedFS or RustFS as
  defaults, any S3 compatible), Valkey (optional), NATS JetStream or Kafka (optional;
  Postgres outbox default), Keycloak (reference IdP, optional), ClamAV (separate process).
  **Not bundled**: MinIO (archived/AGPL), Redis 8 (AGPL/SSPL/RSAL), Redpanda (BSL).

## 13. Security test matrix

| ID | Test | Pass condition | Phase |
|---|---|---|---|
| SEC-01 | Cross-workspace process read | 404 + `authz.denied` audit | P1 alpha |
| SEC-02 | Cross-workspace task approve | 404; no state change | Beta |
| SEC-03 | Field-level redaction parity (API, export, search, notification, AI prompt) | Identical redaction everywhere | Beta |
| SEC-04 | Revoked/deactivated identity cannot act | Immediate 401/403; sessions/tokens dead | Alpha |
| SEC-05 | Webhook replay / bad signature / stale timestamp | Rejected; no duplicate effect | RC |
| SEC-06 | Connector SSRF (private IPs, metadata) | Blocked at egress proxy | RC |
| SEC-07 | Audit export scoped to workspace and permission | No foreign rows | Alpha |
| SEC-08 | Privilege escalation via delegation | Blocked; delegate ≤ delegator | Beta |
| SEC-09 | Unsafe BPMN (scripts, XXE, entity bombs) | Rejected with diagnostics | Beta |
| SEC-10 | Signed URL expiry & workspace binding | Expired/foreign → denied | Alpha |
| SEC-11 | Session fixation | Session id rotated on login/MFA/step-up | Alpha |
| SEC-12 | Brute force / credential stuffing | Backoff + lockout + alert; no user enumeration | Alpha |
| SEC-13 | CSRF on every mutating route | Missing/foreign Origin or token → 403 | Alpha |
| SEC-14 | Refresh token reuse | Family revoked; `security.token_reuse` | Alpha |
| SEC-15 | Magic-link replay / expiry / wrong item | Rejected | RC |
| SEC-16 | MFA bypass attempts (skip step, TOTP replay) | Denied | Alpha |
| SEC-17 | Step-up required actions without recent auth | 401 `step_up_required` | Beta |
| SEC-18 | Signed URL used from another workspace session | Denied | Alpha |
| SEC-19 | Quarantine bypass (download before clean, EICAR) | Never served | Alpha |
| SEC-20 | Malicious file types: SVG XSS, HTML, polyglot PDF, zip bomb, OOXML XXE | Neutralised/rejected | Beta |
| SEC-21 | Route-table authz coverage + BFLA (member calling admin APIs) | Every route declares action; denied | Alpha |
| SEC-22 | Stored/DOM XSS corpus in SOP, comments, labels; CSP violation reports | No execution; CSP blocks | Alpha |
| SEC-23 | Admin access-logging of restricted reads | Every read produces `audit.data_access` | Beta |
| SEC-24 | Rate limits (auth, API, export, search) | 429 with Retry-After; no starvation of other workspaces | Beta |
| SEC-25 | SSRF variants: DNS rebinding, redirects, IPv6-mapped, decimal IPs | Blocked | RC |
| SEC-26 | Archive limits (ratio, depth, entries) | Rejected safely | Beta |
| SEC-27 | Expression/rule budget exhaustion, deep JSON | Incident raised; worker healthy | Beta |
| SEC-28 | Worker isolation: conversion/scan worker egress & DB access | None possible | RC |
| SEC-29 | Custom-role escalation (grant > own, self-grant role:manage) | Rejected; step-up demanded | Alpha |
| SEC-30 | Email-action link GET (prefetch) mutation | No mutation; confirm page only | RC |
| SEC-31 | Cross-workspace via cache/search/notification paths | No leakage | Beta |
| SEC-32 | Query without tenant predicate (lint + RLS with missing setting) | Build fails; runtime 0 rows | Alpha |
| SEC-33 | Separation mode: admin content access removed; self-grant needs 2nd approver | Enforced; audited | RC |
| SEC-34 | System admin content access without break-glass; break-glass visibility | Denied; banner + workspace audit present | RC |
| SEC-35 | Audit chain tampering (update, delete, reorder, truncate) | Verifier detects; DB role cannot update/delete | Alpha (chain) / RC (checkpoints) |
| SEC-36 | AI prompt injection from content; restricted data in prompts | No privileged action; restricted never sent | P2 (with AI) |
| SEC-37 | Supply chain: unsigned/unprovenanced artefact, denied licence, known-vuln dep | Release pipeline fails | Alpha |

## 14. Recovery and chaos tests

Kill API during commit; kill worker after external call but before ack; duplicate broker
message; DB failover; lost broker/object-store connectivity; outbox replay; clock skew ±5 min
on a node; expired leases; missing assignee; revoked employee mid-approval; rotated
integration credential; corrupted attachment; restore from snapshot + WAL; 30-day idle
instance recovery; **KEK rotation under load**; **scanner outage** (uploads queue, never
bypass); **Valkey loss** (sessions fall back to Postgres, rate-limits degrade safely);
**egress proxy down** (connectors incident, no direct egress); **workspace deletion during
active instances**; **restore of one workspace** from backup without affecting others
(dedicated-DB tier; shared tier via logical export).

## 15. Shared-responsibility matrix

| Area | Self-hosted (customer) | Hosted (vendor) | Product provides |
|---|---|---|---|
| Infrastructure, OS, network, TLS certs | Customer | Vendor | Hardening guide, Helm defaults |
| Postgres/object store HA & backups | Customer | Vendor | Backup tooling docs, drill script |
| Key root (KMS/HSM) | Customer | Vendor | Key service adapters, rotation commands |
| Identity provider, MFA policy | Customer | Shared | Local auth + MFA, OIDC, policy floors |
| Workspace roles, SoD, retention configuration | Customer | Customer | Catalogues, invariants, previews |
| Patching product versions | Customer | Vendor | Signed releases, advisories, patch SLAs |
| Monitoring & incident response | Customer | Vendor (+ notify customer) | Metrics, dashboards, runbooks |
| Compliance validation (Part 11, SOC 2 evidence) | Customer | Customer (vendor supplies reports) | Validation pack, controls mapping |
| Data subject requests | Customer (controller) | Customer, vendor assists (processor) | DSAR tooling (PRV-01) |
| Pentest of deployment | Customer | Vendor | Product pentest summary |

## Sources

- OWASP ASVS 5.0.0 (released 30 May 2025): <https://owasp.org/www-project-ASVS/> · <https://softwaremill.com/whats-new-in-asvs-5-0/>
- OpenBao (MPL-2.0, Linux Foundation): <https://wz-it.com/en/blog/openbao-vs-vault-comparison/>
- pgBackRest (MIT): <https://pgbackrest.org/>
- Standard Webhooks, OWASP SSRF, RFC 3161/9162: see [14 §Sources](14-DATA-GOVERNANCE-AND-COMPLIANCE.md#sources)
- RFC 6238 (TOTP), RFC 9449 (DPoP), RFC 9457 (problem details), RFC 7636 (PKCE), NIST SP 800-63B-4, W3C WebAuthn Level 3.
