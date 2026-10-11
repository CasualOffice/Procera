//! Structural checks: ids, edges, outcomes, reachability, parallel blocks, loops, size
//! (docs/11 §11 checks 1-6 and 12; codes shared with the BPMS-002 checker in `conformance/`).

use super::Index;
use crate::diag::{Code, Diagnostic};
use crate::model::{EndResult, NodeKind, Unhandled};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODES: usize = 500;
pub const MAX_BRANCHES: usize = 50;
pub const MAX_NESTING: usize = 10;

pub(super) fn check(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    ids(ix, out);
    edges(ix, out);
    outcomes(ix, out);
    node_refs(ix, out);
    reachability(ix, out);
    parallel(ix, out);
    loops(ix, out);
}

fn ids(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    let doc = ix.doc;
    let mut seen = BTreeSet::new();
    for (i, n) in doc.nodes.iter().enumerate() {
        if !seen.insert(n.id.as_str()) {
            out.push(
                Diagnostic::new(
                    Code::E002,
                    format!("/nodes/{i}/id"),
                    format!("duplicate node id '{}'", n.id),
                )
                .at_node(&n.id),
            );
        }
    }
    let mut seen = BTreeSet::new();
    for (i, e) in doc.edges.iter().enumerate() {
        if !seen.insert(e.id.as_str()) {
            out.push(Diagnostic::new(
                Code::E003,
                format!("/edges/{i}/id"),
                format!("duplicate edge id '{}'", e.id),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for (i, v) in doc.variables.iter().enumerate() {
        if !seen.insert(v.name.as_str()) {
            out.push(Diagnostic::new(
                Code::E016,
                format!("/variables/{i}/name"),
                format!("duplicate variable '{}'", v.name),
            ));
        }
    }
    if doc.nodes.len() > MAX_NODES {
        out.push(Diagnostic::new(
            Code::E012,
            "/nodes",
            format!("{} nodes exceeds the limit of {MAX_NODES}", doc.nodes.len()),
        ));
    }
    let starts: Vec<usize> =
        doc.nodes.iter().enumerate().filter(|(_, n)| n.kind == NodeKind::Start).map(|(i, _)| i).collect();
    match starts.as_slice() {
        [] => out.push(Diagnostic::new(Code::E001, "/nodes", "expected exactly one start node, found 0")),
        [_] => {}
        [_, rest @ ..] => {
            for &i in rest {
                out.push(
                    Diagnostic::new(
                        Code::E001,
                        format!("/nodes/{i}"),
                        format!("expected exactly one start node, found {}", starts.len()),
                    )
                    .at_node(&doc.nodes[i].id),
                );
            }
        }
    }
}

fn edges(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    for (i, e) in ix.doc.edges.iter().enumerate() {
        let (src, dst) = (ix.node(&e.from), ix.node(&e.to));
        for (field, id, node) in [("from", &e.from, src), ("to", &e.to, dst)] {
            if node.is_none() {
                out.push(Diagnostic::new(
                    Code::E004,
                    format!("/edges/{i}/{field}"),
                    format!("edge '{}' references a missing node '{id}'", e.id),
                ));
            }
        }
        let (Some(src), Some(dst)) = (src, dst) else { continue };
        if src.kind == NodeKind::End {
            out.push(
                Diagnostic::new(Code::E013, format!("/edges/{i}"), "end node must not have outgoing edges")
                    .at_node(&src.id),
            );
        }
        if dst.kind == NodeKind::Start {
            out.push(
                Diagnostic::new(Code::E013, format!("/edges/{i}"), "start node must not have incoming edges")
                    .at_node(&dst.id),
            );
        }
        if !src.outcomes().contains(&e.on) {
            out.push(
                Diagnostic::new(
                    Code::E006,
                    format!("/edges/{i}/on"),
                    format!("edge '{}' uses outcome '{}' which node '{}' cannot produce", e.id, e.on, src.id),
                )
                .at_node(&src.id),
            );
        }
    }
}

fn outcomes(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        let mut handled: BTreeSet<&str> = BTreeSet::new();
        for e in ix.out.get(n.id.as_str()).into_iter().flatten() {
            if !handled.insert(e.on) && n.outcomes().iter().any(|o| o == e.on) {
                out.push(
                    Diagnostic::new(
                        Code::E017,
                        format!("/edges/{}/on", e.index),
                        format!("outcome '{}' of '{}' is handled by more than one edge", e.on, n.id),
                    )
                    .at_node(&n.id),
                );
            }
        }
        let mut names = BTreeSet::new();
        for (b, br) in n.branches.iter().flatten().enumerate() {
            if !names.insert(br.name.as_str()) {
                out.push(
                    Diagnostic::new(
                        Code::E034,
                        format!("/nodes/{i}/branches/{b}/name"),
                        format!("branch '{}' is declared twice", br.name),
                    )
                    .at_node(&n.id),
                );
            }
        }
        if n.kind == NodeKind::Decision && !handled.contains("default") {
            out.push(
                Diagnostic::new(Code::E007, format!("/nodes/{i}"), "decision must have a 'default' edge")
                    .at_node(&n.id),
            );
        }
        if n.unhandled == Some(Unhandled::Fail) {
            continue;
        }
        for o in n.outcomes() {
            if !handled.contains(o.as_str()) && !(n.kind == NodeKind::Decision && o == "default") {
                out.push(
                    Diagnostic::new(
                        Code::E005,
                        format!("/nodes/{i}"),
                        format!("outcome '{o}' of '{}' is not handled", n.id),
                    )
                    .at_node(&n.id),
                );
            }
        }
    }
}

fn node_refs(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        for (field, a) in n.assignments() {
            for (x, r) in a.exclude_actors_of.iter().flatten().enumerate() {
                let ptr = format!("/nodes/{i}/{field}/excludeActorsOf/{x}");
                match ix.node(r) {
                    None => out.push(
                        Diagnostic::new(
                            Code::E014,
                            ptr,
                            format!("excludeActorsOf references unknown node '{r}'"),
                        )
                        .at_node(&n.id),
                    ),
                    Some(t) if !t.kind.is_human() => out.push(
                        Diagnostic::new(
                            Code::W002,
                            ptr,
                            format!(
                                "excludeActorsOf references '{r}' ({}), which has no human actors",
                                t.kind.as_str()
                            ),
                        )
                        .at_node(&n.id),
                    ),
                    Some(_) => {}
                }
            }
        }
        for (b, bd) in n.boundary.iter().flatten().enumerate() {
            if ix.node(&bd.to).is_none() {
                out.push(
                    Diagnostic::new(
                        Code::E015,
                        format!("/nodes/{i}/boundary/{b}/to"),
                        format!("boundary target '{}' does not exist", bd.to),
                    )
                    .at_node(&n.id),
                );
            }
        }
    }
}

fn start_id<'a>(ix: &Index<'a>) -> Option<&'a str> {
    ix.doc.nodes.iter().find(|n| n.kind == NodeKind::Start).map(|n| n.id.as_str())
}

fn reachable<'a>(ix: &Index<'a>, from: &'a str) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(x) = stack.pop() {
        if seen.insert(x) {
            stack.extend(ix.succ(x));
        }
    }
    seen
}

fn reachability(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    let Some(start) = start_id(ix) else { return };
    let seen = reachable(ix, start);
    for (&id, &(i, _)) in &ix.nodes {
        if !seen.contains(id) {
            out.push(
                Diagnostic::new(
                    Code::E009,
                    format!("/nodes/{i}"),
                    format!("node '{id}' is unreachable from start"),
                )
                .at_node(id),
            );
        }
    }
    if !ix.nodes.values().any(|(_, n)| n.kind == NodeKind::End && seen.contains(n.id.as_str())) {
        out.push(Diagnostic::new(Code::E010, "/nodes", "no end node is reachable from start"));
    }
}

fn is_terminate(ix: &Index<'_>, id: &str) -> bool {
    ix.node(id).is_some_and(|n| n.result == Some(EndResult::Terminated))
}

/// Block-structured parallelism (docs/11 §1.4, §11 check 5, §13.5).
fn parallel(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    let preds = ix.preds();
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut regions: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (i, n) in ix.doc.nodes.iter().enumerate().filter(|(_, n)| n.kind == NodeKind::ParallelSplit) {
        let diag = |msg: String, ptr: String| Diagnostic::new(Code::E008, ptr, msg).at_node(&n.id);
        let branches = n.branches.as_ref().map_or(0, Vec::len);
        if branches > MAX_BRANCHES {
            out.push(
                Diagnostic::new(
                    Code::E012,
                    format!("/nodes/{i}/branches"),
                    format!("{branches} branches exceeds the limit of {MAX_BRANCHES}"),
                )
                .at_node(&n.id),
            );
        }
        let Some(join) = n.join_id.as_deref() else {
            out.push(diag("parallel.split must declare joinId".to_owned(), format!("/nodes/{i}")));
            continue;
        };
        match ix.node(join) {
            Some(j) if j.kind == NodeKind::ParallelJoin => owners.entry(&j.id).or_default().push(&n.id),
            _ => {
                out.push(diag(
                    format!("joinId '{join}' is not a parallel.join"),
                    format!("/nodes/{i}/joinId"),
                ));
                continue;
            }
        }
        let mut union: BTreeSet<&str> = BTreeSet::new();
        let mut per_branch: Vec<(&str, BTreeSet<&str>)> = Vec::new();
        for e in ix.out.get(n.id.as_str()).into_iter().flatten() {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            let mut stack = vec![e.to];
            while let Some(x) = stack.pop() {
                if x == join || !seen.insert(x) {
                    continue;
                }
                if ix.node(x).is_some_and(|m| m.kind == NodeKind::End) {
                    if !is_terminate(ix, x) {
                        out.push(diag(
                            format!("branch via '{}' reaches end '{x}' without passing join '{join}'", e.to),
                            format!("/nodes/{i}"),
                        ));
                    }
                    continue;
                }
                stack.extend(ix.succ(x));
            }
            // Ends stop a path rather than belong to a branch; a shared terminate end is fine.
            seen.retain(|x| ix.node(x).is_none_or(|m| m.kind != NodeKind::End));
            for (other, region) in &per_branch {
                if let Some(shared) = region.intersection(&seen).next() {
                    out.push(diag(
                        format!(
                            "branches via '{other}' and '{}' cross at '{shared}' before join '{join}'",
                            e.to
                        ),
                        format!("/nodes/{i}"),
                    ));
                }
            }
            union.extend(seen.iter().copied());
            per_branch.push((e.to, seen));
        }
        // Nothing outside the block may enter it, and the join is entered only from inside.
        for &x in union.iter().chain(std::iter::once(&join)) {
            for &p in preds.get(x).into_iter().flatten() {
                if p != n.id && !union.contains(p) {
                    out.push(diag(
                        format!(
                            "'{x}' inside the block '{}'..'{join}' is entered from '{p}' outside it",
                            n.id
                        ),
                        format!("/nodes/{}", ix.pos(x)),
                    ));
                }
            }
        }
        regions.insert(&n.id, union);
    }
    for &split in regions.keys() {
        let depth = 1 + regions.iter().filter(|(other, r)| **other != split && r.contains(split)).count();
        if depth > MAX_NESTING {
            out.push(
                Diagnostic::new(
                    Code::E012,
                    format!("/nodes/{}", ix.pos(split)),
                    format!("parallel nesting depth {depth} exceeds the limit of {MAX_NESTING}"),
                )
                .at_node(split),
            );
        }
    }
    for (i, n) in ix.doc.nodes.iter().enumerate().filter(|(_, n)| n.kind == NodeKind::ParallelJoin) {
        let count = owners.get(n.id.as_str()).map_or(0, Vec::len);
        if count != 1 {
            out.push(
                Diagnostic::new(
                    Code::E008,
                    format!("/nodes/{i}"),
                    format!("parallel.join must be closed by exactly one split, found {count}"),
                )
                .at_node(&n.id),
            );
        }
    }
}

/// Every cycle needs a human or wait node, or `maxIterations` (docs/11 §4, §11 check 6).
fn loops(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    for scc in strongly_connected(ix) {
        let first = scc.iter().min().copied().unwrap_or_default();
        let is_cycle = scc.len() > 1 || ix.succ(first).contains(&first);
        if !is_cycle {
            continue;
        }
        let bounded = scc.iter().any(|id| {
            ix.node(id).is_some_and(|n| n.kind.is_human() || n.kind.is_wait() || n.max_iterations.is_some())
        });
        if !bounded {
            let mut members = scc.clone();
            members.sort_unstable();
            out.push(
                Diagnostic::new(
                    Code::E011,
                    format!("/nodes/{}", ix.pos(first)),
                    format!("loop through {members:?} has no human/wait node and no maxIterations"),
                )
                .at_node(first),
            );
        }
    }
}

/// Tarjan's strongly connected components, iterating nodes in id order.
fn strongly_connected<'a>(ix: &Index<'a>) -> Vec<Vec<&'a str>> {
    #[derive(Default)]
    struct St<'a> {
        index: usize,
        idx: BTreeMap<&'a str, usize>,
        low: BTreeMap<&'a str, usize>,
        stack: Vec<&'a str>,
        on: BTreeSet<&'a str>,
        out: Vec<Vec<&'a str>>,
    }
    fn visit<'a>(v: &'a str, st: &mut St<'a>, ix: &Index<'a>) {
        st.idx.insert(v, st.index);
        st.low.insert(v, st.index);
        st.index += 1;
        st.stack.push(v);
        st.on.insert(v);
        for w in ix.succ(v) {
            if !st.idx.contains_key(w) {
                visit(w, st, ix);
                let lw = st.low.get(w).copied().unwrap_or(usize::MAX);
                if let Some(lv) = st.low.get_mut(v) {
                    *lv = (*lv).min(lw);
                }
            } else if st.on.contains(w) {
                let iw = st.idx.get(w).copied().unwrap_or(usize::MAX);
                if let Some(lv) = st.low.get_mut(v) {
                    *lv = (*lv).min(iw);
                }
            }
        }
        if st.low.get(v) == st.idx.get(v) {
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
    let mut st = St::default();
    for &id in ix.nodes.keys() {
        if !st.idx.contains_key(id) {
            visit(id, &mut st, ix);
        }
    }
    st.out
}
