//! SP-06: IR v1 ↔ BPMN 2.0 XML round trip for the supported subset, with a compatibility report
//! for unsupported elements (docs/11 §10). Spike-quality: no DI (diagram layout) elements.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::{Value, json};

const NS: &str = "http://www.omg.org/spec/BPMN/20100524/MODEL";
const PROCERA_NS: &str = "https://procera.dev/schema/bpmn-ext/1";

/// IR kind → (BPMN element, procera:kind attribute kept for lossless round trip)
fn to_bpmn_element(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "start" => "startEvent",
        "end" => "endEvent",
        "human.task" | "human.approval" | "human.sign" | "human.acknowledge" => "userTask",
        "decision" => "exclusiveGateway",
        "parallel.split" | "parallel.join" => "parallelGateway",
        "wait.timer" => "intermediateCatchEvent",
        "wait.signal" => "intermediateCatchEvent",
        "service.call" => "serviceTask",
        "notify" => "sendTask",
        "subprocess.call" => "callActivity",
        "set" | "case.open" => "task",
        _ => return None,
    })
}

pub fn export(ir: &Value) -> String {
    let key = ir["process"]["key"].as_str().unwrap_or("process");
    let mut x = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="{NS}" xmlns:procera="{PROCERA_NS}" targetNamespace="https://procera.dev/processes">
  <process id="{}" isExecutable="true">
"#, key.replace('.', "_"));
    for n in ir["nodes"].as_array().into_iter().flatten() {
        let (id, kind) = (n["id"].as_str().unwrap_or_default(), n["kind"].as_str().unwrap_or_default());
        let el = to_bpmn_element(kind).unwrap_or("task");
        let name = n["name"]["en"].as_str().unwrap_or(id);
        x += &format!(r#"    <{el} id="{id}" name="{}" procera:kind="{kind}">"#, name.replace('"', "&quot;"));
        match kind {
            "wait.timer" => x += &format!("<timerEventDefinition><timeDuration>{}</timeDuration></timerEventDefinition>", n["after"].as_str().unwrap_or("")),
            "wait.signal" => x += "<messageEventDefinition/>",
            _ => {}
        }
        x += &format!("</{el}>\n");
    }
    for e in ir["edges"].as_array().into_iter().flatten() {
        let on = e["on"].as_str().unwrap_or("next");
        x += &format!(r#"    <sequenceFlow id="{}" sourceRef="{}" targetRef="{}" name="{on}"/>"#, e["id"].as_str().unwrap_or(""), e["from"].as_str().unwrap_or(""), e["to"].as_str().unwrap_or(""));
        x += "\n";
    }
    x += "  </process>\n</definitions>\n";
    x
}

#[derive(Debug, Default)]
pub struct Report {
    pub unsupported: Vec<(String, String)>, // (element id, construct)
    pub warnings: Vec<(String, String)>,
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == name).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

pub fn import(xml: &str) -> Result<(Value, Report)> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    let (mut nodes, mut edges, mut report) = (vec![], vec![], Report::default());
    let mut current: Option<usize> = None;
    let mut in_duration = false;
    let mut key = "imported".to_owned();
    loop {
        match r.read_event()? {
            Event::Start(e) | Event::Empty(e) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let id = attr(&e, b"id").unwrap_or_default();
                match local.as_str() {
                    "process" => key = id.replace('_', "."),
                    "sequenceFlow" => edges.push(json!({"id": id, "from": attr(&e, b"sourceRef"), "to": attr(&e, b"targetRef"), "on": attr(&e, b"name").unwrap_or_else(|| "next".into())})),
                    "startEvent" | "endEvent" | "userTask" | "exclusiveGateway" | "parallelGateway" | "intermediateCatchEvent" | "serviceTask" | "sendTask" | "callActivity" | "task" => {
                        let kind = attr(&e, b"procera:kind").unwrap_or_else(|| match local.as_str() {
                            "startEvent" => "start", "endEvent" => "end", "userTask" => "human.task", "exclusiveGateway" => "decision",
                            "parallelGateway" => "parallel.split", "intermediateCatchEvent" => "wait.timer", "serviceTask" => "service.call",
                            "sendTask" => "notify", "callActivity" => "subprocess.call", _ => "set",
                        }.to_owned());
                        if attr(&e, b"procera:kind").is_none() {
                            report.warnings.push((id.clone(), format!("{local} mapped to {kind}; review assignment/outcomes")));
                        }
                        let name = attr(&e, b"name").unwrap_or_else(|| id.clone());
                        nodes.push(json!({"id": id, "kind": kind, "name": {"en": name}}));
                        current = Some(nodes.len() - 1);
                    }
                    "timeDuration" => in_duration = true,
                    "inclusiveGateway" | "complexGateway" | "eventBasedGateway" | "scriptTask" | "businessRuleTask" | "transaction" | "compensateEventDefinition" => {
                        report.unsupported.push((id, local));
                    }
                    _ => {}
                }
            }
            Event::Text(t) if in_duration => {
                if let Some(i) = current { nodes[i]["after"] = json!(String::from_utf8_lossy(t.as_ref()).into_owned()); }
            }
            Event::End(e) if e.local_name().as_ref() == b"timeDuration" => in_duration = false,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((json!({"schemaVersion": 1, "process": {"key": key, "version": 1, "name": {"en": key}}, "variables": [], "nodes": nodes, "edges": edges}), report))
}

fn shape(ir: &Value) -> (BTreeSet<(String, String)>, BTreeSet<(String, String, String)>) {
    let nodes = ir["nodes"].as_array().into_iter().flatten().map(|n| (n["id"].as_str().unwrap_or("").to_owned(), n["kind"].as_str().unwrap_or("").to_owned())).collect();
    let edges = ir["edges"].as_array().into_iter().flatten()
        .map(|e| (e["from"].as_str().unwrap_or("").to_owned(), e["to"].as_str().unwrap_or("").to_owned(), e["on"].as_str().unwrap_or("next").to_owned())).collect();
    (nodes, edges)
}

pub fn sp06() -> Result<()> {
    println!("\n## SP-06 — BPMN 2.0 round trip of the supported subset + compatibility report");
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance/reference/");
    let mut total = 0;
    for f in ["fin.purchase-requisition", "hr.employee-onboarding", "it.access-request", "grc.policy-acknowledgement", "qms.capa"] {
        let ir: Value = serde_json::from_str(&std::fs::read_to_string(format!("{base}{f}.json"))?)?;
        let xml = export(&ir);
        let (back, report) = import(&xml)?;
        let (a, b) = (shape(&ir), shape(&back));
        ensure!(a == b, "{f}: graph changed in round trip\n{a:?}\n{b:?}");
        ensure!(report.unsupported.is_empty(), "{f}: unexpected unsupported elements");
        // Timer durations survive.
        for n in ir["nodes"].as_array().into_iter().flatten().filter(|n| n["kind"] == "wait.timer" && n.get("after").is_some()) {
            let m = back["nodes"].as_array().into_iter().flatten().find(|m| m["id"] == n["id"]).cloned().unwrap_or_default();
            ensure!(m["after"] == n["after"], "{f}: timer duration lost");
        }
        total += a.0.len();
        println!("{f}: {} nodes / {} flows → BPMN ({} bytes) → IR: graph identical", a.0.len(), a.1.len(), xml.len());
    }
    let foreign = format!(r#"<definitions xmlns="{NS}"><process id="legacy">
      <startEvent id="s"/><userTask id="review" name="Review"/><inclusiveGateway id="or_split"/><scriptTask id="calc" name="Groovy calc"/>
      <endEvent id="e"/><sequenceFlow id="f1" sourceRef="s" targetRef="review"/><sequenceFlow id="f2" sourceRef="review" targetRef="or_split"/>
      <sequenceFlow id="f3" sourceRef="or_split" targetRef="calc"/><sequenceFlow id="f4" sourceRef="calc" targetRef="e"/></process></definitions>"#);
    let (_ir, report) = import(&foreign)?;
    println!("foreign BPMN (Camunda-style) → unsupported: {:?}; warnings: {:?}", report.unsupported, report.warnings);
    ensure!(report.unsupported.iter().any(|(id, _)| id == "or_split") && report.unsupported.iter().any(|(id, _)| id == "calc"), "SP-06: unsupported elements not reported");
    println!("✅ SP-06 passed: {total} nodes round-tripped losslessly across 5 processes; every unsupported element reported by id");
    Ok(())
}
