# 08 — API specification, conventions and event contracts

Status: Draft v0.2 · Owner: Tech lead · Supersedes [v0.1 examples](archive/v0.1/08_API_AND_EVENT_EXAMPLES.md).
The OpenAPI 3.1 document is **generated from code** (`utoipa`) and published per release. This doc defines the conventions it must follow and the resource map.

## 1. Surfaces

| Surface | Base | Auth | Audience |
|---|---|---|---|
| Workspace API | `/api/v1/…` | Session cookie (web) or Bearer workspace token | Web app, integrations, CLI |
| System API | `/sysapi/v1/…` (separate host/route) | System-plane session or token, MFA | System console, ops automation |
| Public signing/guest | `/g/v1/…` | Single-use magic link + OTP | External signers/guests (P1) |
| Webhooks (outbound) | Customer URL | Standard Webhooks signature | Integrations (P1) |
| Inbound events | `/api/v1/events/inbound/{connector}` | HMAC / mTLS | Connectors (P1) |
| Health | `/healthz`, `/readyz`, `/metrics` (internal network) | none / internal | Operators |

## 2. Conventions

- **Versioning.** The major version goes in the path. Changes within a major are additive only: new fields, endpoints and enum values (clients must tolerate unknown enum values). Deprecation is announced with `Deprecation` and `Sunset` headers at least 6 months ahead.
- **Resources.** Plural nouns. Ids are opaque strings, UUIDv7, with a prefixed human `display_id` where useful (`PR-2026-000123`).
- **Methods.** `GET` is safe and never mutates (this includes email links). `POST` creates resources and runs commands as **action sub-resources**: `POST /…/{id}/cancel`, `/…/{id}/suspend`, `/…/{id}/decisions` (plain paths; no `:verb` syntax). `PATCH` uses JSON Merge Patch for drafts. Settings are written one key per `PUT /settings/{key}`. `DELETE` applies only where retention allows.
- **Concurrency.** Every mutable resource returns `ETag: "<revision>"`. Updates require `If-Match`, and a stale revision returns `412` (or `409` for domain conflicts).
- **Idempotency.** `Idempotency-Key` is required on all non-idempotent `POST` commands. The scope is (workspace, actor, operation, key) and keys are kept for 24 h. A replay with the same body gets the stored response. A different body gets `409 idempotency_key_reused`.
- **Pagination.** Cursor-based: `?page[size]=50&page[cursor]=…`. Responses include `next_cursor`. There are no offset pages on large collections.
- **Lists.** These use the shared query model (04-LLD §5). `GET` takes simple params (`?filter[state]=running&sort=-due_at`), and `POST /…/query` takes the full JSON filter AST.
- **Field selection.** `?fields=id,title,state` is optional. Expansions use `?include=assignee,process`.
- **Time.** RFC 3339 with offset in requests; responses are always UTC `Z`. Durations are ISO 8601.
- **Money.** `{ "amount": "1234.50", "currency": "INR" }`. The amount is a string, never a float.
- **Localisation.** `Accept-Language` affects messages only, never data.
- **Rate limits.** `RateLimit-Policy` / `RateLimit` headers (IETF draft) and `429` with `Retry-After`.
- **Redaction.** Restricted values the caller may not read come back as `{"redacted": true, "reason": "classification"}`.

## 3. Errors: RFC 9457 problem details

```json
HTTP/1.1 422 Unprocessable Content
Content-Type: application/problem+json
{
  "type": "https://docs.procera.dev/errors/validation_failed",
  "title": "Validation failed",
  "status": 422,
  "code": "validation_failed",
  "detail": "2 fields are invalid",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "errors": [ { "pointer": "/fields/amount", "code": "minimum", "message": "Must be ≥ 1" } ]
}
```

| Status | `code` examples | Notes |
|---|---|---|
| 400 | `bad_request`, `unknown_field`, `invalid_filter` | Unknown fields are rejected |
| 401 | `unauthenticated`, `session_expired` | — |
| 403 | `forbidden`, `step_up_required` (+ `acr`/`max_age` hints), `sod_violation` | Only when the caller can know the object exists |
| 404 | `not_found` | Also used for objects hidden from the caller (disclosure policy) |
| 409 | `conflict`, `state_conflict`, `idempotency_key_reused`, `already_decided` | — |
| 412 | `precondition_failed` | Stale `If-Match` |
| 413/415 | `payload_too_large`, `unsupported_media_type` | — |
| 422 | `validation_failed`, `compile_failed` (with IR diagnostics) | — |
| 423 | `workspace_suspended`, `record_locked`, `legal_hold` | — |
| 429 | `rate_limited`, `quota_exceeded` | — |
| 5xx | `internal`, `unavailable` | Never leaks internals |

## 4. Resource map: workspace API (v1)

| Area | Endpoints (abridged) | Key permissions |
|---|---|---|
| Authentication | `POST /auth/login` (`{email,password}` → tokens, or `{status:"mfa_required", mfa_token, methods}` with `methods` ⊆ `totp, webauthn, recovery_code`), `POST /auth/mfa` (`{mfa_token}` plus exactly one of `{code}` (TOTP or recovery code) or `{credential}` (passkey assertion, `amr` gets `hwk`)), `POST /auth/mfa/webauthn/options` (`{mfa_token}` → `{public_key}` request options with a single-use 5-minute challenge), `POST /auth/refresh`, `POST /auth/logout` | public / mfa token / refresh cookie |
| MFA enrolment | `POST /me/mfa/totp`, `POST /me/mfa/totp/confirm`; passkeys (BPMS-022, WebAuthn L3): `GET /me/mfa/webauthn` (`{items:[{id,name,created_at,last_used_at,synced,user_verified,transports}]}`), `POST /me/mfa/webauthn/options` (→ `{public_key}` creation options, attestation `none`), `POST /me/mfa/webauthn` (`{name, credential}` → 201 `{passkey, recovery_codes}`; codes only for the first second factor; 409 `passkey_exists`, 422 `passkey_invalid`), `DELETE /me/mfa/webauthn/{id}` (step-up within 5 minutes; 404 for another user's passkey) | self |
| Session/me | `GET /me` (user carries `mfa_enabled` and `mfa_methods`), `GET /me/workspaces`, `POST /session/workspace` (switch), `POST /session/step-up` (`{code}` with TOTP or `{credential}` with a passkey when MFA is on, else `{password}`; returns an access token with a fresh `auth_time`; 401 `step_up_required` marks actions that need it, 401 `passkey_rejected` explains a failed passkey), `POST /session/step-up/webauthn/options` (→ `{public_key}` request options), `GET/PUT /me/preferences`, `POST /me/out-of-office` | self |
| Session & onboarding | `GET /me/workspaces`, `POST /session/workspace` (re-issues the access token with the chosen `ws` claim), `GET /invitations/preview?token=` (public: invited email, workspace, inviter, expiry, `account_exists`; A-24), `POST /invitations/accept`, `POST /invitations/register`, `GET /me/permissions`, `GET /permission-catalog` | authenticated (workspace calls need a selected workspace) |
| Members | `GET /members`, `GET/POST /members/invitations`, `DELETE /members/invitations/{id}` (revoke; resend = invite again), `POST /members/{id}/deactivate` | `member:*` |
| Notifications | `GET /notifications?unread=&limit=` (own inbox + `unread_count`), `POST /notifications/{id}/read`, `POST /notifications/read-all`, `GET\|PUT /me/notification-preferences {digest: off\|daily\|weekly\|null, digest_hour, quiet_enabled, quiet_start, quiet_end}` (effective values + `overrides` + `workspace` defaults; `null` = workspace default; audited `notification_preferences.changed`) | self |
| Directory | `GET/POST /departments`, `PATCH/DELETE /departments/{id}` (move = `parent_id`; head = `head_membership_id`), `GET/POST /groups`, `DELETE /groups/{id}`, `GET/POST /groups/{id}/members`, `DELETE /groups/{id}/members/{membership_id}`, `GET/POST /positions`, `DELETE /positions/{id}`, `PATCH /members/{id}` (title, department, manager, location), `GET /members/{id}/directory` (fields, sources, resolved manager), `POST /members/import?mode=plan\|apply` (`text/csv`: email, department path, title, manager_email, location) | `directory:view` / `directory:manage` |
| Roles | `GET /permission-catalog`, `GET/POST /roles`, `PATCH /roles/{id}`, `POST /roles/{id}/preview`, `GET/POST /role-grants`, `DELETE /role-grants/{id}`, `/access-reviews` | `role:*`, `member:manage_roles` |
| Settings | `GET /settings` (items carry `group`) (every key, effective), `GET /settings/effective?key=…` and `GET /settings/{key}` (explainer, `ETag` = revision), `PUT /settings/{key}` `{value, reason?}` with `If-Match` (one key per call: each key has its own permission, step-up and reason rule), `GET /settings/{key}/history`; department overrides (keys the catalogue marks department-scopable): `GET /settings/departments/{id}` (those keys resolved for the department: `override_value`, `override_revision`, `inherited_from`), `GET /settings/effective?key=…&scope=department:{id}` (explainer with `department` layers carrying `scope_id`; `ETag` = override revision), `PUT /settings/departments/{id}/{key}` `{value, reason?}` with `If-Match` (422 `not_department_scopable`), `DELETE /settings/departments/{id}/{key}` with `If-Match` (404 `no_override`); profile switch: `GET /settings/profile/plan?profile=` (omitted = the current profile, showing drift; items `{key, current, current_source, proposed, after, floor, clamped, customized, kind}`; `ETag` = plan basis), `POST /settings/profile/apply` `{profile, reason}` with `If-Match` = plan basis (required, else 428; stale 412) and step-up; `GET /settings` reports `x-settings-cache: hit\|miss\|off`; `POST /config/export`, `POST /config/import` (dry-run diff) | per key: `workspace:settings.*`, `workspace:branding.manage`, `directory:manage`, `file:manage_retention`; department overrides check the key's permission on the department scope; profile apply needs `workspace:settings.manage` plus every changed key's permission |
| Library | `GET /artifacts?kind=sop\|policy&state=` (kinds the caller may view), `POST /artifacts {kind,key,title,criticality,content}` (creates version 1 as a draft), `GET /artifacts/{id}` (+ versions, `ETag`), `POST /artifacts/{id}/versions` (new draft from the latest content; one open draft), `GET /versions/{id}` (content, `content_hash`, current review round, approvals required), `PATCH /versions/{id} {content}` (draft only, `If-Match`), `POST /versions/{id}/submit`, `/withdraw`, `/reviews {decision: approve\|reject\|changes_requested, comment}`, `/publish {effective_at?, change_reason?, change?: major\|minor}` (BPMS-045: default major, ignored for v1; returns the version with `semver`, `reack_required` and `reack {required, due_at, audience_size, campaign_id}`; a version going live now starts its re-ack campaign in the same call, a scheduled one when it goes live, so `campaign_id` is null and `due_at` = `effective_at` + window), `GET /versions/{id}/publish-preview` (needs `publish`: `{version_no, next_semver_major, next_semver_minor, reack_policy, reack_required_if_major, reack_required_if_minor, reack_window_secs, audience, audience_size, due_at}`), `/retire {reason}`; `GET /versions/{a}/diff/{b}` (same artifact only, else 422: `{from, to, summary{added,removed,changed}, ops[{op: same\|added\|removed\|changed, block \| before/after/words[{op,text}]}]}`); governance (BPMS-044): `artifact.governance {review_cadence_days, next_review_at, last_reviewed_at, review_overdue_at, review_status: none\|current\|due_soon\|overdue, verified, verified_at, verified_by, verification_expires_at}`, `PUT /artifacts/{id}/review-cadence {days\|null}`, `POST /artifacts/{id}/recertify {comment?}` (published only, else 409), `POST\|DELETE /artifacts/{id}/verification`, `GET\|POST /artifacts/{id}/change-requests {title, body?}` (`?state=`), `GET /change-requests/{id}`, `POST /change-requests/{id}/resolve {decision: accepted\|declined\|implemented, resolution? (required to decline), version_id? (required to implement; same artifact)}`; policy portal (BPMS-045): `artifact.{category, applicability, restricted}`, `PUT /artifacts/{id}/applicability {category?, applicability: Audience\|null, restricted}` (restricted needs an audience), `GET /me/policies?q&category` (published policies that apply to the caller: `{items[{id,key,title,category,version_id,version_no,published_at,verified,review_status,what_changed{from_version_no,added,removed,changed,change_reason}\|null}], categories[]}`); readers with only `view` see published history only and never restricted artifacts outside their applicability (A-14: lists, reads, diff, change requests, search); `GET /versions/{id}/export?format=html\|md` (BPMS-042: print-ready HTML that browsers print as a tagged PDF, or Markdown; superseded/retired versions carry a banner; audited `version.exported`); content is validated against the block model (ADR-0013: paragraph, heading 1–3, list ordered?, callout info\|warning, table; runs with bold/italic/safe links), else 422; later: `POST /versions/{id}/compile`, `POST /artifacts/import`, server-side tagged PDF and DOCX | `sop:*`, `policy:*` (submit uses `edit_draft`, retire uses `publish`); approving or publishing a controlled artifact needs step-up. Governance: diff and listing/opening change requests need `view`; cadence, recertify and resolving change requests need `publish`; verification needs `approve` (+ step-up for controlled); applicability needs `publish`; the portal needs `policy:view` |
| Collections, owners, RACI | `GET /collections` (tree ordered by path; managers see all, readers only collections whose path admits them; `documents` = visible documents filed directly; `can_manage`), `POST /collections {name, parent_id?}` (sibling names unique, 409), `PATCH /collections/{id} {name?, parent_id?}` (moves rewrite the subtree's paths; no move under itself, 422), `DELETE /collections/{id}` (empty only, 409), `PUT /collections/{id}/visibility {visibility: Audience\|null}`; `GET /artifacts?collection=` (subtree); `PUT /artifacts/{id}/collection {collection_id\|null}`, `PUT /artifacts/{id}/owner {membership_id}`, `GET\|PUT /artifacts/{id}/raci {responsible, accountable (max 1), consulted, informed: [{membership_id}\|{group_id}]}`, `GET /raci/export?collection=` (CSV of visible documents, formula-safe; audited `raci.exported`) | `collection:view` / `collection:manage`; filing, owner and RACI need `publish` on the kind; RACI reads need `view` (BPMS-041) |
| Collections | `/collections` | `collection:*` |
| Templates | `GET /templates` (gallery), `GET /templates/{key}@{ver}`, `POST /templates/{key}:install {mapping}`, `GET /artifacts/{id}/template-updates` | `template:*` |
| Requests | `POST /requests {process_key, input}`, `POST /requests/test-run`, `GET /requests` + `POST /requests/query`, `GET /requests/{id}`, `GET /requests/{id}/timeline`, `POST /requests/{id}/cancel\|suspend\|resume\|retry`, `POST /requests/{id}/comments`, `POST /requests/{id}/signals/{name}` | `request:*` |
| Work | `GET /work` (my inbox; `?scope=team\|all` for managers/admins), `POST /work/{id}/claim\|unclaim`, `POST /work/{id}/decisions`, `POST /work/{id}/delegate`, `POST /work/bulk-reassign` (with preview) | `work:*` |
| Campaigns | `GET /campaigns?state=`, `POST /campaigns {version_id, audience {all, departments, groups, roles}, title?, due_at?, audience_snapshot? (at_launch\|dynamic), ack_text?}` (draft; the version must be published; omitted fields take the `campaign.*` settings), `GET /campaigns/{id}` (with `origin: manual\|reack` (BPMS-045), `subject.semver`, `progress {assigned, acknowledged, pending, overdue}`; drafts add `audience_preview`; `ETag`), `GET /campaigns/{id}/recipients?state=pending\|completed\|overdue`, `GET /campaigns/{id}/evidence` (`text/csv`: member, email, department, state, assigned_at, due_at, acknowledged_at, version, version_id, content_hash, auth_method; the export is audited), `POST /campaigns/{id}/activate` (snapshots the audience, notifies in-app + email), `POST /campaigns/{id}/close {reason?}`, `POST /campaigns/{id}/cancel {reason}`, `POST /campaigns/{id}/remind` (→ `{reminded}`), `GET /me/acknowledgements?state=pending\|completed` (own; `pending_count`), `POST /acknowledgements/{id}/complete {content_hash?}` (own assignment only, else 404; second time 409 `already_acknowledged`; hash mismatch 409 `content_changed`; ended campaign 409 `state_conflict`) | `campaign:view` (list, detail, recipients, evidence), `campaign:manage` (create, activate, close, cancel, remind); completing needs `sop:acknowledge` for SOPs and `policy:view` for policies |
| Announcements | `GET /announcements?state=` (management list: own drafts; everyone's submitted ones with `publish`; all with `manage`), `POST /announcements {title, content, audience: {all, departments, groups, roles}, priority: normal\|critical, pinned, require_ack?, publish_at?, expires_at?}` (draft; defaults from `announcements.require_ack.default` and `announcements.expiry.default`), `GET /announcements/{id}` (`ETag`), `PATCH /announcements/{id}` (same body, draft only, `If-Match`), `POST /announcements/{id}/submit` (→ `pending_approval`; `approval_required` per `announcements.approval_required`), `/approve` (author ≠ approver when approval applies; publishes, or schedules when `publish_at` is in the future), `/reject {comment}` (→ draft), `/publish` (draft only; 409 when approval applies), `/archive {reason}` (scheduled or published), `GET /announcements/feed?unread=` (published, unexpired, targeted at the caller; pinned first, then newest; with `read_at`/`acked_at`), `POST /announcements/{id}/reads`, `POST /announcements/{id}/acks` (only when `require_ack`; records time, client and content hash), `GET /announcements/{id}/analytics` (recipients, read, acknowledged, pending_ack, by department; counts only) | `announcement:create` (compose, own drafts, own analytics), `announcement:publish` (approve, reject, publish own drafts, archive), `announcement:view` (feed, reads, acks), `announcement:manage` (all of these on any announcement) |
| Signing | `GET/POST /envelopes`, `POST /envelopes/bulk-send`, `POST /envelopes/{id}/send\|void\|remind`, `GET /envelopes/{id}/signing-session` (ceremony), `POST /envelopes/{id}/signers/{ord}:sign\|decline`, `GET /envelopes/{id}/certificate` | signing perms (20) |
| Cases | `GET/POST /cases`, `/cases/{id}/items`, `POST /cases/{id}/close\|reopen`, `POST /cases/{id}/pins` | `case:*` |
| Investigations | `POST /investigations`, `POST /investigations/{id}/pins {audit_seq\|file\|request}`, `POST /investigations/{id}/hold`, `POST /investigations/{id}/export` | `audit:investigate` |
| Files | `POST /files/uploads` (presign/multipart), `POST /files/uploads/{id}/complete`, `GET /files` (admin view), `GET /files/{id}`, `GET /files/{id}/content` (→ short-lived URL), `GET /files/{id}/preview`, `POST /files:bulk {action}` | `file:*` |
| Audit | `GET /audit/events?from&to&actor\|member&action (prefix)&category&object_type&object_id&result&page[size]&page[cursor]` (newest first, keyset on `seq`, `{items, next_cursor}`, actor names for members), `GET /audit/objects/{type}/{id}/timeline?page[cursor]` (oldest first; `artifact` spans its own events and its versions), `GET /audit/verify` (`{ok, checked, first_broken}`), `GET/POST /audit/saved-queries {name, filters}`, `DELETE /audit/saved-queries/{id}` (private per member; audited), `POST /audit/exports {format: csv\|jsonl, filters}` (synchronous, 10 000-row cap, `Procera-Export-Rows`/`Procera-Export-Truncated` headers; audited as `audit.exported`); `POST /audit/query {filter: cond \| {and\|or: [...]}, sort: [{field: occurred_at, dir}], page: {size ≤ 500, cursor}}` (BPMS-064: depth ≤ 2, ≤ 25 conditions; fields occurred_at within\|gte\|lt, actor, object_id, category/result/action/object_type/ip eq, category/result/action in, action prefix, object_field eq), `POST /audit/exports {format, filters, background: true}` → 202 job (no row cap; stored as the requester's encrypted file; notification `audit_export_ready`; audited `audit.export_requested` then `audit.exported`), `GET /audit/exports`, `GET /audit/exports/{id}`; later: `GET /audit/verify-proof?seq=…` | `audit:view` (whole chain) or `audit:view_own` (own events; also saved queries); verify needs `audit:view`; export needs `audit:view` + `audit:export` + step-up |
| Retention | `GET /admin/retention` (classes with period, measured-from, mode automatic\|never and setting key; drafts due in the next 30 days; last run). Periods change through `PUT /settings/{key}` | `workspace:settings.view` (BPMS-066) |
| Encryption | `GET /security/encryption` (`algorithm`, `active_version`, `pending_rewrap`, `versions[{version, state: active\|retired\|destroyed, created_at, retired_at, values}]`), `POST /security/encryption/rotate {reason}` (step-up; audited `key.rotated`); a background job rewraps values to the active version and destroys unused retired versions | `workspace:settings.view` / `workspace:settings.manage` (BPMS-030) |
| Files | `POST /files {name, size, content_type?}` (size ≤ `files.max_size` and within the storage quota, else 413), `PUT /files/{id}/content?offset=N` (raw chunk ≤ 8 MB at the received offset, else 409; resume from `received`), `POST /files/{id}/complete` (SHA-256, magic-byte type, platform deny list → `rejected`, malware scan → `clean`\|`quarantined`), `GET /files/{id}`, `GET /files?state=` (all with `file:view_all`, else own), `DELETE /files/{id}` (not while a version past draft holds it, 409), `GET /files/{id}/download-url` (clean only; 5-minute token bound to file, workspace and member), `GET /files/download/{token}` (no bearer; re-authorized; `nosniff`, `CSP: sandbox`; audited), `GET /files/{id}/preview-url` (same rules as `download-url`; → `{url, kind: image\|pdf\|none, expires_at}`, `url` and `expires_at` null for `none`; 2-minute `file-preview` token) and `GET /files/preview/{token}` (BPMS-061: no bearer; re-authorized; clean PNG/JPEG/GIF/WebP/PDF only, else 415; tampered or expired 403; `inline`, `CSP: sandbox; default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'`, `nosniff`, `CORP: same-origin`, `no-store`; JPEG metadata stripped when `files.exif_strip`; audited `file.previewed`), `POST /files/{id}/links {object_type: artifact_version, object_id}` and `DELETE /files/{id}/links/{type}/{id}` (draft versions only, `edit_draft`), `GET /versions/{id}/attachments`, `GET /files/usage`, `GET /files/orphans`, `POST /files/orphans/purge` | `file:upload`, `file:download`, `file:view_all`, `file:delete` (BPMS-060/062) |
| Search | `GET /search?q=&types=sop,policy,member,department,group&limit=` (`{items: [{type, id, title, subtitle, snippet, score}], types}`; one ranked list; `snippet` is HTML-escaped with `<mark>` around matches; at most 50) | per type: `sop:view`, `policy:view`, `member:view`, `directory:view` (departments, groups); types the caller may not view are left out |
| Insights | `GET /metrics/processes/{id}`, `GET /dashboards`, `POST /dashboards`, `GET /reports/{key}`, `POST /reports/{key}/exports`, `GET /stream` (SSE) | `report:*`, `monitoring:*` |
| Ops | `GET /incidents`, `POST /incidents/{id}/retry\|skip\|resolve` | `request:retry` |
| Views | `GET /views?list=` (mine first, then shared), `POST /views {list, name, state, shared?, is_default?}` (one default per member per list), `PATCH /views/{id}` and `DELETE /views/{id}` (owner only; 403 for shared views of others, 404 otherwise); `POST /exports {list, format: csv, rows, columns, filters}` records a grid export (audited `data.exported`) | self / shared (BPMS-028) |
| Tokens | `GET/POST /api-tokens`, `DELETE /api-tokens/{id}` | `api_token:manage` + step-up |
| AI (optional) | `POST /ai/drafts {kind, prompt, sources}`, `GET /ai/drafts/{id}` | `ai:use_assistant` |

## 5. Resource map: system API (v1)

`POST /bootstrap` (one-time), `GET /me`, `GET/POST /workspaces`, `GET/PATCH /workspaces/{id}`, `POST /workspaces/{id}/suspend|restore|schedule-deletion`,
`PUT /workspaces/{id}/quotas`, `POST /workspaces/{id}/owners`, `GET /usage`, `GET /settings` (items carry `group`), `PUT /settings/{key}` `{value, reason}` (platform floors),
`GET/POST /templates`, `POST /templates/{key}/versions`, `:deprecate`, `GET /health`, `GET /system-audit/events`,
`POST /break-glass` → `POST /break-glass/{id}/approve|revoke`, `GET/POST /system-admins`.

As built (BPMS-065, system console). The system plane returns workspace metadata, counts and health only, never workspace content (docs/12 §1).

| Endpoint | Behaviour | System roles |
|---|---|---|
| `GET /workspaces?state=&profile=&q=` | Workspaces that are not deleted, newest first (≤ 500). `q` matches name or slug, case-insensitive. Each item is the workspace (id, slug, name, profile, state, isolation_tier, region, owner_invite_email, suspended_reason, deletion_due_at, revision, created_at, quotas) plus `usage {members, invited, storage_bytes (null until files ship), last_activity_at}`. Unknown parameters → 400 | any |
| `GET /workspaces/{id}` | The same row; `ETag` = revision | any |
| `POST /workspaces/{id}/suspend {mode: read_only\|locked, reason}`, `/restore`, `/schedule-deletion {grace_days 7–90, reason}` | `If-Match` optional (stale → 412), invalid transition → 409, empty reason → 422. Step-up required (5 min, else 401 `step_up_required`). Answers with the console row. Audited on the system chain with the state change and reason | owner, admin |
| `PUT /workspaces/{id}/quotas` | Full quota bundle (`null` = unlimited). Step-up. Audited `workspace.quotas_changed` | owner, admin |
| `GET /quota-defaults`, `PUT /quota-defaults/{profile}` | The bundle each configuration profile starts with (`{profile, max_members, max_active_instances, max_storage_bytes, api_rpm, ai_tokens_month, revision, updated_at}`). `PUT` takes the five limits, `If-Match` (stale → 412), step-up; limits must be positive or null (422). New workspaces copy it; existing workspaces keep theirs. Audited `platform.quota_defaults_changed` | read: any; write: owner, admin |
| `GET /health` | `{status: ok\|degraded\|down, checked_at, version, database {status, latency_ms}, cache {status, backend}, outbox {status, pending, retrying, oldest_at, lag_seconds, dispatcher}, mail {status, queued, failed, oldest_age_seconds}, migrations {status, applied, latest, expected, expected_latest}}`. Degraded when the oldest pending event or queued email is older than 5 min, any email failed, the cache is degraded, no dispatcher runs or migrations are missing | any |
| `GET /system-audit/events?from&to&actor&action (prefix)&category&object_type&object_id&result&page[size]&page[cursor]` | The system chain through the shared explorer, newest first, keyset on `seq`; actor names are resolved for system administrators only | owner, admin, auditor |
| `GET /system-audit/verify` | `{ok, checked, first_broken}` for the system chain | owner, admin, auditor |

The member quota is enforced at invite time on the workspace plane: `POST /api/v1/members/invitations` answers 429 `quota_exceeded` when active members plus open invitations reach `max_members` (re-inviting the same address needs no extra seat). `GET /usage` is not built yet: the workspace list carries usage until storage and instance usage exist.

## 6. Event envelope (outbox → bus → webhooks)

```json
{
  "id": "0192a4f1-7c3e-7b10-9a3b-1f2e3d4c5b6a",
  "type": "work.completed.v1",
  "schema_version": 1,
  "workspace_id": "0192…",
  "aggregate": { "type": "instance", "id": "0192…", "revision": 8 },
  "occurred_at": "2026-10-11T10:00:00Z",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "actor": { "kind": "user", "subject_ref": "0192…", "on_behalf_of": null },
  "data": { "work_id": "0192…", "node_id": "approval", "outcome": "approved" }
}
```

Rules:
- `data` carries ids and outcomes only. It never contains restricted values, form contents or file bytes.
- Consumers deduplicate on `id`.
- Webhook delivery wraps the envelope with Standard Webhooks headers (`webhook-id` = event id).
- Schemas live in `schemas/events/*.json` (JSON Schema 2020-12). A compatibility check runs in CI, allowing only additive changes within a version.

## 7. Worked examples

### 7.1 Start a request

```http
POST /api/v1/requests
Authorization: Bearer prc_ws_…
Idempotency-Key: 6d0d0f2e-…
Content-Type: application/json

{"process_key":"fin.purchase-requisition","input":{"amount":{"amount":"72000","currency":"INR"},"vendor":"Acme","justification":"Laptops"}}
```
→ `201 Created`, `Location: /api/v1/requests/0192…`, `ETag: "1"`
```json
{"id":"0192…","display_id":"PR-2026-000123","process":{"key":"fin.purchase-requisition","version":4},"state":"running","current_steps":[{"node_id":"approval","name":"Finance approval","assignee":{"id":"…","display_name":"R. Iyer"}}],"due_at":"2026-10-14T12:30:00Z"}
```

### 7.2 Decide a work item

```http
POST /api/v1/work/0192…/decisions
Idempotency-Key: approve-0192…-1
If-Match: "7"
{"outcome":"approved","comment":"Budget confirmed","fields":{"cost_center":"ENG"}}
```
The possible responses are:
- `200` with the new revision.
- `409 already_decided` if someone decided first.
- `403 step_up_required` (with `max_age=300`) if the process demands re-auth.
- `403 sod_violation` if the caller initiated the request.

### 7.3 Publish a version

```http
POST /api/v1/versions/0192…:publish
If-Match: "11"
{"effective_at":"2026-11-01T00:00:00+05:30","change_reason":"Threshold raised to 1,00,000"}
```
→ `202 Accepted` (scheduled) or `200` (immediate), with `content_hash`, `bundle_id` and the triggered `campaign_id` (if a re-ack is required). As built (BPMS-045) the body is the version plus `reack {required, due_at, audience_size, campaign_id}`, and the status is 200 for both immediate and scheduled publishes (202 for scheduled is the target).

### 7.4 Bulk-send a policy for signature

```http
POST /api/v1/envelopes:bulk-send
Idempotency-Key: coc-2026-v7
{"document_file_id":"0192…","audience":{"departments":["*"]},"fields":[{"kind":"signature","page":4,"rect":[72,640,260,690]},{"kind":"date_signed","page":4,"rect":[300,640,420,690]}],"signature_mode":"strong","deadline":"2026-10-31T23:59:59+05:30","reminders":["P3D","P7D"],"escalate_to_manager_after":"P14D"}
```
→ `202 Accepted` `{"campaign_id":"0192…","recipients_estimated":4021}`

### 7.5 Audit query

```http
POST /api/v1/audit/query
{"filter":{"and":[{"field":"category","op":"eq","value":"data_access"},{"field":"object_field","op":"eq","value":"salary"},{"field":"occurred_at","op":"within","value":"P30D"}]},"sort":[{"field":"occurred_at","dir":"desc"}],"page":{"size":100}}
```

## 8. SDKs & CLI (P1)

- The OpenAPI document generates TypeScript and Python clients. The Rust client comes from shared DTO crates.
- `procera` CLI: `procera login`, `procera ws use`, `procera artifacts import`, `procera requests start`, `procera config export|import`, `procera verify <bundle>`.
