//! Compiler conformance fixtures (`conformance/compiler/<case>/{source,expected}.json`), golden
//! checksums of the reference processes, and parity with the BPMS-002 spec-level checker.
// Helpers outside #[test] fns may unwrap too (clippy.toml allows it inside tests).
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn actual(out: &procera_ir::CompileOutput) -> Value {
    let diagnostics: Vec<Value> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity != procera_ir::Severity::Info)
        .map(|d| json!({"code": d.code.as_str(), "pointer": d.pointer}))
        .collect();
    json!({"diagnostics": diagnostics, "checksum": out.compiled.as_ref().map(|c| c.checksum.clone())})
}

fn read(path: &std::path::Path) -> Option<Value> {
    std::fs::read_to_string(path).ok().map(|t| serde_json::from_str(&t).unwrap())
}

#[test]
fn compiler_fixtures_match_expectations() {
    let dir = repo_root().join("conformance/compiler");
    let mut cases = 0;
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for case in entries.into_iter().filter(|p| p.is_dir()) {
        let source = read(&case.join("source.json")).unwrap();
        let got = actual(&offline(&source));
        let want = read(&case.join("expected.json"));
        assert_eq!(want.as_ref(), Some(&got), "{}: actual = {got:#}", case.display());
        cases += 1;
    }
    assert!(cases >= 8, "expected the compiler fixture set, found {cases}");
}

#[test]
fn reference_checksums_are_stable() {
    let golden = read(&repo_root().join("conformance/compiler/reference.checksums.json"));
    let got: serde_json::Map<String, Value> = REFERENCE
        .iter()
        .map(|n| ((*n).to_owned(), json!(offline(&reference(n)).compiled.unwrap().checksum)))
        .collect();
    assert_eq!(golden, Some(Value::Object(got.clone())), "actual = {:#}", Value::Object(got));
}

/// The production compiler reports at least every code the spec-level checker reports.
#[test]
fn parity_with_spec_level_checker() {
    let mut docs: Vec<(String, Value)> = REFERENCE.iter().map(|n| ((*n).to_owned(), reference(n))).collect();
    let dir = repo_root().join("conformance/compiler");
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            docs.push((p.display().to_string(), read(&p.join("source.json")).unwrap()));
        }
    }
    for (name, doc) in docs {
        let spec: BTreeSet<&str> = procera_conformance::validate(&doc).iter().map(|d| d.code).collect();
        let ours: BTreeSet<&str> = offline(&doc).diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(spec.is_subset(&ours), "{name}: spec {spec:?} vs ours {ours:?}");
    }
}
