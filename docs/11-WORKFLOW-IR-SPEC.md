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
| `decision` | Exclusive branch on an expression or rule table. | named branches + `default` (required) | First matching branch in declared order. Rule-table form: `rule` (refs key) + `output` (column); branch names = output values (§13 F1). |
| `parallel.split` | Fork into N branches. | one per branch | Must pair with exactly one `parallel.join` (`joinId`). |
| `parallel.join` | Wait for all branches of its split. | `next` | Releases once, under instance lock (04-LLD §7). |
| `wait.timer` | Pause until instant/duration/business duration. | `fired` | `at` (expr → instant) or `after` (ISO-8601 duration; `business: true` uses calendar). |
| `wait.signal` | Pause until a correlated external message. | `received`, `timeout` | `correlation` expression; message schema reference. |
| `service.call` | Invoke a connector action. | `ok`, `error` (+ typed errors) | Executed by connector workers *after* commit; `idempotencyKey` expression mandatory. |
| `notify` | Fire-and-forget notification. `to` (assignment shape), `template`. | `next` | Channels: `in_app, email, chat, webhook`; does not block on delivery. |
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

### 6.1 As built (BPMS-081): the expression engine

Crate `crates/expr` (`procera-expr`). It implements a **typed, total, budgeted subset of CEL** with Procera extensions, as SP-08 and ADR-0010 recommend. The language tag is `cel` (`"expr:cel: amount > 50000"`). The crate has no Procera dependencies, does no I/O, and reads no clock, randomness or threads, so the IR compiler, the runtime and the browser designer share it. `cargo check --target wasm32-unknown-unknown -p procera-expr` passes. See assumption A-22 in docs/22 for why the engine is our own implementation rather than `cel-rust`.

**API.**

| Call | Purpose |
|---|---|
| `parse(src)` | Source to AST, or the first syntax diagnostic |
| `check(src, ast, &Schema, &CheckOptions)` / `compile(src, …)` | Type-check against the declared variables and get a `CheckedExpr`: the AST, its type, and the variables it `reads` with their classification. Serializable; this is the stored compiled form (docs/04 §4.5) |
| `evaluate(&CheckedExpr, &Bindings, &EvalEnv, &Budget)` | Evaluate. `evaluate_metered` also reports the steps and bytes used |
| `Bindings::new` / `Bindings::from_json` | Variable values, checked against the schema. JSON is marshalled by the declared type: integers stay integers, decimals are read exactly from text, and datetimes need an offset (SP-08 C08) |
| `Type::from_ir(json)` | Maps a §5 IR type to an expression type |
| `split_tagged("expr:cel: …")` | Splits the language tag from the source |

**Grammar** (CEL precedence, loosest first):

```text
expr     = or ["?" or ":" expr]
or       = and {"||" and}
and      = rel {"&&" rel}
rel      = add {("==" | "!=" | "<" | "<=" | ">" | ">=" | "in") add}
add      = mul {("+" | "-") mul}
mul      = unary {("*" | "/" | "%") unary}
unary    = {"!" | "-"} member
member   = primary {"." IDENT ["(" args ")"] | "[" expr "]"}
primary  = IDENT ["(" args ")"] | "(" expr ")" | "[" [args] [","] "]" | literal
literal  = INT | DECIMAL | STRING | "true" | "false" | "null"
```

- `INT` is a run of digits (64-bit).
- `DECIMAL` is `digits.digits`. It is an exact decimal (28 significant digits). There is no floating point, no exponent and no hex.
- Strings use `"…"` or `'…'` with the escapes `\n \r \t \\ \" \' \uXXXX`.
- `//` starts a comment.
- CEL reserved words (`if`, `for`, `let`, …) can't be identifiers.
- Comprehension macros: `list.all(x, p)`, `exists`, `exists_one`, `map(x, e)` and `filter(x, p)`.
- Limits: source ≤ 4096 bytes, AST depth ≤ 32 (SP-08).

**Types.**

| Expression type | IR types (§5) | Notes |
|---|---|---|
| `boolean`, `integer`, `decimal`, `string` | `boolean`, `integer`, `decimal`, `string`/`text` | An integer is accepted wherever a decimal is. The checker inserts an explicit widening, so a decimal-typed expression always evaluates to a decimal |
| `date`, `datetime`, `duration` | same | Datetimes are instants (UTC). Durations follow the §13.9 subset (`W D H M S`, optional leading `-`); months and years are rejected |
| `money<CUR>` | `money{currency}` | The currency is part of the type, so mixing currencies is a compile error. `.amount` and `.currency` fields |
| `enum{…}` | `enum` | Values are strings. Comparing with a string literal outside the enum is an error |
| `principal` | `principal` | `id`, `manager?`, `department?`, `email?` (§13.10) |
| `group` / `file` / `instance` | `group`, `file`, context | `id`, `name?` / `name`, `hash`, `size`, `content_type?` / `id`, `display_id`, `started_at` |
| `list<T>`, `record{…}` | `list`, `record` | Records are structural. Their fields are required |
| `T?` | any variable without `required: true` | May be null |

`Schema::with_process_context()` declares `initiator` and `instance`. `now()` is a function, bound from the evaluation environment.

**Null rules.** These make docs/11 §6 hold at compile time.

- `x.f` on a value that may be null yields `null` (null-safe navigation). The result type is `T?`.
- `==`/`!=` accept null on either side.
- Any other use of a `T?` value is error `EXPR-E016`: arithmetic, ordering, indexing, function arguments, receivers and `&&`/`||`.
- Guards narrow a variable: `x != null ? <x is T here> : …`, `x != null && <here>`, `x == null || <here>`, and `!`. This works for identifiers and field paths (`initiator.manager`).
- A condition (`CheckOptions::condition()`, `?:`, `&&`/`||` operands, macro predicates) whose type is `boolean?` is error `EXPR-E018`. Write `flag == true`.
- Checked expressions never produce a runtime type error. Missing variables are compile errors.

**Operators.**

- **Arithmetic.**
  - Integers are checked, so overflow is an error. Integer `/` truncates.
  - If either side is a decimal, the operation is exact decimal arithmetic. Quotients drop trailing zeros, so `1.0 / 2` is `0.5`.
  - Money: `money ± money` (same currency), `money * n`, `n * money` and `money / n` give money. `money / money` gives a decimal.
- **Time.**
  - `datetime ± duration` gives a datetime, and `datetime - datetime` gives a duration.
  - `date ± duration` gives a date. Whole days only; anything else is a runtime error.
  - `date - date` gives a duration.
  - Durations support `±`, `* integer` and unary `-`.
- **Strings and lists.** `string + string` concatenates; an enum counts as a string. `list + list` joins the lists.
- **Comparison.** `<` `<=` `>` `>=` work on numbers, strings, dates, datetimes, durations and same-currency money. `==` compares deeply, and integers equal decimals by value. `x in list` tests membership.
- **Logic.** `&&` and `||` short-circuit left to right. This is a deliberate simplification of CEL's commutative error absorption.

**Functions.** All are pure and bounded.

| Group | Functions |
|---|---|
| Conversion | `string(x)`, `int(x)` (truncates decimals), `decimal(x)`, `money(n, "CUR")` (literal ISO code), `date(s \| datetime)`, `datetime(s \| date)`, `duration(s)`. Literal string arguments are validated at compile time (`EXPR-E021`) |
| Time | `now()` (the injected event timestamp), `today()` (date of `now` at the injected offset). Methods `.year() .month() .day() .weekday()` (1 = Monday); on datetimes also `.hour() .minute()`, both read at the injected offset. On durations: `.inDays() .inHours() .inMinutes() .inSeconds()` |
| Numbers | `abs`, `round(n[, places])` (half away from zero, 0–28 places; integer input gives a decimal), `floor`, `ceil`, `min(a, b)`, `max(a, b)` |
| Aggregates | `sum(list)` over integers, decimals, money or durations (an empty list gives a typed zero), `avg(list)` (decimal), `min(list)`, `max(list)`. `avg`, `min` and `max` of an empty list are an error |
| Strings | `size`, `.contains .startsWith .endsWith .lower .upper .trim .replace(a, b) .split(sep) .substring(start[, end])` (character indices), and `list.join(sep)` |
| Lists | `size`, `[i]`, `in`, `+`, `all`, `exists`, `exists_one`, `map`, `filter` |

**Budgets** (`Budget::default()`, docs/04 §4.5). Exceeding any limit is a typed `EvalError` (`is_budget()`), which the runtime raises as incident **`IR-R010`**. It is never a panic or a hang.

| Limit | Default | Notes |
|---|---|---|
| `max_steps` | 10 000 | One per AST node visited, plus one per list element or 64 string bytes processed |
| `max_alloc_bytes` | 1 MiB | Cumulative bytes of strings and lists built (16 bytes per list slot) |
| `max_string_len` | 64 KiB | §5 per-value limit. Sizes are computed before building, so `replace` or `join` can't blow up memory |
| `max_list_len` | 10 000 | Lists the expression builds |
| `max_depth` | 64 | Evaluator recursion. Parsed ASTs are already ≤ 32 deep; this covers stored ASTs |

The step budget replaces SP-08's suggested 10 ms wall-clock guard. A wall clock would make evaluation non-deterministic, and `wasm32` has no clock.

**Determinism.** Evaluation depends only on the checked expression, the bindings, `EvalEnv { now, tz }` and the budget. `tz` is a fixed UTC offset that the caller resolves from the process calendar at `now`. Property tests assert that equal inputs give equal results and equal usage.

**Classification.** `CheckedExpr::reads` lists the root variables read; comprehension variables are not reads. `classification()` is the highest classification among them, and a result derived from restricted inputs is restricted. With `ExprContext::EventFacing` (notify templates, values copied into events or idempotency keys), reading a `restricted` variable is error `EXPR-E023` (§11 check 7). Only the variables an expression reads need to be bound, so restricted values can stay sealed otherwise.

**Diagnostic codes.** Compile-time diagnostics carry a stable code, a byte span (`Span::line_col` gives line and column) and a message. They are all errors.

| Code | Meaning | Code | Meaning |
|---|---|---|---|
| EXPR-E001 | Syntax | EXPR-E015 | Argument type |
| EXPR-E002 | Lexical (character, string, escape) | EXPR-E016 | Value may be null |
| EXPR-E003 | Number out of range / too precise | EXPR-E017 | Result is not the expected type |
| EXPR-E004 | Source > 4096 bytes | EXPR-E018 | Condition may be null |
| EXPR-E005 | Nesting > 32 | EXPR-E019 | Not a value of the enum |
| EXPR-E006 | Reserved word | EXPR-E020 | Currency mismatch |
| EXPR-E010 | Unknown variable | EXPR-E021 | Invalid literal argument |
| EXPR-E011 | Unknown field | EXPR-E022 | List mixes types |
| EXPR-E012 | Unknown function or method | EXPR-E023 | Restricted variable in an event-facing context |
| EXPR-E013 | Wrong number of arguments | EXPR-E024 | Malformed comprehension |
| EXPR-E014 | Operator does not apply to these types | EXPR-E025 | Type cannot be ordered |

Runtime errors:

| Code | Meaning |
|---|---|
| EXPR-R001 … R005 | Budget: steps, allocation, string length, list length, depth (→ `IR-R010`) |
| EXPR-R010 | Division by zero |
| EXPR-R011 | Overflow (integer, decimal or time range) |
| EXPR-R012 | Index out of bounds |
| EXPR-R013 | Invalid argument (bad text to parse, substring range, partial day on a date…) |
| EXPR-R014 | `avg`/`min`/`max` of an empty list |
| EXPR-R015 | A required variable the expression reads was not bound |
| EXPR-R090 | Internal type error. Unreachable for checked expressions; property-tested |

Binding errors are `EXPR-B001` (variable not declared) and `EXPR-B002` (value doesn't match its type).

**Not in the subset** (compared with full CEL):
- `double`, `uint`, `bytes`, map literals, protobuf messages, `has()`, `timestamp()`/`duration()` with CEL's own formats, regular expressions (`matches`), and string formatting;
- `[]` has element type "nothing", so functions that need a typed list (e.g. `min`) reject it;
- business-time functions (`business_days_between`) wait for calendars.

**Tests.**
- Unit tests for every feature and diagnostic.
- The reference-process expressions rewritten in CEL (`tests/reference.rs`).
- Property tests: evaluation is deterministic, any budget terminates with a value or a typed error, arbitrary text never panics, and well-typed expressions don't go wrong (the value has exactly the checked type).

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

## 13. v1 clarifications from reference modelling (BPMS-002)

Modelling the five reference processes ([docs/spikes/BPMS-002](spikes/BPMS-002-reference-processes.md)) produced these normative clarifications. They are encoded in [`schemas/ir/v1.schema.json`](../schemas/ir/v1.schema.json) and the structural checker in [`conformance/`](../conformance/src/lib.rs).

1. **Rule-table decisions:** `decision{rule, output, branches[]}`. Branch names are output values. `default` is mandatory.
2. **Assignment on every `human.*` node**, including `human.acknowledge` and `human.sign` (as `signers`).
3. **`notify`:** `to` uses the assignment shape. `template` names a localised message template. Channels: `in_app, email, chat, webhook, push`.
4. **Configuration-dependent outcomes:**
   - `wait.signal` yields `timeout` only when `timeout` is set.
   - `case.open` yields `opened`, or `closed` when `waitForClose: true`.
   - `human.task` with `outcomes: [...]` replaces the default `submitted`.
5. **Terminate inside a branch.** An `end{result: terminated}` reachable inside a parallel branch is allowed and cancels sibling tokens. Any other end inside a branch is IR-E008.
6. **Escalation actions:** `remind | reassign | outcome | notify`.
7. **Signature levels** across approvals and signing: `none | simple | strong | part11` (strong = password/passkey/OTP re-auth; part11 = ESIG-03).
8. **Refs:** `forms, rules, calendars, connectors, documents, processes` (maps), plus `sop` and `policy` (single refs).
9. **Durations:** ISO-8601 subset with W, D, T…H/M/S only. Months and years are rejected (no fixed length). Calendar-based recurrences use `schedule` triggers.
10. **Expression context** (§6): `initiator` and principals expose `.id`, `.manager`, `.department`, `.email`. `instance` exposes `.id`, `.display_id`, `.started_at`. `now` is the triggering event's time. Variables are accessed by name.
11. **Edges** require a stable `id`. An edge without `on` means `next`.
12. **Diagnostic codes** implemented so far:

| Code | Check |
|---|---|
| IR-E001 | Exactly one `start` |
| IR-E002 / E003 | Duplicate node / edge id |
| IR-E004 | Edge endpoint missing |
| IR-E005 | Outcome not handled |
| IR-E006 | Edge uses an outcome the node can't produce |
| IR-E007 | Decision without `default` |
| IR-E008 | Split/join pairing or a branch escaping its join |
| IR-E009 | Unreachable node |
| IR-E010 | No reachable end |
| IR-E011 | Unbounded loop |
| IR-E012 | Size limits |
| IR-E013 | Edges into `start` / out of `end` |
| IR-E014 | Unknown SoD reference |
| IR-E015 | Unknown boundary target |
| IR-E016 | Duplicate variable |
| IR-E100 | Schema violation |
