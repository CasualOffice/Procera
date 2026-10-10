# BPMS-002: the five reference processes, modelled in IR v1

Status: **Done** · Date: 2026-10-11 · Owner: Runtime lead · Feeds G1 (IR v1 freeze).

## What was done

The five reference processes required for Phase 0 ([07](../07-ROADMAP-AND-MILESTONES.md)) were modelled as IR v1 documents:

| Fixture | Exercises |
|---|---|
| [`fin.purchase-requisition`](../../conformance/reference/fin.purchase-requisition.json) | Rule-table decision (DoA), 3 approval levels, SoD (`excludeInitiator`, `excludeActorsOf`), send-back loop, escalation, SLA with pause |
| [`hr.employee-onboarding`](../../conformance/reference/hr.employee-onboarding.json) | Parallel split/join, `wait.signal` with timeout, `human.sign`, `human.acknowledge`, absolute `wait.timer`, restricted variable, terminate end inside a branch |
| [`it.access-request`](../../conformance/reference/it.access-request.json) | Expression decision, interrupting boundary timeout, `service.call` with idempotency key and error path (connector hook point), controlled criticality |
| [`grc.policy-acknowledgement`](../../conformance/reference/grc.policy-acknowledgement.json) | Event trigger, acknowledgement with quiz, reminder/notify escalations |
| [`qms.capa`](../../conformance/reference/qms.capa.json) | `set`, `case.open`, Part 11 approval, `subprocess.call` (pinned), relative timer, custom task outcomes, effectiveness loop |

Together they cover **all 16 v1 node kinds**. Each fixture is checked by:
- [`schemas/ir/v1.schema.json`](../../schemas/ir/v1.schema.json): the shape, as JSON Schema 2020-12;
- the structural checker in [`conformance/`](../../conformance/src/lib.rs): docs/11 §11 checks 1–6, 11 and 12, plus integrity checks, with stable codes `IR-E001…E016`.

Negative tests prove each check fires (`cargo test -p procera-conformance`, 13 tests).

## Findings: gaps in the IR spec found by modelling real processes

These were applied to [docs/11 §13](../11-WORKFLOW-IR-SPEC.md#13-v1-clarifications-from-reference-modelling-bpms-002).

| # | Finding | Resolution |
|---|---|---|
| F1 | A `decision` "on a rule table" had no fields to name the table or its output | Added `rule` (ref key) + `output` (column). Branch names = output values. `default` edge still mandatory. |
| F2 | `human.acknowledge` had no way to say *who* acknowledges | Generic `assignment` is allowed on all `human.*` nodes |
| F3 | `notify` recipients were unspecified | `to` uses the assignment shape. `template` names the localised message |
| F4 | `wait.signal` / `case.open` outcomes depended on config but the spec listed both | `timeout` exists only when `timeout` is set. `case.open` yields `opened`, or `closed` when `waitForClose: true` |
| F5 | A *terminate* end inside a parallel branch (e.g. the offer is declined) conflicted with the "branches must reach the join" rule | Allowed: `end{result: terminated}` inside a branch cancels sibling tokens. Plain `completed` ends inside branches remain an error (IR-E008) |
| F6 | Escalations needed a plain `notify` action (tell the manager without reassigning) | Added `notify` to escalation actions |
| F7 | Approvals need the `strong` signature level (password/passkey re-auth without full Part 11) | `signature` levels unified across `human.approval` and `human.sign`: `none, simple, strong, part11` |
| F8 | Refs lacked documents, processes and policies | Added `refs.documents`, `refs.processes`, `refs.policy` |
| F9 | Durations: months and years have no fixed length | IR durations allow W, D, H, M (minutes) and S only. Month/year schedules use calendar expressions or `schedule` triggers |
| F10 | Expressions reference context (`initiator`, `instance.id`, `employee.manager`) that §6 never defined | Defined the expression context: `initiator`, `instance` (`id`, `display_id`, `started_at`), `now` (event time), variables by name, and principal navigation (`.manager`, `.department`, `.id`) |
| F11 | Every edge needs a stable `id` for diffs and diagnostics | Edge `id` is required |
| F12 | Custom outcomes on `human.task` (e.g. `effective` / `not_effective`) | The `outcomes` list replaces the default `submitted` |

## Not covered here (later items)

- Expression type-checking (check 7): SP-08 / BPMS-081.
- Reference resolution and connector scopes (checks 8–10): BPMS-080 / BPMS-084.
- Execution traces for the conformance suite (docs/15 §4): BPMS-089. These fixtures become its inputs.
