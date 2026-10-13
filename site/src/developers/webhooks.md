# Webhooks

> **In development (0.4).** Outgoing webhooks are being built now. This page describes the design
> they follow; it will become the reference when they ship.

Webhooks let Procera tell your systems when something happens (a request is approved, a policy is
published, someone joins) instead of you polling the API.

## How they will work

- You add an endpoint (an `https` URL) in **Admin › Integrations › Webhooks** and choose which events
  it receives.
- Deliveries follow the [Standard Webhooks](https://www.standardwebhooks.com/) specification:
  `webhook-id`, `webhook-timestamp` and `webhook-signature` headers, signed with HMAC-SHA256 over
  `id.timestamp.body`, with a 5-minute tolerance. Secrets start with `whsec_`, are shown once, and can
  be rolled with an overlap so you never miss an event.
- Payloads are **thin**: the event type, ids and outcomes, never form contents or restricted data.
  Your receiver fetches details through the API with its own token.
- Failed deliveries are retried with backoff and jitter for about a day; a `410 Gone` response turns
  the endpoint off. A delivery log shows each attempt, and you can replay one.
- Procera only connects to public addresses: private, loopback, link-local and cloud-metadata ranges
  are refused, redirects are not followed, and responses are size- and time-capped. Self-hosted
  administrators can allow specific internal ranges explicitly.

## Event envelope

```json
{
  "id": "0192a4f1-7c3e-7b10-9a3b-1f2e3d4c5b6a",
  "type": "instance.completed.v1",
  "occurred_at": "2026-10-14T10:00:00Z",
  "workspace_id": "0192…",
  "data": { "instance_id": "0192…", "status": "approved" }
}
```

Deduplicate on `id`: the same event may arrive more than once.
