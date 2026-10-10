# 13 — Integrations (standalone first, integrations later)

Status: **Draft for G1 sign-off** · Owner: Integrations lead · Closes gap C1 (re-scoped) and A4.

Product direction (2026-10-11): Procera targets every company size (startup → MNC) and every department, and is designed and shipped **standalone**. Every P0 capability works with zero external systems. Integrations are **additive adapters** that come in P1/P2. The seams are designed now so that adding them later needs no core rework.

## 1. Standalone baseline (P0, zero integrations)

| Capability | Built-in implementation | Later adapter (port) |
|---|---|---|
| Authentication | Local accounts, Argon2id, TOTP + WebAuthn MFA, sessions ([12 §3](12-AUTHORIZATION-MODEL.md)) | `IdentityProvider`: OIDC, SAML via broker |
| Directory | Built-in members, departments, groups, positions, manager links; CSV import | `DirectorySource`: SCIM, HRIS, chat platforms |
| Notifications | In-app notification centre + **SMTP email** (configured by System Admin, sender name per workspace) | `NotificationChannel`: webhook, chat platforms, push |
| Files | S3-compatible object store (any vendor) + scan pipeline (ClamAV container) | `FileConverter`: office preview, OpenDoc |
| Decisions/logic | Rule tables + expression language (11-IR §6, §8) | DMN import (P2) |
| Calendars | Built-in business calendars and holiday sets | iCal feed import, Google/M365 |
| Async delivery | Postgres transactional outbox + in-process dispatcher (`LISTEN/NOTIFY` wake-ups, polling as truth) | NATS JetStream / Kafka adapters (ADR-0009) |
| AI | Off by default | `AiProvider`: local model / BYO API ([16](16-AI-ASSISTANCE.md)) |
| Automation from outside | Public REST API + API tokens | Inbound events, webhooks |

A startup on the Starter profile ([20 §0](20-WORKSPACE-CONFIGURATION.md)) never needs to touch an integration. An MNC typically adds SSO + SCIM (P1) first, then HRIS and chat.

**Rule:** a P0 feature may not *require* an adapter other than the built-in one. CI runs the full E2E suite against a Compose stack containing only `web`, `api`, `worker`, `postgres`, `object-store` and `clamav`, with SMTP pointed at a local test catcher.

## 2. Integration architecture (designed in now, shipped later)

### 2.1 Ports

Each port is a Rust trait in `crates/application/ports`. Adapters live in `crates/infra/*` (first-party, in-process) or run as external connectors (§2.3).

| Port | Responsibility | Key methods (sketch) | Called from |
|---|---|---|---|
| `NotificationChannel` | Deliver a rendered, localised notification to a principal | `deliver(envelope) -> DeliveryReceipt`, `capabilities()` (actionable? rich?) | Notification dispatcher (after commit) |
| `DirectorySource` | Pull or receive users, groups, org units, manager links | `sync(cursor) -> DirectoryDelta`, `on_push(delta)` | Directory sync job / SCIM endpoint |
| `IdentityProvider` | Federated authentication and step-up | `authorize_url`, `exchange(code)`, `step_up(acr)` | Auth module |
| `ConnectorAction` | A typed action invoked by `service.call` nodes | `invoke(action, input, idempotency_key) -> Result<Output, TypedError>` | Connector worker (after commit) |
| `InboundEvent` | Verified external events that start instances or satisfy `wait.signal` | `verify(request) -> Event`, `correlate(event)` | Inbound endpoint |
| `FileConverter` | Preview/convert files (docx→pdf, md↔blocks) | `convert(file_ref, target) -> file_ref` | Sandboxed conversion worker |
| `AiProvider` | Completion/embedding with tenant isolation | `complete(req) -> resp`, `embed` | AI assistant service |
| `SecretProvider` | Resolve secret references | `resolve(secret_ref) -> SecretBytes` (never logged) | Connector worker |

Contract rules:
- Every adapter call happens **after** the DB transaction commits, driven by the outbox. No adapter is ever called while holding a row lock.
- Every adapter call is **idempotent from the caller's side**: it carries an idempotency key derived from `(workspace_id, instance_id, token_id, attempt_group)`.
- Inputs and outputs are validated against JSON Schemas declared in the connector manifest. Oversized or invalid output raises a typed error and an incident (FR-E13).
- Restricted variables (11-IR §5) can be passed to a connector only if the manifest declares the `restricted_data` capability **and** the workspace admin granted it for that connector.

### 2.2 Connector manifest (SDK contract)

```jsonc
{
  "manifestVersion": 1,
  "key": "http-generic",              // globally unique, reverse-DNS for third parties
  "version": "1.2.0",
  "publisher": "procera",
  "kind": ["action", "inbound", "notification"],
  "capabilities": ["restricted_data:false", "actionable_notifications:false"],
  "scopes": ["outbound.http"],        // what the connector may do
  "egress": { "hosts": ["*.example.com"], "ports": [443] },   // workspace config narrows further
  "configSchema": { /* JSON Schema 2020-12: non-secret settings */ },
  "secrets": [{ "name": "api_key", "description": "Bearer token" }],   // references only
  "actions": [{
    "key": "post_json",
    "input":  { "$ref": "#/defs/PostInput" },
    "output": { "$ref": "#/defs/PostOutput" },
    "errors": ["HTTP_4XX", "HTTP_5XX", "TIMEOUT"],
    "timeoutMs": 15000,
    "idempotent": true
  }],
  "inbound": [{ "key": "event", "verification": "standard-webhooks" }],
  "license": "Apache-2.0"
}
```

Lifecycle:
1. The **System Admin** installs or allows a connector at platform level (allowlist).
2. The **Workspace Admin** enables it for their workspace, fills in the config, and stores secrets via `connector:manage_secrets`. Secrets go to the secret store; only refs are kept.
3. **Process Designers** can then pick its actions in the builder.

Disabling a connector raises incidents on running instances that need it. It never silently skips them.

### 2.3 Execution isolation

- **First-party connectors** (HTTP, webhook, email) run inside `apps/connector-runner`, a separate process/container with no DB credentials. It receives work from the dispatcher over an internal queue and returns results through an authenticated callback.
- **Third-party connectors** (P2) run as separate containers speaking a small gRPC/HTTP protocol (`connector-protocol v1`). No untrusted code ever runs in the API process (HLD extensibility rule).
- Every outbound connection goes through the **egress proxy** (Smokescreen pattern; SSRF-01…07 in [14 §1.7](14-DATA-GOVERNANCE-AND-COMPLIANCE.md)). DNS is resolved and checked against blocked ranges, the connection goes to the validated IP, redirects are re-validated, and response size and time are capped. Self-hosted admins may allowlist internal CIDRs explicitly; this is audited and denied by default.
- Per-workspace and per-connector concurrency limits and token-bucket rate limits provide backpressure (HLD §consistency).

### 2.4 Delivery pipeline

```
txn: state change + outbox row ──commit──► dispatcher (claims with SKIP LOCKED, lease)
   ──► route by kind: notification | connector action | webhook | projection
   ──► attempt (idempotency key) ──► success: mark done, record receipt, emit completion signal to runtime
                                └─► retryable failure: backoff w/ jitter (5s, 30s, 2m, 10m, 30m, 2h … ≤24h)
                                └─► terminal / exhausted: DLQ row + incident (FR-E13) + alert (FR-A07)
```

- Ordering is guaranteed **per instance** (partition key `instance_id`), not globally.
- The DLQ is shown in the workspace Operations monitor (FR-A05) with replay, skip and cancel. Replays reuse the original idempotency key.

### 2.5 Webhooks (P1)

- **Outbound:** the Standard Webhooks spec (WH-01…04). Headers `webhook-id`, `webhook-timestamp`, `webhook-signature`; Ed25519 `v1a` preferred, HMAC `v1` supported; multiple signatures during rotation. Thin payloads (≤ 20 KB, ids + type, no restricted data). The receiver fetches details through the API with its own token. A `410` response disables the endpoint. The delivery log and replay are in the UI.
- **Inbound** (WH-05): verified by the same spec with per-endpoint secrets, a 5-minute tolerance and `webhook-id` dedup (`inbox_dedup`). An inbound event resolves its workspace from the endpoint, never from the payload.

## 3. Phased integration catalogue

| ID | Integration | Port(s) | Phase | Notes |
|---|---|---|---|---|
| INT-001 | OIDC SSO (any IdP; Keycloak is the reference) | IdentityProvider | P1 | Per-workspace config; auth code + PKCE; domain-verified auto-join; step-up via `max_age`/`acr_values` |
| INT-002 | SAML 2.0 via broker IdP | IdentityProvider | P1 | No in-process XML-DSig ([ADR-0011](adr/ADR-0011-authorization-engine.md)) |
| INT-003 | SCIM 2.0 server (Users, Groups, enterprise `manager`) | DirectorySource | P1 | Own endpoints over the `scim_v2` types; per-workspace bearer token |
| INT-004 | Outbound webhooks | NotificationChannel | P1 | Standard Webhooks |
| INT-005 | Generic HTTP action | ConnectorAction | P1 | JSON in/out, templated body, typed error mapping |
| INT-006 | Inbound events API | InboundEvent | P1 | Start instance / signal correlation |
| INT-007 | iCal holiday/calendar feed import | (calendar import) | P1 | Read-only, scheduled refresh, creates a new calendar version |
| INT-008 | Web push (PWA) | NotificationChannel | P1 | VAPID; no content in payload, just "you have work" + deep link |
| INT-009 | Chat/host-platform connector framework (§4) | IdentityProvider, DirectorySource, NotificationChannel, embed | P2 | Shared contract for all chat platforms |
| INT-010 | Slack / Microsoft Teams-style chat cards | NotificationChannel (actionable) | P2 | Built on the §4 contract |
| INT-011 | Google Workspace / Microsoft 365 calendar & mail | NotificationChannel, calendar | P2 | OOO detection → suggested substitution (never automatic) |
| INT-012 | HRIS: generic CSV/SFTP drop | DirectorySource | P2 | Scheduled, schema-mapped |
| INT-013 | HRIS via HTTP (BambooHR/Workday-style APIs) | DirectorySource, ConnectorAction | P2 | Manager links, hire/termination events → onboarding/offboarding templates |
| INT-014 | ERP/accounting via HTTP (PO creation, vendor master) | ConnectorAction | P2 | Hook points in `fin.*` templates |
| INT-015 | E-signature QTSP (CSC API v2, PAdES) | ConnectorAction | P2 | ESIG-09 QES path; plugs into the document e-signing module as signature type `qes` ([20 §2.16](20-WORKSPACE-CONFIGURATION.md)) |
| INT-016 | OpenDoc / CasualOffice document engine | FileConverter | P2 | `.docx` SOP import → blocks, high-fidelity preview, DOCX export |
| INT-017 | SIEM streaming (syslog/CEF, JSON-HTTPS, OTel logs) | (audit stream) | P1/P2 | FR-T08 |
| INT-018 | NATS JetStream / Kafka event bus | (bus adapter) | P2 | For large installs; outbox stays the source of truth |
| INT-019 | Third-party connector protocol v1 + SDK (Rust, TS) | ConnectorAction | P2 | Sandboxed containers |
| INT-020 | Template hook points (connectors mapped at install) | — | P2 | E.g. `hr.employee-onboarding` "provision accounts" step bound to HTTP/HRIS |
| INT-029 | Announcements & campaign reminders to chat channels (Slack/Teams/other chat cards with Ack button → confirm + re-validate) | NotificationChannel (actionable) | P2 | Same card contract as approvals (§4.4); policy/e-sign acks needing step-up show Open only |
| INT-030 | HRIS joiner events → `policies.attest_on_join` campaign enrolment | DirectorySource | P2 | Standalone path uses member-created event |

## 4. Chat & host-platform integration pattern (P2)

This one vendor-neutral contract covers any messaging or collaboration platform that can host apps, send interactive cards or act as an identity provider. Examples are Slack, Microsoft Teams, Google Chat, Mattermost, Rocket.Chat and Zulip. Each platform is its own connector that implements this contract. Procera never depends on any of them: everything below is behind a connector plus workspace toggles. Items marked **OQ-C** must be verified for each platform before its connector is built.

### 4.1 Workspace ↔ platform tenant mapping

- One Procera workspace maps to **at most one** tenant per platform connector. A platform tenant may map to several workspaces (e.g. HR, Finance), each with its own connector config.
- The Workspace Admin creates the mapping through the platform's app-install / admin-consent flow. It is stored as `connector_configs(<platform>){tenant_id, domain, scopes}`.

### 4.2 Identity federation

- **Preferred:** the platform, or the customer's IdP behind it, acts as an OIDC provider for the workspace (INT-001 reused). Users are matched by `iss`+`sub`. An existing local account is linked only after the user proves ownership.
- **Embedded sessions:** OAuth 2.0 Token Exchange (RFC 8693). The host passes a short-lived, audience-restricted platform token via `postMessage`, and Procera verifies it server-side (JWKS) and exchanges it for a workspace-scoped session. Client claims are never trusted.
- Step-up for Part 11 signatures always happens in Procera's own re-auth flow, even when embedded.

### 4.3 Embed / host contract

- Procera pages (My Work, request detail, start request, SOP/policy view, signing ceremony) can run inside a host iframe or tab under `/embed/*` routes with a compact chrome.
- `Content-Security-Policy: frame-ancestors` is generated from the workspace's allowlist of host origins. `X-Frame-Options` is not used. Embed sessions use `SameSite=None; Secure; Partitioned` cookies (CHIPS).
- The `postMessage` protocol is versioned and origin-checked in both directions. Messages are `{v:1, type, id, payload}`:
  - host → app: `init{token, locale, theme}`, `navigate{route}`
  - app → host: `ready`, `resize{height}`, `notify{count}`, `openExternal{url}`, `close`
  - Unknown types are ignored. No message carries restricted data.
- Theming follows the workspace branding (FR-W04) and the host's `init.theme` (light/dark), with automatic contrast enforcement (05 §7).

### 4.4 Actionable cards (approvals, acknowledgements, reminders)

- When work is assigned and the assignee has the platform linked, a card is sent with the title, requester, key non-restricted fields, due date and buttons (Approve / Reject / Open, or Acknowledge / Open).
- **A button press never commits directly.** It opens a confirm step. The API then re-validates identity (token exchange), assignment, SoD, current revision (`If-Match`) and idempotency (key = card action id). A stale card returns "already decided by X".
- Part 11 processes, restricted fields, required comments and signatures needing step-up show only **Open**.
- Cards are updated in place after a decision where the platform supports it.

### 4.5 Notifications and directory

- The `NotificationChannel` adapter respects user channel preferences, workspace quiet hours and digests (FR-N04). If delivery fails, email is the fallback, per workspace setting.
- `DirectorySource` sync (where the platform exposes it) pulls users, groups and manager links with per-field source priority ([20 §2](20-WORKSPACE-CONFIGURATION.md)).
- Deep links open the embed route, and permissions are re-checked on open.

### 4.6 Per-platform verification checklist (OQ-C)

| ID | Question to verify per platform | Needed by |
|---|---|---|
| OQ-C1 | App install / admin consent model and scopes | Connector design start |
| OQ-C2 | Token format, JWKS, audience restriction, token-exchange support | Connector design start |
| OQ-C3 | Interactive card schema, callbacks, update/delete, rate limits | Connector design start |
| OQ-C4 | Directory / org-chart API, SCIM, manager attribute | Connector design start |
| OQ-C5 | Embedding constraints (allowed origins, mobile WebView, cookie partitioning) | Connector design start |
| OQ-C6 | Data residency of the platform vs Procera hosting regions | Before customer rollout |

## 5. Public API, SDKs and CLI

- **REST API**, versioned in the URL (`/v1`), with only additive changes inside a major version. A major version is deprecated with ≥ 12 months' notice plus `Deprecation`/`Sunset` headers (RFC 8594/9745). Errors are RFC 9457 problem details. Lists use cursor pagination. Mutating commands take `Idempotency-Key`, and updates use `If-Match`. Detail is in [08-API-SPEC](08-API-SPEC.md).
- **OpenAPI 3.1** is generated from code (`utoipa`) and checked in CI. A breaking-change diff (`oasdiff`) is a blocking gate.
- **SDKs** are generated for TypeScript, Python and Rust from the spec, published per release with semantic versions tied to the API major.
- **CLI** `procera` (Rust): login (device code), workspace switch, import/export process bundles and config-as-code (FR-W12, FR-P09), test-run, `procera-verify` for audit bundles (AUD-05). It's built for CI pipelines that promote sandbox → prod.
- **Events catalogue**: every domain event type is published with a JSON Schema and `schema_version` (04-LLD §events). Breaking changes create a new `.vN` type.

## 6. Integration backlog

| ID | Item | Phase | Size | Depends on |
|---|---|---|---|---|
| INT-021 | Port traits + in-process built-in adapters (SMTP, in-app, local directory, local auth) | P0 | M | Outbox (EXEC) |
| INT-022 | Outbox dispatcher with leases, per-instance ordering, retries, DLQ, incidents | P0 | L | Runtime core |
| INT-023 | SMTP adapter with localised templates, bounce handling (VERP/webhook optional) | P0 | M | INT-021 |
| INT-024 | Connector manifest schema + registry + platform allowlist + workspace enablement UI | P1 | M | Config (CFG) |
| INT-025 | Connector-runner process + egress proxy + SSRF test suite | P1 | L | INT-024 |
| INT-026 | Secret store abstraction (DB envelope-encrypted default; Vault/KMS adapters) | P1 | M | Key hierarchy |
| INT-001…008 | P1 catalogue items (§3) | P1 | S–L each | INT-024/025 |
| INT-009…020 | P2 catalogue items (§3) | P2 | M–L each | Pilot feedback; OQ-M answers |
| INT-027 | OpenAPI generation + oasdiff CI gate + SDK generation pipeline | P0 (spec) / P1 (SDKs) | M | API skeleton |
| INT-028 | `procera` CLI (auth, import/export, test-run, verify) | P1 | M | INT-027, FR-W12 |
