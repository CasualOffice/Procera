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
| Session/me | `GET /me`, `GET /me/workspaces`, `POST /session/workspace` (switch), `POST /session/step-up` (`{code}` with TOTP, else `{password}`; returns an access token with a fresh `auth_time`; 401 `step_up_required` marks actions that need it), `GET/PUT /me/preferences`, `POST /me/out-of-office` | self |
| Session & onboarding | `GET /me/workspaces`, `POST /session/workspace` (re-issues the access token with the chosen `ws` claim), `POST /invitations/accept`, `POST /invitations/register`, `GET /me/permissions`, `GET /permission-catalog` | authenticated (workspace calls need a selected workspace) |
| Members | `GET /members`, `GET/POST /members/invitations`, `DELETE /members/invitations/{id}` (revoke; resend = invite again), `POST /members/{id}/deactivate` | `member:*` |
| Notifications | `GET /notifications?unread=&limit=` (own inbox + `unread_count`), `POST /notifications/{id}/read`, `POST /notifications/read-all`, `GET\|PUT /me/notification-preferences {digest: off\|daily\|weekly\|null, digest_hour, quiet_enabled, quiet_start, quiet_end}` (effective values + `overrides` + `workspace` defaults; `null` = workspace default; audited `notification_preferences.changed`) | self |
| Directory | `GET/POST /departments`, `PATCH/DELETE /departments/{id}` (move = `parent_id`; head = `head_membership_id`), `GET/POST /groups`, `DELETE /groups/{id}`, `GET/POST /groups/{id}/members`, `DELETE /groups/{id}/members/{membership_id}`, `GET/POST /positions`, `DELETE /positions/{id}`, `PATCH /members/{id}` (title, department, manager, location), `GET /members/{id}/directory` (fields, sources, resolved manager), `POST /members/import?mode=plan\|apply` (`text/csv`: email, department path, title, manager_email, location) | `directory:view` / `directory:manage` |
| Roles | `GET /permission-catalog`, `GET/POST /roles`, `PATCH /roles/{id}`, `POST /roles/{id}/preview`, `GET/POST /role-grants`, `DELETE /role-grants/{id}`, `/access-reviews` | `role:*`, `member:manage_roles` |
| Settings | `GET /settings` (items carry `group`) (every key, effective), `GET /settings/effective?key=…` and `GET /settings/{key}` (explainer, `ETag` = revision), `PUT /settings/{key}` `{value, reason?}` with `If-Match` (one key per call: each key has its own permission, step-up and reason rule), `GET /settings/{key}/history`, `POST /config/export`, `POST /config/import` (dry-run diff) | per key: `workspace:settings.*`, `workspace:branding.manage`, `directory:manage`, `file:manage_retention` |
| Library | `GET /artifacts?kind=sop\|policy&state=` (kinds the caller may view), `POST /artifacts {kind,key,title,criticality,content}` (creates version 1 as a draft), `GET /artifacts/{id}` (+ versions, `ETag`), `POST /artifacts/{id}/versions` (new draft from the latest content; one open draft), `GET /versions/{id}` (content, `content_hash`, current review round, approvals required), `PATCH /versions/{id} {content}` (draft only, `If-Match`), `POST /versions/{id}/submit`, `/withdraw`, `/reviews {decision: approve\|reject\|changes_requested, comment}`, `/publish {effective_at?, change_reason?}`, `/retire {reason}`; `GET /versions/{a}/diff/{b}` (same artifact only, else 422: `{from, to, summary{added,removed,changed}, ops[{op: same\|added\|removed\|changed, block \| before/after/words[{op,text}]}]}`); governance (BPMS-044): `artifact.governance {review_cadence_days, next_review_at, last_reviewed_at, review_overdue_at, review_status: none\|current\|due_soon\|overdue, verified, verified_at, verified_by, verification_expires_at}`, `PUT /artifacts/{id}/review-cadence {days\|null}`, `POST /artifacts/{id}/recertify {comment?}` (published only, else 409), `POST\|DELETE /artifacts/{id}/verification`, `GET\|POST /artifacts/{id}/change-requests {title, body?}` (`?state=`), `GET /change-requests/{id}`, `POST /change-requests/{id}/resolve {decision: accepted\|declined\|implemented, resolution? (required to decline), version_id? (required to implement; same artifact)}`; policy portal (BPMS-045): `artifact.{category, applicability, restricted}`, `PUT /artifacts/{id}/applicability {category?, applicability: Audience\|null, restricted}` (restricted needs an audience), `GET /me/policies?q&category` (published policies that apply to the caller: `{items[{id,key,title,category,version_id,version_no,published_at,verified,review_status,what_changed{from_version_no,added,removed,changed,change_reason}\|null}], categories[]}`); readers with only `view` see published history only and never restricted artifacts outside their applicability (A-14: lists, reads, diff, change requests, search); later: `POST /versions/{id}/compile`, `POST /artifacts/import`, `GET /versions/{id}/export` | `sop:*`, `policy:*` (submit uses `edit_draft`, retire uses `publish`); approving or publishing a controlled artifact needs step-up. Governance: diff and listing/opening change requests need `view`; cadence, recertify and resolving change requests need `publish`; verification needs `approve` (+ step-up for controlled); applicability needs `publish`; the portal needs `policy:view` |
| Collections | `/collections` | `collection:*` |
| Templates | `GET /templates` (gallery), `GET /templates/{key}@{ver}`, `POST /templates/{key}:install {mapping}`, `GET /artifacts/{id}/template-updates` | `template:*` |
| Requests | `POST /requests {process_key, input}`, `POST /requests/test-run`, `GET /requests` + `POST /requests/query`, `GET /requests/{id}`, `GET /requests/{id}/timeline`, `POST /requests/{id}/cancel\|suspend\|resume\|retry`, `POST /requests/{id}/comments`, `POST /requests/{id}/signals/{name}` | `request:*` |
| Work | `GET /work` (my inbox; `?scope=team\|all` for managers/admins), `POST /work/{id}/claim\|unclaim`, `POST /work/{id}/decisions`, `POST /work/{id}/delegate`, `POST /work/bulk-reassign` (with preview) | `work:*` |
| Campaigns | `GET /campaigns?state=`, `POST /campaigns {version_id, audience {all, departments, groups, roles}, title?, due_at?, audience_snapshot? (at_launch\|dynamic), ack_text?}` (draft; the version must be published; omitted fields take the `campaign.*` settings), `GET /campaigns/{id}` (with `progress {assigned, acknowledged, pending, overdue}`; drafts add `audience_preview`; `ETag`), `GET /campaigns/{id}/recipients?state=pending\|completed\|overdue`, `GET /campaigns/{id}/evidence` (`text/csv`: member, email, department, state, assigned_at, due_at, acknowledged_at, version, version_id, content_hash, auth_method; the export is audited), `POST /campaigns/{id}/activate` (snapshots the audience, notifies in-app + email), `POST /campaigns/{id}/close {reason?}`, `POST /campaigns/{id}/cancel {reason}`, `POST /campaigns/{id}/remind` (→ `{reminded}`), `GET /me/acknowledgements?state=pending\|completed` (own; `pending_count`), `POST /acknowledgements/{id}/complete {content_hash?}` (own assignment only, else 404; second time 409 `already_acknowledged`; hash mismatch 409 `content_changed`; ended campaign 409 `state_conflict`) | `campaign:view` (list, detail, recipients, evidence), `campaign:manage` (create, activate, close, cancel, remind); completing needs `sop:acknowledge` for SOPs and `policy:view` for policies |
| Announcements | `GET /announcements?state=` (management list: own drafts; everyone's submitted ones with `publish`; all with `manage`), `POST /announcements {title, content, audience: {all, departments, groups, roles}, priority: normal\|critical, pinned, require_ack?, publish_at?, expires_at?}` (draft; defaults from `announcements.require_ack.default` and `announcements.expiry.default`), `GET /announcements/{id}` (`ETag`), `PATCH /announcements/{id}` (same body, draft only, `If-Match`), `POST /announcements/{id}/submit` (→ `pending_approval`; `approval_required` per `announcements.approval_required`), `/approve` (author ≠ approver when approval applies; publishes, or schedules when `publish_at` is in the future), `/reject {comment}` (→ draft), `/publish` (draft only; 409 when approval applies), `/archive {reason}` (scheduled or published), `GET /announcements/feed?unread=` (published, unexpired, targeted at the caller; pinned first, then newest; with `read_at`/`acked_at`), `POST /announcements/{id}/reads`, `POST /announcements/{id}/acks` (only when `require_ack`; records time, client and content hash), `GET /announcements/{id}/analytics` (recipients, read, acknowledged, pending_ack, by department; counts only) | `announcement:create` (compose, own drafts, own analytics), `announcement:publish` (approve, reject, publish own drafts, archive), `announcement:view` (feed, reads, acks), `announcement:manage` (all of these on any announcement) |
| Signing | `GET/POST /envelopes`, `POST /envelopes/bulk-send`, `POST /envelopes/{id}/send\|void\|remind`, `GET /envelopes/{id}/signing-session` (ceremony), `POST /envelopes/{id}/signers/{ord}:sign\|decline`, `GET /envelopes/{id}/certificate` | signing perms (20) |
| Cases | `GET/POST /cases`, `/cases/{id}/items`, `POST /cases/{id}/close\|reopen`, `POST /cases/{id}/pins` | `case:*` |
| Investigations | `POST /investigations`, `POST /investigations/{id}/pins {audit_seq\|file\|request}`, `POST /investigations/{id}/hold`, `POST /investigations/{id}/export` | `audit:investigate` |
| Files | `POST /files/uploads` (presign/multipart), `POST /files/uploads/{id}/complete`, `GET /files` (admin view), `GET /files/{id}`, `GET /files/{id}/content` (→ short-lived URL), `GET /files/{id}/preview`, `POST /files:bulk {action}` | `file:*` |
| Audit | `GET /audit/events?from&to&actor\|member&action (prefix)&category&object_type&object_id&result&page[size]&page[cursor]` (newest first, keyset on `seq`, `{items, next_cursor}`, actor names for members), `GET /audit/objects/{type}/{id}/timeline?page[cursor]` (oldest first; `artifact` spans its own events and its versions), `GET /audit/verify` (`{ok, checked, first_broken}`), `GET/POST /audit/saved-queries {name, filters}`, `DELETE /audit/saved-queries/{id}` (private per member; audited), `POST /audit/exports {format: csv\|jsonl, filters}` (synchronous, 10 000-row cap, `Procera-Export-Rows`/`Procera-Export-Truncated` headers; audited as `audit.exported`); later: `POST /audit/query` (full filter AST), `GET /audit/verify-proof?seq=…`, async export job | `audit:view` (whole chain) or `audit:view_own` (own events; also saved queries); verify needs `audit:view`; export needs `audit:view` + `audit:export` + step-up |
| Search | `GET /search?q=&types=sop,policy,member,department,group&limit=` (`{items: [{type, id, title, subtitle, snippet, score}], types}`; one ranked list; `snippet` is HTML-escaped with `<mark>` around matches; at most 50) | per type: `sop:view`, `policy:view`, `member:view`, `directory:view` (departments, groups); types the caller may not view are left out |
| Insights | `GET /metrics/processes/{id}`, `GET /dashboards`, `POST /dashboards`, `GET /reports/{key}`, `POST /reports/{key}/exports`, `GET /stream` (SSE) | `report:*`, `monitoring:*` |
| Ops | `GET /incidents`, `POST /incidents/{id}/retry\|skip\|resolve` | `request:retry` |
| Views | `GET/POST /views?list=…`, `PATCH/DELETE /views/{id}` | self / shared |
| Tokens | `GET/POST /api-tokens`, `DELETE /api-tokens/{id}` | `api_token:manage` + step-up |
| AI (optional) | `POST /ai/drafts {kind, prompt, sources}`, `GET /ai/drafts/{id}` | `ai:use_assistant` |

## 5. Resource map: system API (v1)

`POST /bootstrap` (one-time), `GET /me`, `GET/POST /workspaces`, `GET/PATCH /workspaces/{id}`, `POST /workspaces/{id}/suspend|restore|schedule-deletion`,
`PUT /workspaces/{id}/quotas`, `POST /workspaces/{id}/owners`, `GET /usage`, `GET /settings` (items carry `group`), `PUT /settings/{key}` `{value, reason}` (platform floors),
`GET/POST /templates`, `POST /templates/{key}/versions`, `:deprecate`, `GET /health`, `GET /system-audit/query`,
`POST /break-glass` → `POST /break-glass/{id}/approve|revoke`, `GET/POST /system-admins`.

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
→ `202 Accepted` (scheduled) or `200` (immediate), with `content_hash`, `bundle_id` and the triggered `campaign_id` (if a re-ack is required).

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
