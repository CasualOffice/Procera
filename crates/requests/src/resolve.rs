//! Assignment resolution (docs/04 §4.6, docs/11 §3.1, docs/12 §10), at activation:
//!
//! 1. Expand every candidate against the current directory: `user` from a principal expression
//!    (`initiator.manager`, `beneficiary.manager`) or a member id, `group` by name, `role` by key
//!    (granted directly or through a group, unexpired, any scope), `position` by title (its holder).
//!    Only active members count. `directory_rule` candidates are not supported yet and expand to nobody.
//! 2. Apply SoD: `excludeInitiator` drops the initiator; `excludeActorsOf` drops whoever completed
//!    a work item of those steps on this instance.
//! 3. When nobody is left, expand the `fallback` the same way (SoD still applies).
//! 4. When still nobody is left the work item is created **unassigned**: visible to admins, never
//!    lost. The strategy is claim (first decision wins), quorum one.
//!
//! The snapshot of what each source resolved to is stored on the work item.

use std::collections::BTreeSet;

use procera_db::WsTx;
use procera_engine::Executable;
use procera_ir::model::{Assignment, Candidate, CandidateType};
use procera_runtime::{Evaluator, InstanceState, Scope};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

/// Who a step is offered to.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub candidates: Vec<Uuid>,
    pub snapshot: Value,
}

impl Resolution {
    pub fn unassigned(&self) -> bool {
        self.candidates.is_empty()
    }
}

/// Where the assignment sits on its node, for its compiled expressions: `assignment` or `to`.
#[derive(Debug, Clone, Copy)]
pub struct Site<'a> {
    pub node: &'a str,
    pub field: &'a str,
}

/// Member ids named by an expression result: a principal, a member id, or a list of either.
fn ids(v: &Value, out: &mut Vec<Uuid>) {
    match v {
        Value::String(s) => out.extend(Uuid::parse_str(s).ok()),
        Value::Object(o) => {
            if let Some(id) = o.get("id") {
                ids(id, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| ids(i, out)),
        _ => {}
    }
}

async fn expand(
    tx: &mut WsTx<'_>,
    exe: &Executable,
    st: &InstanceState,
    at: OffsetDateTime,
    node: &str,
    field: &str,
    c: &Candidate,
) -> Result<Vec<Uuid>, String> {
    let reference = match c.reference.as_expr() {
        Some(expr) => {
            let scope = Scope {
                variables: &st.variables,
                instance: &st.instance,
                initiator: &st.initiator,
                now: at,
                schema: exe.program.schema(),
            };
            let v = exe
                .evaluator
                .eval(&expr, exe.program.expression(node, field), &scope)
                .map_err(|e| format!("{node}/{field}: {e}"))?;
            match (c.ty, v) {
                (CandidateType::User, v) => {
                    let mut out = Vec::new();
                    ids(&v, &mut out);
                    return active(tx, &out).await;
                }
                (_, Value::Object(o)) => {
                    o.get("name").or(o.get("id")).and_then(Value::as_str).unwrap_or_default().to_owned()
                }
                (_, Value::String(s)) => s,
                _ => return Ok(vec![]),
            }
        }
        None => c.reference.0.clone(),
    };
    let sql = match c.ty {
        CandidateType::User => {
            let mut out = Vec::new();
            ids(&json!(reference), &mut out);
            return active(tx, &out).await;
        }
        CandidateType::Group => {
            "SELECT gm.membership_id FROM group_members gm JOIN groups g ON g.workspace_id = gm.workspace_id AND g.id = gm.group_id
             JOIN memberships m ON m.workspace_id = gm.workspace_id AND m.id = gm.membership_id
             WHERE lower(g.name) = lower($1) AND m.status = 'active' ORDER BY 1"
        }
        CandidateType::Role => {
            "SELECT m.id FROM memberships m WHERE m.status = 'active' AND EXISTS (
               SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
               WHERE r.key = $1 AND (g.expires_at IS NULL OR g.expires_at > now())
                 AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id)))
             ORDER BY 1"
        }
        CandidateType::Position => {
            "SELECT p.holder_membership_id FROM positions p JOIN memberships m ON m.workspace_id = p.workspace_id AND m.id = p.holder_membership_id
             WHERE lower(p.title) = lower($1) AND m.status = 'active' ORDER BY 1"
        }
        CandidateType::DirectoryRule => {
            tracing::warn!(node, "directory_rule candidates are not supported yet; resolving to nobody");
            return Ok(vec![]);
        }
    };
    sqlx::query_scalar(sql).bind(reference).fetch_all(tx.conn()).await.map_err(|e| e.to_string())
}

async fn active(tx: &mut WsTx<'_>, ids: &[Uuid]) -> Result<Vec<Uuid>, String> {
    sqlx::query_scalar("SELECT id FROM memberships WHERE id = ANY($1) AND status = 'active' ORDER BY id")
        .bind(ids)
        .fetch_all(tx.conn())
        .await
        .map_err(|e| e.to_string())
}

/// The members SoD excludes from a step: the initiator (when asked) and the actors of earlier
/// steps named by `excludeActorsOf`. Re-checked at decision time (docs/12 §10).
pub async fn excluded(
    tx: &mut WsTx<'_>,
    instance: Uuid,
    initiator: Option<Uuid>,
    exclude_initiator: bool,
    exclude_actors_of: &[String],
) -> Result<BTreeSet<Uuid>, String> {
    let mut out: BTreeSet<Uuid> = BTreeSet::new();
    if exclude_initiator {
        out.extend(initiator);
    }
    if !exclude_actors_of.is_empty() {
        let actors: Vec<Uuid> = sqlx::query_scalar(
            "SELECT completed_by FROM work_items WHERE instance_id = $1 AND node_id = ANY($2) AND completed_by IS NOT NULL",
        )
        .bind(instance)
        .bind(exclude_actors_of)
        .fetch_all(tx.conn())
        .await
        .map_err(|e| e.to_string())?;
        out.extend(actors);
    }
    Ok(out)
}

/// One candidate (an escalation `to`, compiled at `field` on `node`) expanded against the current
/// directory, minus `excluded`. Only active members count.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_candidate(
    tx: &mut WsTx<'_>,
    exe: &Executable,
    st: &InstanceState,
    at: OffsetDateTime,
    node: &str,
    field: &str,
    c: &Candidate,
    excluded: &BTreeSet<Uuid>,
) -> Result<Vec<Uuid>, String> {
    let members = expand(tx, exe, st, at, node, field, c).await?;
    Ok(members.into_iter().filter(|m| !excluded.contains(m)).collect::<BTreeSet<_>>().into_iter().collect())
}

/// Resolves an assignment. `sod` is false for notifications (`notify.to`).
pub async fn resolve(
    tx: &mut WsTx<'_>,
    exe: &Executable,
    st: &InstanceState,
    at: OffsetDateTime,
    site: Site<'_>,
    a: &Assignment,
    sod: bool,
) -> Result<Resolution, String> {
    let initiator = st.initiator.get("id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
    let excluded = if sod {
        let actors = a.exclude_actors_of.clone().unwrap_or_default();
        excluded(tx, st.instance_id, initiator, a.exclude_initiator == Some(true), &actors).await?
    } else {
        BTreeSet::new()
    };
    let mut sources = Vec::new();
    let mut found: BTreeSet<Uuid> = BTreeSet::new();
    for (i, c) in a.candidates.iter().enumerate() {
        let field = format!("{}/candidates/{i}/ref", site.field);
        let members = expand(tx, exe, st, at, site.node, &field, c).await?;
        sources.push(json!({ "type": c.ty.as_str(), "ref": c.reference.0, "members": members }));
        found.extend(members);
    }
    let mut left: Vec<Uuid> = found.difference(&excluded).copied().collect();
    let mut fallback_used = false;
    if left.is_empty()
        && let Some(f) = &a.fallback
    {
        fallback_used = true;
        let field = format!("{}/fallback/ref", site.field);
        let members = expand(tx, exe, st, at, site.node, &field, f).await?;
        sources.push(
            json!({ "type": f.ty.as_str(), "ref": f.reference.0, "members": members, "fallback": true }),
        );
        left = members
            .into_iter()
            .filter(|m| !excluded.contains(m))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    let snapshot = json!({
        "sources": sources,
        "excluded": excluded,
        "fallback_used": fallback_used,
        "strategy": "claim",
    });
    Ok(Resolution { candidates: left, snapshot })
}
