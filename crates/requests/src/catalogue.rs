//! Built-in request types (docs/19 §4.1, §4.2, §5.2, §5.5): IR documents compiled at boot into
//! pinned programs, with the display facts and form labels the request API serves.
//!
//! A built-in that does not compile, or whose form names an undeclared variable or a type the form
//! subset cannot show, fails [`Catalogue::builtin`]: the server refuses to start rather than offer a
//! broken type. Each program is pinned by checksum, so a request in flight keeps the version it
//! started on.

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_engine::{Executable, ProgramRef, StaticPrograms};
use procera_ir::model::{ScalarType, VarType};
use procera_ir::{CompileContext, compile};
use procera_runtime::Program;
use serde::Serialize;
use serde_json::Value;

use crate::forms::FieldError;

/// One form field (BPMS-101 subset): `type` is `string`, `text`, `enum`, `money`, `date`,
/// `number`, `boolean` or `principal` (a member id).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Fixed currency of a money field; `None` lets the requester choose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Form {
    pub fields: Vec<Field>,
}

/// A cross-field check run after the per-field validation.
pub type Check = fn(&BTreeMap<String, Value>) -> Vec<FieldError>;

/// A request type ready to start.
#[derive(Debug)]
pub struct RequestType {
    pub key: &'static str,
    /// Display-id prefix (`LR` → `LR-2026-000123`).
    pub prefix: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    pub program: ProgramRef,
    pub exe: Arc<Executable>,
    pub start_form: Form,
    /// Editable fields of human steps, by node id.
    pub task_forms: BTreeMap<String, Form>,
    /// The step where the requester revises a sent-back request.
    pub revise_node: Option<&'static str>,
    /// Request status by end node.
    pub end_status: BTreeMap<String, &'static str>,
    pub check: Option<Check>,
    title: fn(&BTreeMap<String, Value>) -> String,
}

impl RequestType {
    pub fn title(&self, input: &BTreeMap<String, Value>) -> String {
        let t = (self.title)(input);
        let t: String = t.chars().take(300).collect();
        if t.trim().is_empty() { self.name.to_owned() } else { t }
    }

    /// The display name of a node, or its id.
    pub fn node_name(&self, node: &str) -> String {
        self.exe
            .program
            .node(node)
            .and_then(|n| n.name.as_ref())
            .and_then(|n| n.get("en").cloned())
            .unwrap_or_else(|| node.to_owned())
    }
}

struct Def {
    key: &'static str,
    prefix: &'static str,
    name: &'static str,
    description: &'static str,
    category: &'static str,
    icon: &'static str,
    ir: &'static str,
    /// Start-form variables in order, with labels.
    labels: &'static [(&'static str, &'static str)],
    /// Human steps whose people may edit fields: node → variables. `*` = the whole start form.
    tasks: &'static [(&'static str, &'static [&'static str])],
    revise: Option<&'static str>,
    end_status: &'static [(&'static str, &'static str)],
    check: Option<Check>,
    title: fn(&BTreeMap<String, Value>) -> String,
}

fn s<'a>(v: &'a BTreeMap<String, Value>, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn money(v: &BTreeMap<String, Value>, k: &str) -> String {
    let m = v.get(k);
    let field = |f: &str| m.and_then(|m| m.get(f)).and_then(Value::as_str).unwrap_or_default().to_owned();
    format!("{} {}", field("amount"), field("currency")).trim().to_owned()
}

fn capitalised(x: &str) -> String {
    let mut c = x.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn leave_dates(v: &BTreeMap<String, Value>) -> Vec<FieldError> {
    match (v.get("start_date").and_then(Value::as_str), v.get("end_date").and_then(Value::as_str)) {
        // ISO dates compare as text.
        (Some(a), Some(b)) if b < a => {
            vec![FieldError::new("end_date", "before_start", "Must be on or after the start date")]
        }
        _ => vec![],
    }
}

const DEFS: &[Def] = &[
    Def {
        key: "hr.leave-request",
        prefix: "LR",
        name: "Leave request",
        description: "Ask for time off. Your manager approves it.",
        category: "HR",
        icon: "calendar",
        ir: include_str!("../builtin/hr.leave-request.json"),
        labels: &[
            ("leave_type", "Type of leave"),
            ("start_date", "First day"),
            ("end_date", "Last day"),
            ("reason", "Reason"),
        ],
        tasks: &[("revise", &["*"])],
        revise: Some("revise"),
        end_status: &[("end_approved", "approved"), ("end_rejected", "rejected")],
        check: Some(leave_dates),
        title: |v| {
            format!(
                "{} leave, {} to {}",
                capitalised(s(v, "leave_type")),
                s(v, "start_date"),
                s(v, "end_date")
            )
        },
    },
    Def {
        key: "hr.expense-claim",
        prefix: "EC",
        name: "Expense claim",
        description: "Claim back money you spent for work. Your manager approves it; claims over 500 EUR also go to finance.",
        category: "HR",
        icon: "receipt",
        ir: include_str!("../builtin/hr.expense-claim.json"),
        labels: &[
            ("amount", "Amount"),
            ("category", "Category"),
            ("description", "What was it for?"),
            ("expense_date", "Date of the expense"),
        ],
        tasks: &[("revise", &["*"])],
        revise: Some("revise"),
        end_status: &[("end_approved", "approved"), ("end_rejected", "rejected")],
        check: None,
        title: |v| format!("{} expense, {}", capitalised(s(v, "category")), money(v, "amount")),
    },
    Def {
        key: "fin.purchase-requisition",
        prefix: "PR",
        name: "Purchase requisition",
        description: "Ask to buy goods or services. Your manager approves; finance above 50,000 INR and the CFO above 500,000 INR.",
        category: "Finance",
        icon: "cart",
        ir: include_str!("../builtin/fin.purchase-requisition.json"),
        labels: &[
            ("amount", "Total amount"),
            ("vendor", "Vendor"),
            ("cost_center", "Cost centre"),
            ("justification", "Justification"),
        ],
        tasks: &[("revise", &["*"])],
        revise: Some("revise"),
        end_status: &[("end_approved", "approved"), ("end_rejected", "rejected")],
        check: None,
        title: |v| format!("{}, {}", s(v, "vendor"), money(v, "amount")),
    },
    Def {
        key: "it.access-request",
        prefix: "AR",
        name: "Access request",
        description: "Ask for access to a system. The person's manager approves; sensitive systems need the system owner and security.",
        category: "IT",
        icon: "key",
        ir: include_str!("../builtin/it.access-request.json"),
        labels: &[
            ("system", "System"),
            ("access_role", "Access level"),
            ("sensitivity", "Sensitivity"),
            ("beneficiary", "Who needs access?"),
            ("justification", "Why is it needed?"),
        ],
        tasks: &[("revise", &["*"]), ("provision", &["ticket_ref"])],
        revise: Some("revise"),
        end_status: &[("end_granted", "approved"), ("end_rejected", "rejected")],
        check: None,
        title: |v| format!("{} on {}", s(v, "access_role"), s(v, "system")),
    },
];

/// Labels of fields that only appear on a step form.
const TASK_LABELS: &[(&str, &str)] = &[("ticket_ref", "Ticket reference")];

fn field(program: &Program, name: &str, label: &str) -> Result<Field, String> {
    let ir = program.ir();
    let v = ir
        .variables
        .iter()
        .find(|v| v.name == name)
        .ok_or_else(|| format!("{}: form field {name} is not a declared variable", ir.process.key))?;
    let (ty, options, currency) = match &v.ty {
        VarType::Scalar(ScalarType::String) => ("string", None, None),
        VarType::Scalar(ScalarType::Text) => ("text", None, None),
        VarType::Scalar(ScalarType::Integer | ScalarType::Decimal) => ("number", None, None),
        VarType::Scalar(ScalarType::Boolean) => ("boolean", None, None),
        VarType::Scalar(ScalarType::Date) => ("date", None, None),
        VarType::Scalar(ScalarType::Principal) => ("principal", None, None),
        VarType::Enum { values } => ("enum", Some(values.clone()), None),
        VarType::Money { money } => ("money", None, money.currency.clone()),
        other => {
            return Err(format!("{}: field {name} has a type forms cannot show: {other:?}", ir.process.key));
        }
    };
    Ok(Field {
        name: name.to_owned(),
        label: label.to_owned(),
        ty,
        required: v.required == Some(true),
        options,
        currency,
    })
}

/// The built-in request types and the programs they run.
#[derive(Debug)]
pub struct Catalogue {
    types: Vec<Arc<RequestType>>,
    programs: Arc<StaticPrograms>,
}

impl Catalogue {
    /// Compiles every built-in. Any failure is fatal at boot.
    pub fn builtin() -> Result<Self, String> {
        let mut programs = StaticPrograms::default();
        let mut types = Vec::new();
        for d in DEFS {
            let doc: Value = serde_json::from_str(d.ir).map_err(|e| format!("{}: {e}", d.key))?;
            let out = compile(&doc, &CompileContext::default());
            let compiled =
                out.compiled.ok_or_else(|| format!("{} does not compile: {:?}", d.key, out.diagnostics))?;
            let program = Program::from_compiled(&compiled).map_err(|e| format!("{}: {e}", d.key))?;
            let start_form = Form {
                fields: d.labels.iter().map(|(n, l)| field(&program, n, l)).collect::<Result<_, _>>()?,
            };
            let mut task_forms = BTreeMap::new();
            for (node, names) in d.tasks {
                if program.node(node).is_none() {
                    return Err(format!("{}: form for unknown step {node}", d.key));
                }
                let form = if names == &["*"] {
                    start_form.clone()
                } else {
                    let label = |n: &'static str| -> &'static str {
                        TASK_LABELS.iter().find(|(k, _)| *k == n).map_or(n, |(_, l)| *l)
                    };
                    Form {
                        fields: names
                            .iter()
                            .map(|n| field(&program, n, label(n)))
                            .collect::<Result<_, _>>()?,
                    }
                };
                task_forms.insert((*node).to_owned(), form);
            }
            let ends: Vec<&str> = program
                .ir()
                .nodes
                .iter()
                .filter(|n| n.kind == procera_ir::model::NodeKind::End)
                .map(|n| n.id.as_str())
                .collect();
            if let Some(e) = ends.iter().find(|e| !d.end_status.iter().any(|(k, _)| k == *e)) {
                return Err(format!("{}: end node {e} has no request status", d.key));
            }
            let pinned = programs.insert_pinned(Executable::new(program));
            let exe = programs
                .0
                .get(&(pinned.process_key.clone(), pinned.version))
                .cloned()
                .ok_or("lost program")?;
            types.push(Arc::new(RequestType {
                key: d.key,
                prefix: d.prefix,
                name: d.name,
                description: d.description,
                category: d.category,
                icon: d.icon,
                program: pinned,
                exe,
                start_form,
                task_forms,
                revise_node: d.revise,
                end_status: d.end_status.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
                check: d.check,
                title: d.title,
            }));
        }
        Ok(Self { types, programs: Arc::new(programs) })
    }

    pub fn list(&self) -> &[Arc<RequestType>] {
        &self.types
    }

    pub fn get(&self, key: &str) -> Option<&Arc<RequestType>> {
        self.types.iter().find(|t| t.key == key)
    }

    /// The program source for the engine.
    pub fn programs(&self) -> Arc<StaticPrograms> {
        self.programs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_compiles_and_pins_its_checksum() {
        let c = Catalogue::builtin().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.list().len(), 4);
        for t in c.list() {
            assert!(t.program.checksum.is_some(), "{} is pinned", t.key);
            assert!(!t.start_form.fields.is_empty());
            assert_eq!(t.exe.program.default_calendar(), Some(crate::DEFAULT_CALENDAR));
        }
        let leave = c.get("hr.leave-request").unwrap_or_else(|| panic!("leave"));
        let kinds: Vec<&str> = leave.start_form.fields.iter().map(|f| f.ty).collect();
        assert_eq!(kinds, ["enum", "date", "date", "text"]);
        assert!(!leave.start_form.fields[3].required);
        let pr = c.get("fin.purchase-requisition").unwrap_or_else(|| panic!("pr"));
        assert_eq!(pr.start_form.fields[0].currency.as_deref(), Some("INR"));
        let ar = c.get("it.access-request").unwrap_or_else(|| panic!("ar"));
        assert_eq!(ar.task_forms["provision"].fields[0].name, "ticket_ref");
    }
}
