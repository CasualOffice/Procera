//! Tokeniser for the CEL subset.

use crate::diag::{Code, Diagnostic, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Ident(String),
    /// Digits only; range-checked by the parser so `-9223372036854775808` works.
    Int(String),
    /// `digits.digits`; parsed as an exact decimal (there is no floating point).
    Dec(String),
    Str(String),
    True,
    False,
    Null,
    In,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Question,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Eof,
}

impl Tok {
    pub(crate) fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("identifier `{s}`"),
            Tok::Int(s) | Tok::Dec(s) => format!("number `{s}`"),
            Tok::Str(_) => "string".into(),
            Tok::Eof => "end of expression".into(),
            other => format!("`{}`", other.symbol()),
        }
    }

    fn symbol(&self) -> &'static str {
        match self {
            Tok::True => "true",
            Tok::False => "false",
            Tok::Null => "null",
            Tok::In => "in",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Dot => ".",
            Tok::Question => "?",
            Tok::Colon => ":",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::EqEq => "==",
            Tok::Ne => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::AndAnd => "&&",
            Tok::OrOr => "||",
            Tok::Bang => "!",
            _ => "?",
        }
    }
}

/// CEL reserved words that may not be used as identifiers.
const RESERVED: &[&str] = &[
    "as",
    "break",
    "const",
    "continue",
    "else",
    "for",
    "function",
    "if",
    "import",
    "let",
    "loop",
    "package",
    "namespace",
    "return",
    "var",
    "void",
    "while",
];

pub(crate) fn lex(src: &str) -> Result<Vec<(Tok, Span)>, Diagnostic> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            let span = Span::new(start, i);
            let tok = match word {
                "true" => Tok::True,
                "false" => Tok::False,
                "null" => Tok::Null,
                "in" => Tok::In,
                w if RESERVED.contains(&w) => {
                    return Err(Diagnostic::new(Code::Reserved, span, format!("`{w}` is a reserved word")));
                }
                w => Tok::Ident(w.to_string()),
            };
            out.push((tok, span));
            continue;
        }
        if c.is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let is_dec = bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
            if is_dec {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                out.push((Tok::Dec(src[start..i].to_string()), Span::new(start, i)));
            } else {
                out.push((Tok::Int(src[start..i].to_string()), Span::new(start, i)));
            }
            if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                return Err(Diagnostic::new(
                    Code::BadNumber,
                    Span::new(start, i + 1),
                    "number literals are decimal digits only (no suffixes, exponents or hex)",
                ));
            }
            continue;
        }
        if c == b'"' || c == b'\'' {
            let (s, end) = lex_string(src, start)?;
            out.push((Tok::Str(s), Span::new(start, end)));
            i = end;
            continue;
        }
        let two = bytes.get(i + 1).copied();
        let (tok, len) = match (c, two) {
            (b'=', Some(b'=')) => (Tok::EqEq, 2),
            (b'!', Some(b'=')) => (Tok::Ne, 2),
            (b'<', Some(b'=')) => (Tok::Le, 2),
            (b'>', Some(b'=')) => (Tok::Ge, 2),
            (b'&', Some(b'&')) => (Tok::AndAnd, 2),
            (b'|', Some(b'|')) => (Tok::OrOr, 2),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'[', _) => (Tok::LBracket, 1),
            (b']', _) => (Tok::RBracket, 1),
            (b',', _) => (Tok::Comma, 1),
            (b'.', _) => (Tok::Dot, 1),
            (b'?', _) => (Tok::Question, 1),
            (b':', _) => (Tok::Colon, 1),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'!', _) => (Tok::Bang, 1),
            _ => {
                let ch = src[start..].chars().next().unwrap_or('?');
                let hint = if ch == '=' { " (use `==` to compare)" } else { "" };
                return Err(Diagnostic::new(
                    Code::Lexical,
                    Span::new(start, start + ch.len_utf8()),
                    format!("unexpected character `{ch}`{hint}"),
                ));
            }
        };
        out.push((tok, Span::new(start, start + len)));
        i += len;
    }
    out.push((Tok::Eof, Span::new(src.len(), src.len())));
    Ok(out)
}

fn lex_string(src: &str, start: usize) -> Result<(String, usize), Diagnostic> {
    let quote = src.as_bytes()[start] as char;
    let mut out = String::new();
    let mut chars = src[start + 1..].char_indices().map(|(o, c)| (o + start + 1, c));
    while let Some((pos, ch)) = chars.next() {
        match ch {
            c if c == quote => return Ok((out, pos + 1)),
            '\n' => break,
            '\\' => {
                let Some((epos, esc)) = chars.next() else { break };
                let bad = |msg: &str, end: usize| Diagnostic::new(Code::Lexical, Span::new(pos, end), msg);
                match esc {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    'u' => {
                        let mut code = 0u32;
                        let mut end = epos + 1;
                        for _ in 0..4 {
                            let Some((hpos, h)) = chars.next() else {
                                return Err(bad("`\\u` needs four hex digits", end));
                            };
                            end = hpos + h.len_utf8();
                            let Some(d) = h.to_digit(16) else {
                                return Err(bad("`\\u` needs four hex digits", end));
                            };
                            code = code * 16 + d;
                        }
                        match char::from_u32(code) {
                            Some(c) => out.push(c),
                            None => return Err(bad("`\\u` escape is not a valid character", end)),
                        }
                    }
                    other => {
                        return Err(bad(&format!("unknown escape `\\{other}`"), epos + other.len_utf8()));
                    }
                }
            }
            c => out.push(c),
        }
    }
    Err(Diagnostic::new(Code::Lexical, Span::new(start, start + 1), "unterminated string"))
}
