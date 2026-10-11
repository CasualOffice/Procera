//! Property tests (docs/15 §5 "Compiler"): compile is deterministic and idempotent, the checksum
//! ignores key and entity order, and the semantic diff is empty on itself and replays exactly.
// Helpers outside #[test] fns may unwrap too (clippy.toml allows it inside tests).
#![allow(clippy::unwrap_used)]

mod common;

use common::offline;
use procera_ir::{CompileContext, Compiled, Severity, compile, compile_str, diff};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// A generated process: a sequence of blocks between `start` and `end`.
#[derive(Debug, Clone)]
enum Block {
    Simple(u8),
    Decision(Vec<Vec<u8>>),
    Parallel(Vec<Vec<u8>>),
}

#[derive(Debug, Clone)]
struct Spec {
    version: u32,
    blocks: Vec<Block>,
    vars: Vec<(u8, u8)>,
    form_version: Option<u8>,
}

fn block() -> impl Strategy<Value = Block> {
    let chain = |min| prop::collection::vec(0u8..4, min..3);
    prop_oneof![
        (0u8..4).prop_map(Block::Simple),
        prop::collection::vec(chain(0), 1..4).prop_map(Block::Decision),
        prop::collection::vec(chain(1), 2..4).prop_map(Block::Parallel),
    ]
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        1u32..4,
        prop::collection::vec(block(), 0..6),
        prop::collection::vec((0u8..11, 0u8..5), 0..5),
        any::<Option<u8>>(),
    )
        .prop_map(|(version, blocks, vars, form_version)| Spec { version, blocks, vars, form_version })
}

struct Builder {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    next: usize,
}

impl Builder {
    fn id(&mut self, p: &str) -> String {
        self.next += 1;
        format!("{p}{}", self.next)
    }
    fn edge(&mut self, from: &str, on: &str, to: &str) {
        let id = self.id("e");
        self.edges.push(json!({"id": id, "from": from, "to": to, "on": on}));
    }
    fn connect(&mut self, open: &[(String, String)], to: &str) {
        for (from, on) in open {
            self.edge(from, on, to);
        }
    }
    /// Adds a simple node and returns (id, outcome).
    fn simple(&mut self, k: u8) -> (String, String) {
        let id = self.id("n");
        let who = json!({"candidates": [{"type": "role", "ref": "process-owner"}]});
        let (node, on) = match k {
            0 => (json!({"id": id, "kind": "set", "assign": {"amount": "expr:cel: amount + 1"}}), "next"),
            1 => (
                json!({"id": id, "kind": "notify", "to": who, "template": "t", "channels": ["email"]}),
                "next",
            ),
            2 => (json!({"id": id, "kind": "human.task", "assignment": who}), "submitted"),
            _ => (json!({"id": id, "kind": "wait.timer", "after": "PT1H"}), "fired"),
        };
        self.nodes.push(node);
        (id, on.to_owned())
    }
    fn chain(&mut self, from: (String, String), kinds: &[u8], to: Option<&str>) -> Vec<(String, String)> {
        let mut open = vec![from];
        for &k in kinds {
            let (id, on) = self.simple(k);
            self.connect(&open, &id);
            open = vec![(id, on)];
        }
        if let Some(t) = to {
            self.connect(&open, t);
            return vec![];
        }
        open
    }
}

const TYPES: [&str; 11] = [
    "string",
    "text",
    "integer",
    "decimal",
    "boolean",
    "date",
    "datetime",
    "duration",
    "principal",
    "group",
    "file",
];
const CLASSES: [Option<&str>; 5] =
    [None, Some("public"), Some("internal"), Some("confidential"), Some("restricted")];

fn build(s: &Spec) -> Value {
    let mut b = Builder { nodes: vec![json!({"id": "start", "kind": "start"})], edges: vec![], next: 0 };
    let mut open = vec![("start".to_owned(), "next".to_owned())];
    for blk in &s.blocks {
        match blk {
            Block::Simple(k) => {
                let (id, on) = b.simple(*k);
                b.connect(&open, &id);
                open = vec![(id, on)];
            }
            Block::Decision(branches) => {
                let id = b.id("d");
                let names: Vec<String> = (0..branches.len()).map(|i| format!("b{i}")).collect();
                b.nodes.push(json!({"id": id, "kind": "decision", "branches": names.iter().enumerate()
                    .map(|(i, n)| json!({"name": n, "when": format!("expr:cel: amount > {i}")})).collect::<Vec<_>>()}));
                b.connect(&open, &id);
                open = vec![(id.clone(), "default".to_owned())];
                for (name, kinds) in names.iter().zip(branches) {
                    open.extend(b.chain((id.clone(), name.clone()), kinds, None));
                }
            }
            Block::Parallel(branches) => {
                let (split, join) = (b.id("s"), b.id("j"));
                let names: Vec<String> = (0..branches.len()).map(|i| format!("p{i}")).collect();
                b.nodes.push(json!({"id": split, "kind": "parallel.split", "joinId": join,
                    "branches": names.iter().map(|n| json!({"name": n})).collect::<Vec<_>>()}));
                b.nodes.push(json!({"id": join, "kind": "parallel.join"}));
                b.connect(&open, &split);
                for (name, kinds) in names.iter().zip(branches) {
                    b.chain((split.clone(), name.clone()), kinds, Some(&join));
                }
                open = vec![(join, "next".to_owned())];
            }
        }
    }
    b.nodes.push(json!({"id": "end", "kind": "end"}));
    b.connect(&open, "end");
    let mut vars =
        vec![json!({"name": "amount", "type": "decimal", "classification": "internal", "required": true})];
    for (i, (t, c)) in s.vars.iter().enumerate() {
        let mut v = json!({"name": format!("v{i}"), "type": TYPES[*t as usize]});
        if let Some(c) = CLASSES[*c as usize] {
            v["classification"] = json!(c);
        }
        vars.push(v);
    }
    let mut doc = json!({
        "schemaVersion": 1,
        "process": {"key": "gen", "version": s.version, "name": {"en": "Generated"}},
        "variables": vars, "nodes": b.nodes, "edges": b.edges
    });
    if let Some(v) = s.form_version {
        doc["refs"] = json!({"forms": {"f": format!("form:gen.form@{}", v % 4 + 1)}});
    }
    doc
}

fn compiled(doc: &Value) -> Compiled {
    let out = offline(doc);
    assert!(!out.has_errors(), "{:#?}\n{doc:#}", out.diagnostics);
    out.compiled.unwrap()
}

/// Serialises with object keys reversed and entity arrays rotated: same model, different text.
fn scramble(v: &Value, rot: usize, top: bool) -> String {
    match v {
        Value::Object(m) => {
            let parts: Vec<String> = m
                .iter()
                .rev()
                .map(|(k, x)| {
                    let entity = top && matches!(k.as_str(), "nodes" | "edges" | "variables" | "sla");
                    let inner = match (entity, x) {
                        (true, Value::Array(a)) if !a.is_empty() => {
                            let mut a = a.clone();
                            let n = a.len();
                            a.rotate_left(rot % n);
                            let items: Vec<String> = a.iter().map(|i| scramble(i, rot, false)).collect();
                            format!("[{}]", items.join(","))
                        }
                        _ => scramble(x, rot, false),
                    };
                    format!("{}: {inner}", Value::String(k.clone()))
                })
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
        Value::Array(a) => {
            format!("[{}]", a.iter().map(|i| scramble(i, rot, false)).collect::<Vec<_>>().join(","))
        }
        other => other.to_string(),
    }
}

/// Small structural damage, to check determinism on invalid input too.
fn damage(doc: &mut Value, op: u8, at: usize) {
    let pick = |a: &Vec<Value>| if a.is_empty() { 0 } else { at % a.len() };
    match op % 4 {
        0 => {
            let e = doc["edges"].as_array_mut().unwrap();
            let i = pick(e);
            if !e.is_empty() {
                e.remove(i);
            }
        }
        1 => {
            let n = doc["nodes"].as_array_mut().unwrap();
            let i = pick(n);
            let dup = n[i].clone();
            n.push(dup);
        }
        2 => {
            let e = doc["edges"].as_array_mut().unwrap();
            let i = pick(e);
            if let Some(x) = e.get_mut(i) {
                x["to"] = json!("ghost");
            }
        }
        _ => {
            let n = doc["nodes"].as_array_mut().unwrap();
            let i = pick(n);
            if let Some(Value::Object(m)) = n.get_mut(i) {
                let key = m.keys().find(|k| k.as_str() != "id" && k.as_str() != "kind").cloned();
                if let Some(k) = key {
                    let _: Option<Value> = Map::remove(m, &k);
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    #[test]
    fn generated_processes_compile_cleanly(s in spec()) {
        let out = offline(&build(&s));
        let blocking: Vec<_> = out.diagnostics.iter().filter(|d| d.severity != Severity::Info).collect();
        prop_assert!(blocking.is_empty(), "{:#?}", blocking);
    }

    #[test]
    fn compile_is_deterministic_and_idempotent(s in spec()) {
        let doc = build(&s);
        let first = offline(&doc);
        prop_assert_eq!(&first, &offline(&doc));
        let c = first.compiled.unwrap();
        let again = compiled(&serde_json::to_value(&c.ir).unwrap());
        prop_assert_eq!(&again, &c);
        prop_assert_eq!(again.canonical.clone(), procera_ir::canonical_bytes(&c.ir).unwrap());
    }

    #[test]
    fn checksum_ignores_key_and_entity_order(s in spec(), rot in 0usize..7) {
        let doc = build(&s);
        let c = compiled(&doc);
        let text = scramble(&doc, rot, true);
        let out = compile_str(&text, &CompileContext::default());
        prop_assert_eq!(out.compiled.map(|x| x.checksum), Some(c.checksum));
    }

    #[test]
    fn diff_of_self_is_empty_and_diff_replays(a in spec(), b in spec()) {
        let (x, y) = (compiled(&build(&a)).ir, compiled(&build(&b)).ir);
        prop_assert!(diff(&x, &x).is_empty());
        let d = diff(&x, &y);
        prop_assert_eq!(d.is_empty(), x == y);
        prop_assert_eq!(d.apply(&x).unwrap(), y.clone());
        // The compile output carries the same diff against the previous version.
        let out = compile(&build(&b), &CompileContext { previous: Some(&x), ..Default::default() });
        prop_assert_eq!(out.diff, Some(d));
    }

    #[test]
    fn compile_is_deterministic_on_damaged_input(s in spec(), op in 0u8..4, at in 0usize..64) {
        let mut doc = build(&s);
        damage(&mut doc, op, at);
        let first = offline(&doc);
        prop_assert_eq!(&first, &offline(&doc));
        prop_assert_eq!(first.compiled.is_some(), !first.has_errors());
        let mut sorted = first.diagnostics.clone();
        sorted.sort();
        prop_assert_eq!(sorted, first.diagnostics);
    }
}
