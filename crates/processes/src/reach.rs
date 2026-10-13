//! "Who will this go to" (BPMS-100, docs/05 §3.5): each candidate of a step in the designer,
//! expanded against the workspace directory as it is now, with the same rules as the runtime's
//! resolver (only active members; a role counts when granted directly or through a group and
//! unexpired). Candidates that depend on the request (the requester, their manager) have no count;
//! for the manager the note says how many people have none set. Nothing is written.

use procera_db::WsTx;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A candidate as the designer writes it (`{type, ref}` in the IR).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Who one candidate reaches today.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Reach {
    /// How many active people it reaches; `None` when that depends on who asks.
    pub count: Option<usize>,
    /// The first few of them, by name.
    pub people: Vec<String>,
    pub note: Option<String>,
}

const SHOWN: usize = 5;

/// The reach of each candidate, in order.
pub async fn reach(tx: &mut WsTx<'_>, candidates: &[Candidate]) -> Result<Vec<Reach>, sqlx::Error> {
    let mut out = Vec::with_capacity(candidates.len());
    for c in candidates {
        out.push(one(tx, c).await?);
    }
    Ok(out)
}

async fn one(tx: &mut WsTx<'_>, c: &Candidate) -> Result<Reach, sqlx::Error> {
    let r = c.reference.trim();
    if r.starts_with("expr:") {
        let note = if r == "expr:cel: initiator.manager" {
            let none: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM memberships WHERE status = 'active' AND manager_membership_id IS NULL",
            )
            .fetch_one(tx.conn())
            .await?;
            match none {
                0 => "Everyone has a manager set.".to_owned(),
                1 => "1 person has no manager set; their requests go to the fallback.".to_owned(),
                n => format!("{n} people have no manager set; their requests go to the fallback."),
            }
        } else {
            "Depends on who asks.".to_owned()
        };
        return Ok(Reach { count: None, people: vec![], note: Some(note) });
    }
    let sql = match c.ty.as_str() {
        "group" => {
            "SELECT m.id, u.display_name FROM group_members gm JOIN groups g ON g.workspace_id = gm.workspace_id AND g.id = gm.group_id
               JOIN memberships m ON m.workspace_id = gm.workspace_id AND m.id = gm.membership_id JOIN users u ON u.id = m.user_id
              WHERE lower(g.name) = lower($1) AND m.status = 'active'"
        }
        "role" => {
            "SELECT m.id, u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.status = 'active' AND EXISTS (
               SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
               WHERE r.key = $1 AND (g.expires_at IS NULL OR g.expires_at > now())
                 AND (g.membership_id = m.id OR g.group_id IN (SELECT gm.group_id FROM group_members gm WHERE gm.membership_id = m.id)))"
        }
        "position" => {
            "SELECT m.id, u.display_name FROM positions p JOIN memberships m ON m.workspace_id = p.workspace_id AND m.id = p.holder_membership_id
               JOIN users u ON u.id = m.user_id WHERE lower(p.title) = lower($1) AND m.status = 'active'"
        }
        "user" => {
            "SELECT m.id, u.display_name FROM memberships m JOIN users u ON u.id = m.user_id
              WHERE m.status = 'active' AND (lower(u.email::text) = lower($1) OR m.id::text = $1)"
        }
        _ => return Ok(Reach { count: None, people: vec![], note: Some("Depends on the directory rule.".into()) }),
    };
    let mut rows: Vec<(Uuid, String)> = sqlx::query_as(sql).bind(r).fetch_all(tx.conn()).await?;
    rows.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    rows.dedup_by_key(|x| x.0);
    let note = rows.is_empty().then(|| "Nobody yet: it would go to the fallback.".to_owned());
    Ok(Reach {
        count: Some(rows.len()),
        people: rows.iter().take(SHOWN).map(|x| x.1.clone()).collect(),
        note,
    })
}
