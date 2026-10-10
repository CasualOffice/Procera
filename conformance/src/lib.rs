//! Structural validation of workflow IR v1 documents (docs/11-WORKFLOW-IR-SPEC.md §11).
//!
//! This is the *spec-level* checker used by the reference fixtures (BPMS-002). The production
//! compiler (BPMS-080) must produce the same diagnostics, with the same codes, for the same input.
//! Expression type-checking (check 7) and reference resolution (checks 8–10) need the expression
//! engine and the library, so they are out of scope here.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const SCHEMA_V1: &str = include_str!("../../schemas/ir/v1.schema.json");
const MAX_NODES: usize = 500;

/// A compiler diagnostic with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub node: Option<String>,
    pub message: String,
}

fn diag(code: &'static str, node: Option<&str>, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code, node: node.map(str::to_owned), message: message.into() }
}

/// Validates the document shape against the IR v1 JSON Schema.
pub fn validate_schema(doc: &Value) -> Vec<Diagnostic> {
    let schema: Value = match serde_json::from_str(SCHEMA_V1) {
        Ok(s) => s,
        Err(e) => return vec![diag("IR-E000", None, format!("schema unreadable: {e}"))],
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => return vec![diag("IR-E000", None, format!("schema invalid: {e}"))],
    };
    validator
        .iter_errors(doc)
        .map(|e| diag("IR-E100", None, format!("{} at {}", e, e.instance_path())))
        .collect()
}

/// Full validation: schema first, then structural checks if the shape is valid.
pub fn validate(doc: &Value) -> Vec<Diagnostic> {
    let schema = validate_schema(doc);
    if !schema.is_empty() {
        return schema;
    }
    validate_structure(doc)
}

struct Node<'a> {
    id: &'a str,
    kind: &'a str,
    raw: &'a Value,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Outcomes a node can produce (docs/11 §3). `default` is included for decisions.
fn outcomes_of(n: &Node<'_>) -> BTreeSet<String> {
    let names = |key: &str| -> Vec<String> {
        n.raw
            .get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|b| b.get("name").and_then(Value::as_str).or_else(|| b.as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let fixed = |xs: &[&str]| xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mut out: Vec<String> = match n.kind {
        "end" => vec![],
        "human.task" => {
            let custom = names("outcomes");
            if custom.is_empty() { fixed(&["submitted"]) } else { custom }
        }
        "human.approval" => {
            let mut v = fixed(&["approved", "rejected", "changes_requested"]);
            v.extend(names("outcomes"));
            v
        }
        "human.sign" => fixed(&["signed", "declined", "expired"]),
        "human.acknowledge" => fixed(&["acknowledged", "expired"]),
        "decision" => {
            let mut v = names("branches");
            v.push("default".to_owned());
            v
        }
        "parallel.split" => names("branches"),
        "wait.timer" => fixed(&["fired"]),
        "wait.signal" => {
            if n.raw.get("timeout").is_some() {
                fixed(&["received", "timeout"])
            } else {
                fixed(&["received"])
            }
        }
        "service.call" => fixed(&["ok", "error"]),
        "subprocess.call" => fixed(&["completed", "failed", "cancelled"]),
        "case.open" => {
            if n.raw.get("waitForClose").and_then(Value::as_bool).unwrap_or(false) {
                fixed(&["closed"])
            } else {
                fixed(&["opened"])
            }
        }
        _ => fixed(&["next"]), // start, notify, set, parallel.join
    };
    out.sort();
    out.into_iter().collect()
}

fn is_wait_or_human(kind: &str) -> bool {
    kind.starts_with("human.") || kind.starts_with("wait.")
}

/// Structural checks from docs/11 §11 (1–6, 11–12) plus integrity checks.
pub fn validate_structure(doc: &Value) -> Vec<Diagnostic> {
    let mut d = Vec::new();
    let empty = Vec::new();
    let raw_nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let raw_edges = doc.get("edges").and_then(Value::as_array).unwrap_or(&empty);

    // Nodes, unique ids (IR-E002).
    let mut nodes: BTreeMap<&str, Node<'_>> = BTreeMap::new();
    for raw in raw_nodes {
        let (Some(id), Some(kind)) = (str_of(raw, "id"), str_of(raw, "kind")) else { continue };
        if nodes.insert(id, Node { id, kind, raw }).is_some() {
            d.push(diag("IR-E002", Some(id), format!("duplicate node id '{id}'")));
        }
    }
    if nodes.len() > MAX_NODES {
        d.push(diag("IR-E012", None, format!("{} nodes exceeds the limit of {MAX_NODES}", nodes.len())));
    }

    // Variables unique (IR-E016).
    let mut vars = HashSet::new();
    for v in doc.get("variables").and_then(Value::as_array).unwrap_or(&empty) {
        if let Some(name) = str_of(v, "name")
            && !vars.insert(name)
        {
            d.push(diag("IR-E016", None, format!("duplicate variable '{name}'")));
        }
    }

    // Exactly one start (IR-E001).
    let starts: Vec<&str> = nodes.values().filter(|n| n.kind == "start").map(|n| n.id).collect();
    if starts.len() != 1 {
        d.push(diag("IR-E001", None, format!("expected exactly one start node, found {}", starts.len())));
    }

    // Edges: unique ids, endpoints exist, outcomes declared (IR-E003, IR-E004, IR-E006, IR-E013).
    let mut edge_ids = HashSet::new();
    let mut out_edges: HashMap<&str, Vec<(&str, String)>> = HashMap::new();
    for e in raw_edges {
        let (Some(id), Some(from), Some(to)) = (str_of(e, "id"), str_of(e, "from"), str_of(e, "to")) else {
            continue;
        };
        if !edge_ids.insert(id) {
            d.push(diag("IR-E003", None, format!("duplicate edge id '{id}'")));
        }
        let on = str_of(e, "on").unwrap_or("next").to_owned();
        match (nodes.get(from), nodes.get(to)) {
            (Some(src), Some(dst)) => {
                if src.kind == "end" {
                    d.push(diag("IR-E013", Some(from), "end node must not have outgoing edges"));
                }
                if dst.kind == "start" {
                    d.push(diag("IR-E013", Some(to), "start node must not have incoming edges"));
                }
                if !outcomes_of(src).contains(&on) {
                    d.push(diag(
                        "IR-E006",
                        Some(from),
                        format!("edge '{id}' uses outcome '{on}' which node '{from}' cannot produce"),
                    ));
                }
                out_edges.entry(from).or_default().push((to, on));
            }
            _ => d.push(diag(
                "IR-E004",
                None,
                format!("edge '{id}' references a missing node ({from} → {to})"),
            )),
        }
    }

    // Every outcome handled (IR-E005); decisions need a default (IR-E007).
    for n in nodes.values() {
        let handled: BTreeSet<String> =
            out_edges.get(n.id).map(|v| v.iter().map(|(_, on)| on.clone()).collect()).unwrap_or_default();
        if n.kind == "decision" && !handled.contains("default") {
            d.push(diag("IR-E007", Some(n.id), "decision must have a 'default' edge"));
        }
        if str_of(n.raw, "unhandled") == Some("fail") {
            continue;
        }
        for o in outcomes_of(n) {
            if !handled.contains(&o) && !(n.kind == "decision" && o == "default") {
                d.push(diag("IR-E005", Some(n.id), format!("outcome '{o}' of '{}' is not handled", n.id)));
            }
        }
    }

    // References inside nodes (IR-E014 excludeActorsOf, IR-E015 boundary targets).
    let mut boundary_edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in nodes.values() {
        if let Some(ex) = n.raw.pointer("/assignment/excludeActorsOf").and_then(Value::as_array) {
            for r in ex.iter().filter_map(Value::as_str) {
                if !nodes.contains_key(r) {
                    d.push(diag(
                        "IR-E014",
                        Some(n.id),
                        format!("excludeActorsOf references unknown node '{r}'"),
                    ));
                }
            }
        }
        for b in n.raw.get("boundary").and_then(Value::as_array).unwrap_or(&empty) {
            if let Some(to) = str_of(b, "to") {
                match nodes.get_key_value(to) {
                    Some((k, _)) => boundary_edges.entry(n.id).or_default().push(k),
                    None => {
                        d.push(diag("IR-E015", Some(n.id), format!("boundary target '{to}' does not exist")))
                    }
                }
            }
        }
    }

    let succ = |id: &str| -> Vec<&str> {
        let mut v: Vec<&str> =
            out_edges.get(id).map(|e| e.iter().map(|(t, _)| *t).collect()).unwrap_or_default();
        v.extend(boundary_edges.get(id).cloned().unwrap_or_default());
        v
    };

    // Reachability (IR-E009) and a reachable end (IR-E010).
    if let Some(start) = starts.first() {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack = vec![*start];
        while let Some(x) = stack.pop() {
            if seen.insert(x) {
                stack.extend(succ(x));
            }
        }
        for n in nodes.values() {
            if !seen.contains(n.id) {
                d.push(diag("IR-E009", Some(n.id), format!("node '{}' is unreachable from start", n.id)));
            }
        }
        if !nodes.values().any(|n| n.kind == "end" && seen.contains(n.id)) {
            d.push(diag("IR-E010", None, "no end node is reachable from start"));
        }
    }

    // Split/join pairing and structured branches (IR-E008).
    let mut join_owners: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in nodes.values().filter(|n| n.kind == "parallel.split") {
        let Some(join) = str_of(n.raw, "joinId") else {
            d.push(diag("IR-E008", Some(n.id), "parallel.split must declare joinId"));
            continue;
        };
        match nodes.get(join) {
            Some(j) if j.kind == "parallel.join" => join_owners.entry(j.id).or_default().push(n.id),
            _ => {
                d.push(diag("IR-E008", Some(n.id), format!("joinId '{join}' is not a parallel.join")));
                continue;
            }
        }
        // Every path from each branch must reach the join before any end node.
        for (first, _) in out_edges.get(n.id).cloned().unwrap_or_default() {
            let mut seen: HashSet<&str> = HashSet::new();
            let mut stack = vec![first];
            while let Some(x) = stack.pop() {
                if x == join || !seen.insert(x) {
                    continue;
                }
                if nodes.get(x).is_some_and(|m| m.kind == "end") {
                    let terminate = nodes.get(x).and_then(|m| str_of(m.raw, "result")) == Some("terminated");
                    if !terminate {
                        d.push(diag(
                            "IR-E008",
                            Some(n.id),
                            format!("branch via '{first}' reaches end '{x}' without passing join '{join}'"),
                        ));
                    }
                    continue;
                }
                stack.extend(succ(x));
            }
        }
    }
    for n in nodes.values().filter(|n| n.kind == "parallel.join") {
        let owners = join_owners.get(n.id).map_or(0, Vec::len);
        if owners != 1 {
            d.push(diag(
                "IR-E008",
                Some(n.id),
                format!("parallel.join must be closed by exactly one split, found {owners}"),
            ));
        }
    }

    // Bounded loops (IR-E011): every cycle needs a human/wait node or maxIterations.
    for scc in strongly_connected(&nodes, &succ) {
        let is_cycle = scc.len() > 1 || succ(scc[0]).contains(&scc[0]);
        if !is_cycle {
            continue;
        }
        let bounded = scc.iter().any(|id| {
            nodes.get(id).is_some_and(|n| is_wait_or_human(n.kind) || n.raw.get("maxIterations").is_some())
        });
        if !bounded {
            d.push(diag(
                "IR-E011",
                Some(scc[0]),
                format!("loop through {scc:?} has no human/wait node and no maxIterations"),
            ));
        }
    }

    d
}

/// Tarjan's strongly connected components.
fn strongly_connected<'a>(
    nodes: &BTreeMap<&'a str, Node<'a>>,
    succ: &dyn Fn(&str) -> Vec<&'a str>,
) -> Vec<Vec<&'a str>> {
    struct St<'a> {
        index: usize,
        idx: HashMap<&'a str, usize>,
        low: HashMap<&'a str, usize>,
        stack: Vec<&'a str>,
        on: HashSet<&'a str>,
        out: Vec<Vec<&'a str>>,
    }
    fn visit<'a>(v: &'a str, st: &mut St<'a>, succ: &dyn Fn(&str) -> Vec<&'a str>) {
        st.idx.insert(v, st.index);
        st.low.insert(v, st.index);
        st.index += 1;
        st.stack.push(v);
        st.on.insert(v);
        for w in succ(v) {
            if !st.idx.contains_key(w) {
                visit(w, st, succ);
                let lw = st.low[w];
                let lv = st.low[v];
                st.low.insert(v, lv.min(lw));
            } else if st.on.contains(w) {
                let iw = st.idx[w];
                let lv = st.low[v];
                st.low.insert(v, lv.min(iw));
            }
        }
        if st.low[v] == st.idx[v] {
            let mut comp = Vec::new();
            while let Some(w) = st.stack.pop() {
                st.on.remove(w);
                comp.push(w);
                if w == v {
                    break;
                }
            }
            st.out.push(comp);
        }
    }
    let mut st = St {
        index: 0,
        idx: HashMap::new(),
        low: HashMap::new(),
        stack: vec![],
        on: HashSet::new(),
        out: vec![],
    };
    for id in nodes.keys() {
        if !st.idx.contains_key(id) {
            visit(id, &mut st, succ);
        }
    }
    st.out
}
