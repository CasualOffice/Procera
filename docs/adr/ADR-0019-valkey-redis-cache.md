# ADR-0019 — Redis-protocol cache (Valkey default) as a standard, non-authoritative component

Status: **Accepted** · Date: 2026-10-11 · Deciders: Architect, DevOps · Amends the "optional Valkey" wording in 01/03.

## Context

The owner expects Redis in the stack. Running several API replicas needs shared, fast, ephemeral state for things like rate limits, live-update fan-out and hot caches. Redis 8+ is licensed RSALv2 / SSPLv1 / AGPLv3. Valkey (Linux Foundation) is BSD-3 and speaks the same protocol.

## Decision

1. A **Redis-protocol server is a standard component** in the Standard, Enterprise and hosted topologies. The single-node Trial can run without it (in-process fallback).
2. The **default image we ship and test is Valkey** (8.x/9.x). Customers may point at **Redis OSS ≤ 7.2**, **Redis 8+** (running it unmodified as a separate server is their licence choice, and we neither bundle nor modify it), or a managed service such as ElastiCache, MemoryDB, Azure Cache or Memorystore. The Rust client is `redis`/`fred` (MIT/Apache) through a `Cache` port.
3. **What it is used for:**

| Use | Notes |
|---|---|
| Rate limiting (per IP, session, workspace, token) | Token buckets with Lua/functions |
| Session lookup cache | Postgres stays authoritative for revocation. The cache entry is invalidated on revoke, with a TTL of 60 s max. |
| Effective-permission cache (compiled Cedar policy set per workspace) and effective-settings cache | Keyed by `ws:{id}:access_rev` / `settings_rev`. A miss rebuilds from Postgres. |
| SSE / live-update fan-out across API replicas | Pub/Sub carries ids/counters only, never content |
| Short-lived OTP / magic-link attempt counters, CAPTCHA/anti-abuse | Expire automatically |
| Dashboard tile cache (per workspace and viewer role) | Short TTL, invalidated by outbox events |
| Distributed debounce/singleflight for expensive rebuilds | Advisory only |

4. **What it must never hold:**
   - jobs, timers or the outbox. These stay in Postgres (ADR-0005, ADR-0009), because they must commit atomically with state.
   - the only copy of anything.
   - restricted field values or file contents.
   - audit data.
   - idempotency records. These are Postgres-only.
5. **Losing the cache costs performance, not correctness.** It can be flushed at any time. CI runs the full E2E suite with the cache disabled (in-process fallback) to prove this.
6. **Tenancy:** every key is prefixed `ws:{workspace_id}:`, and the system plane uses `sys:`. Enable ACL users and TLS in production. Persistence (RDB/AOF) is unnecessary.

## Alternatives considered

- **Postgres-only for rate limits and caches:** works for the Trial (the fallback), but adds write load at scale.
- **Memcached:** no pub/sub or Lua.
- **Bundling Redis 8:** rejected under the licence policy (ADR-0014).

## Consequences

- Compose and Helm ship a Valkey service.
- The admin guide covers sizing (typically < 1 GB for the reference load), ACLs and TLS.
- Health checks treat the cache as **degraded, not down**.
