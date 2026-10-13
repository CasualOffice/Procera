//! Test runs (0.3, BPMS-100; docs/05 §3.5 "Test run"): what a draft does for these answers, worked
//! out in memory by the runtime's pure interpreter. Nothing is stored and nobody is told. Each step
//! the request reaches takes the outcome the owner chose for it (a list, consumed in order, so
//! "send back, then approve" works) or its natural yes (`approved`, `submitted`, `done`,
//! `acknowledged`). Decisions follow the answers. The run stops when the request ends, gets stuck,
//! or after 100 steps.

use std::collections::{BTreeMap, VecDeque};

use procera_engine::Executable;
use procera_ir::model::{Assignment, CandidateType, IrDocument};
use procera_ir::{CompileContext, compile};
use procera_runtime::{Command, Event, InstanceStatus, Interpreter, Program, Stimulus, TraceEvent};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ir_of;

/// One step the request reached.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SimStep {
    pub node: String,
    pub name: String,
    /// `approval`, `task`, `acknowledge` or `sign`.
    pub kind: String,
    /// Who it would go to, in words ("the requester's manager", "the Finance group").
    pub who: String,
    pub outcome: String,
    /// The outcomes the step offers.
    pub outcomes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Simulation {
    pub steps: Vec<SimStep>,
    /// `approved`, `rejected`, `completed`, `stuck` or `too_long`.
    pub result: String,
    /// The end step's name, when it ended.
    pub ended_at: Option<String>,
    /// Why it stopped early, when it did.
    pub note: Option<String>,
}

fn name_of(ir: &IrDocument, node: &str) -> String {
    ir.nodes
        .iter()
        .find(|n| n.id == node)
        .and_then(|n| n.name.as_ref())
        .and_then(|m| m.get("en").or_else(|| m.values().next()).cloned())
        .unwrap_or_else(|| node.to_owned())
}

/// "the requester's manager", "the Finance group", … for an assignment.
fn who(a: Option<&Assignment>) -> String {
    let Some(a) = a else { return "nobody yet".into() };
    let one = |ty: CandidateType, r: &str| match (ty, r.trim()) {
        (_, "expr:cel: initiator") => "the requester".to_owned(),
        (_, "expr:cel: initiator.manager") => "the requester's manager".to_owned(),
        (_, "expr:cel: initiator.manager.manager") => "the requester's manager's manager".to_owned(),
        (_, r) if r.starts_with("expr:") => "someone chosen from the answers".to_owned(),
        (CandidateType::Group, r) => format!("the {r} group"),
        (CandidateType::Role, r) => format!("people with the {r} role"),
        (CandidateType::Position, r) => format!("the {r}"),
        (CandidateType::User, r) => r.to_owned(),
        (CandidateType::DirectoryRule, r) => format!("people matching {r}"),
    };
    let names: Vec<String> = a.candidates.iter().map(|c| one(c.ty, &c.reference.0)).collect();
    if names.is_empty() { "nobody yet".into() } else { names.join(" or ") }
}

const YES: [&str; 4] = ["approved", "submitted", "done", "acknowledged"];

/// Runs `content` for `input` with `choices` (step id → outcomes in order).
pub fn simulate(
    content: &Value,
    input: BTreeMap<String, Value>,
    initiator: Value,
    choices: &BTreeMap<String, Vec<String>>,
    now: OffsetDateTime,
) -> Result<Simulation, String> {
    let out = compile(ir_of(content), &CompileContext::default());
    let compiled = out.compiled.ok_or("this process doesn't compile yet: validate it first")?;
    let exe = Executable::new(Program::from_compiled(&compiled).map_err(|e| e.to_string())?);
    let ir = exe.program.ir().clone();
    let interp = Interpreter::new(&exe.program, &exe.evaluator);
    let start =
        Stimulus { at: now, event: Event::Start { instance_id: Uuid::nil(), initiator, variables: input } };
    let mut step =
        interp.start(&start).map_err(|e| format!("the request can't start with these answers: {e}"))?;
    let mut pending: VecDeque<(String, String, String, Option<Assignment>, Vec<String>)> = VecDeque::new();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut steps = Vec::new();
    loop {
        for c in &step.commands {
            if let Command::CreateWorkItem { key, node, kind, assignment, outcomes, .. } = c {
                let kind = serde_json::to_value(kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                pending.push_back((key.clone(), node.clone(), kind, assignment.clone(), outcomes.clone()));
            }
        }
        if matches!(
            step.state.status,
            InstanceStatus::Completed | InstanceStatus::Terminated | InstanceStatus::Failed
        ) {
            break;
        }
        let Some((key, node, kind, assignment, outcomes)) = pending.pop_front() else {
            return Ok(Simulation {
                steps,
                result: "stuck".into(),
                ended_at: None,
                note: Some(
                    "The request stops here: no step is waiting for anyone, and it hasn't ended.".into(),
                ),
            });
        };
        if steps.len() >= 100 {
            return Ok(Simulation {
                steps,
                result: "too_long".into(),
                ended_at: None,
                note: Some("Stopped after 100 steps: check for a loop.".into()),
            });
        }
        let n = used.entry(node.clone()).or_insert(0);
        let picked = choices.get(&node).and_then(|c| c.get(*n)).cloned();
        *n += 1;
        let outcome = match picked {
            Some(o) if outcomes.contains(&o) => o,
            Some(o) => return Err(format!("“{}” doesn't offer “{o}”", name_of(&ir, &node))),
            None => YES
                .iter()
                .find(|y| outcomes.iter().any(|o| o == *y))
                .map(|y| (*y).to_owned())
                .or_else(|| outcomes.first().cloned())
                .unwrap_or_default(),
        };
        steps.push(SimStep {
            name: name_of(&ir, &node),
            node,
            kind,
            who: who(assignment.as_ref()),
            outcome: outcome.clone(),
            outcomes,
        });
        let ev = Stimulus {
            at: now,
            event: Event::WorkItemCompleted { key, outcome, variables: BTreeMap::new() },
        };
        step = interp.handle(&step.state, &ev).map_err(|e| format!("the request couldn't go on: {e}"))?;
    }
    let end = step
        .state
        .trace
        .iter()
        .rev()
        .filter(|t| matches!(t.event, TraceEvent::NodeEntered))
        .find_map(|t| {
            t.node
                .as_deref()
                .filter(|n| ir.nodes.iter().any(|x| x.id == *n && x.kind == procera_ir::model::NodeKind::End))
        })
        .map(str::to_owned);
    let result = match (step.state.status, end.as_deref()) {
        (InstanceStatus::Completed, Some(e)) if e.contains("reject") => "rejected",
        (InstanceStatus::Completed, _) => "approved",
        (InstanceStatus::Terminated, _) => "cancelled",
        _ => "stuck",
    };
    Ok(Simulation { steps, result: result.into(), ended_at: end.map(|e| name_of(&ir, &e)), note: None })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;

    fn doc(path: &str) -> Value {
        serde_json::from_str(path).unwrap()
    }
    const LEAVE: &str = include_str!("../../requests/builtin/hr.leave-request.json");
    const EXPENSE: &str = include_str!("../../requests/builtin/hr.expense-claim.json");

    fn leave() -> BTreeMap<String, Value> {
        [
            ("leave_type", json!("annual")),
            ("start_date", json!("2026-12-21")),
            ("end_date", json!("2026-12-23")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
    }
    fn run(d: &Value, input: BTreeMap<String, Value>, choices: &[(&str, &[&str])]) -> Simulation {
        let c = choices
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
            .collect();
        simulate(d, input, json!({ "id": "u1" }), &c, OffsetDateTime::UNIX_EPOCH).unwrap()
    }
    fn path(s: &Simulation) -> Vec<(String, String)> {
        s.steps.iter().map(|x| (x.name.clone(), x.outcome.clone())).collect()
    }

    #[test]
    fn a_leave_request_is_approved_by_default_and_who_reads_in_words() {
        let s = run(&doc(LEAVE), leave(), &[]);
        assert_eq!(path(&s), [("Manager approval".to_owned(), "approved".to_owned())]);
        assert_eq!(s.steps[0].who, "the requester's manager");
        assert_eq!((s.result.as_str(), s.ended_at.as_deref()), ("approved", Some("Approved")));
    }

    #[test]
    fn send_back_then_approve_loops_through_the_requester() {
        let s = run(&doc(LEAVE), leave(), &[("manager_approval", &["changes_requested", "approved"])]);
        let p = path(&s);
        assert_eq!(p.len(), 3, "{p:?}");
        assert_eq!(p[0].1, "changes_requested");
        assert_eq!(s.steps[1].who, "the requester");
        assert_eq!((p[2].1.as_str(), s.result.as_str()), ("approved", "approved"));
        let r = run(&doc(LEAVE), leave(), &[("manager_approval", &["rejected"])]);
        assert_eq!(r.result, "rejected");
    }

    #[test]
    fn decisions_follow_the_answers() {
        let expense = |amount: &str| -> BTreeMap<String, Value> {
            [
                ("amount", json!({ "amount": amount, "currency": "EUR" })),
                ("category", json!("travel")),
                ("description", json!("Client visit")),
                ("expense_date", json!("2026-10-01")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect()
        };
        let small = run(&doc(EXPENSE), expense("120"), &[]);
        assert_eq!(small.steps.len(), 1);
        let big = run(&doc(EXPENSE), expense("900"), &[]);
        assert_eq!(
            big.steps.iter().map(|s| s.node.as_str()).collect::<Vec<_>>(),
            ["manager_approval", "finance_approval"]
        );
        assert!(big.steps[1].who.contains("Finance"), "{}", big.steps[1].who);
    }

    #[test]
    fn an_outcome_the_step_does_not_offer_is_refused() {
        let c: BTreeMap<String, Vec<String>> =
            [("manager_approval".to_owned(), vec!["maybe".to_owned()])].into_iter().collect();
        assert!(
            simulate(&doc(LEAVE), leave(), json!({}), &c, OffsetDateTime::UNIX_EPOCH)
                .unwrap_err()
                .contains("doesn't offer")
        );
    }
}
