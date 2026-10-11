//! The five BPMS-002 reference processes run end to end with scripted events (BPMS-082), with
//! every expression evaluated by the production CEL engine (BPMS-081).
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::*;
use procera_runtime::{CelEvaluator, Command, InstanceStatus, TimerDue, WaitKind, WorkKind};
use serde_json::json;

fn harness(name: &str, eval: CelEvaluator) -> Harness {
    Harness::with(program(&reference(name)), eval)
}

/// The purchase requisition's `approver` rule table is a stand-in until BPMS-087: the decision
/// output is the `approval_level` variable.
fn requisition() -> Harness {
    let p = program(&reference("fin.purchase-requisition"));
    let eval = CelEvaluator::default().with_rule("approver", "approval_level", p.schema()).unwrap();
    Harness::with(p, eval)
}

fn notified(h: &Harness) -> Vec<String> {
    h.commands
        .iter()
        .filter_map(|c| match c {
            Command::Notify { template, .. } => template.clone(),
            _ => None,
        })
        .collect()
}

#[test]
fn purchase_requisition_revise_then_finance_approves() {
    let mut h = requisition();
    h.start(vars(&[
        ("amount", json!({ "amount": "1200.00", "currency": "INR" })),
        ("approval_level", json!("manager")),
    ]));
    assert_eq!(h.waiting_at(), ["manager_approval"]);
    h.complete("manager_approval", "changes_requested");
    assert_eq!(h.waiting_at(), ["revise"]);
    h.try_complete("revise", "submitted", vars(&[("approval_level", json!("finance"))])).unwrap();
    assert_eq!(h.waiting_at(), ["finance_approval"]);
    h.complete("finance_approval", "approved");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(notified(&h), ["purchase-approved"]);
    assert_eq!(
        h.path(),
        [
            "start",
            "route",
            "manager_approval",
            "revise",
            "route",
            "finance_approval",
            "notify_approved",
            "end_approved"
        ]
    );
    h.assert_replays();
}

#[test]
fn purchase_requisition_cfo_rejects() {
    let mut h = requisition();
    h.start(vars(&[("approval_level", json!("cfo"))]));
    h.complete("cfo_approval", "rejected");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(notified(&h), ["purchase-rejected"]);
}

#[test]
fn policy_acknowledgement_on_time_and_overdue() {
    let mut h = harness("grc.policy-acknowledgement", CelEvaluator::default());
    let cmds = h.start(vars(&[("employee", json!({ "id": "u-9" }))]));
    assert!(cmds.iter().any(|c| matches!(c, Command::CreateWorkItem { kind: WorkKind::Acknowledge, .. })));
    h.complete("acknowledge", "acknowledged");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert!(h.path().ends_with(&["end_ack".to_owned()]));

    // An `outcome`/deadline escalation completes the work item as `expired` (BPMS-085).
    let mut h = harness("grc.policy-acknowledgement", CelEvaluator::default());
    h.start(vars(&[]));
    h.complete("acknowledge", "expired");
    assert!(h.path().ends_with(&["notify_manager".to_owned(), "end_overdue".to_owned()]));
    h.assert_replays();
}

/// `datetime(start_date) + duration("PT9H")`: midnight at the injected offset, plus nine hours.
fn onboarding_eval() -> CelEvaluator {
    CelEvaluator::default().with_tz(time::macros::offset!(+5:30))
}

#[test]
fn employee_onboarding_runs_three_branches_to_the_welcome() {
    let mut h = harness("hr.employee-onboarding", onboarding_eval());
    h.start(vars(&[("employee_id", json!("E-100")), ("start_date", json!("2026-11-02"))]));
    h.complete("manager_confirm", "approved");
    assert_eq!(h.waiting_at(), ["facilities", "it_setup", "offer_sign"]);
    h.complete("it_setup", "submitted");
    assert_eq!(h.wait_kind("laptop_delivered"), WaitKind::Signal);
    assert!(h.last.iter().any(
        |c| matches!(c, Command::SubscribeSignal { correlation, .. } if correlation == &json!("E-100"))
    ));
    h.fire_timeout("laptop_delivered");
    h.complete("it_followup", "submitted");
    h.complete("facilities", "submitted");
    h.complete("offer_sign", "signed");
    assert_eq!(h.waiting_at(), ["policy_ack"]);
    h.complete("policy_ack", "acknowledged");
    // The join released once: the instance now waits for the start-date timer.
    assert_eq!(h.waiting_at(), ["wait_start"]);
    let at = time::macros::datetime!(2026-11-02 09:00 +05:30);
    assert!(
        h.last
            .iter()
            .any(|c| matches!(c, Command::ScheduleTimer { due: TimerDue::At { at: a }, .. } if *a == at))
    );
    h.fire("wait_start");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(notified(&h), ["welcome"]);
    h.assert_replays();
}

#[test]
fn employee_onboarding_declined_offer_terminates_every_branch() {
    let mut h = harness("hr.employee-onboarding", onboarding_eval());
    h.start(vars(&[("employee_id", json!("E-101"))]));
    h.complete("manager_confirm", "approved");
    h.complete("it_setup", "submitted");
    h.complete("offer_sign", "declined");
    let s = h.state();
    assert_eq!(s.status, InstanceStatus::Terminated);
    assert!(s.tokens.is_empty());
    // The facilities work item and the laptop subscription (with its timeout) are withdrawn.
    assert_eq!(commands_named(&h.last, "cancel_work_item").len(), 1);
    assert_eq!(commands_named(&h.last, "cancel_subscription").len(), 1);
    assert_eq!(commands_named(&h.last, "cancel_timer").len(), 1);
    h.assert_replays();
}

/// A complete access request: every required variable the expressions read is set.
fn request(sensitivity: &str) -> std::collections::BTreeMap<String, serde_json::Value> {
    vars(&[
        ("sensitivity", json!(sensitivity)),
        ("system", json!("erp")),
        ("access_role", json!("approver")),
        ("beneficiary", json!({ "id": "u-5", "manager": "u-6" })),
    ])
}

#[test]
fn access_request_high_sensitivity_is_provisioned() {
    let mut h = harness("it.access-request", CelEvaluator::default());
    h.start(request("high"));
    h.complete("owner_approval", "approved");
    h.complete("security_approval", "approved");
    let Some(Command::CallConnector { connector_key, input, .. }) =
        h.last.iter().find(|c| matches!(c, Command::CallConnector { .. }))
    else {
        panic!("{:#?}", h.last)
    };
    assert_eq!(connector_key, &format!("{}:grant", h.state().instance_id));
    assert_eq!(input["user"], json!("u-5"));
    h.service("provision", "ok");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert!(h.path().ends_with(&["end_granted".to_owned()]));
    h.assert_replays();
}

#[test]
fn access_request_manager_timeout_boundary_expires_it() {
    let mut h = harness("it.access-request", CelEvaluator::default());
    h.start(request("standard"));
    assert_eq!(h.waiting_at(), ["manager_approval"]);
    h.fire_boundary("manager_approval", 0);
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert!(h.path().ends_with(&["notify_expired".to_owned(), "end_expired".to_owned()]));
    assert_eq!(commands_named(&h.last, "cancel_work_item").len(), 1);
}

#[test]
fn access_request_connector_error_falls_back_to_manual() {
    let mut h = harness("it.access-request", CelEvaluator::default());
    h.start(request("standard"));
    h.complete("manager_approval", "approved");
    h.service("provision", "error");
    h.complete("manual_provision", "submitted");
    assert_eq!(h.state().status, InstanceStatus::Completed);
}

fn capa_eval() -> CelEvaluator {
    CelEvaluator::default()
}

#[test]
fn capa_loops_through_effectiveness_and_closes() {
    let mut h = harness("qms.capa", capa_eval());
    let cmds = h.start(vars(&[("severity", json!("critical")), ("occurrence", json!(3))]));
    assert_eq!(h.state().variables["risk_score"], json!(15));
    assert_eq!(commands_named(&cmds, "open_case").len(), 1);
    h.complete("rca", "submitted");
    h.try_complete("qa_approval", "approved", vars(&[("needs_doc_change", json!(true))])).unwrap();
    assert_eq!(h.wait_kind("change_control"), WaitKind::Subprocess);
    h.subprocess("change_control", "completed");
    h.now += time::Duration::days(30);
    h.fire("wait_effectiveness");
    h.complete("effectiveness", "not_effective");
    h.complete("rca", "submitted");
    h.try_complete("qa_approval", "approved", vars(&[("needs_doc_change", json!(false))])).unwrap();
    h.fire("wait_effectiveness");
    h.complete("effectiveness", "effective");
    assert_eq!(h.state().status, InstanceStatus::Completed);
    assert_eq!(h.state().entries["rca"], 2);
    h.assert_replays();
}

#[test]
fn capa_failed_change_control_terminates() {
    let mut h = harness("qms.capa", capa_eval());
    h.start(vars(&[("severity", json!("minor")), ("occurrence", json!(1))]));
    h.complete("rca", "submitted");
    h.try_complete("qa_approval", "approved", vars(&[("needs_doc_change", json!(true))])).unwrap();
    h.subprocess("change_control", "failed");
    assert_eq!(h.state().status, InstanceStatus::Terminated);
}
