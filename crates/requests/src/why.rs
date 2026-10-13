//! "Why this happened" (PR-08): plain lines that explain each routing decision and why each step
//! went to the people it went to, built from facts recorded when they happened.
//!
//! - Routes come from `instance_decisions` (the engine's decision trace): the branch taken, the
//!   conditions checked and the values they read. "Sent to Finance approval because the amount
//!   640.00 EUR is over 500."
//! - Assignments come from the work item's `resolution` snapshot: the rule behind each candidate
//!   source, the SoD exclusions and out-of-office substitutions. "Ana Manager decides Manager
//!   approval as Ben's manager."
//!
//! Restricted values are never stored, so they can never appear here.

use std::collections::{BTreeMap, BTreeSet};

use procera_db::WsTx;
use procera_ir::model::NodeKind;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::RequestError;
use crate::catalogue::RequestType;
use crate::handler::name;
use crate::views::{Release, WhyLine};

type R<T> = Result<T, RequestError>;

async fn who(tx: &mut WsTx<'_>, m: Uuid) -> R<String> {
    name(tx, m).await.map_err(RequestError::Internal)
}

fn uuids(v: &Value) -> Vec<Uuid> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str())
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect()
}

fn list(names: &[String]) -> String {
    match names {
        [] => "nobody".to_owned(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// A value as a person reads it: money as "640.00 EUR", text as itself.
fn shown(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => {
            match (o.get("amount").and_then(Value::as_str), o.get("currency").and_then(Value::as_str)) {
                (Some(a), Some(c)) => Some(format!("{a} {c}")),
                (Some(a), None) => Some(a.to_owned()),
                _ => None,
            }
        }
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A literal as written ("500.0" → "500", `"high"` → "high"), or `None` for anything else.
fn literal(s: &str) -> Option<String> {
    if let Some(q) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(q.to_owned());
    }
    if let Some(q) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return Some(q.to_owned());
    }
    s.parse::<f64>().ok().map(|_| {
        let t = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s };
        t.to_owned()
    })
}

/// One condition in words: "the amount 640.00 EUR is over 500" (or its negation when it failed).
/// Anything beyond `<input> <op> <literal>` is quoted as written.
fn condition(rt: &RequestType, check: &Value, inputs: &Value) -> String {
    let src = check["expression"].as_str().unwrap_or_default();
    let held = check["result"].as_bool().unwrap_or(false);
    let parts: Vec<&str> = src.split_whitespace().collect();
    if let [lhs, op, rhs] = parts.as_slice()
        && let Some(lit) = literal(rhs)
    {
        let root = lhs.split('.').next().unwrap_or_default();
        let words = match (*op, held) {
            (">", true) | ("<=", false) => Some("is over"),
            (">", false) | ("<=", true) => Some("is not over"),
            (">=", true) | ("<", false) => Some("is at least"),
            (">=", false) | ("<", true) => Some("is under"),
            ("==", true) | ("!=", false) => Some("is"),
            ("==", false) | ("!=", true) => Some("is not"),
            _ => None,
        };
        if let Some(words) = words {
            let label = rt
                .start_form
                .fields
                .iter()
                .find(|f| f.name == root)
                .map_or_else(|| root.replace('_', " "), |f| f.label.to_lowercase());
            let value = inputs[root].get("value").and_then(shown);
            return match value {
                Some(v) => format!("the {label} {v} {words} {lit}"),
                None => format!("the {label} {words} {lit}"),
            };
        }
    }
    if held { format!("\u{201c}{src}\u{201d} held") } else { format!("\u{201c}{src}\u{201d} did not hold") }
}

/// The text of one routing decision.
fn route_text(rt: &RequestType, node: &str, branch: &str, facts: &Value) -> String {
    let program = &rt.exe.program;
    let target = program.target(node, branch).and_then(|t| program.node(t));
    let verb = match target {
        Some(t) if t.kind == NodeKind::End => format!("Finished as {}", rt.node_name(&t.id)),
        Some(t) if matches!(t.kind, NodeKind::Decision) => format!("Went on to {}", rt.node_name(&t.id)),
        Some(t) => format!("Sent to {}", rt.node_name(&t.id)),
        None => format!("Took the {branch} branch"),
    };
    let checks: Vec<&Value> = facts["checks"].as_array().into_iter().flatten().collect();
    if let Some(rule) = facts["rule"].as_str() {
        return format!("{verb}, as decided by the rule {rule}");
    }
    let reasons: Vec<String> = if branch == "default" {
        checks.iter().map(|c| condition(rt, c, &facts["inputs"])).collect()
    } else {
        checks
            .iter()
            .filter(|c| c["result"] == json!(true))
            .map(|c| condition(rt, c, &facts["inputs"]))
            .collect()
    };
    if reasons.is_empty() { verb } else { format!("{verb} because {}", reasons.join(" and ")) }
}

/// Whose manager a `manager_of` rule means: the requester, or a principal field of the request.
async fn subject(tx: &mut WsTx<'_>, of: &str, requester: &str, vars: &BTreeMap<String, Value>) -> R<String> {
    if of == "initiator" {
        return Ok(requester.to_owned());
    }
    match vars.get(of).and_then(|v| v.get("id")).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok())
    {
        Some(m) => who(tx, m).await,
        None => Ok(format!("the {}", of.replace('_', " "))),
    }
}

/// The candidate rule as a phrase about the people it picked ("Ben's manager", "holders of the
/// finance.approver role"); `you` phrases it for the viewer ("You're Ben's manager").
async fn rule_phrase(
    tx: &mut WsTx<'_>,
    rule: &Value,
    requester: &str,
    vars: &BTreeMap<String, Value>,
    many: bool,
) -> R<Option<String>> {
    Ok(Some(match rule["kind"].as_str() {
        Some("manager_of") => {
            let of = subject(tx, rule["of"].as_str().unwrap_or("initiator"), requester, vars).await?;
            let levels = rule["levels"].as_u64().unwrap_or(1);
            format!("{of}'s {}", vec!["manager"; usize::try_from(levels).unwrap_or(1)].join("'s "))
        }
        Some("requester") => "the requester".to_owned(),
        Some("field") => {
            format!("the {} on this request", rule["name"].as_str().unwrap_or_default().replace('_', " "))
        }
        Some("named") => "the person the process names".to_owned(),
        Some("group") if many => {
            format!("members of the {} group", rule["name"].as_str().unwrap_or_default())
        }
        Some("group") => format!("a member of the {} group", rule["name"].as_str().unwrap_or_default()),
        Some("role") if many => format!("holders of the {} role", rule["key"].as_str().unwrap_or_default()),
        Some("role") => format!("holder of the {} role", rule["key"].as_str().unwrap_or_default()),
        Some("position") => format!("the {}", rule["title"].as_str().unwrap_or_default()),
        Some("expression") => format!("picked by the rule {}", rule["source"].as_str().unwrap_or_default()),
        _ => return Ok(None),
    }))
}

/// The lines for one step's assignment.
#[allow(clippy::too_many_arguments)]
async fn assignment_lines(
    tx: &mut WsTx<'_>,
    rt: &RequestType,
    at: OffsetDateTime,
    step: &str,
    node: &str,
    resolution: &Value,
    unassigned: bool,
    requester: &str,
    vars: &BTreeMap<String, Value>,
) -> R<Vec<WhyLine>> {
    let mut out = Vec::new();
    let excluded: BTreeSet<Uuid> = uuids(&resolution["excluded"]).into_iter().collect();
    for src in resolution["sources"].as_array().into_iter().flatten() {
        let members: Vec<Uuid> =
            uuids(&src["members"]).into_iter().filter(|m| !excluded.contains(m)).collect();
        if members.is_empty() || src.get("rule").is_none() {
            continue;
        }
        let mut names = Vec::new();
        for m in &members {
            names.push(who(tx, *m).await?);
        }
        let many = names.len() > 1;
        let Some(phrase) = rule_phrase(tx, &src["rule"], requester, vars, many).await? else { continue };
        let verb = if many { "can decide" } else { "decides" };
        let fallback = src["fallback"] == json!(true);
        let mut text = format!("{} {verb} {step} as {phrase}", list(&names));
        if fallback {
            text.push_str(" (the fallback: nobody else could decide)");
        }
        out.push(WhyLine {
            at,
            step: step.to_owned(),
            text,
            facts: json!({ "node": node, "kind": "assignment", "rule": src["rule"], "fallback": fallback, "people": names }),
        });
    }
    // Only exclusions that took someone off the step are worth saying.
    let offered: BTreeSet<Uuid> =
        resolution["sources"].as_array().into_iter().flatten().flat_map(|s| uuids(&s["members"])).collect();
    for ex in resolution["excluded_why"].as_array().into_iter().flatten() {
        let Some(m) = ex["member"].as_str().and_then(|s| Uuid::parse_str(s).ok()) else { continue };
        if !offered.contains(&m) {
            continue;
        }
        let n = who(tx, m).await?;
        let why = match ex["reason"].as_str() {
            Some("requester") => "they started this request".to_owned(),
            _ => format!("they decided {}", rt.node_name(ex["step"].as_str().unwrap_or_default())),
        };
        out.push(WhyLine {
            at,
            step: step.to_owned(),
            text: format!("{n} is left out of {step} because {why} (separation of duties)"),
            facts: json!({ "node": node, "kind": "excluded", "person": n, "reason": ex["reason"], "step": ex["step"] }),
        });
    }
    for s in resolution["substitutions"].as_array().into_iter().flatten() {
        let (Some(f), Some(d)) = (s["for"].as_str(), s["delegate"].as_str()) else { continue };
        if s.get("skipped").is_some_and(|v| !v.is_null()) {
            continue;
        }
        let (Ok(f), Ok(d)) = (Uuid::parse_str(f), Uuid::parse_str(d)) else { continue };
        let (fname, dname) = (who(tx, f).await?, who(tx, d).await?);
        out.push(WhyLine {
            at,
            step: step.to_owned(),
            text: format!("{dname} covers {step} for {fname}, who is out of office"),
            facts: json!({ "node": node, "kind": "substitution", "for": fname, "delegate": dname }),
        });
    }
    if unassigned {
        out.push(WhyLine {
            at,
            step: step.to_owned(),
            text: format!("Nobody could be found to decide {step}; an admin needs to assign it"),
            facts: json!({ "node": node, "kind": "unassigned" }),
        });
    }
    Ok(out)
}

/// The "why" lines of a request, oldest first, and the build it runs on.
pub(crate) async fn explain(
    tx: &mut WsTx<'_>,
    rt: &RequestType,
    id: Uuid,
    requester: &str,
    vars: &BTreeMap<String, Value>,
) -> R<(Vec<WhyLine>, Release)> {
    let (version, checksum): (i32, Option<String>) =
        sqlx::query_as("SELECT process_version, program_checksum FROM process_instances WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
    let release = Release { version: u32::try_from(version).unwrap_or_default(), checksum };
    let mut lines: Vec<(OffsetDateTime, u8, WhyLine)> = Vec::new();
    let routes: Vec<(String, String, Value, OffsetDateTime, i32, Option<String>)> = sqlx::query_as(
        "SELECT node_id, branch, facts, decided_at, process_version, program_checksum FROM instance_decisions
          WHERE instance_id = $1 ORDER BY seq, ord",
    )
    .bind(id)
    .fetch_all(tx.conn())
    .await?;
    for (node, branch, facts, at, version, checksum) in routes {
        let text = route_text(rt, &node, &branch, &facts);
        let mut f = facts.clone();
        f["kind"] = json!("route");
        f["node"] = json!(node);
        f["branch"] = json!(branch);
        f["release"] = json!({ "version": version, "checksum": checksum });
        lines.push((at, 0, WhyLine { at, step: rt.node_name(&node), text, facts: f }));
    }
    let items: Vec<(String, String, OffsetDateTime, Value, bool)> = sqlx::query_as(
        "SELECT node_id, name, created_at, resolution, unassigned FROM work_items WHERE instance_id = $1 ORDER BY created_at, token_id, work_key",
    )
    .bind(id)
    .fetch_all(tx.conn())
    .await?;
    for (node, step, at, resolution, unassigned) in items {
        for l in assignment_lines(tx, rt, at, &step, &node, &resolution, unassigned, requester, vars).await? {
            lines.push((at, 1, l));
        }
    }
    lines.sort_by_key(|(at, k, _)| (*at, *k));
    Ok((lines.into_iter().map(|(_, _, l)| l).collect(), release))
}

/// One line for the decision panel: why this step is offered to `me` ("You're Ben's manager").
pub(crate) async fn why_you(
    tx: &mut WsTx<'_>,
    resolution: &Value,
    me: Uuid,
    requester: &str,
    vars: &BTreeMap<String, Value>,
) -> R<Option<String>> {
    let mine = |v: &Value| uuids(v).contains(&me);
    for s in resolution["substitutions"].as_array().into_iter().flatten() {
        if s["delegate"].as_str() == Some(me.to_string().as_str())
            && s.get("skipped").is_none_or(Value::is_null)
            && let Some(f) = s["for"].as_str().and_then(|x| Uuid::parse_str(x).ok())
        {
            return Ok(Some(format!("You're covering for {}, who is out of office", who(tx, f).await?)));
        }
    }
    // A reassignment replaces the candidates, so the latest one is why a current holder has it.
    if let Some(h) = resolution["history"]
        .as_array()
        .and_then(|h| h.iter().rev().find(|e| e["action"] == json!("reassigned")))
    {
        return Ok(Some(match h["by"].as_str() {
            Some(b) => format!("{b} reassigned this step to you"),
            None => "This step was escalated to you".to_owned(),
        }));
    }
    for src in resolution["sources"].as_array().into_iter().flatten() {
        if !mine(&src["members"]) {
            continue;
        }
        let rule = &src["rule"];
        let text = match rule["kind"].as_str() {
            Some("manager_of") => {
                let p = rule_phrase(tx, rule, requester, vars, false).await?.unwrap_or_default();
                format!("You're {p}")
            }
            Some("requester") => "You started this request".to_owned(),
            Some("group") => format!("You're in the {} group", rule["name"].as_str().unwrap_or_default()),
            Some("role") => format!("You have the {} role", rule["key"].as_str().unwrap_or_default()),
            Some("position") => format!("You're the {}", rule["title"].as_str().unwrap_or_default()),
            Some("field") => {
                format!(
                    "You're the {} on this request",
                    rule["name"].as_str().unwrap_or_default().replace('_', " ")
                )
            }
            Some(_) => "The process picked you".to_owned(),
            None => continue,
        };
        let fallback = if src["fallback"] == json!(true) { " (nobody else could decide)" } else { "" };
        return Ok(Some(format!("{text}{fallback}")));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_values_read_plainly() {
        assert_eq!(literal("500.0").as_deref(), Some("500"));
        assert_eq!(literal("50000").as_deref(), Some("50000"));
        assert_eq!(literal("\"high\"").as_deref(), Some("high"));
        assert_eq!(literal("amount"), None);
        assert_eq!(shown(&json!({ "amount": "640.00", "currency": "EUR" })).as_deref(), Some("640.00 EUR"));
        assert_eq!(list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
    }
}
