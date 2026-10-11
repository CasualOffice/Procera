//! A compiled IR indexed for execution.

use procera_ir::model::{IrDocument, Node, NodeKind};
use procera_ir::{COMPILER_VERSION, Compiled};
use std::collections::{BTreeMap, BTreeSet};

/// Why an IR document cannot be executed. A document that compiled cleanly never fails these
/// checks; they guard against a hand-built or corrupted document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProgramError {
    #[error("unsupported schemaVersion {0}")]
    SchemaVersion(u32),
    #[error("the process needs exactly one start node, found {0}")]
    Start(usize),
    #[error("duplicate node id {0}")]
    DuplicateNode(String),
    #[error("edge {edge} references unknown node {node}")]
    UnknownNode { edge: String, node: String },
    #[error("outcome {outcome} of node {node} is handled by more than one edge")]
    AmbiguousOutcome { node: String, outcome: String },
    #[error("split {split} has no matching join")]
    Join { split: String },
}

/// The executable form of one published process version: the normal-form IR plus lookups.
/// Build it once per release bundle and share it across instances.
#[derive(Debug, Clone)]
pub struct Program {
    ir: IrDocument,
    checksum: Option<String>,
    nodes: BTreeMap<String, usize>,
    edges: BTreeMap<(String, String), String>,
    start: String,
}

impl Program {
    /// From a successful compile (the normal path).
    pub fn from_compiled(compiled: &Compiled) -> Result<Self, ProgramError> {
        let mut p = Self::new(compiled.ir.clone())?;
        p.checksum = Some(compiled.checksum.clone());
        Ok(p)
    }

    /// From an IR document. It should be the compiler's normal form; this re-checks only what
    /// execution relies on (one start, endpoints exist, one edge per outcome, joins exist).
    pub fn new(ir: IrDocument) -> Result<Self, ProgramError> {
        if ir.schema_version != procera_ir::model::SCHEMA_VERSION {
            return Err(ProgramError::SchemaVersion(ir.schema_version));
        }
        let mut nodes = BTreeMap::new();
        for (i, n) in ir.nodes.iter().enumerate() {
            if nodes.insert(n.id.clone(), i).is_some() {
                return Err(ProgramError::DuplicateNode(n.id.clone()));
            }
        }
        let starts: Vec<&Node> = ir.nodes.iter().filter(|n| n.kind == NodeKind::Start).collect();
        let start = match starts.as_slice() {
            [one] => one.id.clone(),
            other => return Err(ProgramError::Start(other.len())),
        };
        let mut edges = BTreeMap::new();
        for e in &ir.edges {
            for end in [&e.from, &e.to] {
                if !nodes.contains_key(end) {
                    return Err(ProgramError::UnknownNode { edge: e.id.clone(), node: end.clone() });
                }
            }
            if edges.insert((e.from.clone(), e.on.clone()), e.to.clone()).is_some() {
                return Err(ProgramError::AmbiguousOutcome { node: e.from.clone(), outcome: e.on.clone() });
            }
        }
        let joins: BTreeSet<&str> =
            ir.nodes.iter().filter(|n| n.kind == NodeKind::ParallelJoin).map(|n| n.id.as_str()).collect();
        for n in ir.nodes.iter().filter(|n| n.kind == NodeKind::ParallelSplit) {
            if !n.join_id.as_deref().is_some_and(|j| joins.contains(j)) {
                return Err(ProgramError::Join { split: n.id.clone() });
            }
        }
        Ok(Self { ir, checksum: None, nodes, edges, start })
    }

    pub fn ir(&self) -> &IrDocument {
        &self.ir
    }

    /// The compiled checksum, when built from a [`Compiled`].
    pub fn checksum(&self) -> Option<&str> {
        self.checksum.as_deref()
    }

    /// The compiler version the IR was normalised by, or this build's when absent.
    pub fn compiler_version(&self) -> &str {
        self.ir.meta.compiler_version.as_deref().unwrap_or(COMPILER_VERSION)
    }

    pub fn start(&self) -> &str {
        &self.start
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id).and_then(|i| self.ir.nodes.get(*i))
    }

    /// The target of the edge leaving `from` on `outcome`.
    pub fn target(&self, from: &str, outcome: &str) -> Option<&str> {
        self.edges.get(&(from.to_owned(), outcome.to_owned())).map(String::as_str)
    }

    /// Whether `name` is a declared variable.
    pub fn declares(&self, name: &str) -> bool {
        self.ir.variables.iter().any(|v| v.name == name)
    }

    /// The default business calendar (`refs.calendars.default`).
    pub fn default_calendar(&self) -> Option<&str> {
        self.ir.refs.calendars.get("default").map(String::as_str)
    }
}
