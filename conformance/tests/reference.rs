//! BPMS-002: the five reference processes are valid IR v1, and every structural check fires
//! on a deliberately broken document (negative tests keep the checker honest).

use procera_conformance::{Diagnostic, validate, validate_schema};
use serde_json::{Value, json};
use std::path::PathBuf;

fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reference")
}

fn load(name: &str) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(reference_dir().join(format!("{name}.json")))?)?)
}

fn codes(d: &[Diagnostic]) -> Vec<&'static str> {
    d.iter().map(|x| x.code).collect()
}

#[test]
fn all_reference_processes_are_valid() -> anyhow::Result<()> {
    let mut count = 0;
    for entry in std::fs::read_dir(reference_dir())? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        let diags = validate(&doc);
        assert!(diags.is_empty(), "{}: {diags:#?}", path.display());
        count += 1;
    }
    assert_eq!(count, 5, "expected the five reference processes from docs/07 M0");
    Ok(())
}

#[test]
fn reference_set_covers_every_v1_node_kind() -> anyhow::Result<()> {
    let mut kinds = std::collections::BTreeSet::new();
    for name in [
        "fin.purchase-requisition",
        "hr.employee-onboarding",
        "it.access-request",
        "grc.policy-acknowledgement",
        "qms.capa",
    ] {
        for n in load(name)?["nodes"].as_array().into_iter().flatten() {
            kinds.insert(n["kind"].as_str().unwrap_or_default().to_owned());
        }
    }
    assert_eq!(kinds.len(), 16, "{kinds:?}");
    Ok(())
}

/// Applies `mutate` to a copy of the purchase requisition and asserts `code` is reported.
fn expect_code(code: &str, mutate: impl FnOnce(&mut Value)) -> anyhow::Result<()> {
    let mut doc = load("fin.purchase-requisition")?;
    mutate(&mut doc);
    let diags = validate(&doc);
    assert!(codes(&diags).contains(&code), "expected {code}, got {diags:#?}");
    Ok(())
}

fn edges(doc: &mut Value) -> &mut Vec<Value> {
    doc["edges"].as_array_mut().unwrap_or_else(|| unreachable!("fixture has edges"))
}

fn nodes(doc: &mut Value) -> &mut Vec<Value> {
    doc["nodes"].as_array_mut().unwrap_or_else(|| unreachable!("fixture has nodes"))
}

#[test]
fn schema_rejects_unknown_fields_and_scripts() -> anyhow::Result<()> {
    let mut doc = load("fin.purchase-requisition")?;
    nodes(&mut doc)[0]["script"] = json!("rm -rf /");
    assert!(!validate_schema(&doc).is_empty(), "scripts must be rejected (SEC-09)");
    let mut doc = load("fin.purchase-requisition")?;
    nodes(&mut doc)[1]["kind"] = json!("bpmn.inclusiveGateway");
    assert!(!validate_schema(&doc).is_empty(), "unsupported kinds must be rejected");
    Ok(())
}

#[test]
fn second_start_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E001", |d| nodes(d).push(json!({"id":"start2","kind":"start"})))
}

#[test]
fn duplicate_node_id_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E002", |d| nodes(d).push(json!({"id":"revise","kind":"set","assign":{}})))
}

#[test]
fn dangling_edge_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E004", |d| edges(d).push(json!({"id":"e99","from":"start","to":"ghost"})))
}

#[test]
fn unhandled_outcome_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E005", |d| {
        edges(d).retain(|e| !(e["from"] == "manager_approval" && e["on"] == "changes_requested"))
    })
}

#[test]
fn undeclared_outcome_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E006", |d| {
        edges(d).push(json!({"id":"e99","from":"manager_approval","to":"end_approved","on":"maybe"}))
    })
}

#[test]
fn decision_without_default_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E007", |d| edges(d).retain(|e| !(e["from"] == "route" && e["on"] == "default")))
}

#[test]
fn unreachable_node_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E009", |d| nodes(d).push(json!({"id":"orphan","kind":"end"})))
}

#[test]
fn unbounded_loop_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E011", |d| {
        nodes(d).push(json!({"id":"spin_a","kind":"set","assign":{}}));
        nodes(d).push(json!({"id":"spin_b","kind":"set","assign":{}}));
        edges(d).retain(|e| !(e["from"] == "notify_approved"));
        edges(d).push(json!({"id":"e90","from":"notify_approved","to":"spin_a"}));
        edges(d).push(json!({"id":"e91","from":"spin_a","to":"spin_b"}));
        edges(d).push(json!({"id":"e92","from":"spin_b","to":"spin_a"}));
    })
}

#[test]
fn unknown_sod_reference_is_rejected() -> anyhow::Result<()> {
    expect_code("IR-E014", |d| nodes(d)[3]["assignment"]["excludeActorsOf"] = json!(["nobody"]))
}

#[test]
fn split_branch_escaping_join_is_rejected() -> anyhow::Result<()> {
    let mut doc = load("hr.employee-onboarding")?;
    edges(&mut doc).retain(|e| !(e["from"] == "facilities"));
    edges(&mut doc).push(json!({"id":"e90","from":"facilities","to":"end_done","on":"submitted"}));
    let diags = validate(&doc);
    assert!(codes(&diags).contains(&"IR-E008"), "{diags:#?}");
    Ok(())
}
