# SP-01 … SP-06: Native Rust runtime on PostgreSQL

Status: **Done. All six pass. Recommendation: native interpreter (ADR-0015 primary path).** · Date: 2026-10-11 · Tracker: BPMS-003 · Decides: [ADR-0015](../adr/ADR-0015-runtime-selection.md) at G1

Code: [`spikes/sp01-runtime`](../../spikes/sp01-runtime/src/).
- `engine.rs`: advance loop under the instance row lock, tokens, splits/joins, work items, timers, service calls, cancellation, idempotent decisions, deduplicated signals.
- `workers.rs`: job and timer workers with **leases + fencing**, chaos injection and supervised respawn.
- `bpmn.rs`: IR ↔ BPMN subset converter.

Run it against a scratch Postgres 16 with `PG_URL=… cargo run --release -p sp01-runtime [sp01…sp06|all]`. Every scenario hard-asserts its pass condition and a set of global invariants:
- gap-free event sequences per instance;
- no live tokens in finished instances;
- joins release only when complete, and exactly once;
- no failed instances.

Environment: Apple Silicon laptop, local Postgres 16.15, sqlx pool of 80.

## Results

| Spike | Scenario | Result |
|---|---|---|
| **SP-01** durability | 10 000 instances wait at an approval. **All workers are killed and restarted.** 10 000 decisions are then made by 32 concurrent deciders while 16 workers run with **3 % crash probability per job** (mid-transaction, and *after* the external side effect but before recording it), plus a **hard kill of a random worker every 150 ms**. | ✅ **10 000 / 10 000 completed**, 6.7 s after the last decision. 1 329 crashes injected. **0 lost transitions**. 0 stuck jobs. 1 stale worker was rejected by fencing. Invariants hold. |
| **SP-02** double approval | 1 000 work items × 2 concurrent deciders with conflicting outcomes. Then a replay of a used idempotency key. | ✅ **Exactly 1 000 committed, 1 000 "already decided"**, 1 000 `work.completed` events. The replay returned the stored result (no second transition). |
| **SP-03** join / timer / escalation / cancellation | 600 instances with a 3-way parallel split (two human tasks + a 1 h timer → join). +31 min fires the escalation timers. 100 instances are cancelled mid-parallel. The remaining tasks are completed concurrently **while the clock jumps past the timers** (branches racing into the join). Workers crash at 2 % plus a hard kill every 300 ms. | ✅ **All 600 escalations fired. Join released exactly 500 times for 500 instances.** Cancelled instances: 0 live tokens, 0 wait-timers fired, late decision → `AlreadyDecided`. |
| **SP-04** idempotent side effects | (a) In SP-01, service calls are retried after crashes. (b) 1 000 instances waiting on a signal, each delivered **3× concurrently**. | ✅ (a) 7 023 approvals → **7 023 effects applied** (7 433 attempts: 410 retries absorbed by idempotency keys). 0 instances with more than one effect. (b) 3 000 deliveries → **1 000 accepted**, 1 000 `signal.received`. |
| **SP-05** 30-day dormant, version pinning | 100 instances start on v1 (timer P30D → review task). v2 is published (timer → end). 100 instances start on v2. Clock +30 days. | ✅ All 100 v1 instances are at `review` (their pinned graph). All 100 v2 instances completed. |
| **SP-06** BPMN round trip | Export the five reference processes to BPMN 2.0 XML and import them back. Import a foreign Camunda-style file with an inclusive gateway and a script task. | ✅ **54 nodes / 75 flows round-trip losslessly** (graph and timer durations identical, via a `procera:kind` extension attribute). Foreign file: both unsupported elements reported **by id**, plus mapping warnings for un-annotated elements. |

## What made it work (design points now proven)

1. **One writer per instance:** `SELECT … FOR UPDATE` on the instance row before any change. Decisions, timers, signals and advance jobs all serialise on it. This alone makes SP-02 and the join race in SP-03 correct.
2. **Leases + fencing for jobs and timers:**
   - `UPDATE … SET fence = fence + 1 … FOR UPDATE SKIP LOCKED RETURNING fence` on claim;
   - the completing write checks `fence = $claimed AND lease_owner = $me`;
   - an expired lease is reclaimable.

   Crashed or killed workers lose nothing. Stale workers can't overwrite results.
3. **Side effects outside the transaction, idempotent by key.** The key is derived from the instance and the event sequence of the request (`svc:{instance}:{seq}`), so it is stable across retries and unique across loop iterations.
4. **Signals through an inbox dedup table** in the same transaction as the token resume.
5. **The advance loop is idempotent:** it processes whatever tokens are ready, so duplicate advance jobs are harmless. That allows `INSERT … ON CONFLICT DO NOTHING` enqueueing without a global "one job per instance" lock.
6. **Version pinning** is just the instance's `def_version`. The interpreter loads that version's IR; release bundles (ADR-0012) extend this to forms, rules and calendars.

## Gaps the spike did not cover (carried into R2 work, BPMS-080…089)

- Expressions are a toy evaluator here (SP-08 decided CEL).
- Assignment resolution, SoD, OOO and quorum (BPMS-085/086).
- Boundary events and non-interrupting branches.
- Incidents for permanent connector errors (the spike treats every connector call as eventually succeeding).
- The outbox dispatcher (BPMS-029).
- Workspace RLS in the runtime tables (SP-09 covered RLS separately).
- Long histories and event partitioning.
- Load beyond about 10k instances. M10 load tests use reference hardware.

## Comparison and recommendation (ADR-0015)

| Option | Evidence | Verdict |
|---|---|---|
| **Native interpreter on Postgres** | SP-01…06 all pass with about 700 lines of spike code. Single binary, single database. Data-driven definitions are native. Every guarantee is visible and testable in our own conformance suite. | **Recommended** |
| Temporal + Rust SDK (fallback) | Not prototyped. Docker is unavailable in this environment, and the native path met every bar. Desk analysis is unchanged (01 §2): it adds a cluster plus its own persistence, needs a generic interpreter workflow, has history limits, and Rust worker-versioning support is unverified. | Keep as documented fallback; no spike needed unless native fails at R2 |
| Operaton adapter | Reference only: BPMN semantics benchmark and possible C7-migration adapter (P2) | Not chosen |

**ADR-0015 → Recommended: native interpreter.** Final acceptance at G1 (BPMS-009), together with ADR-0010 (CEL) and ADR-0011 (Cedar).
