# SP-10: Hash-chained audit log at scale

Status: **Done. The LLD audit design (04 §2.10) is validated with the batched sealer; targets exceeded.** · Date: 2026-10-11 · Tracker: BPMS-007

Code: [`spikes/sp10-audit-chain`](../../spikes/sp10-audit-chain/src/main.rs). Run it against a scratch Postgres 16 with `PG_URL=… cargo run --release -p sp10-audit-chain`. Set `SP10_QUERIES_ONLY=1` to re-run only the explorer queries.

Environment: Apple Silicon laptop (10 cores, 16 GB), local Postgres 16.15 (`shared_buffers=512MB`, `synchronous_commit=on`), sqlx 0.8 pool of 32.

Schema as in [04-LLD §2.10](../04-LLD.md):
- `audit_events` range-partitioned by month;
- `audit_heads` per workspace;
- an unlogged `audit_pending` table for the batched path;
- SHA-256 chain over canonical JSON.

## Targets (docs/01 §8, docs/02 FR-T03)

- Append ≥ 2 000 events/s per workspace, with batching allowed.
- Audit-explorer P95 < 2 s at 10 M events.
- The verifier detects tampering.

## Results

| Test | Result | vs target |
|---|---|---|
| **T1** single writer, one event per transaction, head row lock | 5 000 events in 0.69 s → **7 261 events/s** | ✅ 3.6× |
| **T2** 16 concurrent writers, *same* workspace, one event per transaction | 8 000 events → **9 675 events/s**. Append latency P50 0.59 ms, **P95 6.3 ms, P99 11 ms** | ✅. Contention shows in the tail. |
| **T3** batched sealer: 16 producers insert into `audit_pending` (no chain lock), 1 sealer chains 1 000 per transaction | 50 000 events sealed end-to-end at **65 062 events/s** | ✅ 32× |
| **T5** verify the chain | **536 161 events/s** recomputed | — |
| **T5** tamper: `UPDATE` one event's payload directly in the DB | Verifier reports **first broken seq = 4242** (the edited row) | ✅ |
| **T4** load 10 M events + 4 composite indexes | Load 23 s. Indexes + `ANALYZE` 43 s. **3.98 GB** table + indexes | — |
| **T4** Q1 actor, last 30 days, newest 100 | P50 0.31 ms, **P95 0.58 ms** (cold first run: P95 3.1 ms) | ✅ |
| **T4** Q2 object timeline | P50 0.40 ms, **P95 0.53 ms** | ✅ |
| **T4** Q3 category + action, last 7 days | P50 0.17 ms, **P95 0.46 ms** | ✅ |
| **T4** Q4 keyset page 90 days back | P50 0.10 ms, **P95 0.29 ms** | ✅ |

Caveat: the explorer numbers are warm-cache on a single laptop with the hot workspace mostly in memory. Production sizing must assume colder caches. Even 100× slower would remain inside the 2 s budget. M10 load tests re-measure on reference hardware (03 §12).

## Decisions

1. **Keep the per-workspace head lock** for normal business audit events, which are appended in the same transaction as the business change (atomicity, ADR-0005). About 9.7k events/s per workspace under contention, with P99 11 ms, is ample: the reference load is 100 transitions/s burst.
2. **Use the batched sealer for high-volume categories.** These are `data_access` reads (FR-T01), bulk exports and SIEM-heavy auth events.
   - Hot read paths write to `audit_pending` only, with no chain lock.
   - A sealer job chains up to 1 000 events per transaction, every ≤ 1 s.
   - The pending table must be **logged** in production, not unlogged as in the spike, so a crash can't lose access events. Expect lower but still large headroom.
   - The chain is gap-free and per-workspace ordering is preserved.
3. **Indexes** as in 04 §2.10 are sufficient: `(ws, occurred_at)`, `(ws, actor, occurred_at)`, `(ws, object_type, object_id, occurred_at)`, `(ws, category, action, occurred_at)`. Monthly partitions keep index builds and retention drops cheap.
4. **Storage planning:** about **400 bytes per event** including indexes with a small payload. Size retention and partitions accordingly: 10 M events ≈ 4 GB.
5. **Verifier:** at about 0.5 M events/s, a full re-verification of 100 M events takes about 3–4 minutes. Checkpoints (AUD-02) make incremental verification O(new events).

## Follow-ups (tracker)

- BPMS-026 (R1): head-lock append in the business transaction, plus the sealer job for `data_access` with a **logged** pending table. Property test: chain is gap-free under concurrent producers and sealer crashes.
- BPMS-064 (R1): explorer queries as tested, with keyset pagination.
- BPMS-162 (R5): Merkle checkpoints and the offline `procera-verify` CLI reuse the T5 verifier logic.
