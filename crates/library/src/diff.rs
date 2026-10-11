//! Semantic diff between two document versions (BPMS-043, FR-G05).
//!
//! Content is a list of blocks (`{"type": "paragraph"|"heading"|"list", "text"?, "level"?, "items"?}`).
//! Blocks are aligned with a longest-common-subsequence over their canonical text; a removed block
//! immediately followed by an added block of the same type is reported as one `changed` block with
//! a word-level diff. Unknown block shapes are compared by their canonical JSON, so nothing is ever
//! silently dropped.

use serde::Serialize;
use serde_json::Value;

/// One word-level operation inside a changed block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum WordOp {
    Same(String),
    Added(String),
    Removed(String),
}

/// One block-level operation, in order of the target version.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BlockOp {
    Same { block: Value },
    Added { block: Value },
    Removed { block: Value },
    Changed { before: Value, after: Value, words: Vec<WordOp> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocDiff {
    pub ops: Vec<BlockOp>,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

fn blocks(content: &Value) -> Vec<Value> {
    content.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// The text a block shows to readers (lists joined by newlines).
pub fn block_text(b: &Value) -> String {
    if let Some(items) = b.get("items").and_then(Value::as_array) {
        return items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n");
    }
    if let Some(rows) = b.get("rows").and_then(Value::as_array) {
        return rows
            .iter()
            .map(|r| {
                r.as_array()
                    .map(|c| c.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\t"))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    match b.get("text").and_then(Value::as_str) {
        Some(t) => t.to_owned(),
        None => serde_json::to_string(b).unwrap_or_default(),
    }
}

fn key(b: &Value) -> String {
    format!(
        "{}|{}|{}",
        b.get("type").and_then(Value::as_str).unwrap_or(""),
        b.get("level").and_then(Value::as_i64).unwrap_or(0),
        block_text(b)
    )
}

/// LCS alignment: returns pairs of (Some(i), Some(j)) for matches and one-sided entries for edits,
/// in order.
fn align<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(Option<usize>, Option<usize>)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n + m);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((Some(i), Some(j)));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push((Some(i), None));
            i += 1;
        } else {
            out.push((None, Some(j)));
            j += 1;
        }
    }
    out.extend((i..n).map(|i| (Some(i), None)));
    out.extend((j..m).map(|j| (None, Some(j))));
    out
}

/// Word-level diff (whitespace-separated tokens, punctuation kept on the word).
pub fn word_diff(before: &str, after: &str) -> Vec<WordOp> {
    let a: Vec<&str> = before.split_whitespace().collect();
    let b: Vec<&str> = after.split_whitespace().collect();
    let mut out: Vec<WordOp> = vec![];
    let mut push = |op: WordOp| match (out.last_mut(), &op) {
        (Some(WordOp::Same(t)), WordOp::Same(s))
        | (Some(WordOp::Added(t)), WordOp::Added(s))
        | (Some(WordOp::Removed(t)), WordOp::Removed(s)) => {
            t.push(' ');
            t.push_str(s);
        }
        _ => out.push(op),
    };
    for (i, j) in align(&a, &b) {
        match (i, j) {
            (Some(i), Some(_)) => push(WordOp::Same(a[i].to_owned())),
            (Some(i), None) => push(WordOp::Removed(a[i].to_owned())),
            (None, Some(j)) => push(WordOp::Added(b[j].to_owned())),
            (None, None) => {}
        }
    }
    out
}

/// Diffs two version contents.
pub fn diff(before: &Value, after: &Value) -> DocDiff {
    let (a, b) = (blocks(before), blocks(after));
    let (ka, kb): (Vec<String>, Vec<String>) = (a.iter().map(key).collect(), b.iter().map(key).collect());
    let raw = align(&ka, &kb);
    let mut ops = vec![];
    // Between two matched blocks, removals and additions are paired by position; a pair of the
    // same block type is one `changed` block with a word diff.
    let (mut rem, mut add): (Vec<usize>, Vec<usize>) = (vec![], vec![]);
    let flush = |ops: &mut Vec<BlockOp>, rem: &mut Vec<usize>, add: &mut Vec<usize>| {
        for k in 0..rem.len().max(add.len()) {
            match (rem.get(k), add.get(k)) {
                (Some(&i), Some(&j)) if a[i].get("type") == b[j].get("type") => ops.push(BlockOp::Changed {
                    before: a[i].clone(),
                    after: b[j].clone(),
                    words: word_diff(&block_text(&a[i]), &block_text(&b[j])),
                }),
                (r, ad) => {
                    if let Some(&i) = r {
                        ops.push(BlockOp::Removed { block: a[i].clone() });
                    }
                    if let Some(&j) = ad {
                        ops.push(BlockOp::Added { block: b[j].clone() });
                    }
                }
            }
        }
        rem.clear();
        add.clear();
    };
    for pair in raw {
        match pair {
            (Some(i), Some(_)) => {
                flush(&mut ops, &mut rem, &mut add);
                ops.push(BlockOp::Same { block: a[i].clone() });
            }
            (Some(i), None) => rem.push(i),
            (None, Some(j)) => add.push(j),
            (None, None) => {}
        }
    }
    flush(&mut ops, &mut rem, &mut add);
    let count = |f: fn(&BlockOp) -> bool| ops.iter().filter(|o| f(o)).count();
    DocDiff {
        added: count(|o| matches!(o, BlockOp::Added { .. })),
        removed: count(|o| matches!(o, BlockOp::Removed { .. })),
        changed: count(|o| matches!(o, BlockOp::Changed { .. })),
        ops,
    }
}

/// Rebuilds the target content from a diff (used by the property test: diffs never lose content).
pub fn apply(d: &DocDiff) -> Vec<Value> {
    d.ops
        .iter()
        .filter_map(|o| match o {
            BlockOp::Same { block } | BlockOp::Added { block } => Some(block.clone()),
            BlockOp::Changed { after, .. } => Some(after.clone()),
            BlockOp::Removed { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(t: &str) -> Value {
        json!({"type": "paragraph", "text": t})
    }

    #[test]
    fn detects_added_removed_and_changed_blocks_with_word_changes() {
        let a = json!({"blocks": [p("Please wear a badge."), {"type": "heading", "level": 2, "text": "Visitors"}, p("Sign in at reception.")]});
        let b = json!({"blocks": [p("Please always wear a visible badge."), {"type": "heading", "level": 2, "text": "Visitors"}, p("Escort visitors at all times.")]});
        let d = diff(&a, &b);
        assert_eq!((d.added, d.removed, d.changed), (0, 0, 2));
        let BlockOp::Changed { words, .. } = &d.ops[0] else { panic!("first block changed") };
        assert_eq!(
            words,
            &vec![
                WordOp::Same("Please".into()),
                WordOp::Added("always".into()),
                WordOp::Same("wear a".into()),
                WordOp::Added("visible".into()),
                WordOp::Same("badge.".into())
            ]
        );
        assert!(matches!(d.ops[1], BlockOp::Same { .. }));
        assert_eq!(apply(&d), blocks(&b));
    }

    #[test]
    fn type_changes_are_remove_plus_add_and_lists_compare_items() {
        let a = json!({"blocks": [p("Steps"), {"type": "list", "items": ["One", "Two"]}]});
        let b = json!({"blocks": [{"type": "heading", "level": 2, "text": "Steps"}, {"type": "list", "items": ["One", "Two", "Three"]}]});
        let d = diff(&a, &b);
        assert_eq!((d.added, d.removed, d.changed), (1, 1, 1));
        assert_eq!(apply(&d), blocks(&b));
        assert_eq!(diff(&b, &b).ops.iter().filter(|o| !matches!(o, BlockOp::Same { .. })).count(), 0);
    }
}
