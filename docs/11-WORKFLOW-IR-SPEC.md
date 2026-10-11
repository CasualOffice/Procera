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

The production compiler (BPMS-080, §14) keeps these codes and meanings and adds the ones below.

## 14. As built (BPMS-080)

**Crate.** `crates/ir` (`procera-ir`) holds the IR v1 model, the compiler, the canonical form and the semantic diff. It depends on serde, `serde_jcs`, `sha2` and `jsonschema` only. It does no I/O and reads no clock or randomness, so the same input always gives the same output.

**Pipeline.**

```text
source JSON ─ JSON Schema (IR-E100) ─ typed parse ─ §11 checks ─┬─ any error → diagnostics only
                                                                └─ none → normal form → JCS bytes → sha256
```

**Source format.** The compiler's input is an IR v1 document as authored, which need not be in normal form. Each editor maps its own canvas JSON to this shape before compiling. See A-21 in [22](22-UNKNOWNS-ASSUMPTIONS-AND-OPEN-QUESTIONS.md).

**Normal form.** In the normal form:
- every edge carries `on` (`next` when it was absent, §13.11);
- nodes and edges are sorted by `id`, variables by `name`, and SLAs by `id`;
- empty `sla`, `refs` and `meta` are omitted;
- `meta.compilerVersion` is set to the compiler's output version (`0.1.0`).

Order inside a node (branches, candidates, escalations) is meaningful and is kept as authored. The canonical bytes are the RFC 8785 (JCS) serialisation of the normal form. The checksum is the lowercase hex SHA-256 of those bytes, so key order and entity order in the source do not change it. Compiling the normal form again returns it unchanged.

**Public API.**

| Item | Purpose |
|---|---|
| `compile(&Value, &CompileContext) -> CompileOutput` / `compile_str` | Compile. `CompileOutput { compiled: Option<Compiled { ir, canonical, checksum }>, diagnostics, diff }`. `compiled` is `None` when any diagnostic is an error. |
| `CompileContext { expressions, environment, previous }` | Injected ports. `Default` uses the built-in `LexicalChecker` and no environment. `previous` is the last published IR, used for the diff. |
| `ExpressionChecker` (port) | `analyse(lang, body) -> Result<Analysis { references }, String>`. `LexicalChecker` checks brackets and strings, collects root variable names (§13.10 context roots excluded) and is a stand-in until BPMS-081 (CEL, ADR-0010) supplies the real parser and type checker. |
| `Environment` (port) | Workspace snapshot for checks 8–10: `resolve(ref) -> Resolution`, `connector_usable(ref)`, `principal_exists(type, name)`. Without one, those checks are skipped and `IR-I001` is reported. |
| `normalize`, `canonical_bytes`, `checksum_of`, `COMPILER_VERSION` | Canonical form and hash. |
| `diff(old, new) -> SemanticDiff`, `SemanticDiff::apply(base)` | Semantic diff, explained below. |
| `validate_schema`, `SCHEMA_V1`, `model::*`, `Code`, `Severity`, `Diagnostic` | Shape check, the data model and diagnostics. |

**Diagnostics.** Each diagnostic has a stable `code`, a fixed `severity` (`error`, `warning` or `info`), a JSON `pointer` into the *source* document (RFC 6901), the `node` id when there is one, and a message. Diagnostics are sorted by code, then pointer. Only errors block the output.

**Semantic diff.** Entities are matched by stable id:
- nodes and edges by `id`;
- variables by `name`;
- SLAs by `id`;
- refs by `<section>.<key>` (for example `forms.request`), or `sop` and `policy`;
- the process header as `process`.

Each change is `added`, `removed` or `changed`. A changed entity lists the top-level members that differ and carries its before and after values. `meta` is not diffed. `diff(a, b).apply(a)` reproduces the normal form of `b`, and `apply` refuses a base that does not match the diff's before side.

**Conformance fixtures.** Compiler fixtures live in `conformance/compiler/<case>/source.json`, each with an `expected.json` that lists the non-info diagnostics (`code`, `pointer`) and the checksum, which is `null` when the compile fails. Golden checksums of the five reference processes are in `conformance/compiler/reference.checksums.json`. A test also checks that the compiler reports every code the BPMS-002 checker (`conformance/src/lib.rs`) reports for the same input.

**Diagnostic codes.**

| Code | Severity | §11 check | Meaning |
|---|---|---|---|
| IR-E001 | error | 1 | Exactly one `start` node |
| IR-E002 | error | 2 | Duplicate node id |
| IR-E003 | error | 2 | Duplicate edge id |
| IR-E004 | error | 2 | Edge endpoint does not exist |
| IR-E005 | error | 3 | Outcome not handled (no edge and no `unhandled: fail`) |
| IR-E006 | error | 3 | Edge uses an outcome the node cannot produce |
| IR-E007 | error | 4 | Decision without a `default` edge |
| IR-E008 | error | 5 | One of: a split/join pairing error; a branch reaching a non-terminate end before its join; two branches sharing a node; the block being entered from outside |
| IR-E009 | error | 1 | Node unreachable from `start` |
| IR-E010 | error | 1 | No `end` reachable from `start` |
| IR-E011 | error | 6 | Loop without a human/wait node or `maxIterations` |
| IR-E012 | error | 12 | More than 500 nodes, more than 50 branches on a split, or parallel nesting deeper than 10 |
| IR-E013 | error | 2 | Edge into `start` or out of `end` |
| IR-E014 | error | 10 | `excludeActorsOf` references an unknown node |
| IR-E015 | error | 2 | Boundary target does not exist |
| IR-E016 | error | 2 | Duplicate variable name |
| IR-E017 | error | 3 | One outcome handled by more than one edge (that would be an implicit fork, §1.4) |
| IR-E018 | error | 3 | A field the node kind requires is missing. The required fields are: `assignment` on `human.task`, `human.approval` and `human.acknowledge`; `signers` and `document` on `human.sign`; `subject` on `human.acknowledge`; `branches` on a decision or split; `rule` and `output` together on a rule-table decision; `when` on each branch of an expression decision; `correlation` on `wait.signal`; `connector`, `action` and `idempotencyKey` on `service.call`; `to` and `template` on `notify`; `process` on `subprocess.call` |
| IR-E019 | error | 7 | Expression is malformed or uses an unknown language tag |
| IR-E020 | error | 7 | Expression reads, or `set` assigns, an undeclared variable |
| IR-E021 | error | 7 | `restricted` variable used in an event-facing field: any `notify` field, a `correlation` key or an `idempotencyKey` |
| IR-E022 | error | 8 | `form`, `inputForm`, `quiz`, `rule`, `calendar`, `connector`, `process`, `document` or `subject` names a `refs` key that is not declared. `document` may instead name a `file` variable |
| IR-E023 | error | 8 | Reference not found, not published, or not usable by this author (environment) |
| IR-E024 | error | 9 | Connector not enabled for the workspace, or scopes not granted (environment) |
| IR-E025 | error | 10 | Named group, role, position or user does not exist (environment) |
| IR-E026 | error | 10 | SoD cannot be satisfied: every candidate is the excluded initiator and there is no fallback |
| IR-E027 | error | 11 | Business-time duration (`due`, `wait.timer`, SLA) with no calendar (`calendar`, or `refs.calendars.default`) |
| IR-E028 | error | 11 | Timer misconfigured: a `wait.timer` without exactly one of `at` or `after`; a `timeout` boundary without `after`; or a zero-length duration |
| IR-E029 | error | 11 | Absolute instant literal without a UTC offset |
| IR-E030 | error | §3.2 | Escalation invalid: `reassign` without `to`; `outcome` without a valid outcome; or any `outcome` escalation in a `controlled` process |
| IR-E031 | error | §4.1 | Boundary on a node kind that cannot have one |
| IR-E032 | error | §9 | SLA `scope` or `pauseWhile` names an unknown node |
| IR-E033 | error | 2 | Duplicate SLA id |
| IR-E034 | error | 2 | Duplicate branch name on a decision or split |
| IR-E100 | error | — | Schema violation (shape, unknown field, pattern), or the source is not JSON |
| IR-W001 | warning | 7 | `readers` on a variable that is not `restricted` has no effect |
| IR-W002 | warning | 10 | `excludeActorsOf` names a node that has no human actors |
| IR-I001 | info | 8–10 | Compiled without an environment, so refs, connectors and directory names were not checked |

**Gaps against §11.**
- *Check 1:* there is no IR field that declares a "perpetual wait", so a process with no reachable end is always IR-E010.
- *Check 7:* types are not checked. That needs the BPMS-081 type checker behind `ExpressionChecker`. Today's checks cover syntax, variable references and classification only.
- *Check 10:* "SoD satisfiable" is limited to the initiator-only case. A full check needs directory data at publish time.
- *Checks 8–10* run only when the caller supplies an `Environment`. The publish integration (BPMS-084) must always supply one.
- *§1.5:* only `schemaVersion: 1` exists, so there is no migration function yet.

**Fixture corrections.** Two BPMS-002 reference fixtures broke rules from §13 and §11:
- `hr.employee-onboarding` `policy_ack` had no `assignment` (§13.2);
- `qms.capa` used business-time `due` with no `refs.calendars` (check 11).

Both fixtures are fixed.

## 15. As built (BPMS-082)

**Crate.** `crates/runtime` (`procera-runtime`) is the native interpreter. It depends on `procera-ir`, `procera-kernel` (`Clock`, `IdGen`), serde, `time` and `uuid`. It does no I/O, reads no clock and uses no randomness.

**Contract.** The interpreter is a pure function:

```text
(Program, InstanceState, Stimulus) ──► (InstanceState', [Command])      or EventError (state unchanged)
```

- `Program` is the compiled normal-form IR indexed for execution (`Program::from_compiled`). It re-checks only what execution relies on: one start, edge endpoints, one edge per outcome, and a join for every split.
- A `Stimulus` is an `Event` with its recorded time `at`. The application stamps it once with the injected `Clock` (`Stimulus::stamp`); `Stimulus::start` also takes the instance id from the injected `IdGen`. `at` is `now` for every expression the stimulus triggers (§6). Replaying the recorded stimuli needs neither port and reproduces the state, the trace and the commands exactly (`Interpreter::replay`).
- Token, split and activation ids come from a counter in the state.
- Expressions go through the `Evaluator` port: `eval(expr, scope)` and `eval_rule(rule, ref, output, scope)`. The scope holds the variables, `instance`, `initiator` and `now`. BPMS-081 implements the port and owns the expression budget. `testkit::MiniEvaluator` is a small test stand-in, not a FEEL or CEL implementation.

**State model** (`InstanceState`, serde-serialisable, all collections ordered):

| Field | Meaning |
|---|---|
| `status` | `running`, `waiting` (derived: every live token waits), `completed`, `terminated` (end-terminate or cancel; docs/04 `cancelled`), `failed` |
| `tokens` | Live tokens only: `id`, `node`, `state` (`ready`, `waiting{kind, key, boundary_timers, timeout_timer}`, `incident{code, message, key}`), and `frames` (the enclosing split branches, innermost last) |
| `splits` | Open parallel splits: split node, join node, expected branches, arrived branches |
| `variables` | Process variables (plain values; envelope encryption of `restricted` values is the persistence layer's job, BPMS-083) |
| `entries` | Entries per node (loop guard) |
| `trace` | Ordered history: `instance_started`, `node_entered`, `node_left{outcome}`, `waiting{wait,key}`, `resumed{key}`, `boundary_triggered`, `forked`, `join_arrived`, `join_released`, `variables_set{names}`, `token_consumed`, `token_cancelled`, `incident_raised{code}`, `yielded`, `instance_completed`, `instance_terminated{reason}`, `instance_failed{outcome}`. Entries carry variable names, never values (§5) |
| `revision`, `next_id` | +1 per accepted stimulus; id counter |

**Advance loop.** The loop repeatedly takes the ready token with the lowest id and executes its node, until no token is ready or the instance is final:

| Node | Behaviour |
|---|---|
| `start` | Leaves on `next` |
| `end` | Consumes the token. `result: terminated` cancels every token and open activation, and the instance is `terminated` |
| `set` | Evaluates every `assign` against the variables as they were before the node, then leaves on `next` |
| `decision` | Takes the first branch whose `when` is `true`, in declared order, else `default`. A non-boolean result raises incident IR-R002. A rule-table decision takes the branch named by the rule output, else `default` |
| `parallel.split` | Consumes the token and opens a split. Spawns one token per branch, in declared order, each with a new frame |
| `parallel.join` | Records the token's branch in its split's arrived set and consumes the token. When every expected branch has arrived, it removes the split and releases one token on `next`. The join therefore fires exactly once |
| `human.*` | `CreateWorkItem` (assignment or signers, form, outcomes, due, escalations, quorum, signature, subject), then waits |
| `wait.timer` | `ScheduleTimer`: `at` (expression to an RFC 3339 instant), `after` (absolute `at + duration`), or `after` + `business` (left to the scheduler with the calendar). Then waits |
| `wait.signal` | `SubscribeSignal` with the evaluated correlation value, plus a timeout timer when `timeout` is set. Then waits |
| `service.call` | `CallConnector` with evaluated inputs and the evaluated `idempotencyKey`, then waits |
| `notify` | `Notify`, then leaves on `next` |
| `subprocess.call` | `StartSubprocess`, then waits |
| `case.open` | `OpenCase`. Waits when `waitForClose`, else leaves on `opened` |

On entering a waiting node, every `timeout` boundary arms a `ScheduleTimer`. An outcome with no edge fails the instance when the node says `unhandled: fail`, and raises incident IR-R003 otherwise. When no live token is left, the instance is `completed`.

**Events.**

| Event | Effect |
|---|---|
| `start` | Creates the instance (`Interpreter::start`) |
| `work_item_completed{key, outcome, variables}` | The outcome must be one of the node's outcomes and the variables must be declared. Disarms the boundary timers and leaves on the outcome |
| `timer_fired{key}` | A `wait.timer` leaves on `fired`. A signal timeout withdraws the subscription and leaves on `timeout`. A boundary timer runs its boundary: interrupting (the default) cancels the activation and moves the token; non-interrupting spawns a token at the target and keeps the wait |
| `signal_received{key, variables}` | Cancels the timeout timer and leaves on `received` |
| `service_completed{key, outcome, variables}` | `ok` or `error`. `error:<CODE>` goes to a matching `error:<CODE>` boundary, else to `error` |
| `subprocess_finished{key, outcome}` | `completed`, `failed` or `cancelled` |
| `case_closed{key}` | Leaves on `closed` |
| `cancel{reason}` | Terminates the instance |
| `continue` | Resumes after a budget yield |

A stimulus for an unknown or already-completed activation, a final instance, a wrong outcome or an undeclared variable returns a typed `EventError` and changes nothing. Duplicate completions are therefore no-ops (docs/04 §4.2).

**Commands** (effects as data, executed after commit): `create_work_item`, `cancel_work_item`, `schedule_timer` (purpose `wait`, `boundary` or `signal_timeout`; due `at` or `business`), `cancel_timer`, `subscribe_signal`, `cancel_subscription`, `call_connector`, `notify`, `start_subprocess`, `cancel_subprocess`, `open_case`, `raise_incident`, `emit_event` (`instance.started`, `instance.completed`, `instance.cancelled`, `instance.failed`; ids only), `complete_instance` (`completed`, `terminated` or `failed`), and `continue_advance`.

Every command has a `key` derived only from the instance id and its history:
- activations use `<instance>/<node>/<n>`;
- boundary timers add `/boundary/<i>`, and signal timeouts add `/timeout`;
- events use `<instance>/event/<topic>`, and completion uses `<instance>/complete`.

**Budgets** (`Budget`):

| Budget | Default | On exhaustion |
|---|---|---|
| `max_steps` (node executions per advance) | 200 (docs/04 §4.2) | The advance yields with `continue_advance`. Nothing is lost, and a later `continue` resumes |
| `max_node_entries` (entries per node when it declares no `maxIterations`) | 1,000 | Incident IR-R004 on the token. A node that declares `maxIterations` uses that value instead |

Expression budget overruns are incident IR-R010 (docs/04 §4.5). Runtime incident codes are IR-R001 (expression failed), IR-R002 (non-boolean condition), IR-R003 (no edge), IR-R004 (loop guard), IR-R005 (join outside its split), IR-R006 (bad value) and IR-R010 (A-23).

**Conformance fixtures.** These live in `conformance/runtime/<group>/<case>/`:
- `process.ir.json`: the authored IR, compiled by the runner;
- `bundle.json` (optional): rule stand-ins, expression overrides and a budget;
- `script.json`: ordered steps, which are `start`, `complete`, `fire_timer`, `fire_boundary`, `fire_timeout`, `signal`, `service`, `subprocess`, `case_closed`, `cancel`, `continue`, `advance_clock`, and `reject` (the step must be refused and leave the state unchanged);
- `expected.trace.jsonl`: the trace without `seq` and `at`;
- `expected.final.json`: the status, revision, tokens, open splits, variables and the command list.

The runner (`crates/runtime/tests/conformance.rs`) also checks replay equality. `PROCERA_BLESS=1` rewrites the goldens. There are 22 cases covering the docs/15 §4 groups sequence, decision, parallel, loop, human, timer, signal, service, boundary, subprocess, terminate, cancel and incident, plus a `budget` group. Location and file format differ from docs/15 §4 (A-23).

**Tests.**
- Unit tests cover every node kind, gateway, boundary and budget.
- The five reference processes run end to end with scripted events.
- Property tests cover:
  - determinism, replay equality, and equality after a JSON round trip of the state;
  - small budgets terminating with the same result as the unbounded run;
  - joins releasing once, after every branch arrives;
  - terminate leaving no tokens and refusing later events;
  - every opened activation being resumed, cancelled, fired or still waiting.

**Gaps against the spec.**
- Inclusive (OR) and event-based gateways are not in v1 (§1.4, §10), so the interpreter has none.
- Assignment resolution, quorum, delegation, escalations and the work-item lifecycle belong to BPMS-085. `CreateWorkItem` carries the node's fields, and an escalation `outcome` comes back as a `work_item_completed`.
- Business-calendar arithmetic, timer persistence and firing, and SLA clocks belong to BPMS-083.
- The incident lifecycle (retry, skip) and suspend/resume belong to BPMS-088.
- `cancel` boundaries are not triggered. Compensation is P2.
- Start-form validation and `required` variables belong to BPMS-101. Variable types are not checked at runtime.
- Persistence, the instance lock and the outbox belong to BPMS-083 and BPMS-084. The trace stays in the state until persistence moves it into `instance_events`.
- Child-instance variable mapping for `subprocess.call` is not specified.
