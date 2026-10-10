# SP-08: Expression language, FEEL (dsntk) vs CEL (cel-rust)

Status: **Done. Recommendation: CEL with Procera extensions.** Final decision at G1 (BPMS-009). · Date: 2026-10-11 · Tracker: BPMS-005 · Decides: [ADR-0010](../adr/ADR-0010-expression-language.md)

Code: [`spikes/sp08-expr`](../../spikes/sp08-expr/src/main.rs). Run it with `cd spikes && cargo run --release -p sp08-expr`. Measured on an Apple Silicon laptop, release build.

## Question

Which expression engine should IR v1 use for conditions, assignments, rule-table cells, SLA expressions and form logic ([docs/11 §6](../11-WORKFLOW-IR-SPEC.md))? The requirements are:
- typed against IR variables;
- total, with an evaluation budget;
- pure;
- null-safe;
- running in the **browser (WASM)** as well, for form logic and test runs.

## Candidates

| | dsntk FEEL | cel-rust (`cel` 0.15) |
|---|---|---|
| Licence | MIT OR Apache-2.0 | MIT |
| Maturity | v0.3, small project | v0.15, used in production by several projects; CEL spec by Google |
| Standard alignment | OMG FEEL (DMN) | CNCF/Google CEL (Kubernetes, Envoy, Firebase rules) |

## Results: the expressions our reference processes use

| Case | What | FEEL | CEL |
|---|---|---|---|
| C01 | string equality | ✅ true | ✅ true |
| C02 | boolean variable | ✅ true | ✅ true |
| C03 | **decimal exactness** `0.1 + 0.2 = 0.3` | ✅ **true** (decimal numbers) | ❌ **false** (IEEE double) |
| C04/C05 | money threshold 50000.01 > 50000 | ✅ | ✅ (through double, see C03 risk) |
| C06 | principal navigation `initiator.manager.id` | ✅ | ✅ |
| C07 | string concatenation (idempotency key) | ✅ | ✅ |
| C08 | conditional × integer from JSON | ✅ 15 | ❌ error: `Int(5) * UInt(3)`. JSON integers arrive as UInt. **Fix: marshal values using the IR variable type.** |
| C09 | list total (line items) | ✅ `sum(for …)` = 40.98 | ⚠️ no `sum`. `map` returns a list of doubles. **Fix: built-in `sum()` extension.** |
| C10 | missing variable | ⚠️ `null` (three-valued; can leak into branch selection) | ✅ explicit error "undeclared reference" |
| C11 | date + duration | ✅ | ✅ (timestamp + duration) |
| C12 | type error `string > 5` | ⚠️ `null` (silent) | ✅ explicit "no matching overload" |

## Results: safety, portability, performance

| Property | FEEL (dsntk) | CEL (cel-rust) |
|---|---|---|
| **Runaway evaluation** | ❌ `count(for i in 1..N return i)` scales **quadratically**: 1 000 → 30 ms, 10 000 → 2.2 s, 50 000 → **73.7 s**. There is no evaluation budget API. A hostile or careless expression can stall a worker. | ✅ Non-Turing-complete. There are no range constructors, so iteration is bounded by the input data, which IR limits (`maxItems`, 64 KB/value). |
| **WASM (browser)** | ❌ **does not build** for `wasm32-unknown-unknown`. Its decimal type depends on Intel's C DFP library (`dfp-number-sys`: "sys/signal.h not found"). | ✅ builds cleanly |
| Evaluate (parse once) | 324 ns/op | 177 ns/op |
| Parse | 11.1 µs | 10.8 µs |
| Static type-checker in Rust | none | none in cel-rust (cel-go has one). We type-check against IR variable declarations ourselves. |
| Error model | `null` with a reason string | typed errors |

## Recommendation

**Adopt CEL as the IR v1 expression language (`expr:cel: …`), with these Procera extensions and constraints:**

1. **Exact decimals for money.**
   - Add a `decimal` type and `money` helpers through the custom-function API, backed by `rust_decimal` (MIT/Apache) natively and a JS-safe equivalent in WASM. Examples: `decimal("50000.01")`, `amount.gt(decimal("50000"))`, `sum()` over decimals.
   - Variables typed `decimal`/`money` in the IR are marshalled as decimals, never as doubles.
   - The compiler **rejects** float arithmetic on `money`/`decimal` variables (fixes C03).
2. **Typed marshalling.** Values enter the CEL context according to the IR variable type (`integer` → Int, `decimal` → Decimal, `date` → Timestamp at 00:00 in the calendar's timezone). Untyped JSON never passes straight through (fixes C08).
3. **Procera standard library:**
   - aggregates: `sum`, `min`, `max`, `avg` over lists;
   - business time: `business_days_between(a, b, calendar)`;
   - principal helpers.

   All of them are pure and bounded (fixes C09).
4. **Our own static type-checker** over the CEL AST, using IR variable declarations and the stdlib signatures. It provides docs/11 §11 check 7 at publish time. Missing variables and type errors become **compile errors**, not runtime nulls.
5. **Budgets anyway** (defence in depth):
   - expression source ≤ 4 KB;
   - AST depth ≤ 32;
   - list inputs bounded by IR `maxItems`;
   - each evaluation runs with a wall-clock guard (10 ms default). Exceeding it raises incident `IR-R010`.
6. **DMN / FEEL interop (P2).** DMN import translates the FEEL subset that maps cleanly (comparisons, ranges as unary tests, arithmetic, if-then-else) into CEL rule-table cells. Anything else is reported in the import compatibility report. dsntk can be used **offline in the importer** (native only) to parse FEEL, but never at runtime.

### Why not FEEL, despite DMN alignment?

Each of the following on its own would block FEEL:
- The **quadratic iteration with no budget** is a denial-of-service risk inside the workflow runtime.
- The engine **can't run in the browser**, which the form builder and test runs need.
- Its silent-`null` semantics conflict with docs/11 §6 ("null must not leak into branch selection").

FEEL's only real advantage, exact decimals, is fixable in CEL with an extension. Its other advantage, DMN alignment, is a P2 import concern.

## Consequences

- ADR-0010 → **Recommended: CEL** (accepted at G1 by BPMS-009).
- The IR fixtures use `expr:feel:` placeholders today. They will be rewritten to `expr:cel:` when G1 accepts. The conformance schema already allows both tags.
- New tracker work for R2 (BPMS-081 expression engine): decimal extension, typed marshalling, stdlib, type-checker, budgets, WASM build of the evaluator for the web.
- SP-08 doesn't cover the type-checker design; it's a BPMS-081 design note.
