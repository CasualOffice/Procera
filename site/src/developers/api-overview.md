# API overview

Everything the Procera web app does goes through the same HTTP API, so anything you can do in the app
you can automate. This page covers the base URL, how to authenticate, and a first request. The
[API reference](docs-reference.html) lists every endpoint, generated from the server's router.

> **Pre-release.** Procera is at 0.4 in development. The API is versioned under `/api/v1` and changes
> only additively within a version, but until 1.0 we may still adjust endpoints; the release notes say so.

## Base URL

Your server's address plus a version prefix:

| Prefix | What it serves |
|---|---|
| `/api/v1` | The workspace API: library, requests, work, people, settings, audit… |
| `/sysapi/v1` | The system plane: workspaces and platform settings, for system administrators |
| `/healthz`, `/readyz`, `/version` | Liveness, readiness and the running version (no authentication) |

Requests and responses are JSON (`Content-Type: application/json`), times are RFC 3339 in UTC, and ids
are UUIDs.

## Authenticate with an API token

For scripts and integrations, use a **personal API token**:

1. In Procera, open the account menu › **API tokens** › **New token**.
2. Name it, choose how long it lasts (up to a year) and pick only the permissions it needs. **Read only**
   picks every "view" permission you hold.
3. Copy the token. It is shown once and looks like `prc_live_<12 characters>_<43 characters>`.

Send it in the `Authorization` header:

```bash
curl https://procera.example.com/api/v1/requests?scope=mine \
  -H "Authorization: Bearer $PROCERA_TOKEN"
```

What a token can do:

- It acts as **you**, in the workspace where you made it, with only the permissions you picked. If you
  lose a permission, so does the token; if your account is deactivated, the token stops working.
- It **never** approves or rejects work, delegates it, acknowledges a document or signs, and it can't
  change your account, your sessions or tokens. Those need a person signed in. Such calls return `403`
  with the code `token_not_allowed`.
- Actions that need a recent sign-in ("confirm it's you") always fail with a token.
- You or a security admin (`api_token:manage`) can revoke it at any time; it stops at once. Rotating
  the server's master key invalidates every token.

Procera records when and from which address each token was last used, and audits creating and revoking
tokens (`api_token.created`, `api_token.revoked`). Everything a token does is in the audit trail as you.

## Signed-in sessions

The web app signs in with `POST /api/v1/auth/login` (email and password, then a second step when
required), which returns a short-lived `access_token` and sets a refresh cookie. That flow is for
browsers; prefer API tokens for automation.

## A first request

Start a leave request and read it back:

```bash
curl -X POST https://procera.example.com/api/v1/requests \
  -H "Authorization: Bearer $PROCERA_TOKEN" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: $(uuidgen)" \
  -d '{"type": "hr.leave-request", "input": {"leave_type": "annual", "start_date": "2026-12-21", "end_date": "2026-12-23"}}'
```

The response is `201` with the request's `id`, `display_id` (such as `LR-2026-000123`) and `status`.
`GET /api/v1/requests/{id}` returns its steps, timeline and why each step went to whom. The token needs
`request:start` and `request:view_own`.

Next: [Conventions](docs-conventions.html) for errors, retries and concurrency.
