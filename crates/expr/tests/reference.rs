//! Every expression in the five reference processes (conformance/reference, docs/11 §13),
//! rewritten from the `expr:feel:` placeholders to CEL, compiles against the process's own
//! variable declarations, with the type its field needs.

// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use procera_expr::*;
use serde_json::Value as J;
use std::path::Path;

/// FEEL placeholder → CEL. Expressions not listed are compiled as written, so a fixture
/// expression that is not already valid CEL needs a row here.
const CEL: &[(&str, &str)] = &[
    (
        "(if severity = \"critical\" then 5 else if severity = \"major\" then 3 else 1) * occurrence",
        // `occurrence` is optional, so the CEL form handles the missing value explicitly.
        "occurrence != null ? (severity == \"critical\" ? 5 : severity == \"major\" ? 3 : 1) * occurrence : 0",
    ),
    ("date and time(start_date, time(\"09:00:00\"))", "datetime(start_date) + duration(\"PT9H\")"),
    ("instance.id + \":grant\"", "instance.id + \":grant\""),
    ("sensitivity = \"high\"", "sensitivity == \"high\""),
    // Optional boolean: a branch condition must not be null (docs/11 §6).
    ("needs_doc_change", "needs_doc_change == true"),
];

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
            let (_, feel) = split_tagged(tagged).unwrap();
            let cel = CEL.iter().find(|(f, _)| *f == feel).map_or(feel, |(_, c)| c);
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
