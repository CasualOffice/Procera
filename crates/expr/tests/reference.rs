//! Every expression in the five reference processes (conformance/reference, docs/11 §13), written
//! in CEL since BPMS-080 wired the engine into the compiler, compiles against the process's own
//! variable declarations, with the type its field needs.

// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use procera_expr::*;
use serde_json::Value as J;
use std::path::Path;

fn schema_of(process: &J) -> Schema {
    let mut s = Schema::with_process_context();
    for v in process["variables"].as_array().unwrap() {
        let class = match v["classification"].as_str().unwrap_or("internal") {
            "public" => Classification::Public,
            "confidential" => Classification::Confidential,
            "restricted" => Classification::Restricted,
            _ => Classification::Internal,
        };
        let ty = Type::from_ir(&v["type"]).unwrap();
        s.declare(v["name"].as_str().unwrap(), ty, v["required"].as_bool().unwrap_or(false), class);
    }
    s
}

fn collect<'a>(key: &str, j: &'a J, out: &mut Vec<(String, &'a str)>) {
    match j {
        J::String(s) if s.starts_with("expr:") => out.push((key.to_string(), s)),
        J::Array(a) => a.iter().for_each(|x| collect(key, x, out)),
        J::Object(o) => o.iter().for_each(|(k, x)| collect(k, x, out)),
        _ => {}
    }
}

#[test]
fn reference_process_expressions_compile() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/reference");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let process: J = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let schema = schema_of(&process);
        let mut exprs = Vec::new();
        collect("", &process, &mut exprs);
        for (key, tagged) in exprs {
            let (lang, cel) = split_tagged(tagged).unwrap();
            assert_eq!(lang, LANGUAGE_TAG, "{}: {tagged}", path.display());
            let expected = match key.as_str() {
                "when" => Some(Type::Bool),
                "idempotencyKey" => Some(Type::String),
                "at" => Some(Type::DateTime),
                _ => None,
            };
            let opts = CheckOptions { expected, context: ExprContext::General };
            let c =
                compile(cel, &schema, &opts).unwrap_or_else(|d| panic!("{}: {cel}: {d:?}", path.display()));
            if key == "ref" {
                let t = c.ty.non_null();
                assert!(
                    matches!(t, Type::Principal | Type::String | Type::Group),
                    "{cel}: assignment ref has type {t}"
                );
            }
            seen += 1;
        }
    }
    assert!(seen >= 20, "only {seen} expressions found");
}

#[test]
fn feel_forms_are_rejected_or_flagged() {
    let mut s = Schema::new();
    s.declare("occurrence", Type::Int, false, Classification::Internal);
    s.declare("needs_doc_change", Type::Bool, false, Classification::Internal);
    // FEEL's `=` is not CEL.
    assert_eq!(compile("occurrence = 1", &s, &CheckOptions::default()).unwrap_err()[0].code, Code::Lexical);
    // Arithmetic on an optional variable needs a guard.
    assert_eq!(compile("occurrence * 2", &s, &CheckOptions::default()).unwrap_err()[0].code, Code::Nullable);
    // An optional boolean can't decide a branch on its own.
    let d = compile("needs_doc_change", &s, &CheckOptions::condition()).unwrap_err();
    assert_eq!(d[0].code, Code::NullableCondition);
}
