//! Semantic diff between two IR versions (docs/11 §11 "compiler output").
//!
//! Entities are matched by their stable ids: nodes and edges by `id`, variables by `name`, SLAs
//! by `id`, references by `<section>.<key>` (`forms.request`, `sop`), and the process header as
//! `process`. `meta` is bookkeeping and is not diffed. Each change carries the before/after value
//! so a diff can be reviewed and replayed: `diff(a, b).apply(a) == normalize(b)` (meta from `a`).

use crate::canon::normalize;
use crate::model::{IrDocument, Refs};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Process,
    Variable,
    Node,
    Edge,
    Sla,
    Ref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

/// One added, removed or changed entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub section: Section,
    pub id: String,
    pub kind: ChangeKind,
    /// Top-level members that differ (for `changed` objects), sorted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

/// The changes from one version to the next, ordered by section then id.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SemanticDiff {
    pub changes: Vec<Change>,
}

/// Why a diff could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffError {
    /// The base does not match the diff's `before` side for this entity.
    Conflict { section: Section, id: String },
    /// An `after` value is not a valid entity.
    Invalid { section: Section, id: String, reason: String },
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { section, id } => write!(f, "base does not match the diff at {section:?} '{id}'"),
            Self::Invalid { section, id, reason } => write!(f, "invalid {section:?} '{id}': {reason}"),
        }
    }
}

impl std::error::Error for DiffError {}

fn to_value<T: Serialize>(v: &T) -> Value {
    // Serialising these plain data types cannot fail; Null would only surface as a diff.
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn keyed<T: Serialize>(items: &[T], key: impl Fn(&T) -> &str) -> BTreeMap<String, Value> {
    items.iter().map(|x| (key(x).to_owned(), to_value(x))).collect()
}

fn refs_map(r: &Refs) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    for (section, map) in [
        ("forms", &r.forms),
        ("rules", &r.rules),
        ("calendars", &r.calendars),
        ("connectors", &r.connectors),
        ("documents", &r.documents),
        ("processes", &r.processes),
    ] {
        for (k, v) in map {
            m.insert(format!("{section}.{k}"), Value::String(v.clone()));
        }
    }
    for (id, v) in [("sop", &r.sop), ("policy", &r.policy)] {
        if let Some(v) = v {
            m.insert(id.to_owned(), Value::String(v.clone()));
        }
    }
    m
}

fn sections(doc: &IrDocument) -> BTreeMap<Section, BTreeMap<String, Value>> {
    BTreeMap::from([
        (Section::Process, BTreeMap::from([("process".to_owned(), to_value(&doc.process))])),
        (Section::Variable, keyed(&doc.variables, |v| &v.name)),
        (Section::Node, keyed(&doc.nodes, |n| &n.id)),
        (Section::Edge, keyed(&doc.edges, |e| &e.id)),
        (Section::Sla, keyed(&doc.sla, |s| &s.id)),
        (Section::Ref, refs_map(&doc.refs)),
    ])
}

fn changed_fields(a: &Value, b: &Value) -> Vec<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            keys.into_iter().filter(|k| x.get(*k) != y.get(*k)).cloned().collect()
        }
        _ => vec![],
    }
}

/// Computes the semantic diff from `old` to `new`.
pub fn diff(old: &IrDocument, new: &IrDocument) -> SemanticDiff {
    let (a, b) = (sections(old), sections(new));
    let mut changes = Vec::new();
    for (section, before) in &a {
        let empty = BTreeMap::new();
        let after = b.get(section).unwrap_or(&empty);
        let ids: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for id in ids {
            let (x, y) = (before.get(id), after.get(id));
            let kind = match (x, y) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Removed,
                (Some(x), Some(y)) if x != y => ChangeKind::Changed,
                _ => continue,
            };
            let fields = match (x, y) {
                (Some(x), Some(y)) => changed_fields(x, y),
                _ => vec![],
            };
            changes.push(Change {
                section: *section,
                id: id.clone(),
                kind,
                fields,
                before: x.cloned(),
                after: y.cloned(),
            });
        }
    }
    SemanticDiff { changes }
}

fn decode<T: DeserializeOwned>(section: Section, id: &str, v: Value) -> Result<T, DiffError> {
    serde_json::from_value(v).map_err(|e| DiffError::Invalid {
        section,
        id: id.to_owned(),
        reason: e.to_string(),
    })
}

impl SemanticDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Changes in one section.
    pub fn section(&self, section: Section) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(move |c| c.section == section)
    }

    /// Replays the diff on `base`, checking each `before` value, and returns the normal form.
    pub fn apply(&self, base: &IrDocument) -> Result<IrDocument, DiffError> {
        let mut s = sections(base);
        for c in &self.changes {
            let map = s.entry(c.section).or_default();
            if map.get(&c.id) != c.before.as_ref() {
                return Err(DiffError::Conflict { section: c.section, id: c.id.clone() });
            }
            match &c.after {
                Some(v) => map.insert(c.id.clone(), v.clone()),
                None => map.remove(&c.id),
            };
        }
        let mut take = |section: Section| s.remove(&section).unwrap_or_default();
        let process = take(Section::Process)
            .remove("process")
            .ok_or(DiffError::Conflict { section: Section::Process, id: "process".to_owned() })?;
        let mut out = base.clone();
        out.process = decode(Section::Process, "process", process)?;
        out.variables = collect(Section::Variable, take(Section::Variable))?;
        out.nodes = collect(Section::Node, take(Section::Node))?;
        out.edges = collect(Section::Edge, take(Section::Edge))?;
        out.sla = collect(Section::Sla, take(Section::Sla))?;
        let mut refs = Refs::default();
        for (id, v) in take(Section::Ref) {
            let value: String = decode(Section::Ref, &id, v)?;
            let _previous = match id.split_once('.') {
                Some(("forms", k)) => refs.forms.insert(k.to_owned(), value),
                Some(("rules", k)) => refs.rules.insert(k.to_owned(), value),
                Some(("calendars", k)) => refs.calendars.insert(k.to_owned(), value),
                Some(("connectors", k)) => refs.connectors.insert(k.to_owned(), value),
                Some(("documents", k)) => refs.documents.insert(k.to_owned(), value),
                Some(("processes", k)) => refs.processes.insert(k.to_owned(), value),
                _ if id == "sop" => refs.sop.replace(value),
                _ if id == "policy" => refs.policy.replace(value),
                _ => {
                    return Err(DiffError::Invalid {
                        section: Section::Ref,
                        id,
                        reason: "unknown ref id".to_owned(),
                    });
                }
            };
        }
        out.refs = refs;
        Ok(normalize(&out))
    }
}

fn collect<T: DeserializeOwned>(section: Section, map: BTreeMap<String, Value>) -> Result<Vec<T>, DiffError> {
    map.into_iter().map(|(id, v)| decode(section, &id, v)).collect()
}
