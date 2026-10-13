//! "Who approves this" (BPMS-103, slice 3): a dry run of a request's routing before it is sent.
//!
//! Nothing is written. The first human step(s) are resolved with the real resolver
//! ([`crate::resolve::resolve`]: manager, groups, roles, positions, fallback, separation of duties
//! and out-of-office substitution) against a throwaway instance state; later steps are described in
//! words from the IR, with each decision branch read as a condition ("the amount is over 500 EUR")
//! and checked against what the form holds so far. Input is validated partially: a field that is
//! missing or not valid yet is left out rather than refused, because the form is still being filled.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use procera_db::WsTx;
use procera_engine::{Executable, ProgramSource};
use procera_ir::model::{CandidateType, IrDocument as Process, NodeKind};
use procera_runtime::{InstanceState, InstanceStatus};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::catalogue::{Form, RequestType};
use crate::forms;
use crate::resolve::{self, Site};
use crate::{RequestError, Requests};

type R<T> = Result<T, RequestError>;

/// Shown when nobody can be resolved for the first step.
pub const NOBODY: &str = "Nobody is set up to approve this yet — an admin will assign it";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewPerson {
    pub id: Uuid,
    pub name: String,
    /// Why they are on the step: "your manager", "Finance", "standing in for Olivia Owner, who is away".
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreviewStep {
    pub node: String,
    pub name: String,
    /// The first person-facing step on its path (resolved to people); later steps are described.
    pub first: bool,
    /// Who, in words: "Olivia Owner (your manager)", "Finance".
    pub who: String,
    pub people: Vec<PreviewPerson>,
    /// A first step that resolved to nobody: an admin assigns it.
    pub unassigned: bool,
    /// When the step happens, in words; `None` = always.
    pub condition: Option<String>,
    /// Whether the condition holds for the answers so far (`None` = not known yet).
    pub applies: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preview {
    /// One sentence: "Olivia Owner (your manager), then Finance if the amount is over 500 EUR".
    pub summary: String,
    /// The step that would start now has nobody to decide it.
    pub unassigned: bool,
    pub steps: Vec<PreviewStep>,
}

// ---- conditions ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
}

impl Op {
    fn negate(self) -> Self {
        match self {
            Self::Gt => Self::Le,
            Self::Ge => Self::Lt,
            Self::Lt => Self::Ge,
            Self::Le => Self::Gt,
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Lit {
    Num(Decimal),
    Text(String),
}

/// One condition on the path to a step: `var op lit`, or an opaque text when the expression is not
/// a simple comparison.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cond {
    Cmp { var: String, op: Op, lit: Lit },
    Opaque(String),
}

/// Parses `expr:cel: amount.amount > 500.0` or `sensitivity == "high"`.
fn parse(expr: &str) -> Option<Cond> {
    let e = expr.strip_prefix("expr:cel:")?.trim();
    let ops = [(">=", Op::Ge), ("<=", Op::Le), ("==", Op::Eq), ("!=", Op::Ne), (">", Op::Gt), ("<", Op::Lt)];
    let (at, sym, op) = ops.iter().find_map(|(s, o)| e.find(s).map(|i| (i, *s, *o)))?;
    let lhs = e[..at].trim();
    let rhs = e[at + sym.len()..].trim();
    let var = lhs.strip_suffix(".amount").unwrap_or(lhs);
    if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let lit = if let Some(s) = rhs.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Lit::Text(s.to_owned())
    } else {
        Lit::Num(Decimal::from_str(rhs).ok()?)
    };
    Some(Cond::Cmp { var: var.to_owned(), op, lit })
}

fn negated(c: &Cond) -> Cond {
    match c {
        Cond::Cmp { var, op, lit } => Cond::Cmp { var: var.clone(), op: op.negate(), lit: lit.clone() },
        Cond::Opaque(t) => Cond::Opaque(format!("not: {t}")),
    }
}

/// `5000` → `5,000`; `500.5` → `500.5`.
pub(crate) fn grouped(d: Decimal) -> String {
    let s = d.normalize().to_string();
    let (int, frac) = s.split_once('.').map_or((s.as_str(), None), |(i, f)| (i, Some(f)));
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}{}", frac.map(|f| format!(".{f}")).unwrap_or_default())
}

impl Cond {
    /// The condition in words, with field labels and the currency of money fields.
    fn text(&self, form: &Form) -> String {
        let Cond::Cmp { var, op, lit } = self else {
            let Cond::Opaque(t) = self else { return String::new() };
            return t.clone();
        };
        let field = form.fields.iter().find(|f| &f.name == var);
        let label = field.map_or_else(|| var.replace('_', " "), |f| f.label.to_lowercase());
        let value = match lit {
            Lit::Num(n) => match field.and_then(|f| f.currency.as_deref()) {
                Some(c) => format!("{} {c}", grouped(*n)),
                None => grouped(*n),
            },
            Lit::Text(t) => t.replace(['_', '-'], " "),
        };
        let words = match op {
            Op::Gt => format!("over {value}"),
            Op::Ge => format!("at least {value}"),
            Op::Lt => format!("under {value}"),
            Op::Le => format!("{value} or less"),
            Op::Eq => value,
            Op::Ne => format!("not {value}"),
        };
        format!("the {label} is {words}")
    }

    /// Whether it holds for `vars` (`None` when the value is not there yet).
    fn holds(&self, vars: &BTreeMap<String, Value>) -> Option<bool> {
        let Cond::Cmp { var, op, lit } = self else { return None };
        let v = vars.get(var)?;
        let ord = match lit {
            Lit::Num(n) => {
                let raw = v.get("amount").unwrap_or(v);
                let x = match raw {
                    Value::String(s) => Decimal::from_str(s).ok()?,
                    Value::Number(n) => Decimal::from_str(&n.to_string()).ok()?,
                    _ => return None,
                };
                x.cmp(n)
            }
            Lit::Text(t) => {
                let s = v.as_str()?;
                return Some(match op {
                    Op::Eq => s == t,
                    Op::Ne => s != t,
                    _ => return None,
                });
            }
        };
        use std::cmp::Ordering::{Equal, Greater, Less};
        Some(match op {
            Op::Gt => ord == Greater,
            Op::Ge => ord != Less,
            Op::Lt => ord == Less,
            Op::Le => ord != Greater,
            Op::Eq => ord == Equal,
            Op::Ne => ord != Equal,
        })
    }
}

// ---- the walk ----

/// A person-facing step on some path from the start, with the conditions on that path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Route {
    pub node: String,
    pub conds: Vec<Cond>,
    pub first: bool,
    /// Reached through a parallel split: it runs at the same time as its sibling branches.
    pub parallel: bool,
}

/// Adds a condition, keeping only the tighter of two bounds on the same value in the same
/// direction ("over 5,000" and "over 50,000" read as "over 50,000").
fn add(conds: &mut Vec<Cond>, c: Cond) {
    if let Cond::Cmp { var, op, lit: Lit::Num(n) } = &c {
        for e in conds.iter_mut() {
            if let Cond::Cmp { var: v, op: o, lit: Lit::Num(m) } = e
                && v == var
                && o == op
            {
                let tighter = match op {
                    Op::Gt | Op::Ge => n > m,
                    Op::Lt | Op::Le => n < m,
                    Op::Eq | Op::Ne => continue,
                };
                if tighter {
                    *m = *n;
                }
                return;
            }
        }
    }
    if !conds.contains(&c) {
        conds.push(c);
    }
}

/// An edge that moves a request forward (not a rejection, send-back or cancellation).
fn forward(on: &str) -> bool {
    !["reject", "changes", "send_back", "cancel", "expired"].iter().any(|p| on.starts_with(p))
}

fn human(k: NodeKind) -> bool {
    matches!(
        k,
        NodeKind::HumanApproval | NodeKind::HumanTask | NodeKind::HumanSign | NodeKind::HumanAcknowledge
    )
}

/// Every person-facing step reachable from the start, first steps first, in path order. A step
/// reached by paths with different conditions is treated as unconditional.
pub(crate) fn routes(ir: &Process) -> Vec<Route> {
    #[allow(clippy::too_many_arguments)]
    fn visit(
        ir: &Process,
        id: &str,
        conds: Vec<Cond>,
        first: bool,
        parallel: bool,
        path: &mut Vec<String>,
        out: &mut Vec<Route>,
    ) {
        if path.iter().any(|p| p == id) {
            return;
        }
        let Some(node) = ir.nodes.iter().find(|n| n.id == id) else { return };
        path.push(id.to_owned());
        let edges = ir.edges.iter().filter(|e| e.from == id);
        match node.kind {
            NodeKind::End => {}
            k if human(k) => {
                match out.iter_mut().find(|r| r.node == id) {
                    Some(r) => {
                        if r.conds != conds {
                            r.conds.clear();
                        }
                        r.first |= first;
                    }
                    None => out.push(Route { node: id.to_owned(), conds: conds.clone(), first, parallel }),
                }
                for e in edges.filter(|e| forward(&e.on)) {
                    visit(ir, &e.to, conds.clone(), false, false, path, out);
                }
            }
            NodeKind::Decision => {
                let branches: Vec<(String, Cond)> = node
                    .branches
                    .iter()
                    .flatten()
                    .map(|b| {
                        let c = b.when.as_ref().and_then(|w| parse(&w.0)).unwrap_or_else(|| {
                            let n = node.name.as_ref().and_then(|n| n.get("en").cloned());
                            Cond::Opaque(format!("{} ({})", n.unwrap_or_else(|| node.id.clone()), b.name))
                        });
                        (b.name.clone(), c)
                    })
                    .collect();
                for e in edges {
                    let mut next = conds.clone();
                    match branches.iter().find(|(n, _)| n == &e.on) {
                        Some((_, c)) => add(&mut next, c.clone()),
                        None => branches.iter().for_each(|(_, c)| add(&mut next, negated(c))),
                    }
                    visit(ir, &e.to, next, first, parallel, path, out);
                }
            }
            k => {
                let parallel = parallel || k == NodeKind::ParallelSplit;
                for e in edges.filter(|e| forward(&e.on)) {
                    visit(ir, &e.to, conds.clone(), first, parallel, path, out);
                }
            }
        }
        path.pop();
    }
    let mut out = Vec::new();
    if let Some(start) = ir.nodes.iter().find(|n| n.kind == NodeKind::Start) {
        visit(ir, &start.id.clone(), vec![], true, false, &mut Vec::new(), &mut out);
    }
    let (mut firsts, later): (Vec<Route>, Vec<Route>) = out.into_iter().partition(|r| r.first);
    firsts.extend(later);
    firsts
}

/// "Ana", "Ana or Ben", "Ana, Ben or 2 others".
fn either(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} or {b}"),
        [a, b, c] => format!("{a}, {b} or {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} or {} others", rest.len()),
    }
}

impl Requests {
    /// A role's display name, or its key in words.
    async fn role_name(&self, tx: &mut WsTx<'_>, key: &str) -> R<String> {
        let n: Option<String> = sqlx::query_scalar("SELECT name FROM roles WHERE key = $1")
            .bind(key)
            .fetch_optional(tx.conn())
            .await?;
        Ok(n.unwrap_or_else(|| key.replace(['.', '_'], " ")))
    }

    /// A candidate source in words, relative to the requester.
    async fn source_words(
        &self,
        tx: &mut WsTx<'_>,
        ty: &str,
        reference: &str,
        form: &Form,
        vars: &BTreeMap<String, Value>,
    ) -> R<Option<String>> {
        Ok(match ty {
            "group" | "position" => Some(reference.to_owned()),
            "role" => Some(self.role_name(tx, reference).await?),
            "user" => {
                let Some(e) = reference.strip_prefix("expr:cel:").map(str::trim) else { return Ok(None) };
                let mut parts = e.split('.');
                let root = parts.next().unwrap_or_default();
                let depth = parts.filter(|p| *p == "manager").count();
                let whose = if root == "initiator" {
                    "your".to_owned()
                } else {
                    let id = vars.get(root).and_then(|v| v.get("id")).and_then(Value::as_str);
                    match id.and_then(|s| Uuid::parse_str(s).ok()) {
                        Some(id) => {
                            format!(
                                "{}'s",
                                crate::handler::name(tx, id).await.map_err(RequestError::Internal)?
                            )
                        }
                        None => {
                            let label = form.fields.iter().find(|f| f.name == root).map(|f| f.label.clone());
                            format!(
                                "the {}'s",
                                label.unwrap_or_else(|| root.replace('_', " ")).to_lowercase()
                            )
                        }
                    }
                };
                match depth {
                    0 if root == "initiator" => Some("you".to_owned()),
                    0 => Some(whose.strip_suffix("'s").unwrap_or(&whose).to_owned()),
                    1 => Some(format!("{whose} manager")),
                    _ => Some(format!("{whose} manager's manager")),
                }
            }
            _ => None,
        })
    }

    /// A later step in words, from its assignment: a group, a position, a role, or a manager.
    async fn step_words(
        &self,
        tx: &mut WsTx<'_>,
        exe: &Executable,
        node: &str,
        form: &Form,
        vars: &BTreeMap<String, Value>,
    ) -> R<String> {
        let n = exe.program.ir().nodes.iter().find(|n| n.id == node);
        let cands = n.and_then(|n| n.assignment.as_ref()).map(|a| a.candidates.clone()).unwrap_or_default();
        for pick in [CandidateType::Group, CandidateType::Position, CandidateType::Role, CandidateType::User]
        {
            if let Some(c) = cands.iter().find(|c| c.ty == pick)
                && let Some(w) = self.source_words(tx, c.ty.as_str(), &c.reference.0, form, vars).await?
            {
                return Ok(w);
            }
        }
        Ok(n.and_then(|n| n.name.as_ref())
            .and_then(|n| n.get("en").cloned())
            .unwrap_or_else(|| node.to_owned()))
    }

    /// Dry-runs the routing of a request `member` is about to send (no writes, no audit).
    pub async fn preview(
        &self,
        tx: &mut WsTx<'_>,
        member: Uuid,
        key: &str,
        input: &Map<String, Value>,
    ) -> R<Preview> {
        let rt = self.type_of(tx, key).await?;
        let offer = self.offer(tx).await?;
        if !self.may_start(tx, &offer, key, member).await? {
            return Err(RequestError::Forbidden(format!("{} is not offered to you", rt.name)));
        }
        let pref = rt
            .program_for(&offer.currency)
            .cloned()
            .ok_or_else(|| RequestError::Internal(format!("{key} has no build for {}", offer.currency)))?;
        let exe = self.catalogue.programs().load(tx.ctx, &pref).await.map_err(RequestError::Internal)?;
        let form = RequestType::priced(&rt.start_form, &offer.currency);
        // Partial: drop what is not valid yet, and attachments (routing never reads them).
        let mut input: Map<String, Value> = input
            .iter()
            .filter(|(k, _)| form.fields.iter().any(|f| &f.name == *k && f.ty != "file"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut vars = loop {
            match forms::validate(&form, &input, true) {
                Ok(v) => break v,
                Err(errs) => {
                    let before = input.len();
                    for e in errs {
                        input.remove(e.pointer.trim_start_matches('/'));
                    }
                    if input.len() == before {
                        break BTreeMap::new();
                    }
                }
            }
        };
        for f in form.fields.iter().filter(|f| f.ty == "principal") {
            let id = vars.get(&f.name).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
            let active = match id {
                Some(id) => sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
                )
                .bind(id)
                .fetch_one(tx.conn())
                .await?
                .then_some(id),
                None => None,
            };
            match active {
                Some(id) => {
                    let p = self.principal(tx, id).await?;
                    vars.insert(f.name.clone(), p);
                }
                None => {
                    vars.remove(&f.name);
                }
            }
        }
        let now = self.clock.now();
        let instance_id = Uuid::nil();
        let st = InstanceState {
            instance_id,
            process_key: rt.key.to_owned(),
            process_version: pref.version,
            status: InstanceStatus::Running,
            started_at: now,
            ended_at: None,
            initiator: self.principal(tx, member).await?,
            instance: json!({ "id": instance_id.to_string(), "display_id": "preview", "started_at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default() }),
            variables: vars.clone(),
            tokens: BTreeMap::new(),
            splits: BTreeMap::new(),
            entries: BTreeMap::new(),
            trace: vec![],
            next_id: 0,
            revision: 0,
        };
        let mut steps = Vec::new();
        let all = routes(exe.program.ir());
        let first_parallel = all.iter().filter(|r| r.first).all(|r| r.parallel);
        for r in all {
            let conds: Vec<String> = r.conds.iter().map(|c| c.text(&form)).collect();
            let holds: Vec<Option<bool>> = r.conds.iter().map(|c| c.holds(&vars)).collect();
            // An unconditional step always applies (no conditions: every one holds).
            let applies = if holds.contains(&Some(false)) {
                Some(false)
            } else if holds.iter().all(Option::is_some) {
                Some(true)
            } else {
                None
            };
            let condition = (!conds.is_empty()).then(|| conds.join(" and "));
            let name = rt.node_name(&r.node);
            let assignment = exe.program.node(&r.node).and_then(|n| n.assignment.clone());
            let mut step = PreviewStep {
                node: r.node.clone(),
                name,
                first: r.first,
                who: String::new(),
                people: vec![],
                unassigned: false,
                condition,
                applies,
            };
            match (r.first, assignment) {
                (true, Some(a)) => {
                    let site = Site { node: &r.node, field: "assignment" };
                    match resolve::resolve(tx, &exe, &st, now, site, &a, true, true).await {
                        Ok(res) => {
                            step.people = self.explain(tx, &res, &form, &vars).await?;
                            step.unassigned = res.unassigned();
                            step.who = if res.unassigned() {
                                NOBODY.to_owned()
                            } else {
                                let mut by_reason: Vec<(Option<String>, Vec<String>)> = Vec::new();
                                for p in &step.people {
                                    match by_reason.iter_mut().find(|(r, _)| r == &p.reason) {
                                        Some((_, n)) => n.push(p.name.clone()),
                                        None => by_reason.push((p.reason.clone(), vec![p.name.clone()])),
                                    }
                                }
                                by_reason
                                    .into_iter()
                                    .map(|(r, n)| match r {
                                        Some(r) => format!("{} ({r})", either(&n)),
                                        None => either(&n),
                                    })
                                    .collect::<Vec<_>>()
                                    .join(", or ")
                            };
                        }
                        // Something the form has not given yet (e.g. who needs access).
                        Err(_) => step.who = self.step_words(tx, &exe, &r.node, &form, &vars).await?,
                    }
                }
                _ => step.who = self.step_words(tx, &exe, &r.node, &form, &vars).await?,
            }
            steps.push(step);
        }
        let shown: Vec<&PreviewStep> = steps.iter().filter(|s| s.applies != Some(false)).collect();
        let suffix = |s: &PreviewStep| match (&s.condition, s.applies) {
            (None, _) => String::new(),
            (Some(c), Some(true)) => format!(" ({c})"),
            (Some(c), _) => format!(" if {c}"),
        };
        let firsts: Vec<String> =
            shown.iter().filter(|s| s.first).map(|s| format!("{}{}", s.who, suffix(s))).collect();
        let together = firsts.len() > 1 && first_parallel;
        let mut parts = vec![];
        match firsts.as_slice() {
            [] => {}
            [init @ .., last] if together => {
                parts.push(format!("{} and {last} at the same time", init.join(", ")));
            }
            all => parts.push(all.join(", or ")),
        }
        parts.extend(shown.iter().filter(|s| !s.first).map(|s| format!("then {}{}", s.who, suffix(s))));
        let summary =
            if parts.is_empty() { "Nobody needs to approve this".to_owned() } else { parts.join(", ") };
        let unassigned = shown.iter().any(|s| s.first && s.unassigned && s.applies == Some(true));
        Ok(Preview { summary, unassigned, steps })
    }

    /// The resolved people with the reason each is on the step.
    async fn explain(
        &self,
        tx: &mut WsTx<'_>,
        res: &resolve::Resolution,
        form: &Form,
        vars: &BTreeMap<String, Value>,
    ) -> R<Vec<PreviewPerson>> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let subs = res.snapshot["substitutions"].as_array().cloned().unwrap_or_default();
        for id in &res.candidates {
            if !seen.insert(*id) {
                continue;
            }
            let standing_in = subs.iter().find(|s| {
                s.get("skipped").is_none() && s["delegate"].as_str() == Some(id.to_string().as_str())
            });
            let reason = if let Some(s) = standing_in {
                let f = s["for"].as_str().and_then(|v| Uuid::parse_str(v).ok());
                match f {
                    Some(f) => Some(format!(
                        "standing in for {}, who is away",
                        crate::handler::name(tx, f).await.map_err(RequestError::Internal)?
                    )),
                    None => None,
                }
            } else {
                let mut why = None;
                for src in res.snapshot["sources"].as_array().into_iter().flatten() {
                    let has = src["members"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|m| m.as_str() == Some(id.to_string().as_str()));
                    if has {
                        let w = self
                            .source_words(
                                tx,
                                src["type"].as_str().unwrap_or_default(),
                                src["ref"].as_str().unwrap_or_default(),
                                form,
                                vars,
                            )
                            .await?;
                        why = match (w, src.get("fallback").is_some()) {
                            (Some(w), true) => Some(format!("{w}, as nobody else could be found")),
                            (w, _) => w,
                        };
                        break;
                    }
                }
                why
            };
            let name = crate::handler::name(tx, *id).await.map_err(RequestError::Internal)?;
            out.push(PreviewPerson { id: *id, name, reason });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Catalogue;

    fn route_names(key: &str) -> Vec<(String, bool, usize)> {
        let c = Catalogue::builtin().unwrap_or_else(|e| panic!("{e}"));
        let t = c.get(key).unwrap_or_else(|| panic!("{key}"));
        routes(t.exe.program.ir()).into_iter().map(|r| (r.node, r.first, r.conds.len())).collect()
    }

    #[test]
    fn routes_follow_approvals_and_read_branches_as_conditions() {
        let n = |v: &[(&str, bool, usize)]| -> Vec<(String, bool, usize)> {
            v.iter().map(|(a, b, c)| ((*a).to_owned(), *b, *c)).collect()
        };
        assert_eq!(route_names("hr.leave-request"), n(&[("manager_approval", true, 0)]));
        assert_eq!(
            route_names("hr.expense-claim"),
            n(&[("manager_approval", true, 0), ("finance_approval", false, 1)])
        );
        assert_eq!(
            route_names("fin.purchase-requisition"),
            n(&[("manager_approval", true, 0), ("finance_approval", false, 1), ("cfo_approval", false, 1)])
        );
        // Access: owner or manager first, by sensitivity; provisioning on both paths is unconditional.
        assert_eq!(
            route_names("it.access-request"),
            n(&[
                ("owner_approval", true, 1),
                ("manager_approval", true, 1),
                ("security_approval", false, 1),
                ("provision", false, 0)
            ])
        );
        // Onboarding: three tasks at once, then the new member's acknowledgement.
        assert_eq!(
            route_names("hr.onboarding"),
            n(&[
                ("it_setup", true, 0),
                ("workspace", true, 0),
                ("first_week", true, 0),
                ("welcome", false, 0)
            ])
        );
    }

    #[test]
    fn conditions_read_in_words_and_check_the_answers() {
        let c = Catalogue::builtin().unwrap_or_else(|e| panic!("{e}"));
        let form =
            RequestType::priced(&c.get("hr.expense-claim").unwrap_or_else(|| panic!()).start_form, "EUR");
        let cond = parse("expr:cel: amount.amount > 500.0").unwrap_or_else(|| panic!("parse"));
        assert_eq!(cond.text(&form), "the amount is over 500 EUR");
        assert_eq!(negated(&cond).text(&form), "the amount is 500 EUR or less");
        let vars = |a: &str| BTreeMap::from([("amount".to_owned(), json!({"amount": a, "currency": "EUR"}))]);
        assert_eq!(cond.holds(&vars("900")), Some(true));
        assert_eq!(cond.holds(&vars("500")), Some(false));
        assert_eq!(cond.holds(&BTreeMap::new()), None);
        let s = parse("expr:cel: sensitivity == \"high\"").unwrap_or_else(|| panic!("parse"));
        let ar = c.get("it.access-request").unwrap_or_else(|| panic!()).start_form.clone();
        assert_eq!(s.text(&ar), "the sensitivity is high");
        assert_eq!(negated(&s).text(&ar), "the sensitivity is not high");
        assert_eq!(grouped(Decimal::from(50_000)), "50,000");
        assert_eq!(grouped(Decimal::from_str("1234567.5").unwrap_or_default()), "1,234,567.5");
        assert_eq!(either(&["A".into(), "B".into(), "C".into(), "D".into()]), "A, B or 2 others");
    }
}
