# Conventions

The rules every endpoint follows: errors, safe retries, concurrent edits, lists and limits.

## Errors

Errors are [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) problem details with
`Content-Type: application/problem+json`:

```json
{
  "type": "https://procera.dev/errors/validation_failed",
  "title": "Validation failed",
  "status": 422,
  "code": "validation_failed",
  "detail": "Choose at least one permission.",
  "errors": [{ "pointer": "/input/start_date", "code": "required", "message": "Enter a start date." }]
}
```

- `code` is stable and machine-readable (`type` is the same code as a URI); branch on it, not on `title` or `detail`. `trace_id`, when present, helps the administrator find the request in the logs.
- `errors[]` appears on `422` with one entry per field, pointing into the request body.
- `401` carries `WWW-Authenticate: Bearer`; `429` and some `503`s carry `Retry-After` (seconds).

| Status | Meaning |
|---|---|
| `400` | The request is malformed, or a required header (such as `Idempotency-Key`) is missing |
| `401` | No valid session or token |
| `403` | Signed in, but not allowed (the `detail` names the permission) |
| `404` | Not found, or you may not know it exists (Procera doesn't reveal other workspaces' ids) |
| `409` | The action conflicts with the current state (already decided, not in draft…) |
| `412` | `If-Match` didn't match: someone changed it first |
| `422` | Validation failed |
| `423` | The workspace is suspended (read-only or locked) |
| `428` | `If-Match` is required for this update |
| `429` | Too many attempts; wait for `Retry-After` |

## Retrying safely: Idempotency-Key

Creating things such as requests and decisions takes an `Idempotency-Key` header: any unique string,
for example a UUID you generate. If the network fails, send the same request with the **same key**:
Procera returns the original result instead of doing it twice. Reusing a key for a different request
is a `409`. Starting a request without a key is a `400` (`idempotency_key_required`).

## Concurrent edits: If-Match

Things that several people edit (drafts, work items, settings) have a `revision`. Updates send it
back as `If-Match: "<revision>"`:

```bash
curl -X PUT .../api/v1/processes/$ID/draft \
  -H 'If-Match: "7"' -d '{"content": {...}}'
```

A missing `If-Match` is `428`; a stale one is `412`. Read the item again, merge, and retry.

## Lists

Lists return `{"items": [...]}`. The audit trail and other long lists page with a cursor:
pass `page[cursor]` from the previous response's `next_cursor` until it is `null`. Lists without a
cursor return everything you may see, with documented caps (for example at most 200 requests).

## Who can see what

Every call runs as one member of one workspace and is checked against that workspace's roles and
grants, including department and collection scopes. Reads of restricted data are recorded in the audit
trail. Workspace data is isolated at the database level (row-level security), so another workspace's
ids simply don't exist for you.

## Time

All times are RFC 3339 in UTC (`2026-10-14T09:30:00Z`). Dates without a time (leave start dates, due
dates) are `YYYY-MM-DD` in the workspace's time zone.
