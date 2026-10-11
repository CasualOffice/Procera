//! Semantic checks: required fields, expressions and classification, references, assignments,
//! timers, escalations, SLAs (docs/11 §11 checks 7-11, §3.1-§3.2, §4.1, §9, §13).

use super::Index;
use crate::diag::{Code, Diagnostic, escape_pointer};
use crate::env::{Environment, Resolution};
use crate::expr::ExpressionChecker;
use crate::model::{
    Assignment, CandidateType, Criticality, EscalationAction, Expr, Node, NodeKind, ScalarType, VarType,
};
use std::collections::BTreeMap;

pub(super) fn check(
    ix: &Index<'_>,
    expressions: &dyn ExpressionChecker,
    env: Option<&dyn Environment>,
    out: &mut Vec<Diagnostic>,
) {
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        required(i, n, out);
        boundaries(i, n, out);
        ref_keys(ix, i, n, out);
        sod(i, n, out);
        timers(ix, i, n, out);
        escalations(ix, i, n, out);
    }
    expressions_and_classification(ix, expressions, out);
    slas(ix, out);
    variables(ix, out);
    match env {
        Some(env) => environment(ix, env, out),
        None => out.push(Diagnostic::new(
            Code::I001,
            "",
            "compiled without a workspace environment: refs, connectors and directory names were not checked",
        )),
    }
}

/// Fields the spec makes mandatory per kind (docs/11 §3, §13.1-§13.3).
fn required(i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    let mut missing = |field: &str, why: &str| {
        out.push(
            Diagnostic::new(
                Code::E018,
                format!("/nodes/{i}"),
                format!("{} '{}' needs `{field}` ({why})", n.kind.as_str(), n.id),
            )
            .at_node(&n.id),
        );
    };
    match n.kind {
        NodeKind::HumanTask | NodeKind::HumanApproval | NodeKind::HumanAcknowledge
            if n.assignment.is_none() =>
        {
            missing("assignment", "every human node is assigned, §13.2");
        }
        NodeKind::HumanSign if n.signers.is_none() => missing("signers", "§13.2"),
        NodeKind::Decision if n.branches.is_none() => missing("branches", "named branches, §3"),
        NodeKind::ParallelSplit if n.branches.is_none() => missing("branches", "one per branch, §3"),
        NodeKind::WaitSignal if n.correlation.is_none() => missing("correlation", "§3"),
        NodeKind::Notify if n.to.is_none() => missing("to", "§13.3"),
        NodeKind::SubprocessCall if n.process.is_none() => missing("process", "§3"),
        _ => {}
    }
    match n.kind {
        NodeKind::HumanSign if n.document.is_none() => missing("document", "§3"),
        NodeKind::HumanAcknowledge if n.subject.is_none() => missing("subject", "§3"),
        NodeKind::Notify if n.template.is_none() => missing("template", "§13.3"),
        NodeKind::ServiceCall => {
            for (field, present) in [
                ("connector", n.connector.is_some()),
                ("action", n.action.is_some()),
                ("idempotencyKey", n.idempotency_key.is_some()),
            ] {
                if !present {
                    missing(field, "§3");
                }
            }
        }
        NodeKind::Decision if n.rule.is_some() != n.output.is_some() => {
            missing(if n.rule.is_some() { "output" } else { "rule" }, "rule-table decision, §13.1");
        }
        NodeKind::Decision if n.rule.is_none() => {
            for (b, br) in n.branches.iter().flatten().enumerate() {
                if br.when.is_none() {
                    out.push(
                        Diagnostic::new(
                            Code::E018,
                            format!("/nodes/{i}/branches/{b}"),
                            format!(
                                "branch '{}' of expression decision '{}' needs `when` (§3)",
                                br.name, n.id
                            ),
                        )
                        .at_node(&n.id),
                    );
                }
            }
        }
        _ => {}
    }
}

fn boundaries(i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    let Some(list) = &n.boundary else { return };
    if !n.kind.allows_boundary() {
        out.push(
            Diagnostic::new(
                Code::E031,
                format!("/nodes/{i}/boundary"),
                format!("{} nodes cannot declare boundary handlers (§4.1)", n.kind.as_str()),
            )
            .at_node(&n.id),
        );
    }
    for (b, bd) in list.iter().enumerate() {
        if bd.on == "timeout" && bd.after.is_none() {
            out.push(
                Diagnostic::new(
                    Code::E028,
                    format!("/nodes/{i}/boundary/{b}"),
                    "timeout boundary needs `after`",
                )
                .at_node(&n.id),
            );
        }
    }
}

/// Node fields that name a `refs` entry (docs/11 §2, §13.8).
fn ref_keys(ix: &Index<'_>, i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    let refs = &ix.doc.refs;
    let mut unknown = |field: &str, key: &str, section: &str| {
        out.push(
            Diagnostic::new(
                Code::E022,
                format!("/nodes/{i}/{field}"),
                format!("'{key}' is not declared in refs.{section}"),
            )
            .at_node(&n.id),
        );
    };
    let checks: [(&str, &Option<String>, &BTreeMap<String, String>, &str); 6] = [
        ("form", &n.form, &refs.forms, "forms"),
        ("inputForm", &n.input_form, &refs.forms, "forms"),
        ("quiz", &n.quiz, &refs.forms, "forms"),
        ("rule", &n.rule, &refs.rules, "rules"),
        ("calendar", &n.calendar, &refs.calendars, "calendars"),
        ("connector", &n.connector, &refs.connectors, "connectors"),
    ];
    for (field, value, map, section) in checks {
        if let Some(key) = value
            && !map.contains_key(key)
        {
            unknown(field, key, section);
        }
    }
    if n.kind == NodeKind::SubprocessCall
        && let Some(key) = &n.process
        && !refs.processes.contains_key(key)
    {
        unknown("process", key, "processes");
    }
    if let Some(key) = &n.document {
        let file_var =
            ix.doc.variables.iter().any(|v| &v.name == key && v.ty == VarType::Scalar(ScalarType::File));
        if !refs.documents.contains_key(key) && !file_var {
            unknown("document", key, "documents (or a `file` variable)");
        }
    }
    if let Some(key) = &n.subject {
        let ok = match key.as_str() {
            "policy" => refs.policy.is_some(),
            "sop" => refs.sop.is_some(),
            other => refs.documents.contains_key(other),
        };
        if !ok {
            unknown("subject", key, "policy / sop / documents");
        }
    }
}

fn is_initiator(c: &crate::model::Candidate) -> bool {
    c.ty == CandidateType::User
        && c.reference
            .as_expr()
            .as_ref()
            .and_then(Expr::parse)
            .is_some_and(|(_, body)| body.trim() == "initiator")
}

/// An assignment that can only ever yield the excluded initiator (docs/11 §11 check 10).
fn sod(i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    for (field, a) in n.assignments() {
        if a.exclude_initiator == Some(true)
            && a.fallback.is_none()
            && !a.candidates.is_empty()
            && a.candidates.iter().all(is_initiator)
        {
            out.push(
                Diagnostic::new(
                    Code::E026,
                    format!("/nodes/{i}/{field}"),
                    "every candidate is the initiator, who is excluded, and there is no fallback",
                )
                .at_node(&n.id),
            );
        }
    }
}

fn timers(ix: &Index<'_>, i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    let calendars = &ix.doc.refs.calendars;
    let has_calendar =
        n.calendar.as_ref().map_or(calendars.contains_key("default"), |c| calendars.contains_key(c));
    let mut business = Vec::new();
    if n.due.as_ref().and_then(|d| d.business) == Some(true) {
        business.push(format!("/nodes/{i}/due"));
    }
    if n.kind == NodeKind::WaitTimer && n.business == Some(true) {
        business.push(format!("/nodes/{i}/business"));
    }
    // An unknown explicit calendar is already IR-E022.
    if !has_calendar && n.calendar.is_none() {
        for ptr in business {
            out.push(
                Diagnostic::new(
                    Code::E027,
                    ptr,
                    "business-time duration needs refs.calendars.default or `calendar`",
                )
                .at_node(&n.id),
            );
        }
    }
    if n.kind == NodeKind::WaitTimer && n.at.is_some() == n.after.is_some() {
        out.push(
            Diagnostic::new(
                Code::E028,
                format!("/nodes/{i}"),
                "wait.timer needs exactly one of `at` or `after`",
            )
            .at_node(&n.id),
        );
    }
    let mut durations = vec![("after", n.after.as_ref()), ("timeout", n.timeout.as_ref())];
    durations.push(("due/after", n.due.as_ref().map(|d| &d.after)));
    for (field, d) in durations {
        if let Some(d) = d
            && d.seconds().unwrap_or(0) == 0
        {
            out.push(
                Diagnostic::new(
                    Code::E028,
                    format!("/nodes/{i}/{field}"),
                    format!("duration '{}' must be longer than zero", d.0),
                )
                .at_node(&n.id),
            );
        }
    }
    if let Some((_, body)) = n.at.as_ref().and_then(Expr::parse)
        && let Some(lit) = instant_without_offset(body)
    {
        out.push(
            Diagnostic::new(
                Code::E029,
                format!("/nodes/{i}/at"),
                format!("instant '{lit}' has no UTC offset"),
            )
            .at_node(&n.id),
        );
    }
}

/// Finds a quoted `YYYY-MM-DDThh:mm[:ss[.f]]` literal that lacks `Z` or `±hh:mm`.
fn instant_without_offset(body: &str) -> Option<String> {
    for (k, part) in body.split(['"', '\'']).enumerate() {
        if k % 2 == 0 {
            continue; // outside quotes
        }
        let b = part.as_bytes();
        let shape = b.len() >= 16
            && b[..4].iter().all(u8::is_ascii_digit)
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':';
        if !shape {
            continue;
        }
        let tail = &part[16..];
        let has_offset = tail.ends_with('Z') || tail.ends_with('z') || {
            let t = tail.as_bytes();
            t.len() >= 6 && matches!(t[t.len() - 6], b'+' | b'-') && t[t.len() - 3] == b':'
        };
        if !has_offset {
            return Some(part.to_owned());
        }
    }
    None
}

fn escalations(ix: &Index<'_>, i: usize, n: &Node, out: &mut Vec<Diagnostic>) {
    let controlled = ix.doc.process.criticality == Some(Criticality::Controlled);
    for (k, esc) in n.escalations.iter().flatten().enumerate() {
        let ptr = format!("/nodes/{i}/escalations/{k}");
        let problem = match esc.action {
            EscalationAction::Reassign if esc.to.is_none() => Some("reassign needs `to`".to_owned()),
            EscalationAction::Outcome if controlled => {
                Some("controlled processes forbid automatic outcomes (§3.2)".to_owned())
            }
            EscalationAction::Outcome => match &esc.outcome {
                None => Some("outcome escalation needs `outcome`".to_owned()),
                Some(o) if !n.outcomes().contains(o) => {
                    Some(format!("'{}' cannot produce outcome '{o}'", n.id))
                }
                Some(_) => None,
            },
            _ => None,
        };
        if let Some(msg) = problem {
            out.push(Diagnostic::new(Code::E030, ptr, msg).at_node(&n.id));
        }
    }
}

/// Every expression in the document with its pointer and whether it is event-facing.
fn expression_sites<'a>(ix: &Index<'a>) -> Vec<(String, Expr, bool, &'a str)> {
    let mut sites = Vec::new();
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        let notify = n.kind == NodeKind::Notify;
        let mut push = |ptr: String, e: &Expr, facing: bool| {
            sites.push((ptr, e.clone(), facing || notify, n.id.as_str()))
        };
        if let Some(e) = &n.at {
            push(format!("/nodes/{i}/at"), e, false);
        }
        // Correlation keys and idempotency keys are recorded in events and job rows.
        if let Some(e) = &n.correlation {
            push(format!("/nodes/{i}/correlation"), e, true);
        }
        if let Some(e) = &n.idempotency_key {
            push(format!("/nodes/{i}/idempotencyKey"), e, true);
        }
        for (field, map) in [("input", &n.input), ("assign", &n.assign)] {
            for (k, e) in map.iter().flatten() {
                push(format!("/nodes/{i}/{field}/{}", escape_pointer(k)), e, false);
            }
        }
        for (b, br) in n.branches.iter().flatten().enumerate() {
            if let Some(e) = &br.when {
                push(format!("/nodes/{i}/branches/{b}/when"), e, false);
            }
        }
        let mut candidate_exprs = |base: String, a: &Assignment| {
            for (c, cand) in a.candidates.iter().enumerate() {
                if let Some(e) = cand.reference.as_expr() {
                    push(format!("{base}/candidates/{c}/ref"), &e, false);
                }
            }
            if let Some(e) = a.fallback.as_ref().and_then(|f| f.reference.as_expr()) {
                push(format!("{base}/fallback/ref"), &e, false);
            }
        };
        for (field, a) in n.assignments() {
            candidate_exprs(format!("/nodes/{i}/{field}"), a);
        }
        for (k, esc) in n.escalations.iter().flatten().enumerate() {
            if let Some(e) = esc.to.as_ref().and_then(|t| t.reference.as_expr()) {
                push(format!("/nodes/{i}/escalations/{k}/to/ref"), &e, false);
            }
        }
    }
    sites
}

fn expressions_and_classification(
    ix: &Index<'_>,
    checker: &dyn ExpressionChecker,
    out: &mut Vec<Diagnostic>,
) {
    let vars: BTreeMap<&str, &crate::model::Variable> =
        ix.doc.variables.iter().map(|v| (v.name.as_str(), v)).collect();
    for (ptr, expr, facing, node) in expression_sites(ix) {
        let Some((lang, body)) = expr.parse() else {
            out.push(
                Diagnostic::new(
                    Code::E019,
                    ptr,
                    format!("'{}' is not `expr:feel: …` or `expr:cel: …`", expr.0),
                )
                .at_node(node),
            );
            continue;
        };
        let analysis = match checker.analyse(lang, body) {
            Ok(a) => a,
            Err(e) => {
                out.push(
                    Diagnostic::new(Code::E019, ptr, format!("expression does not parse: {e}")).at_node(node),
                );
                continue;
            }
        };
        for name in &analysis.references {
            match vars.get(name.as_str()) {
                None => out.push(
                    Diagnostic::new(Code::E020, ptr.clone(), format!("'{name}' is not a declared variable"))
                        .at_node(node),
                ),
                Some(v) if facing && v.is_restricted() => out.push(
                    Diagnostic::new(
                        Code::E021,
                        ptr.clone(),
                        format!("restricted variable '{name}' must not reach notifications or events"),
                    )
                    .at_node(node),
                ),
                Some(_) => {}
            }
        }
    }
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        for key in n.assign.iter().flat_map(BTreeMap::keys) {
            if !vars.contains_key(key.as_str()) {
                out.push(
                    Diagnostic::new(
                        Code::E020,
                        format!("/nodes/{i}/assign/{}", escape_pointer(key)),
                        format!("'{key}' is assigned but not declared"),
                    )
                    .at_node(&n.id),
                );
            }
        }
    }
}

fn slas(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    let mut seen = std::collections::BTreeSet::new();
    let has_default = ix.doc.refs.calendars.contains_key("default");
    for (s, sla) in ix.doc.sla.iter().enumerate() {
        if !seen.insert(sla.id.as_str()) {
            out.push(Diagnostic::new(
                Code::E033,
                format!("/sla/{s}/id"),
                format!("duplicate SLA id '{}'", sla.id),
            ));
        }
        if let Some(id) = sla.scope_node()
            && ix.node(id).is_none()
        {
            out.push(Diagnostic::new(
                Code::E032,
                format!("/sla/{s}/scope"),
                format!("SLA scope node '{id}' does not exist"),
            ));
        }
        for (p, entry) in sla.pause_while.iter().flatten().enumerate() {
            let (kind, id) = match entry.rsplit_once(':') {
                Some((k, id)) => (Some(k), id),
                None => (None, entry.as_str()),
            };
            let ok = ix.node(id).is_some_and(|n| kind.is_none_or(|k| k == n.kind.as_str()));
            if !ok {
                out.push(Diagnostic::new(
                    Code::E032,
                    format!("/sla/{s}/pauseWhile/{p}"),
                    format!("pauseWhile '{entry}' does not name a node"),
                ));
            }
        }
        if sla.business == Some(true) && !has_default {
            out.push(Diagnostic::new(
                Code::E027,
                format!("/sla/{s}/business"),
                "business-time SLA needs refs.calendars.default",
            ));
        }
        if sla.target.seconds().unwrap_or(0) == 0 {
            out.push(Diagnostic::new(
                Code::E028,
                format!("/sla/{s}/target"),
                "SLA target must be longer than zero",
            ));
        }
    }
}

fn variables(ix: &Index<'_>, out: &mut Vec<Diagnostic>) {
    for (k, v) in ix.doc.variables.iter().enumerate() {
        if v.readers.is_some() && !v.is_restricted() {
            out.push(Diagnostic::new(
                Code::W001,
                format!("/variables/{k}/readers"),
                format!("'{}' is not restricted, so `readers` has no effect", v.name),
            ));
        }
    }
}

/// Checks 8-10 against the caller's workspace snapshot.
fn environment(ix: &Index<'_>, env: &dyn Environment, out: &mut Vec<Diagnostic>) {
    for (ptr, reference) in ix.doc.refs.entries() {
        let ptr = format!("/refs{ptr}");
        if ptr.starts_with("/refs/connectors/") {
            if !env.connector_usable(reference) {
                out.push(Diagnostic::new(
                    Code::E024,
                    ptr,
                    format!("connector '{reference}' is not enabled or its scopes are not granted"),
                ));
            }
            continue;
        }
        let why = match env.resolve(reference) {
            Resolution::Published => continue,
            Resolution::NotFound => "does not exist",
            Resolution::NotPublished => "is not published",
            Resolution::NotPermitted => "may not be used by this author",
        };
        out.push(Diagnostic::new(Code::E023, ptr, format!("'{reference}' {why}")));
    }
    for (i, n) in ix.doc.nodes.iter().enumerate() {
        let mut cands = Vec::new();
        for (field, a) in n.assignments() {
            for (c, cand) in a.candidates.iter().enumerate() {
                cands.push((format!("/nodes/{i}/{field}/candidates/{c}/ref"), cand));
            }
            if let Some(f) = &a.fallback {
                cands.push((format!("/nodes/{i}/{field}/fallback/ref"), f));
            }
        }
        for (k, esc) in n.escalations.iter().flatten().enumerate() {
            if let Some(t) = &esc.to {
                cands.push((format!("/nodes/{i}/escalations/{k}/to/ref"), t));
            }
        }
        for (ptr, cand) in cands {
            if cand.ty == CandidateType::DirectoryRule || cand.reference.as_expr().is_some() {
                continue;
            }
            if !env.principal_exists(cand.ty, &cand.reference.0) {
                out.push(
                    Diagnostic::new(
                        Code::E025,
                        ptr,
                        format!("{} '{}' does not exist", cand.ty.as_str(), cand.reference.0),
                    )
                    .at_node(&n.id),
                );
            }
        }
    }
}
