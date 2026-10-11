//! Audit query language (BPMS-064; docs/08 §7.5 `POST /audit/query`): a filter tree of conditions and
//! AND/OR groups (nesting depth ≤ 2, ≤ 25 conditions) over allow-listed fields, sorted on the chain
//! sequence, keyset-paginated. Values are always bound, never spliced into SQL.

use serde::Deserialize;
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::WsTx;
use crate::explorer::{CATEGORIES, EventPage, EventView, ExplorerError, RESULTS};

const MAX_DEPTH: usize = 2;
const MAX_CONDITIONS: usize = 25;
pub const MAX_PAGE: i64 = 500;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Node {
    And { and: Vec<Node> },
    Or { or: Vec<Node> },
    Cond(Cond),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cond {
    pub field: String,
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sort {
    pub field: String,
    pub dir: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub size: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryReq {
    pub filter: Option<Node>,
    #[serde(default)]
    pub sort: Vec<Sort>,
    #[serde(default)]
    pub page: Page,
}

fn bad(m: impl Into<String>) -> ExplorerError {
    ExplorerError::Invalid(m.into())
}

fn s(v: &Value, field: &str) -> Result<String, ExplorerError> {
    v.as_str()
        .map(str::to_owned)
        .filter(|x| !x.is_empty() && x.len() <= 200 && !x.chars().any(char::is_control))
        .ok_or_else(|| bad(format!("{field}: value must be text")))
}

fn uuid(v: &Value, field: &str) -> Result<Uuid, ExplorerError> {
    s(v, field)?.parse().map_err(|_| bad(format!("{field}: value must be an id")))
}

fn time(v: &Value, field: &str) -> Result<OffsetDateTime, ExplorerError> {
    OffsetDateTime::parse(&s(v, field)?, &time::format_description::well_known::Rfc3339)
        .map_err(|_| bad(format!("{field}: value must be an RFC 3339 time")))
}

fn strings(v: &Value, field: &str, allowed: Option<&[&str]>) -> Result<Vec<String>, ExplorerError> {
    let list = v
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 20)
        .ok_or_else(|| bad(format!("{field}: value must be a list of 1–20 items")))?;
    let out: Vec<String> = list.iter().map(|x| s(x, field)).collect::<Result<_, _>>()?;
    if let Some(ok) = allowed
        && let Some(x) = out.iter().find(|x| !ok.contains(&x.as_str()))
    {
        return Err(bad(format!("{field}: unknown value {x}")));
    }
    Ok(out)
}

/// ISO-8601 days/hours duration like `P30D`, `PT12H`, `P1DT6H` (for `within`).
pub fn parse_duration(v: &str) -> Option<Duration> {
    let rest = v.strip_prefix('P')?;
    let (days, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut total = Duration::ZERO;
    if !days.is_empty() {
        total +=
            Duration::days(days.strip_suffix('D')?.parse::<i64>().ok().filter(|d| (0..=3660).contains(d))?);
    }
    if let Some(t) = time {
        let mut num = String::new();
        for c in t.chars() {
            if c.is_ascii_digit() {
                num.push(c);
                continue;
            }
            let n: i64 = num.parse().ok()?;
            num.clear();
            total += match c {
                'H' => Duration::hours(n),
                'M' => Duration::minutes(n),
                'S' => Duration::seconds(n),
                _ => return None,
            };
        }
        if !num.is_empty() {
            return None;
        }
    }
    (total > Duration::ZERO).then_some(total)
}

fn count(n: &Node, depth: usize) -> Result<usize, ExplorerError> {
    match n {
        Node::Cond(_) => Ok(1),
        Node::And { and: k } | Node::Or { or: k } => {
            if depth >= MAX_DEPTH {
                return Err(bad(format!("groups nest at most {MAX_DEPTH} deep")));
            }
            if k.is_empty() {
                return Err(bad("a group needs at least one condition"));
            }
            k.iter().map(|c| count(c, depth + 1)).sum()
        }
    }
}

fn push_cond(
    qb: &mut QueryBuilder<'_, Postgres>,
    c: &Cond,
    now: OffsetDateTime,
) -> Result<(), ExplorerError> {
    let f = c.field.as_str();
    match (f, c.op.as_str()) {
        ("occurred_at", "within") => {
            let d = parse_duration(&s(&c.value, f)?)
                .ok_or_else(|| bad("occurred_at within: value is a duration like P30D or PT12H"))?;
            qb.push("e.occurred_at >= ").push_bind(now - d);
        }
        ("occurred_at", "gte") => {
            qb.push("e.occurred_at >= ").push_bind(time(&c.value, f)?);
        }
        ("occurred_at", "lt") => {
            qb.push("e.occurred_at < ").push_bind(time(&c.value, f)?);
        }
        ("actor", "eq") => {
            qb.push("e.actor_subject_ref = ").push_bind(uuid(&c.value, f)?);
        }
        ("object_id", "eq") => {
            qb.push("e.object_id = ").push_bind(uuid(&c.value, f)?);
        }
        ("category", "eq") | ("result", "eq") | ("object_type", "eq") | ("action", "eq") | ("ip", "eq") => {
            let v = s(&c.value, f)?;
            if f == "category" && !CATEGORIES.contains(&v.as_str()) {
                return Err(bad(format!("category: unknown value {v}")));
            }
            if f == "result" && !RESULTS.contains(&v.as_str()) {
                return Err(bad(format!("result: unknown value {v}")));
            }
            let col = if f == "ip" { "host(e.ip)".to_owned() } else { format!("e.{f}") };
            qb.push(col).push(" = ").push_bind(v);
        }
        ("category", "in") | ("result", "in") | ("action", "in") => {
            let allowed: Option<&[&str]> = match f {
                "category" => Some(CATEGORIES),
                "result" => Some(RESULTS),
                _ => None,
            };
            qb.push(format!("e.{f} = ANY(")).push_bind(strings(&c.value, f, allowed)?).push(")");
        }
        ("action", "prefix") => {
            let p = s(&c.value, f)?.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            qb.push("e.action LIKE ").push_bind(format!("{p}%"));
        }
        // A field named in an event's recorded diff (e.g. which restricted field a data-access read).
        ("object_field", "eq") => {
            let v = s(&c.value, f)?;
            qb.push("(e.diff ? ")
                .push_bind(v.clone())
                .push(" OR e.diff->>'field' = ")
                .push_bind(v.clone())
                .push(" OR e.diff->'fields' ? ")
                .push_bind(v)
                .push(")");
        }
        _ => return Err(bad(format!("unsupported condition {f} {}", c.op))),
    }
    Ok(())
}

fn push_node(
    qb: &mut QueryBuilder<'_, Postgres>,
    n: &Node,
    now: OffsetDateTime,
) -> Result<(), ExplorerError> {
    match n {
        Node::Cond(c) => push_cond(qb, c, now),
        Node::And { and: k } | Node::Or { or: k } => {
            let joiner = if matches!(n, Node::And { .. }) { " AND " } else { " OR " };
            qb.push("(");
            for (i, c) in k.iter().enumerate() {
                if i > 0 {
                    qb.push(joiner);
                }
                push_node(qb, c, now)?;
            }
            qb.push(")");
            Ok(())
        }
    }
}

/// Runs a query. `own` limits it to one actor (audit:view_own); `now` anchors `within`.
pub async fn run(
    tx: &mut WsTx<'_>,
    req: &QueryReq,
    own: Option<Uuid>,
    now: OffsetDateTime,
) -> Result<EventPage, ExplorerError> {
    if let Some(f) = &req.filter
        && count(f, 0)? > MAX_CONDITIONS
    {
        return Err(bad(format!("at most {MAX_CONDITIONS} conditions")));
    }
    let desc = match req.sort.as_slice() {
        [] => true,
        [s] if s.field == "occurred_at" || s.field == "seq" => match s.dir.as_str() {
            "desc" => true,
            "asc" => false,
            _ => return Err(bad("sort dir is asc or desc")),
        },
        _ => return Err(bad("sort by occurred_at (one key)")),
    };
    let size = req.page.size.unwrap_or(100).clamp(1, MAX_PAGE);
    let cursor = crate::explorer::parse_cursor(req.page.cursor.as_deref())?;
    let mut qb = QueryBuilder::<Postgres>::new(crate::explorer::COLS);
    if let Some(f) = &req.filter {
        qb.push(" AND ");
        push_node(&mut qb, f, now)?;
    }
    if let Some(v) = own {
        qb.push(" AND e.actor_subject_ref = ").push_bind(v);
    }
    if let Some(c) = cursor {
        qb.push(if desc { " AND e.seq < " } else { " AND e.seq > " }).push_bind(c);
    }
    qb.push(if desc { " ORDER BY e.seq DESC LIMIT " } else { " ORDER BY e.seq ASC LIMIT " })
        .push_bind(size + 1);
    let mut rows: Vec<crate::explorer::Raw> = qb.build_query_as().fetch_all(tx.conn()).await?;
    let more = rows.len() as i64 > size;
    rows.truncate(usize::try_from(size).unwrap_or(0));
    let next_cursor = if more { rows.last().map(|r| r.seq.to_string()) } else { None };
    Ok(EventPage { items: rows.into_iter().map(EventView::from).collect(), next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durations_and_tree_limits() {
        assert_eq!(parse_duration("P30D"), Some(Duration::days(30)));
        assert_eq!(parse_duration("PT12H"), Some(Duration::hours(12)));
        assert_eq!(
            parse_duration("P1DT6H30M"),
            Some(Duration::days(1) + Duration::hours(6) + Duration::minutes(30))
        );
        assert!(
            parse_duration("30D").is_none()
                && parse_duration("P").is_none()
                && parse_duration("P1W").is_none()
        );
        let deep: Node = serde_json::from_value(
            json!({"and": [{"or": [{"and": [{"field": "category", "op": "eq", "value": "auth"}]}]}]}),
        )
        .expect("parses");
        assert!(count(&deep, 0).is_err(), "depth > 2 is refused");
        let ok: Node = serde_json::from_value(json!({"and": [{"field": "category", "op": "eq", "value": "auth"}, {"or": [{"field": "result", "op": "eq", "value": "denied"}, {"field": "action", "op": "prefix", "value": "login."}]}]})).expect("parses");
        assert_eq!(count(&ok, 0).expect("valid"), 3);
    }
}
