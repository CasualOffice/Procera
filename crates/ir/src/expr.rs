//! Compile-time expression hook (docs/11 §6, §11 check 7).
//!
//! The expression engine is BPMS-081 (CEL with Procera extensions, ADR-0010). Until it lands the
//! compiler needs only one thing from an expression: which variables it reads. That is the
//! [`ExpressionChecker`] port. [`LexicalChecker`] is the built-in implementation: a tokenizer that
//! checks the source is well formed (balanced brackets, closed strings, non-empty) and collects
//! root identifiers. BPMS-081 replaces it with the real parser and type checker.

use crate::model::ExprLang;
use std::collections::BTreeSet;

/// Names every expression may use without declaring them (docs/11 §13.10).
pub const CONTEXT_ROOTS: &[&str] = &["initiator", "instance", "now"];

/// What the compiler needs to know about one expression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis {
    /// Variable names the expression reads (context roots and bound names excluded).
    pub references: BTreeSet<String>,
}

/// Parses and analyses expression source. Implementations must be pure and deterministic.
pub trait ExpressionChecker {
    /// Returns the analysis, or a human-readable syntax error.
    fn analyse(&self, lang: ExprLang, body: &str) -> Result<Analysis, String>;
}

/// Built-in lexical checker (see the module docs). Conservative: it reports identifiers at the
/// root of a member path that are not function names, keywords, bound names or context roots.
#[derive(Debug, Clone, Copy, Default)]
pub struct LexicalChecker;

const KEYWORDS: &[&str] = &[
    "and",
    "between",
    "every",
    "else",
    "external",
    "false",
    "for",
    "function",
    "if",
    "in",
    "instance",
    "not",
    "null",
    "of",
    "or",
    "return",
    "satisfies",
    "some",
    "then",
    "true",
];
/// CEL macros whose first argument binds a name (`items.all(i, i.qty > 0)`).
const CEL_BINDING_MACROS: &[&str] = &["all", "exists", "exists_one", "map", "filter"];
/// FEEL built-ins whose names contain spaces.
const FEEL_SPACED_BUILTINS: &[(&str, &str)] = &[
    ("date and time", "date_and_time"),
    ("years and months duration", "years_and_months_duration"),
    ("days and time duration", "days_and_time_duration"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Dot,
    Comma,
    Open,
    Other,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut stack: Vec<char> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '"' | '\'' => {
                let quote = c;
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated string literal".to_owned()),
                        Some('\\') => i += 2,
                        Some(&q) if q == quote => break,
                        Some(_) => i += 1,
                    }
                }
                i += 1;
                toks.push(Tok::Other);
            }
            c if c.is_ascii_digit() => {
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                    // A dot followed by a letter is member access on a number literal; stop there.
                    if chars[i] == '.' && !chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                        break;
                    }
                    i += 1;
                }
                toks.push(Tok::Other);
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                toks.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            '(' | '[' | '{' => {
                stack.push(c);
                toks.push(if c == '(' { Tok::Open } else { Tok::Other });
                i += 1;
            }
            ')' | ']' | '}' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if stack.pop() != Some(want) {
                    return Err(format!("unbalanced '{c}'"));
                }
                toks.push(Tok::Other);
                i += 1;
            }
            '.' => {
                toks.push(Tok::Dot);
                i += 1;
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            _ => {
                toks.push(Tok::Other);
                i += 1;
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(format!("unclosed '{open}'"));
    }
    Ok(toks)
}

impl ExpressionChecker for LexicalChecker {
    fn analyse(&self, lang: ExprLang, body: &str) -> Result<Analysis, String> {
        if body.trim().is_empty() {
            return Err("empty expression".to_owned());
        }
        let mut src = body.to_owned();
        if lang == ExprLang::Feel {
            for (spaced, joined) in FEEL_SPACED_BUILTINS {
                src = src.replace(spaced, joined);
            }
        }
        let toks = tokenize(&src)?;
        let ident = |i: usize| match toks.get(i) {
            Some(Tok::Ident(s)) => Some(s.as_str()),
            _ => None,
        };
        let mut bound: BTreeSet<&str> = BTreeSet::new();
        for i in 0..toks.len() {
            let Some(name) = ident(i) else { continue };
            let after_dot = i > 0 && toks[i - 1] == Tok::Dot;
            if after_dot
                && CEL_BINDING_MACROS.contains(&name)
                && toks.get(i + 1) == Some(&Tok::Open)
                && toks.get(i + 3) == Some(&Tok::Comma)
                && let Some(var) = ident(i + 2)
            {
                bound.insert(var);
            }
            if matches!(name, "for" | "some" | "every")
                && let Some(var) = ident(i + 1)
                && ident(i + 2) == Some("in")
            {
                bound.insert(var);
            }
        }
        let mut references = BTreeSet::new();
        for i in 0..toks.len() {
            let Some(name) = ident(i) else { continue };
            let after_dot = i > 0 && toks[i - 1] == Tok::Dot;
            let is_call = toks.get(i + 1) == Some(&Tok::Open);
            if after_dot || is_call || KEYWORDS.contains(&name) || bound.contains(name) {
                continue;
            }
            if name == "vars" {
                // `vars.amount` reads `amount` (docs/11 §3.1 example form).
                if toks.get(i + 1) == Some(&Tok::Dot)
                    && let Some(var) = ident(i + 2)
                {
                    references.insert(var.to_owned());
                }
                continue;
            }
            if !CONTEXT_ROOTS.contains(&name) {
                references.insert(name.to_owned());
            }
        }
        Ok(Analysis { references })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(lang: ExprLang, body: &str) -> Vec<String> {
        LexicalChecker.analyse(lang, body).unwrap().references.into_iter().collect()
    }

    #[test]
    fn collects_roots_and_skips_members_calls_keywords() {
        assert_eq!(refs(ExprLang::Feel, "initiator.manager"), Vec::<String>::new());
        assert_eq!(refs(ExprLang::Feel, "sensitivity = \"high\""), vec!["sensitivity"]);
        assert_eq!(
            refs(ExprLang::Feel, "(if severity = \"critical\" then 5 else 1) * occurrence"),
            vec!["occurrence", "severity"]
        );
        assert_eq!(refs(ExprLang::Feel, "date and time(start_date, time(\"09:00:00\"))"), vec!["start_date"]);
        assert_eq!(refs(ExprLang::Cel, "vars.amount > 50000 && size(items) > 0"), vec!["amount", "items"]);
        assert_eq!(refs(ExprLang::Cel, "items.all(i, i.quantity > 0)"), vec!["items"]);
        assert_eq!(refs(ExprLang::Feel, "some x in items satisfies x.qty > 1"), vec!["items"]);
        assert_eq!(refs(ExprLang::Cel, "instance.id + \":grant\""), Vec::<String>::new());
    }

    #[test]
    fn rejects_malformed_source() {
        assert!(LexicalChecker.analyse(ExprLang::Cel, "   ").is_err());
        assert!(LexicalChecker.analyse(ExprLang::Cel, "size(items").is_err());
        assert!(LexicalChecker.analyse(ExprLang::Cel, "a)").is_err());
        assert!(LexicalChecker.analyse(ExprLang::Feel, "\"open").is_err());
    }
}
