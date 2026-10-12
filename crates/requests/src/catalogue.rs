//! Built-in request types (docs/19 §4.1, §4.2, §5.2, §5.5): IR documents compiled at boot into
//! pinned programs, with the display facts and form labels the request API serves.
//!
//! A built-in that does not compile, or whose form names an undeclared variable or a type the form
//! subset cannot show, fails [`Catalogue::builtin`]: the server refuses to start rather than offer a
//! broken type. Each program is pinned by checksum, so a request in flight keeps the version it
//! started on.
//!
//! A type with a money field is compiled once per supported currency (`requests.currency`,
//! [`procera_settings::catalogue::CURRENCIES`]): the IR declares the currency as part of the money
//! type, while approval thresholds stay plain numbers that read in the workspace's currency. The
//! variants share key and version and differ by checksum, which [`Programs`] loads by.

use std::collections::BTreeMap;
use std::sync::Arc;

use procera_db::WsCtx;
use procera_engine::{BoxFuture, Executable, ProgramRef, ProgramSource};
use procera_ir::model::{ScalarType, VarType};
use procera_ir::{CompileContext, compile};
use procera_runtime::Program;
use serde::Serialize;
use serde_json::Value;

use crate::forms::FieldError;

/// One form field (BPMS-101 subset): `type` is `string`, `text`, `enum`, `money`, `date`,
/// `number`, `boolean`, `principal` (a member id) or `file` (uploaded file ids; `multiple` for a list).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub required: bool,
    /// Allowed values; sent as `[{value, label}]` with a readable label ("unpaid" → "Unpaid").
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "options_with_labels")]
    pub options: Option<Vec<String>>,
    /// Currency of a money field: the workspace's `requests.currency` when served.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// A `file` field that takes several files.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub multiple: bool,
}

fn options_with_labels<S: serde::Serializer>(opts: &Option<Vec<String>>, ser: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let opts = opts.as_deref().unwrap_or_default();
    let mut seq = ser.serialize_seq(Some(opts.len()))?;
    for o in opts {
        let words = o.replace(['_', '-'], " ");
        let mut c = words.chars();
        let label = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        seq.serialize_element(&serde_json::json!({ "value": o, "label": label }))?;
    }
    seq.end()
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
    /// `{currency}` stands for the workspace currency; see [`RequestType::description`].
    description: &'static str,
    pub category: &'static str,
    pub icon: &'static str,
    /// The program as authored (its IR currency); see [`RequestType::program_for`].
    pub program: ProgramRef,
    pub exe: Arc<Executable>,
    /// The currency the IR declares for its money fields, when it has any.
    pub money: Option<String>,
    /// The program per currency, for types with money fields.
    variants: BTreeMap<String, ProgramRef>,
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

    /// The description, with thresholds read in `currency`.
    pub fn description(&self, currency: &str) -> String {
        self.description.replace("{currency}", currency)
    }

    /// The program that prices money in `currency` (the authored one for types without money).
    pub fn program_for(&self, currency: &str) -> Option<&ProgramRef> {
        match &self.money {
            None => Some(&self.program),
            Some(_) => self.variants.get(currency),
        }
    }

    /// A form with its money fields in `currency`.
    pub fn priced(form: &Form, currency: &str) -> Form {
        let mut f = form.clone();
        for field in f.fields.iter_mut().filter(|x| x.ty == "money") {
            field.currency = Some(currency.to_owned());
        }
        f
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
    /// Optional file fields added to the authored IR (a list of files each), after `labels`.
    files: &'static [(&'static str, &'static str)],
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
        files: &[("supporting_document", "Supporting document"), ("attachments", "Attachments")],
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
        description: "Claim back money you spent for work. Your manager approves it; claims over 500 {currency} also go to finance.",
        category: "HR",
        icon: "receipt",
        ir: include_str!("../builtin/hr.expense-claim.json"),
        labels: &[
            ("amount", "Amount"),
            ("category", "Category"),
            ("description", "What was it for?"),
            ("expense_date", "Date of the expense"),
        ],
        files: &[("receipts", "Receipts"), ("attachments", "Attachments")],
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
        description: "Ask to buy goods or services. Your manager approves; finance above 5,000 {currency} and the CFO above 50,000 {currency}.",
        category: "Finance",
        icon: "cart",
        ir: include_str!("../builtin/fin.purchase-requisition.json"),
        labels: &[
            ("amount", "Total amount"),
            ("vendor", "Vendor"),
            ("cost_center", "Cost centre"),
            ("justification", "Justification"),
        ],
        files: &[("quote", "Quote"), ("attachments", "Attachments")],
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
        files: &[("attachments", "Attachments")],
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
    let mut multiple = false;
    let (ty, options, currency) = match &v.ty {
        VarType::Scalar(ScalarType::String) => ("string", None, None),
        VarType::Scalar(ScalarType::Text) => ("text", None, None),
        VarType::Scalar(ScalarType::Integer | ScalarType::Decimal) => ("number", None, None),
        VarType::Scalar(ScalarType::Boolean) => ("boolean", None, None),
        VarType::Scalar(ScalarType::Date) => ("date", None, None),
        VarType::Scalar(ScalarType::Principal) => ("principal", None, None),
        VarType::Enum { values } => ("enum", Some(values.clone()), None),
        VarType::Money { money } => ("money", None, money.currency.clone()),
        VarType::Scalar(ScalarType::File) => ("file", None, None),
        VarType::List { list } if **list == VarType::Scalar(ScalarType::File) => {
            multiple = true;
            ("file", None, None)
        }
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
        multiple,
    })
}

/// Every compiled program, by key and version; several builds of one key and version (currency
/// variants) are told apart by checksum. A reference without a checksum gets the authored build.
#[derive(Debug, Default)]
pub struct Programs(BTreeMap<(String, u32), Vec<Arc<Executable>>>);

impl Programs {
    fn insert(&mut self, exe: Executable) -> (ProgramRef, Arc<Executable>) {
        let ir = exe.program.ir();
        let r = ProgramRef {
            process_key: ir.process.key.clone(),
            version: ir.process.version,
            checksum: exe.program.checksum().map(str::to_owned),
        };
        let exe = Arc::new(exe);
        self.0.entry((r.process_key.clone(), r.version)).or_default().push(exe.clone());
        (r, exe)
    }
}

impl ProgramSource for Programs {
    fn load<'a>(&'a self, _ctx: WsCtx, p: &'a ProgramRef) -> BoxFuture<'a, Result<Arc<Executable>, String>> {
        let builds = self.0.get(&(p.process_key.clone(), p.version));
        let found = match &p.checksum {
            None => builds.and_then(|b| b.first()).cloned(),
            Some(want) => {
                builds.and_then(|b| b.iter().find(|e| e.program.checksum() == Some(want.as_str()))).cloned()
            }
        };
        Box::pin(async move {
            found.ok_or_else(|| {
                format!("program {}@{} ({:?}) not found", p.process_key, p.version, p.checksum)
            })
        })
    }
}

/// The IR currency of the first money variable, if any.
fn money_currency(doc: &Value) -> Option<String> {
    doc["variables"]
        .as_array()?
        .iter()
        .find_map(|v| v["type"]["money"]["currency"].as_str().map(str::to_owned))
}

/// The document with an optional list-of-files variable for each of `files` (A-28: internal).
fn with_files(doc: &Value, files: &[(&str, &str)]) -> Value {
    let mut d = doc.clone();
    if let Some(vars) = d["variables"].as_array_mut() {
        for (name, _) in files {
            vars.push(
                serde_json::json!({ "name": name, "type": { "list": "file" }, "classification": "internal" }),
            );
        }
    }
    d
}

/// The document with every money variable priced in `currency`.
fn in_currency(doc: &Value, currency: &str) -> Value {
    let mut d = doc.clone();
    if let Some(vars) = d["variables"].as_array_mut() {
        for v in vars.iter_mut().filter(|v| v["type"]["money"].is_object()) {
            v["type"]["money"]["currency"] = Value::String(currency.to_owned());
        }
    }
    d
}

fn build(key: &str, doc: &Value) -> Result<Program, String> {
    let out = compile(doc, &CompileContext::default());
    let compiled = out.compiled.ok_or_else(|| format!("{key} does not compile: {:?}", out.diagnostics))?;
    Program::from_compiled(&compiled).map_err(|e| format!("{key}: {e}"))
}

/// The built-in request types and the programs they run.
#[derive(Debug)]
pub struct Catalogue {
    types: Vec<Arc<RequestType>>,
    programs: Arc<Programs>,
}

impl Catalogue {
    /// Compiles every built-in. Any failure is fatal at boot.
    pub fn builtin() -> Result<Self, String> {
        let mut programs = Programs::default();
        let mut types = Vec::new();
        for d in DEFS {
            let authored: Value = serde_json::from_str(d.ir).map_err(|e| format!("{}: {e}", d.key))?;
            let doc = with_files(&authored, d.files);
            let program = build(d.key, &doc)?;
            let start_form = Form {
                fields: d
                    .labels
                    .iter()
                    .chain(d.files)
                    .map(|(n, l)| field(&program, n, l))
                    .collect::<Result<_, _>>()?,
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
            let money = money_currency(&doc);
            let (pinned, exe) = programs.insert(Executable::new(program));
            let mut variants = BTreeMap::new();
            if let Some(authored) = &money {
                for cur in procera_settings::catalogue::CURRENCIES {
                    let r = if cur == authored {
                        pinned.clone()
                    } else {
                        programs.insert(Executable::new(build(d.key, &in_currency(&doc, cur))?)).0
                    };
                    variants.insert((*cur).to_owned(), r);
                }
            }
            // Builds without the file fields stay loadable (after the current ones, which stay the
            // default), so requests started before attachments keep the program they pinned.
            let legacy: Vec<Value> = match &money {
                Some(_) => procera_settings::catalogue::CURRENCIES
                    .iter()
                    .map(|c| in_currency(&authored, c))
                    .collect(),
                None => vec![authored.clone()],
            };
            for doc in &legacy {
                programs.insert(Executable::new(build(d.key, doc)?));
            }
            types.push(Arc::new(RequestType {
                key: d.key,
                prefix: d.prefix,
                name: d.name,
                description: d.description,
                category: d.category,
                icon: d.icon,
                program: pinned,
                exe,
                money,
                variants,
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
    pub fn programs(&self) -> Arc<Programs> {
        self.programs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_builtin_compiles_and_pins_its_checksum() {
        let c = Catalogue::builtin().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.list().len(), 4);
        for t in c.list() {
            assert!(t.program.checksum.is_some(), "{} is pinned", t.key);
            assert!(!t.start_form.fields.is_empty());
            assert_eq!(t.exe.program.default_calendar(), Some(crate::DEFAULT_CALENDAR));
        }
        let leave = c.get("hr.leave-request").unwrap_or_else(|| panic!("leave"));
        let kinds: Vec<&str> = leave.start_form.fields.iter().map(|f| f.ty).collect();
        assert_eq!(kinds, ["enum", "date", "date", "text", "file", "file"]);
        assert!(leave.start_form.fields[5].multiple && !leave.start_form.fields[5].required);
        assert_eq!(leave.task_forms["revise"].fields.len(), 6, "a sent-back request can change its files");
        assert!(!leave.start_form.fields[3].required);
        let pr = c.get("fin.purchase-requisition").unwrap_or_else(|| panic!("pr"));
        assert_eq!(pr.start_form.fields[0].currency.as_deref(), Some("EUR"));
        assert!(pr.description("USD").contains("5,000 USD"));
        // One build per currency, each with its own checksum; types without money have one.
        let usd = pr.program_for("USD").unwrap_or_else(|| panic!("usd"));
        assert_ne!(usd.checksum, pr.program.checksum);
        assert_eq!(pr.program_for("EUR"), Some(&pr.program));
        assert!(pr.program_for("XXX").is_none());
        assert_eq!(leave.program_for("USD"), Some(&leave.program));
        let keys: Vec<&str> =
            c.list().iter().map(|t| t.key).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        assert_eq!(
            keys,
            procera_settings::catalogue::REQUEST_TYPES,
            "the settings catalogue lists every type"
        );
        let ar = c.get("it.access-request").unwrap_or_else(|| panic!("ar"));
        assert_eq!(ar.task_forms["provision"].fields[0].name, "ticket_ref");
        // Builds from before the file fields still load, for requests pinned to them.
        let authored: Value = serde_json::from_str(DEFS[1].ir).unwrap_or_default();
        let old = build("x", &in_currency(&authored, "USD")).unwrap_or_else(|e| panic!("{e}"));
        let mut r = c.get("hr.expense-claim").unwrap_or_else(|| panic!("ec")).program.clone();
        assert_ne!(r.checksum.as_deref(), old.checksum());
        r.checksum = old.checksum().map(str::to_owned);
        let ws = procera_db::WsCtx { workspace_id: uuid::Uuid::nil() };
        let found = c.programs().load(ws, &r).await;
        assert_eq!(found.map(|e| e.program.checksum().map(str::to_owned)), Ok(r.checksum.clone()));
    }
}
