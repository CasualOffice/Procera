//! Semantic diff and canonical form on hand-made changes to a reference process.
// Helpers outside #[test] fns may unwrap too (clippy.toml allows it inside tests).
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use procera_ir::{ChangeKind, CompileContext, DiffError, Section, compile, diff, normalize};
use serde_json::json;

#[test]
fn diff_reports_changes_by_stable_id() {
    let base = reference("fin.purchase-requisition");
    let mut next = base.clone();
    next["process"]["version"] = json!(2);
    next["nodes"][5]["maxIterations"] = json!(3);
    next["variables"].as_array_mut().unwrap().retain(|v| v["name"] != "approval_level");
    next["edges"][14]["to"] = json!("manager_approval"); // e15: revise → manager_approval
    next["refs"]["forms"]["request"] = json!("form:fin.purchase-request@2");

    let old = offline(&base).compiled.unwrap().ir;
    let out = compile(&next, &CompileContext { previous: Some(&old), ..Default::default() });
    let d = out.diff.unwrap();
    let summary: Vec<(Section, &str, ChangeKind, Vec<String>)> =
        d.changes.iter().map(|c| (c.section, c.id.as_str(), c.kind, c.fields.clone())).collect();
    assert_eq!(
        summary,
        vec![
            (Section::Process, "process", ChangeKind::Changed, vec!["version".to_owned()]),
            (Section::Variable, "approval_level", ChangeKind::Removed, vec![]),
            (Section::Node, "revise", ChangeKind::Changed, vec!["maxIterations".to_owned()]),
            (Section::Edge, "e15", ChangeKind::Changed, vec!["to".to_owned()]),
            (Section::Ref, "forms.request", ChangeKind::Changed, vec![]),
        ]
    );
    let new = out.compiled.unwrap().ir;
    assert_eq!(d.apply(&old).unwrap(), new);
    assert_eq!(d.section(Section::Node).count(), 1);
}

#[test]
fn apply_refuses_a_base_that_does_not_match() {
    let a = offline(&reference("fin.purchase-requisition")).compiled.unwrap().ir;
    let mut b = a.clone();
    b.nodes.retain(|n| n.id != "end_rejected");
    let other = offline(&reference("qms.capa")).compiled.unwrap().ir;
    let err = diff(&a, &b).apply(&other).unwrap_err();
    assert!(matches!(err, DiffError::Conflict { section: Section::Node, .. }), "{err}");
}

#[test]
fn normal_form_sorts_entities_and_fills_edge_outcomes() {
    let out = offline(&reference("it.access-request")).compiled.unwrap();
    let ids: Vec<&str> = out.ir.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert!(out.ir.edges.iter().all(|e| !e.on.is_empty()));
    let text = String::from_utf8(out.canonical.clone()).unwrap();
    assert!(text.contains(r#""on":"next""#), "missing `on` defaults to next");
    assert_eq!(out.ir.meta.compiler_version.as_deref(), Some(procera_ir::COMPILER_VERSION));
    assert_eq!(normalize(&out.ir), out.ir, "normalize is idempotent");
}
