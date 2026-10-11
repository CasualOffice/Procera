//! IR v1 data model (docs/11 §2-§9, §13; shape in `schemas/ir/v1.schema.json`).
//!
//! The structs mirror the JSON Schema one to one. Objects the schema closes
//! (`additionalProperties: false`) deny unknown fields; the few it leaves open (`meta`, a money
//! type, an `n_of_m` quorum, an SLA breach action) keep extra members verbatim so a document
//! round-trips without silent loss. Expressions, durations and artifact references stay typed
//! source strings: the expression engine (BPMS-081) parses them, the compiler only inspects them.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The only schema version this crate reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Localised text: language tag (`en`, `en-GB`) to string.
pub type I18n = BTreeMap<String, String>;

/// A whole IR v1 document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IrDocument {
    pub schema_version: u32,
    pub process: Process,
    pub variables: Vec<Variable>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sla: Vec<Sla>,
    #[serde(default, skip_serializing_if = "Refs::is_empty")]
    pub refs: Refs,
    #[serde(default, skip_serializing_if = "Meta::is_empty")]
    pub meta: Meta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Process {
    pub key: String,
    pub version: u32,
    pub name: I18n,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criticality: Option<Criticality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Criticality {
    Standard,
    /// Forbids automatic outcomes from escalations (docs/11 §3.2).
    Controlled,
}

/// A process variable (docs/11 §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: VarType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

impl Variable {
    /// `restricted` values never appear in events, logs, notifications or search (docs/11 §5).
    pub fn is_restricted(&self) -> bool {
        self.classification == Some(Classification::Restricted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    Internal,
    Confidential,
    Restricted,
}

/// Variable types (docs/11 §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum VarType {
    Scalar(ScalarType),
    Money {
        money: MoneySpec,
    },
    Enum {
        #[serde(rename = "enum")]
        values: Vec<String>,
    },
    List {
        list: Box<VarType>,
    },
    Record {
        record: BTreeMap<String, VarType>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    String,
    Text,
    Integer,
    Decimal,
    Boolean,
    Date,
    Datetime,
    Duration,
    Principal,
    Group,
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoneySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A node (docs/11 §3). One flat struct, as in the schema: which fields apply depends on `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<I18n>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcomes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unhandled: Option<Unhandled>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<EndResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quorum: Option<Quorum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<Due>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalations: Option<Vec<Escalation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Boundary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Vec<Branch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<BTreeMap<String, Expr>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<Channel>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_policy: Option<VersionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_for_close: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assign: Option<BTreeMap<String, Expr>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signers: Option<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<SignOrder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiz: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
}

/// Node kinds (docs/11 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "end")]
    End,
    #[serde(rename = "human.task")]
    HumanTask,
    #[serde(rename = "human.approval")]
    HumanApproval,
    #[serde(rename = "human.sign")]
    HumanSign,
    #[serde(rename = "human.acknowledge")]
    HumanAcknowledge,
    #[serde(rename = "decision")]
    Decision,
    #[serde(rename = "parallel.split")]
    ParallelSplit,
    #[serde(rename = "parallel.join")]
    ParallelJoin,
    #[serde(rename = "wait.timer")]
    WaitTimer,
    #[serde(rename = "wait.signal")]
    WaitSignal,
    #[serde(rename = "service.call")]
    ServiceCall,
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "subprocess.call")]
    SubprocessCall,
    #[serde(rename = "case.open")]
    CaseOpen,
    #[serde(rename = "set")]
    Set,
}

impl NodeKind {
    /// The wire name, e.g. `human.task`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::HumanTask => "human.task",
            Self::HumanApproval => "human.approval",
            Self::HumanSign => "human.sign",
            Self::HumanAcknowledge => "human.acknowledge",
            Self::Decision => "decision",
            Self::ParallelSplit => "parallel.split",
            Self::ParallelJoin => "parallel.join",
            Self::WaitTimer => "wait.timer",
            Self::WaitSignal => "wait.signal",
            Self::ServiceCall => "service.call",
            Self::Notify => "notify",
            Self::SubprocessCall => "subprocess.call",
            Self::CaseOpen => "case.open",
            Self::Set => "set",
        }
    }

    pub fn is_human(self) -> bool {
        matches!(self, Self::HumanTask | Self::HumanApproval | Self::HumanSign | Self::HumanAcknowledge)
    }

    pub fn is_wait(self) -> bool {
        matches!(self, Self::WaitTimer | Self::WaitSignal)
    }

    /// Kinds that may declare `boundary` handlers (docs/11 §4.1).
    pub fn allows_boundary(self) -> bool {
        self.is_human() || self.is_wait() || matches!(self, Self::ServiceCall | Self::SubprocessCall)
    }
}

impl Node {
    /// Outcomes this node can produce (docs/11 §3, §13.4). Decisions include `default`.
    pub fn outcomes(&self) -> Vec<String> {
        let fixed = |xs: &[&str]| xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let branch_names = || self.branches.iter().flatten().map(|b| b.name.clone()).collect::<Vec<_>>();
        let custom = self.outcomes.clone().unwrap_or_default();
        let mut out = match self.kind {
            NodeKind::End => vec![],
            NodeKind::HumanTask if !custom.is_empty() => custom,
            NodeKind::HumanTask => fixed(&["submitted"]),
            NodeKind::HumanApproval => {
                let mut v = fixed(&["approved", "rejected", "changes_requested"]);
                v.extend(custom);
                v
            }
            NodeKind::HumanSign => fixed(&["signed", "declined", "expired"]),
            NodeKind::HumanAcknowledge => fixed(&["acknowledged", "expired"]),
            NodeKind::Decision => {
                let mut v = branch_names();
                v.push("default".to_owned());
                v
            }
            NodeKind::ParallelSplit => branch_names(),
            NodeKind::WaitTimer => fixed(&["fired"]),
            NodeKind::WaitSignal if self.timeout.is_some() => fixed(&["received", "timeout"]),
            NodeKind::WaitSignal => fixed(&["received"]),
            NodeKind::ServiceCall => fixed(&["ok", "error"]),
            NodeKind::SubprocessCall => fixed(&["completed", "failed", "cancelled"]),
            NodeKind::CaseOpen if self.wait_for_close == Some(true) => fixed(&["closed"]),
            NodeKind::CaseOpen => fixed(&["opened"]),
            NodeKind::Start | NodeKind::Notify | NodeKind::Set | NodeKind::ParallelJoin => fixed(&["next"]),
        };
        out.sort();
        out.dedup();
        out
    }

    /// The assignment-shaped fields of this node with their JSON member names.
    pub fn assignments(&self) -> Vec<(&'static str, &Assignment)> {
        [("assignment", &self.assignment), ("to", &self.to), ("signers", &self.signers)]
            .into_iter()
            .filter_map(|(k, a)| a.as_ref().map(|a| (k, a)))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unhandled {
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Api,
    Schedule,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndResult {
    Completed,
    /// Cancels every sibling token (docs/11 §3, §13.5).
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signature {
    None,
    Simple,
    Strong,
    Part11,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Quorum {
    Mode(QuorumMode),
    NOfM(QuorumN),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuorumMode {
    One,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuorumN {
    pub n: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Assignment (docs/11 §3.1); also the shape of `notify.to` and `human.sign.signers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<Strategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_initiator: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_actors_of: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    #[serde(rename = "type")]
    pub ty: CandidateType,
    #[serde(rename = "ref")]
    pub reference: CandidateRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateType {
    User,
    Group,
    Role,
    Position,
    DirectoryRule,
}

impl CandidateType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Group => "group",
            Self::Role => "role",
            Self::Position => "position",
            Self::DirectoryRule => "directory_rule",
        }
    }
}

/// A candidate reference: an expression (`expr:<lang>: …`) or a directory name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateRef(pub String);

impl CandidateRef {
    /// `Some` when the reference is written as an expression (it may still be malformed).
    pub fn as_expr(&self) -> Option<Expr> {
        self.0.starts_with("expr:").then(|| Expr(self.0.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Claim,
    RoundRobin,
    LeastLoaded,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Due {
    pub after: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<bool>,
}

/// Escalation step (docs/11 §3.2, §13.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Escalation {
    pub at: Duration,
    pub action: EscalationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Candidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationAction {
    Remind,
    Reassign,
    Outcome,
    Notify,
}

/// Boundary handler (docs/11 §4.1). `on` is `timeout`, `cancel` or `error:<CODE>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupting: Option<bool>,
    pub to: String,
}

/// A decision or parallel branch. Decision branches without a rule carry `when`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    InApp,
    Email,
    Chat,
    Webhook,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionPolicy {
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignOrder {
    Sequential,
    Parallel,
}

fn default_on() -> String {
    "next".to_owned()
}

/// An edge (docs/11 §4). A missing `on` normalises to `next` (§13.11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default = "default_on")]
    pub on: String,
}

/// SLA definition (docs/11 §9). `scope` is `instance` or `node:<id>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sla {
    pub id: String,
    pub scope: String,
    pub target: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_while: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breach: Option<Vec<Breach>>,
}

impl Sla {
    /// The node id when the SLA is node-scoped.
    pub fn scope_node(&self) -> Option<&str> {
        self.scope.strip_prefix("node:")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Breach {
    pub action: BreachAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreachAction {
    Notify,
    Escalate,
}

/// Versioned artifact reference, e.g. `form:purchase-request@4` (docs/11 §2).
pub type ArtifactRef = String;

/// Named references resolved into the release bundle at publish (docs/11 §2, §13.8).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refs {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub forms: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub calendars: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connectors: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub documents: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub processes: BTreeMap<String, ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sop: Option<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ArtifactRef>,
}

impl Refs {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Every reference as `(json pointer below /refs, reference)`, in a stable order.
    pub fn entries(&self) -> Vec<(String, &ArtifactRef)> {
        let maps = [
            ("forms", &self.forms),
            ("rules", &self.rules),
            ("calendars", &self.calendars),
            ("connectors", &self.connectors),
            ("documents", &self.documents),
            ("processes", &self.processes),
        ];
        let mut out = Vec::new();
        for (section, map) in maps {
            for (k, v) in map {
                out.push((format!("/{section}/{}", crate::diag::escape_pointer(k)), v));
            }
        }
        out.extend(self.sop.iter().map(|v| ("/sop".to_owned(), v)));
        out.extend(self.policy.iter().map(|v| ("/policy".to_owned(), v)));
        out
    }
}

/// Compiler metadata. Open object: unknown members are kept verbatim.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_ref: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Expression language tag (docs/11 §6, ADR-0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExprLang {
    Feel,
    Cel,
}

/// An expression stored as source text with its language tag: `expr:<lang>: <body>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Expr(pub String);

impl Expr {
    /// Splits `expr:<lang>: <body>`. `None` when the tag or language is not recognised.
    pub fn parse(&self) -> Option<(ExprLang, &str)> {
        let rest = self.0.strip_prefix("expr:")?;
        let (lang, body) = rest.split_once(": ")?;
        let lang = match lang {
            "feel" => ExprLang::Feel,
            "cel" => ExprLang::Cel,
            _ => return None,
        };
        Some((lang, body))
    }
}

/// An ISO-8601 duration in the v1 subset: weeks, days, hours, minutes, seconds (docs/11 §13.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Duration(pub String);

impl Duration {
    /// Total length in seconds, or `None` when the text is outside the v1 subset.
    pub fn seconds(&self) -> Option<u64> {
        let rest = self.0.strip_prefix('P')?;
        if rest.is_empty() {
            return None;
        }
        let (date, time) = match rest.split_once('T') {
            Some((_, "")) => return None,
            Some((d, t)) => (d, Some(t)),
            None => (rest, None),
        };
        let mut total: u64 = 0;
        let mut add = |part: &str, units: &[(char, u64)]| -> Option<()> {
            let mut num = String::new();
            let mut idx = 0;
            for c in part.chars() {
                if c.is_ascii_digit() {
                    num.push(c);
                    continue;
                }
                // Units must appear once each, in order.
                let pos = units.iter().skip(idx).position(|(u, _)| *u == c)? + idx;
                let n: u64 = num.parse().ok()?;
                total = total.checked_add(n.checked_mul(units[pos].1)?)?;
                num.clear();
                idx = pos + 1;
            }
            num.is_empty().then_some(())
        };
        add(date, &[('W', 604_800), ('D', 86_400)])?;
        if let Some(t) = time {
            add(t, &[('H', 3_600), ('M', 60), ('S', 1)])?;
        }
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_subset() {
        let s = |x: &str| Duration(x.to_owned()).seconds();
        assert_eq!(s("P2D"), Some(172_800));
        assert_eq!(s("P1W1DT1H30M5S"), Some(604_800 + 86_400 + 5_405));
        assert_eq!(s("PT0S"), Some(0));
        for bad in ["P", "PT", "P1M", "P1Y", "PT1D", "P1D1W", "1D", "P1DT"] {
            assert_eq!(s(bad), None, "{bad}");
        }
    }

    #[test]
    fn expression_tags() {
        assert_eq!(Expr("expr:cel: a > 1".into()).parse(), Some((ExprLang::Cel, "a > 1")));
        assert_eq!(Expr("expr:feel:  x".into()).parse(), Some((ExprLang::Feel, " x")));
        assert_eq!(Expr("expr:js: x".into()).parse(), None);
        assert_eq!(Expr("amount".into()).parse(), None);
    }
}
