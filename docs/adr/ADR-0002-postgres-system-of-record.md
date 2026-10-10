# ADR-0002 — PostgreSQL 16+ is the only system of record (not MariaDB/MySQL)

Status: **Accepted** · Date: 2026-10-11 (rationale extended 2026-10-11 after the owner asked "MariaDB or Postgres?") · Deciders: Architect, Product, Security lead

## Context

Self-hosters need few moving parts, and correctness needs a single transaction that spans state, events, outbox and audit. The design ([04-LLD](../04-LLD.md)) leans on specific database features. The owner asked whether MariaDB would work instead.

## Decision

Use **PostgreSQL 16+** (17 recommended) as the only authoritative store.
- Object storage holds file bytes; their metadata and hashes stay in Postgres.
- Valkey/Redis ([ADR-0019](ADR-0019-valkey-redis-cache.md)), search indexes and brokers are derived from Postgres and can be rebuilt.
- MariaDB/MySQL are **not supported** as the primary database.

## Why not MariaDB: features the design depends on

| Needed by the design | PostgreSQL | MariaDB (11.x) | Impact if missing |
|---|---|---|---|
| **Row-level security** (`FORCE RLS`) for workspace isolation (12 §12, 03 §7) | Native | **None** | Loses the database-level isolation layer. Every cross-tenant bug becomes a leak. **Blocking.** |
| **Partial indexes** (`WHERE state='queued'`, open-incident uniqueness, inbox index) | Native | **None** | Larger, slower indexes on hot queue/inbox paths. No "one open incident per token" constraint. |
| **Transactional DDL** (migrations roll back atomically) | Yes | No (implicit commit) | Riskier upgrades for self-hosters with live instances. |
| **`jsonb`** binary JSON with GIN indexes (IR, form schemas, settings, audit diffs) | Yes | `JSON` = alias of LONGTEXT; indexing only via generated columns | Slower and clumsier querying of definitions, settings and audit. |
| `ltree` (department/collection subtree scopes) | Extension, built in | None (recursive CTEs only) | Scoped-permission checks get slower. |
| `LISTEN/NOTIFY` (outbox/job wakeups without a broker) | Yes | None | Polling only, or the broker stops being optional ([ADR-0009](ADR-0009-broker-optional.md)). |
| `timestamptz`, `interval`, ranges, exclusion constraints (no overlapping delegations) | Yes | `TIMESTAMP` without zone semantics; no interval type or exclusion constraints | More timezone/SLA bugs; weaker integrity guarantees. |
| `FOR UPDATE SKIP LOCKED` (jobs, timers) | Yes | Yes (10.6+) | — |
| Range partitioning (events/audit by month) | Yes | Yes | — |
| `REVOKE UPDATE/DELETE` + triggers for append-only audit | Yes | Partial (triggers yes; per-column privilege model weaker) | Weaker AUD-07 evidence. |
| Full-text search (`tsvector`, weights, `pg_trgm`) | Strong | Basic `FULLTEXT` | Weaker catalogue search. |
| `citext`, `pgcrypto`, `uuid` type | Yes | Partial | Minor. |
| Rust `sqlx` compile-time checked queries | Yes | Yes | — |
| HA / backup ecosystem: CloudNativePG, Patroni, pgBackRest (MIT), PITR, every managed cloud | Excellent | Good (Galera, MaxScale under BSL) | — |
| Licence for self-hosters | PostgreSQL Licence (permissive) | Server GPLv2 (fine to run unmodified); MaxScale BSL | — |

Missing RLS alone rules MariaDB out for a multi-tenant product where workspace isolation is the top risk (R3).

## Alternatives considered

- **MariaDB/MySQL:** rejected, see the table above.
- **Database abstraction supporting both:** rejected. It would reduce the design to the lowest common denominator and drop RLS, partial indexes and jsonb, and it would double the test matrix.
- **Event store plus a separate projections database:** rejected as too complex.
- **Temporal/Cassandra as the state store:** see ADR-0015.

## Consequences

- One backup/restore story (PITR via WAL archiving).
- Support targets PostgreSQL 16 and 17. The minimum version is enforced at startup. CI tests both.
- Managed options work: AWS RDS/Aurora PostgreSQL, Azure Flexible Server, GCP Cloud SQL/AlloyDB, Neon, Supabase. Self-host uses CloudNativePG or Patroni.
- Indexes and partitions need careful design, validated in SP-01 and SP-10.
- Customers standardised on MariaDB run a dedicated Postgres for Procera. This is documented in the install guide as a hard requirement.
