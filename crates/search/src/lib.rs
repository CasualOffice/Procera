//! Workspace search (BPMS-049, docs/04 §6, docs/05 §2.4).
//!
//! - **Full-text** with `websearch_to_tsquery` over the `english` and `simple` configurations, so
//!   stemmed words ("laptops" finds "laptop") and exact short words ("IT") both match.
//! - **Fuzzy and prefix** with `pg_trgm`: substring (`ILIKE`, trigram-indexed from 3 characters)
//!   word similarity (`<%`) and whole-string similarity (`%`) catch typos and first letters.
//! - **Index freshness:** artifacts carry a trigger-maintained `tsv` (migration 0015); members,
//!   departments and groups are matched live through expression indexes, so they are never stale.
//! - **Permission filtering** happens before the query: the caller passes only the result types the
//!   member may view (the HTTP layer asks the policy evaluator), and forced RLS scopes every row to
//!   the workspace. Nothing classified as restricted is indexed (library content has no classified
//!   fields; member data is name, email and title).
//! - **Snippets** come from `ts_headline` with control-character delimiters, then are HTML-escaped
//!   here and the delimiters turned into `<mark>`, so indexed text can never inject markup.

use procera_db::WsTx;
use serde::Serialize;
use uuid::Uuid;

/// Result types, in facet order. Artifact kinds use their kind name.
pub const TYPES: &[&str] = &["sop", "policy", "member", "department", "group"];
pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_LIMIT: i64 = 50;
/// `<%` threshold (pg_trgm.word_similarity_threshold) used for fuzzy matches.
pub const WORD_SIMILARITY: f32 = 0.45;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for SearchError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Uuid,
    pub title: String,
    pub subtitle: Option<String>,
    /// HTML-escaped excerpt with matched terms wrapped in `<mark>…</mark>`.
    pub snippet: Option<String>,
    pub score: f32,
}

/// Parses a comma-separated `types` parameter. `None` or empty means every type.
pub fn parse_types(raw: Option<&str>) -> Result<Vec<&'static str>, SearchError> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else { return Ok(TYPES.to_vec()) };
    let mut out = Vec::new();
    for t in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let known = TYPES
            .iter()
            .copied()
            .find(|k| *k == t)
            .ok_or_else(|| SearchError::Invalid(format!("types must be among {}", TYPES.join(", "))))?;
        if !out.contains(&known) {
            out.push(known);
        }
    }
    Ok(out)
}

/// Normalises a query: trims, drops control characters, enforces 1..=200 characters.
pub fn clean_query(q: &str) -> Result<String, SearchError> {
    let q: String = q.chars().filter(|c| !c.is_control()).collect();
    let q = q.trim();
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS {
        return Err(SearchError::Invalid(format!("q must be 1-{MAX_QUERY_CHARS} characters")));
    }
    Ok(q.to_owned())
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

const MARK_START: char = '\u{2}';
const MARK_END: char = '\u{3}';

/// HTML-escapes a headline and turns the delimiters into `<mark>` elements.
pub fn render_snippet(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(raw.len() + 16);
    for c in raw.chars() {
        match c {
            MARK_START => out.push_str("<mark>"),
            MARK_END => out.push_str("</mark>"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    Some(out)
}

// One statement: candidates from every allowed type, ranked together, top `limit` kept, and only
// then headlines computed (ts_headline re-parses the text, so it runs on the page only).
// Rank = text rank + trigram word similarity of the title + a bonus when the title starts with q.
const SQL: &str = r#"
WITH q AS (
  SELECT websearch_to_tsquery('english', $1) || websearch_to_tsquery('simple', $1) AS ts,
         lower($1) AS raw, $2::text AS pat, $3::text AS prefix, $4::text[] AS types
), hits AS (
  SELECT a.kind AS type, a.id, a.title, a.key AS subtitle, a.search_body AS body, 'english'::regconfig AS cfg,
         ts_rank_cd(a.tsv, q.ts) + word_similarity(q.raw, lower(a.title))
           + CASE WHEN lower(a.title) LIKE q.prefix THEN 0.5::real ELSE 0::real END AS score
  FROM artifacts a, q
  WHERE a.kind = ANY(q.types) AND (a.tsv @@ q.ts OR a.title ILIKE q.pat OR q.raw <% a.title OR q.raw % a.title)
  UNION ALL
  SELECT 'member', m.id, u.display_name, concat_ws(' · ', m.title, u.email::text),
         concat_ws(' · ', u.display_name, m.title, u.email::text), 'simple'::regconfig,
         ts_rank_cd(to_tsvector('simple', u.display_name || ' ' || coalesce(m.title, '')), q.ts)
           + word_similarity(q.raw, lower(u.display_name))
           + CASE WHEN lower(u.display_name) LIKE q.prefix THEN 0.5::real ELSE 0::real END
  FROM memberships m JOIN users u ON u.id = m.user_id, q
  WHERE 'member' = ANY(q.types) AND m.status = 'active'
    AND (to_tsvector('simple', u.display_name || ' ' || coalesce(m.title, '')) @@ q.ts
         OR u.display_name ILIKE q.pat OR u.email::text ILIKE q.pat OR m.title ILIKE q.pat
         OR q.raw <% u.display_name OR q.raw % u.display_name)
  UNION ALL
  SELECT 'department', d.id, d.name, NULL, d.name, 'simple'::regconfig,
         ts_rank_cd(to_tsvector('simple', d.name), q.ts) + word_similarity(q.raw, lower(d.name))
           + CASE WHEN lower(d.name) LIKE q.prefix THEN 0.5::real ELSE 0::real END
  FROM departments d, q
  WHERE 'department' = ANY(q.types)
    AND (to_tsvector('simple', d.name) @@ q.ts OR d.name ILIKE q.pat OR q.raw <% d.name OR q.raw % d.name)
  UNION ALL
  SELECT 'group', g.id, g.name, g.description, g.name || ' ' || coalesce(g.description, ''), 'simple'::regconfig,
         ts_rank_cd(to_tsvector('simple', g.name || ' ' || coalesce(g.description, '')), q.ts)
           + word_similarity(q.raw, lower(g.name))
           + CASE WHEN lower(g.name) LIKE q.prefix THEN 0.5::real ELSE 0::real END
  FROM groups g, q
  WHERE 'group' = ANY(q.types)
    AND (to_tsvector('simple', g.name || ' ' || coalesce(g.description, '')) @@ q.ts
         OR g.name ILIKE q.pat OR q.raw <% g.name OR q.raw % g.name)
)
SELECT h.type, h.id, h.title, h.subtitle,
       ts_headline(h.cfg, translate(h.body, chr(2) || chr(3), ''), q.ts,
                   'StartSel=' || chr(2) || ', StopSel=' || chr(3) || ', MaxWords=24, MinWords=8, MaxFragments=2, FragmentDelimiter=" … "'),
       h.score::float4
FROM (SELECT * FROM hits ORDER BY score DESC, lower(title), id LIMIT $5) h, q
ORDER BY h.score DESC, lower(h.title), h.id
"#;

type Row = (String, Uuid, String, Option<String>, Option<String>, f32);

/// Searches the current workspace for `q` within `types` (already filtered to what the caller may
/// view). Results are ranked across types, best first.
pub async fn search(tx: &mut WsTx<'_>, q: &str, types: &[&str], limit: i64) -> Result<Vec<Hit>, SearchError> {
    let q = clean_query(q)?;
    if types.is_empty() {
        return Ok(vec![]);
    }
    // One typo in a short word ("expnse") scores 0.5 word similarity; the pg_trgm default (0.6)
    // misses it. Transaction-local, and the trigram indexes honour it.
    sqlx::query("SELECT set_config('pg_trgm.word_similarity_threshold', $1, true)")
        .bind(WORD_SIMILARITY.to_string())
        .execute(tx.conn())
        .await?;
    let lower = q.to_lowercase();
    let types: Vec<String> = types.iter().map(|t| (*t).to_owned()).collect();
    let rows: Vec<Row> = sqlx::query_as(SQL)
        .bind(&q)
        .bind(format!("%{}%", like_escape(&q)))
        .bind(format!("{}%", like_escape(&lower)))
        .bind(&types)
        .bind(limit.clamp(1, MAX_LIMIT))
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .into_iter()
        .map(|(kind, id, title, subtitle, snippet, score)| Hit {
            kind,
            id,
            title,
            subtitle,
            snippet: snippet.as_deref().and_then(render_snippet),
            score,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_parse_and_reject_unknown() {
        assert_eq!(parse_types(None).ok(), Some(TYPES.to_vec()));
        assert_eq!(parse_types(Some(" ")).ok(), Some(TYPES.to_vec()));
        assert_eq!(parse_types(Some("member, sop,member")).ok(), Some(vec!["member", "sop"]));
        assert!(matches!(parse_types(Some("process")), Err(SearchError::Invalid(_))));
    }

    #[test]
    fn queries_are_trimmed_and_bounded() {
        assert_eq!(clean_query("  a\u{7}b ").ok().as_deref(), Some("ab"));
        assert!(clean_query("   ").is_err());
        assert!(clean_query(&"x".repeat(201)).is_err());
        assert!(clean_query(&"é".repeat(200)).is_ok());
    }

    #[test]
    fn snippets_escape_markup_and_mark_matches() {
        assert_eq!(
            render_snippet("a <script>\u{2}x\u{3}</script> & 'b'").as_deref(),
            Some("a &lt;script&gt;<mark>x</mark>&lt;/script&gt; &amp; &#39;b&#39;")
        );
        assert_eq!(render_snippet("  "), None);
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like_escape(r"50%_a\b"), r"50\%\_a\\b");
    }
}
