//! Shared test helpers.
#![allow(dead_code)]

use procera_ir::{CompileContext, CompileOutput, Environment, Resolution, compile};
use serde_json::Value;
use std::path::PathBuf;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub const REFERENCE: [&str; 5] = [
    "fin.purchase-requisition",
    "grc.policy-acknowledgement",
    "hr.employee-onboarding",
    "it.access-request",
    "qms.capa",
];

pub fn reference(name: &str) -> Value {
    let path = repo_root().join("conformance/reference").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

pub fn offline(doc: &Value) -> CompileOutput {
    compile(doc, &CompileContext::default())
}

pub fn codes(out: &CompileOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// An environment where everything resolves except the names listed.
#[derive(Debug, Default)]
pub struct FakeEnv {
    pub missing_refs: Vec<(&'static str, Resolution)>,
    pub disabled_connectors: Vec<&'static str>,
    pub missing_principals: Vec<&'static str>,
}

impl Environment for FakeEnv {
    fn resolve(&self, reference: &str) -> Resolution {
        self.missing_refs.iter().find(|(r, _)| *r == reference).map_or(Resolution::Published, |(_, s)| *s)
    }
    fn connector_usable(&self, reference: &str) -> bool {
        !self.disabled_connectors.contains(&reference)
    }
    fn principal_exists(&self, _ty: procera_ir::model::CandidateType, name: &str) -> bool {
        !self.missing_principals.contains(&name)
    }
}
