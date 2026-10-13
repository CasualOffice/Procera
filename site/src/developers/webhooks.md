# Webhooks

Webhooks tell your systems when something happens in Procera (a request finishes, a document is
signed, someone joins) instead of you polling the API. An administrator adds endpoints in
**Admin › Webhooks** (permission `connector:configure`; rolling a secret needs `connector:manage_secrets`).

## Events

| Type | When |
|---|---|
| `request.started` | Someone started a request |
| `request.completed` | A request finished (approved, rejected or done) |
| `request.overdue` | A request step went past its deadline |
| `document.published` | An SOP or policy version was published |
| `document.review_overdue` | An SOP or policy is past its review date |
| `announcement.published` | An announcement went out |
| `acknowledgement.completed` | Someone acknowledged a document |
| `member.joined`, `member.deactivated`, `member.reactivated` | Membership changes |
| `signature.requested`, `signature.completed`, `signature.declined`, `signature.cancelled`, `signature.expired` | E-signing |
| `webhook.test` | Sent when you press **Send test event** |

`GET /api/v1/webhook-events` lists them. An endpoint subscribes to chosen types, or to every type
(including ones added later).

## What arrives

A `POST` with a small JSON body:

```json
{
  "id": "01a137a0-cbd0-71f6-a684-cc7cc5802125",
  "type": "request.completed",
  "occurred_at": "2026-10-14T10:00:00Z",
  "workspace_id": "01a13…",
  "data": { "request_id": "01a13…", "status": "approved" }
}
```

- **Thin by design**: `data` carries ids, states, outcomes and version numbers only, never form answers,
  names or document contents. Fetch the details through the [API](docs-api.html) with your own token.
- **At least once**: the same event can arrive more than once; deduplicate on `id`.
- Answer with any `2xx` within 10 seconds. Procera keeps up to 4 KB of your response in its delivery log.

## Verifying signatures

Deliveries follow the [Standard Webhooks](https://www.standardwebhooks.com/) specification, so its
libraries verify them. Each request has three headers:

| Header | Value |
|---|---|
| `webhook-id` | The event `id` |
| `webhook-timestamp` | Unix seconds when it was sent |
| `webhook-signature` | `v1,<signature>`, space-separated when there is more than one |

The signature is base64 HMAC-SHA256 of `{webhook-id}.{webhook-timestamp}.{body}`, keyed with the base64
part of your secret (after `whsec_`). Reject a timestamp more than five minutes from your clock.

```js
import { createHmac, timingSafeEqual } from 'node:crypto';

function verify(secret, headers, body) {
  const key = Buffer.from(secret.replace(/^whsec_/, ''), 'base64');
  const signed = `${headers['webhook-id']}.${headers['webhook-timestamp']}.${body}`;
  const want = Buffer.from(createHmac('sha256', key).update(signed).digest('base64'));
  const fresh = Math.abs(Date.now() / 1000 - Number(headers['webhook-timestamp'])) < 300;
  return fresh && headers['webhook-signature'].split(' ').some((s) => {
    const got = Buffer.from(s.replace(/^v1,/, ''));
    return got.length === want.length && timingSafeEqual(got, want);
  });
}
```

The secret is shown once, when the endpoint is created or its secret is rolled. After a roll, events
are signed with both the new and the previous secret for 24 hours.

## Retries and turning off

- Anything other than `2xx` is retried after 1 minute, 5 minutes, 30 minutes, 2 hours, 6 hours and
  12 hours (with a little jitter), then the delivery is marked failed. A `Retry-After` header is
  honoured, up to an hour.
- `410 Gone` turns the endpoint off at once. So do three events in a row that fail after all their
  retries; the person who created the endpoint is told. Turn it back on when it's fixed. Events that
  happened while it was off aren't queued.
- The delivery log (**Deliveries**) shows each attempt's status, duration, response excerpt and the
  event body, and can **replay** a delivery (same `id`).

## Where Procera will send

Procera only sends to public `https` addresses on ports 443, 80, 8443 or 8080. It resolves the host
and refuses private, loopback, link-local, carrier-grade NAT, cloud-metadata and other reserved
addresses (including names that resolve to them), connects only to the address it checked, follows no
redirects, and gives up after 10 seconds. Self-hosted operators can allow internal ranges with
`PROCERA_EGRESS_ALLOW` (see the [configuration reference](docs-configuration.html)).

## API

`GET/POST /api/v1/webhooks`, `GET/PATCH/DELETE /api/v1/webhooks/{id}`,
`POST /api/v1/webhooks/{id}/rotate-secret`, `POST /api/v1/webhooks/{id}/test`,
`GET /api/v1/webhooks/{id}/deliveries`, `POST /api/v1/webhooks/deliveries/{id}/replay`. See the
[API reference](docs-reference.html#outgoing-webhooks).
