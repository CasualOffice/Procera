//! The decision trace (PR-08, `instance_decisions`): for every decision node an advance passed
//! through, the branch taken, the conditions checked in order (source text and result) and the
//! inputs those conditions read, with their values.
//!
//! The interpreter stays pure: the trace is rebuilt here from the step's new trace entries
//! (`node_left` on a decision node) and the compiled expressions, in the advance transaction.
//! Values come from the state after the step; an input set again later in the same advance is
//! recorded without a value (`changed_after`), so a stored value is always the one the condition
//! saw. Restricted inputs never have a value stored.

use procera_db::WsTx;
use procera_ir::model::NodeKind;
use procera_runtime::{InstanceState, TraceEvent};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use crate::EngineError;
use crate::engine::{Executable, ProgramRef};

/// One decision, as stored in `instance_decisions.facts` (plus its node and branch).
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub node: String,
    pub branch: String,
    pub facts: Value,
}

/// The decisions taken by the trace entries from index `from` on.
pub fn decisions(exe: &Executable, st: &InstanceState, from: usize) -> Vec<Decision> {
    let new = st.trace.get(from..).unwrap_or(&st.trace[..]);
    let mut out = Vec::new();
    for (i, entry) in new.iter().enumerate() {
        let (TraceEvent::NodeLeft { outcome }, Some(id)) = (&entry.event, &entry.node) else { continue };
        let Some(node) = exe.program.node(id).filter(|n| n.kind == NodeKind::Decision) else { continue };
        let later = &new[i + 1..];
        let set_later = |name: &str| {
            later.iter().any(
                |e| matches!(&e.event, TraceEvent::VariablesSet { names } if names.iter().any(|n| n == name)),
            )
        };
        let mut checks = Vec::new();
        let mut inputs = Map::new();
        if let Some(rule) = &node.rule {
            out.push(Decision {
                node: id.clone(),
                branch: outcome.clone(),
                facts: json!({ "rule": rule, "output": node.output, "checks": [], "inputs": {} }),
            });
            continue;
        }
        for (b, branch) in node.branches.iter().flatten().enumerate() {
            if branch.when.is_none() {
                continue;
            }
            let Some(expr) = exe.program.expression(id, &format!("branches/{b}/when")) else { continue };
            let taken = &branch.name == outcome;
            checks.push(json!({ "branch": branch.name, "expression": expr.source, "result": taken }));
            for (name, class) in &expr.reads {
                let class = serde_json::to_value(class).unwrap_or(Value::Null);
                let mut fact = json!({ "classification": class });
                if class == "restricted" {
                    fact["redacted"] = json!(true);
                } else if set_later(name) {
                    fact["changed_after"] = json!(true);
                } else if let Some(v) = st.variables.get(name) {
                    fact["value"] = v.clone();
                }
                inputs.insert(name.clone(), fact);
            }
            if taken {
                break;
            }
        }
        out.push(Decision {
            node: id.clone(),
            branch: outcome.clone(),
            facts: json!({ "checks": checks, "inputs": inputs }),
        });
    }
    out
}

/// Stores the decisions of one advance under the instance revision `st.revision`.
pub(crate) async fn record(
    tx: &mut WsTx<'_>,
    program: &ProgramRef,
    exe: &Executable,
    st: &InstanceState,
    from: usize,
    at: OffsetDateTime,
) -> Result<(), EngineError> {
    let seq = i64::try_from(st.revision).map_err(|_| EngineError::State("revision out of range".into()))?;
    let version =
        i32::try_from(program.version).map_err(|_| EngineError::Program("version out of range".into()))?;
    for (ord, d) in (0_i32..).zip(decisions(exe, st, from)) {
        sqlx::query(
            "INSERT INTO instance_decisions (workspace_id, instance_id, seq, ord, node_id, branch, facts, process_version, program_checksum, decided_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(st.instance_id)
        .bind(seq)
        .bind(ord)
        .bind(&d.node)
        .bind(&d.branch)
        .bind(&d.facts)
        .bind(version)
        .bind(&program.checksum)
        .bind(at)
        .execute(tx.conn())
        .await?;
    }
    Ok(())
}
