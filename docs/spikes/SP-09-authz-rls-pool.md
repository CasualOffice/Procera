# SP-09: Cedar + Postgres RLS + sqlx connection pool

Status: **Done. ADR-0011 validated (Cedar + RLS), with one LLD correction.** · Date: 2026-10-11 · Tracker: BPMS-006 · Decides: [ADR-0011](../adr/ADR-0011-authorization-engine.md)

Code: [`spikes/sp09-authz-rls`](../../spikes/sp09-authz-rls/src/main.rs). Run it against a scratch Postgres 16 with `PG_URL=postgres://postgres@127.0.0.1:55432/postgres cargo run --release -p sp09-authz-rls`. Measured on a laptop, local Postgres 16.15, `cedar-policy` 4.13, `sqlx` 0.8.

## A. Workspace isolation with RLS through a pool

Setup:
- 600 000 rows across 3 workspaces;
- `FORCE ROW LEVEL SECURITY`;
- the app role `procera_app` is NOSUPERUSER, NOBYPASSRLS and not the table owner;
- pool of 20 connections.

| Test | Result |
|---|---|
| **A1** 5 000 concurrent transactions, random workspace, 10 % "forgot to set context", each trying to read 300 ids sampled from **all** workspaces | ✅ **0 cross-workspace rows leaked**. Transactions with context saw only their own rows. Transactions without context saw **0 rows**. |
| **A2** One connection uses *session-level* `set_config(..., false)` and is returned to the pool | ❌ (as intended) The next borrower, with no context, saw **200 000 rows of workspace A**. This proves why session-level settings are forbidden (docs/24 §3.2). |
| **A3** List query (filter + sort, 50 rows) | With RLS + `BEGIN` + `set_config` + `COMMIT`: **P50 261 µs, P95 360 µs**. Superuser without RLS: P50 129 µs, P95 171 µs. The ~190 µs overhead is mostly the 3 extra round-trips, not the policy. It is far inside the 250 ms list budget (04 §10). |

### Finding A-1: the LLD's policy SQL must use `NULLIF` (fixed)

After a transaction-local `set_config('app.workspace_id', …, true)` commits, Postgres keeps the custom setting **defined as an empty string** on that pooled connection. The LLD policy `current_setting('app.workspace_id', true)::uuid` therefore raises `invalid input syntax for type uuid: ""` for the next transaction that forgets to set context. That fails closed, but as a 500 error instead of "zero rows".

**Corrected policy** (applied to [04-LLD §2.2](../04-LLD.md) and [12 §12](../12-AUTHORIZATION-MODEL.md)):

```sql
USING      (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid)
WITH CHECK (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid)
```

### Finding A-2: round-trip cost

`BEGIN; SELECT set_config(...); <query>; COMMIT` costs three extra round-trips. In the repository layer, send `set_config` in the same pipelined batch as the first statement. sqlx supports this through its pipelining executor. Budget: ≤ 0.2 ms per request. No change to the design.

## B. Cedar

Policies generated from role grants:
- "HR Designer" (custom; `process:view`, `process:edit_draft`; scope collection `hr`);
- "Workspace Admin" (all process actions; scope workspace);

plus two fixed `forbid` invariants (tenant isolation; SoD author ≠ approver for controlled processes).

| Case | Expected | Got |
|---|---|---|
| B1 HR designer edits a draft in `hr/onboarding` (subtree scope via entity parents) | Allow | ✅ Allow |
| B2 HR designer edits outside scope (`fin`) | Deny | ✅ Deny |
| B3 HR designer approves (permission not in role) | Deny | ✅ Deny |
| B4 Admin approves a standard process | Allow | ✅ Allow |
| B5 Admin approves **own controlled** change: the `forbid` invariant beats the `permit` | Deny | ✅ Deny |
| B6 A ws2 member holding a ws1 role grant (misconfiguration) is still isolated | Deny | ✅ Deny |

**Latency:** P50 **5.6 µs**, P99 **7.3 µs** per decision with an in-memory entity set. There's ample headroom for per-row checks on 50-row pages.

### Partial evaluation for list filters (experimental feature)

Request: `(alice, process:view, resource = unknown)`. The residuals are:

```
permit … when { unknown("resource") in Collection::"ws1/hr" }
forbid … when { !(Workspace::"ws1" == unknown("resource").workspace) }
```

Both translate mechanically to SQL:
- `resource in Collection::X` → `collection_path <@ 'ws1.hr'` (ltree subtree, 04 §2.4);
- the isolation residual is already enforced by RLS.

**Decision:**
- Use partial evaluation to build list predicates for a **closed residual grammar**: `in` entity, `==`/`!=` on attributes, `&&`/`||`/`!`, `has`.
- Any residual outside the grammar falls back to **hand-compiled scope predicates** per list (04 §5) plus a per-row `is_authorized` check, with page refill.
- Because `partial-eval` is behind Cedar's `experimental` flag, the fallback path is mandatory and tested by the same equivalence property test (12 §13: "partial-evaluation equivalence").

### Finding B-1: reasons must map to stable ids

`diagnostics().reason()` returns positional ids (`policy0`…), not the `@id` annotation. The policy compiler must create policies with explicit ids, using `Policy::new_id` or a `PolicySet` built with ids, so audit records store `role:hr-designer` or `invariant:sod-author-approver` (12 §1 "explainable decisions").

## Verdict

- **ADR-0011 → Accepted at G1.** Embed Cedar, use data-driven policies compiled from the permission catalogue, keep fixed `forbid` invariants, and use RLS as defence in depth.
- **LLD corrections applied:** `NULLIF` in RLS policies, pipelined `set_config`, explicit policy ids.
- **Tracker follow-ups:** BPMS-024 (policy compiler with explicit ids, residual→SQL translator with fallback), BPMS-025 (RLS migrations with `NULLIF`, CI sweep including the "context not set ⇒ 0 rows" case).
