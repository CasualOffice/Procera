# 11 — Workflow Intermediate Representation (IR) v1

Status: **Draft for G1 sign-off** · Owner: Runtime lead · Closes gaps B1, B2, B5, B6.

The IR is the contract between everything that *authors* processes (step builder, BPMN
import, AI drafts, API) and everything that *executes* them (the native interpreter or an
engine adapter). It is deliberately smaller than BPMN 2.0. Anything the IR cannot express
is either rejected at compile time or reported as unsupported on import — never silently
dropped or approximated.

```
canvas JSON (editor-specific, lossy-free for the editor)
        │  compile (compiler_version)
        ▼
normalized IR v1 (this spec, canonical JSON, checksummed)
        │  bundle (+ forms, rules, calendars, connector refs, SOP ref)
        ▼
release bundle (immutable, content-addressed)  ──► runtime adapter
```

## 1. Design rules

1. **Total and deterministic.** Given the same IR, inputs, clock and IDs, execution produces
   the same transitions. No ambient time, randomness or I/O inside the runtime; all of that
   enters through recorded events (completion signals, timer firings, connector results).
2. **No user code.** Logic is expressed with the restricted expression language (§6) and
   rule tables (§8). Scripts are rejected on import (SEC-09).
3. **Explicit outcomes.** Every node that can end in more than one way declares its
   outcomes; every outcome must be handled by an edge or by an explicit `unhandled: fail`.
4. **Structured parallelism only in v1.** Parallel splits must be closed by a matching join
   (block-structured). Unstructured joins, inclusive (OR) joins and event-based gateways
   are *not* in v1 (see §10).
5. **Versioned and migratable.** `schemaVersion` is an integer; every bump ships a pure
   migration function `vN → vN+1` and fixtures proving round-trip.

## 2. Document shape

```jsonc
{
  "schemaVersion": 1,
  "process": { "key": "purchase-request", "version": 3, "name": { "en": "Purchase request" } },
  "variables": [ /* §5 */ ],
  "nodes":     [ /* §3 */ ],
  "edges":     [ /* §4 */ ],
  "sla":       [ /* §9 */ ],
  "refs": {                       // resolved into the release bundle at publish
    "forms":     { "request": "form:purchase-request@4" },
    "rules":     { "approver": "rule:purchase-approver@2" },
    "calendars": { "default": "calendar:in-blr@2026.2" },
    "connectors":{ "notify": "connector:chat-notify" },
    "sop":       "sop:purchasing-policy@7"
  },
  "meta": { "compilerVersion": "0.4.0", "sourceFormat": "step-builder" }
}
```

Canonical serialization: UTF-8 JSON, keys sorted, no insignificant whitespace, numbers in
shortest round-trip form (RFC 8785 JCS). `checksum = sha256(canonical bytes)`.

## 3. Node kinds (v1)

| Kind | Purpose | Outcomes | Notes |
|---|---|---|---|
| `start` | Entry. Exactly one per process (event-specific starts are `trigger` metadata). | `next` | `trigger`: `manual \| api \| schedule \| event`; `inputForm` optional. |
| `end` | Completes the current path. | — | `result`: `completed \| terminated` (terminate cancels all sibling tokens). |
| `human.task` | Form to fill. | `submitted` (+ custom) | `assignment` §3.1, `form`, `due`, `escalations`. |
| `human.approval` | Decision. | `approved`, `rejected`, `changes_requested` (+ custom) | `quorum`: `one \| all \| n_of_m{n}`; `signature`: `none \| simple \| part11`. |
| `decision` | Exclusive branch on an expression or rule table. | named branches + `default` (required) | First matching branch in declared order. |
| `parallel.split` | Fork into N branches. | one per branch | Must pair with exactly one `parallel.join` (`joinId`). |
| `parallel.join` | Wait for all branches of its split. | `next` | Releases once, under instance lock (04-LLD §7). |
| `wait.timer` | Pause until instant/duration/business duration. | `fired` | `at` (expr → instant) or `after` (ISO-8601 duration; `business: true` uses calendar). |
| `wait.signal` | Pause until a correlated external message. | `received`, `timeout` | `correlation` expression; message schema reference. |
| `service.call` | Invoke a connector action. | `ok`, `error` (+ typed errors) | Executed by connector workers *after* commit; `idempotencyKey` expression mandatory. |
| `notify` | Fire-and-forget notification. | `next` | Channels: `in_app, email, chat, webhook`; does not block on delivery. |
| `subprocess.call` | Start a child instance of another published process. | `completed`, `failed`, `cancelled` | `versionPolicy`: `pinned` (bundle-locked version) only in v1. |
| `case.open` | Open a case and optionally wait for closure. | `opened` or `closed` | Links instance ↔ case. |
| `set` | Assign variables from expressions. | `next` | Pure; no I/O. |
| `human.sign` *(v1.1)* | Send a document for e-signature (FR-DS07) via the campaign engine. | `signed`, `declined`, `expired` | `document` (file var or template + merge fields), `signers` (assignment), `order`: `sequential \| parallel`, `signature`: `simple \| strong \| part11`. |
| `human.acknowledge` *(v1.1)* | Require read/ack of a policy/SOP version or a document. | `acknowledged`, `expired` | `subject` ref, optional `quiz` ref. |

### 3.1 Assignment

```jsonc
"assignment": {
  "candidates": [
    { "type": "user",  "ref": "expr: vars.requester.manager" },
    { "type": "group", "ref": "finance-approvers" },
    { "type": "role",  "ref": "process-owner" }      // contextual role on the process
  ],
  "strategy": "claim",          // claim | round_robin | least_loaded | all (for quorum)
  "excludeInitiator": true,     // segregation of duties
  "excludeActorsOf": ["approval-1"],
  "fallback": { "type": "role", "ref": "process-owner" }   // when resolution yields nobody
}
```

Resolution happens at activation and is recorded in the event log (who was resolved, from
which directory snapshot). Delegation/substitution rules (12-AUTHZ §6) apply *after*
resolution and are recorded separately.

### 3.2 Escalation

```jsonc
"due": { "after": "P2D", "business": true },
"escalations": [
  { "at": "P1D",  "action": "remind" },
  { "at": "P2D",  "action": "reassign", "to": { "type": "user", "ref": "expr: vars.requester.manager.manager" } },
  { "at": "P5D",  "action": "outcome",  "outcome": "rejected", "reason": "auto-expired" }
]
```

`outcome` escalations require the process to be in a policy class that permits automatic
outcomes; controlled (`criticality: controlled`) processes forbid auto-approval.

## 4. Edges

```jsonc
{ "id": "e7", "from": "approval", "on": "approved", "to": "notify-ok" }
```

`on` defaults to `next`. Back-edges (loops) are allowed only if the loop contains a
human node or a `wait.*` node *or* declares `maxIterations` (compiler enforces a bound;
default 50). This prevents hot loops while allowing "request changes → resubmit".

### 4.1 Boundary handling

Any `human.*`, `wait.*`, `service.call` and `subprocess.call` node may declare:

```jsonc
"boundary": [
  { "on": "timeout", "after": "P10D", "interrupting": true,  "to": "escalate-case" },
  { "on": "error:CREDIT_LIMIT", "to": "manual-review" },
  { "on": "cancel", "to": "compensate-provisioning" }
]
```

Non-interrupting boundaries spawn a parallel token and must reach an `end` or rejoin
explicitly — no implicit joins.

## 5. Variables and types

```jsonc
{ "name": "amount", "type": "decimal", "classification": "internal", "required": true },
{ "name": "salary", "type": "decimal", "classification": "restricted", "readers": ["role:hr"] },
{ "name": "requester", "type": "principal" },
{ "name": "items", "type": { "list": { "record": "LineItem" } }, "maxItems": 200 }
```

Types: `string, text, integer, decimal (arbitrary precision), boolean, date, datetime
(always with offset, stored UTC), duration, principal, group, money{currency}, file
(object-store ref + hash), enum{values}, list<T>, record<Name>`.
Classifications: `public, internal, confidential, restricted`. `restricted` values are
envelope-encrypted at rest and only decrypted for principals the policy allows
(12-AUTHZ §5); they never appear in events, logs, notifications or search.
Limits (configurable per tenant): 256 KB serialized variables per instance, 64 KB per
value, files by reference only.

## 6. Expression language

Decision in [ADR-0010](adr/ADR-0010-expression-language.md), selected by spike SP-08
between a **FEEL subset** (aligns with DMN/BPMN; Rust implementation available) and
**CEL** (small, well-specified, non-Turing-complete). Whichever wins, the IR contract is:

- typed against the variable declarations at compile time;
- total: no unbounded iteration, no recursion, evaluation budget (steps + memory);
- pure: no clock, no randomness, no I/O. `now()` is replaced by the triggering event's
  recorded timestamp;
- null-safe with explicit semantics for missing values (three-valued logic is *not*
  allowed to leak into branch selection — a `null` condition result is a compile error
  where detectable and a runtime incident otherwise);
- expressions are stored as source text plus the language tag:
  `"expr:feel: amount > 50000"`; the compiler stores a parsed/validated form.

## 7. Forms

A form is a separately versioned artifact `form:<key>@<n>`:

```jsonc
{
  "schema":   { /* JSON Schema 2020-12: the data contract */ },
  "ui":       { /* layout: sections, widgets, conditional visibility (expressions) */ },
  "bindings": { "amount": "vars.amount", "justification": "vars.justification" },
  "i18n":     { "en": { /* labels */ }, "hi": { /* labels */ } }
}
```

Server-side validation uses the same JSON Schema as the client. Field-level privacy is
inherited from the bound variable's classification; a form cannot downgrade it.

## 8. Rule tables

Decision tables with hit policies `UNIQUE`, `FIRST`, `COLLECT`, typed input/output
columns and expression cells. Versioned as `rule:<key>@<n>`. Every evaluation records rule
version, matched row(s) and inputs (inputs redacted per classification). Full DMN import is
P2 (FR-E10); the table model is designed to be a DMN DRD single-decision subset.

## 9. SLAs

```jsonc
{ "id": "sla-total", "scope": "instance", "target": "P5D", "business": true,
  "pauseWhile": ["wait.signal:vendor-quote"], "breach": [{ "action": "notify", "to": "role:process-owner" }] }
```

Scopes: `instance` or a node id. Clock start/stop/pause events are emitted as domain
events so analytics can recompute SLA attainment independently of the runtime.

## 10. Explicitly unsupported in v1 (and BPMN mapping)

| BPMN construct | v1 status | On import |
|---|---|---|
| Start/end/none events, user task, exclusive gateway, parallel gateway (structured), timer events, message catch (as `wait.signal`), call activity, boundary timer/error | Supported | Mapped |
| Service task, send task | Supported as `service.call` / `notify` if bound to a registered connector | Warning until bound |
| Inclusive gateway, complex gateway, event-based gateway | **Not supported** | Error with element id |
| Script task, scripts in listeners/conditions | **Never** (security) | Error |
| Compensation events, transactions, escalation events | P2 (compensation via `boundary: cancel` only) | Error |
| Signal broadcast, conditional events, multi-instance | P1 candidate (multi-instance → `quorum` covers approvals) | Error/warning |
| Pools/lanes | Documentation only (lanes may seed assignment) | Kept as metadata |

Import produces a **compatibility report** (element id, construct, status, suggested fix)
stored with the draft (SP-06).

## 11. Compiler checks (normative)

1. Exactly one `start`; at least one reachable `end` (or a declared perpetual wait).
2. Unique node and edge ids; every edge endpoint exists.
3. Every declared outcome of every node handled (or `unhandled: fail`).
4. Every `decision` has a `default` edge.
5. Every `parallel.split` has exactly one matching `parallel.join`; branches don't cross.
6. Loops bounded (§4).
7. All expressions type-check against variables; no reference to `restricted` variables in
   `notify` templates or event-facing fields.
8. All `refs` resolve to *published* artifact versions the author may use.
9. Connectors referenced are enabled for the tenant and their scopes are granted.
10. Assignments resolvable in principle (groups/roles exist); SoD rules satisfiable.
11. Timers: calendar exists; durations valid; absolute instants carry an offset.
12. Size/complexity limits: ≤ 500 nodes, ≤ 50 parallel branches, nesting ≤ 10.

Compiler output: normalized IR, diagnostics (error/warning/info with node ids, stable codes
`IR-E001…`), and a semantic diff against the previous published version.

## 12. Conformance suite

`crates/runtime/conformance/` holds IR fixtures + expected event traces. Every runtime
adapter (native or engine-backed) must pass it before it can be enabled; it is the basis of
spikes SP-01…SP-06 and the R1 mitigation. See [15-TEST-STRATEGY §4](15-TEST-STRATEGY-AND-CONFORMANCE.md).
