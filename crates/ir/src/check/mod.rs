//! The docs/11 §11 checks over a parsed document. Pointers refer to source positions.

mod graph;

use crate::diag::Diagnostic;
use crate::env::Environment;
use crate::expr::ExpressionChecker;
use crate::model::{IrDocument, Node};
use std::collections::BTreeMap;

/// An outgoing edge whose endpoints both exist.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OutEdge<'a> {
    pub index: usize,
    pub to: &'a str,
    pub on: &'a str,
}

/// Lookup tables shared by the checks. Duplicated ids keep their first occurrence.
#[derive(Debug)]
pub(crate) struct Index<'a> {
    pub doc: &'a IrDocument,
    pub nodes: BTreeMap<&'a str, (usize, &'a Node)>,
    pub out: BTreeMap<&'a str, Vec<OutEdge<'a>>>,
    /// Boundary targets that exist, per source node.
    pub boundary: BTreeMap<&'a str, Vec<&'a str>>,
}

impl<'a> Index<'a> {
    fn new(doc: &'a IrDocument) -> Self {
        let mut nodes = BTreeMap::new();
        for (i, n) in doc.nodes.iter().enumerate() {
            nodes.entry(n.id.as_str()).or_insert((i, n));
        }
        let mut out: BTreeMap<&str, Vec<OutEdge<'_>>> = BTreeMap::new();
        for (i, e) in doc.edges.iter().enumerate() {
            if nodes.contains_key(e.from.as_str()) && nodes.contains_key(e.to.as_str()) {
                out.entry(e.from.as_str()).or_default().push(OutEdge { index: i, to: &e.to, on: &e.on });
            }
        }
        let mut boundary: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for n in &doc.nodes {
            for b in n.boundary.iter().flatten() {
                if nodes.contains_key(b.to.as_str()) {
                    boundary.entry(n.id.as_str()).or_default().push(b.to.as_str());
                }
            }
        }
        Self { doc, nodes, out, boundary }
    }

    pub fn node(&self, id: &str) -> Option<&'a Node> {
        self.nodes.get(id).map(|(_, n)| *n)
    }

    pub fn pos(&self, id: &str) -> usize {
        self.nodes.get(id).map_or(0, |(i, _)| *i)
    }

    /// Successors through edges and boundary handlers, in a stable order.
    pub fn succ(&self, id: &str) -> Vec<&'a str> {
        let mut v: Vec<&str> = self.out.get(id).into_iter().flatten().map(|e| e.to).collect();
        v.extend(self.boundary.get(id).into_iter().flatten().copied());
        v
    }

    /// Predecessors of every node (edges and boundary handlers).
    pub fn preds(&self) -> BTreeMap<&'a str, Vec<&'a str>> {
        let mut p: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for &id in self.nodes.keys() {
            for s in self.succ(id) {
                p.entry(s).or_default().push(id);
            }
        }
        p
    }
}

/// Runs every check and returns the findings, unsorted.
pub(crate) fn run(
    doc: &IrDocument,
    expressions: &dyn ExpressionChecker,
    env: Option<&dyn Environment>,
) -> Vec<Diagnostic> {
    let ix = Index::new(doc);
    let mut out = Vec::new();
    graph::check(&ix, &mut out);
    let _ = (expressions, env); // semantic checks land in the next slice
    out
}
