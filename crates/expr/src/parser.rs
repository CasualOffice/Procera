//! Recursive-descent parser for the CEL subset (grammar in docs/11 §6.1).
//!
//! Precedence, loosest first: `?:`, `||`, `&&`, relations (`== != < <= > >= in`), `+ -`,
//! `* / %`, unary `! -`, member/index/call.

use crate::ast::{BinaryOp, Expr, ExprKind, Literal, MacroKind, UnaryOp};
use crate::diag::{Code, Diagnostic, Span};
use crate::lexer::{Tok, lex};
use crate::{MAX_AST_DEPTH, MAX_SOURCE_LEN};
use rust_decimal::Decimal;

type PResult<T> = Result<T, Diagnostic>;

/// Parse expression source into an AST. Returns the first syntax problem found.
pub fn parse(src: &str) -> Result<Expr, Diagnostic> {
    if src.len() > MAX_SOURCE_LEN {
        return Err(Diagnostic::new(
            Code::SourceTooLong,
            Span::new(MAX_SOURCE_LEN, src.len()),
            format!("expression is {} bytes; the limit is {MAX_SOURCE_LEN}", src.len()),
        ));
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, nest: 0 };
    let e = p.expr()?;
    match p.peek() {
        Tok::Eof => Ok(e),
        t => Err(Diagnostic::new(Code::Syntax, p.span(), format!("unexpected {}", t.describe()))),
    }
}

struct Parser {
    toks: Vec<(Tok, Span)>,
    pos: usize,
    nest: u32,
}

fn mk(kind: ExprKind, span: Span, children: &[&Expr]) -> PResult<Expr> {
    let depth = 1 + children.iter().map(|c| c.depth).max().unwrap_or(0);
    if depth > MAX_AST_DEPTH {
        return Err(too_deep(span));
    }
    Ok(Expr { kind, span, depth })
}

fn too_deep(span: Span) -> Diagnostic {
    Diagnostic::new(Code::TooDeep, span, format!("expression nests deeper than {MAX_AST_DEPTH} levels"))
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].0
    }

    fn span(&self) -> Span {
        self.toks[self.pos.min(self.toks.len() - 1)].1
    }

    fn bump(&mut self) -> (Tok, Span) {
        let t = self.toks[self.pos.min(self.toks.len() - 1)].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> PResult<Span> {
        if self.peek() == t {
            Ok(self.bump().1)
        } else {
            Err(Diagnostic::new(
                Code::Syntax,
                self.span(),
                format!("expected {what}, found {}", self.peek().describe()),
            ))
        }
    }

    fn enter(&mut self) -> PResult<()> {
        self.nest += 1;
        if self.nest > MAX_AST_DEPTH { Err(too_deep(self.span())) } else { Ok(()) }
    }

    fn expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let cond = self.or()?;
        let out = if self.eat(&Tok::Question) {
            let then = self.or()?;
            self.expect(&Tok::Colon, "`:` in a conditional")?;
            let otherwise = self.expr()?;
            let span = cond.span.to(otherwise.span);
            mk(
                ExprKind::Cond {
                    cond: Box::new(cond.clone()),
                    then: Box::new(then.clone()),
                    otherwise: Box::new(otherwise.clone()),
                },
                span,
                &[&cond, &then, &otherwise],
            )
        } else {
            Ok(cond)
        };
        self.nest -= 1;
        out
    }

    fn binary(lhs: Expr, op: BinaryOp, rhs: Expr) -> PResult<Expr> {
        let span = lhs.span.to(rhs.span);
        let (l, r) = (lhs.depth, rhs.depth);
        let depth = 1 + l.max(r);
        if depth > MAX_AST_DEPTH {
            return Err(too_deep(span));
        }
        Ok(Expr { kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span, depth })
    }

    fn or(&mut self) -> PResult<Expr> {
        let mut lhs = self.and()?;
        while self.eat(&Tok::OrOr) {
            let rhs = self.and()?;
            lhs = Self::binary(lhs, BinaryOp::Or, rhs)?;
        }
        Ok(lhs)
    }

    fn and(&mut self) -> PResult<Expr> {
        let mut lhs = self.relation()?;
        while self.eat(&Tok::AndAnd) {
            let rhs = self.relation()?;
            lhs = Self::binary(lhs, BinaryOp::And, rhs)?;
        }
        Ok(lhs)
    }

    fn relation(&mut self) -> PResult<Expr> {
        let mut lhs = self.addition()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinaryOp::Eq,
                Tok::Ne => BinaryOp::Ne,
                Tok::Lt => BinaryOp::Lt,
                Tok::Le => BinaryOp::Le,
                Tok::Gt => BinaryOp::Gt,
                Tok::Ge => BinaryOp::Ge,
                Tok::In => BinaryOp::In,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.addition()?;
            lhs = Self::binary(lhs, op, rhs)?;
        }
    }

    fn addition(&mut self) -> PResult<Expr> {
        let mut lhs = self.multiplication()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinaryOp::Add,
                Tok::Minus => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.multiplication()?;
            lhs = Self::binary(lhs, op, rhs)?;
        }
    }

    fn multiplication(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinaryOp::Mul,
                Tok::Slash => BinaryOp::Div,
                Tok::Percent => BinaryOp::Rem,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Self::binary(lhs, op, rhs)?;
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            Tok::Bang => UnaryOp::Not,
            Tok::Minus => UnaryOp::Neg,
            _ => return self.member(),
        };
        let (_, start) = self.bump();
        self.enter()?;
        // `-9223372036854775808` must parse even though its magnitude overflows i64.
        let out = if op == UnaryOp::Neg && matches!(self.peek(), Tok::Int(_) | Tok::Dec(_)) {
            self.negative_literal(start)
        } else {
            let operand = self.unary()?;
            let span = start.to(operand.span);
            mk(ExprKind::Unary { op, operand: Box::new(operand.clone()) }, span, &[&operand])
        };
        self.nest -= 1;
        out
    }

    fn negative_literal(&mut self, minus: Span) -> PResult<Expr> {
        let (tok, span) = self.bump();
        let full = minus.to(span);
        let lit = match tok {
            Tok::Int(digits) => Literal::Int(parse_int(&format!("-{digits}"), full)?),
            Tok::Dec(text) => Literal::Decimal(parse_dec(&format!("-{text}"), full)?),
            _ => return Err(Diagnostic::new(Code::Syntax, span, "expected a number")),
        };
        let base = Expr { kind: ExprKind::Lit(lit), span: full, depth: 1 };
        self.postfix(base)
    }

    fn member(&mut self) -> PResult<Expr> {
        let base = self.primary()?;
        self.postfix(base)
    }

    fn postfix(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.bump();
                    let (tok, name_span) = self.bump();
                    let Tok::Ident(name) = tok else {
                        return Err(Diagnostic::new(
                            Code::Syntax,
                            name_span,
                            format!("expected a field or method name after `.`, found {}", tok.describe()),
                        ));
                    };
                    if self.peek() == &Tok::LParen {
                        let (args, close) = self.args()?;
                        e = self.method(e, name, name_span, args, close)?;
                    } else {
                        let span = e.span.to(name_span);
                        e = mk(ExprKind::Member { target: Box::new(e.clone()), field: name }, span, &[&e])?;
                    }
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.expr()?;
                    let close = self.expect(&Tok::RBracket, "`]`")?;
                    let span = e.span.to(close);
                    e = mk(
                        ExprKind::Index { target: Box::new(e.clone()), index: Box::new(index.clone()) },
                        span,
                        &[&e, &index],
                    )?;
                }
                _ => return Ok(e),
            }
        }
    }

    fn method(
        &mut self,
        target: Expr,
        name: String,
        name_span: Span,
        args: Vec<Expr>,
        close: Span,
    ) -> PResult<Expr> {
        let span = target.span.to(close);
        if let Some(kind) = MacroKind::from_name(&name) {
            let shape_err = || {
                Diagnostic::new(
                    Code::MacroShape,
                    name_span.to(close),
                    format!("`{name}` takes a variable name and an expression, e.g. `items.{name}(x, …)`"),
                )
            };
            let [var, body]: [Expr; 2] = args.try_into().map_err(|_| shape_err())?;
            let ExprKind::Ident(var_name) = &var.kind else { return Err(shape_err()) };
            let var_name = var_name.clone();
            return mk(
                ExprKind::Macro {
                    kind,
                    target: Box::new(target.clone()),
                    var: var_name,
                    var_span: var.span,
                    body: Box::new(body.clone()),
                },
                span,
                &[&target, &body],
            );
        }
        let mut children: Vec<&Expr> = args.iter().collect();
        children.push(&target);
        let depth = 1 + children.iter().map(|c| c.depth).max().unwrap_or(0);
        if depth > MAX_AST_DEPTH {
            return Err(too_deep(span));
        }
        Ok(Expr {
            kind: ExprKind::Call { target: Some(Box::new(target)), name, name_span, args, ret: None },
            span,
            depth,
        })
    }

    fn args(&mut self) -> PResult<(Vec<Expr>, Span)> {
        self.expect(&Tok::LParen, "`(`")?;
        let mut args = Vec::new();
        if let Some(close) = self.close_if(&Tok::RParen) {
            return Ok((args, close));
        }
        loop {
            args.push(self.expr()?);
            if let Some(close) = self.close_if(&Tok::RParen) {
                return Ok((args, close));
            }
            self.expect(&Tok::Comma, "`,` or `)`")?;
        }
    }

    fn close_if(&mut self, t: &Tok) -> Option<Span> {
        if self.peek() == t { Some(self.bump().1) } else { None }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let (tok, span) = self.bump();
        let lit = |l| Ok(Expr { kind: ExprKind::Lit(l), span, depth: 1 });
        match tok {
            Tok::Null => lit(Literal::Null),
            Tok::True => lit(Literal::Bool(true)),
            Tok::False => lit(Literal::Bool(false)),
            Tok::Str(s) => lit(Literal::String(s)),
            Tok::Int(d) => lit(Literal::Int(parse_int(&d, span)?)),
            Tok::Dec(d) => lit(Literal::Decimal(parse_dec(&d, span)?)),
            Tok::Ident(name) => {
                if self.peek() == &Tok::LParen {
                    let (args, close) = self.args()?;
                    let full = span.to(close);
                    let children: Vec<&Expr> = args.iter().collect();
                    let depth = 1 + children.iter().map(|c| c.depth).max().unwrap_or(0);
                    if depth > MAX_AST_DEPTH {
                        return Err(too_deep(full));
                    }
                    Ok(Expr {
                        kind: ExprKind::Call { target: None, name, name_span: span, args, ret: None },
                        span: full,
                        depth,
                    })
                } else {
                    lit_ident(name, span)
                }
            }
            Tok::LParen => {
                let inner = self.expr()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(inner)
            }
            Tok::LBracket => {
                self.enter()?;
                let mut items = Vec::new();
                let close = loop {
                    if let Some(close) = self.close_if(&Tok::RBracket) {
                        break close;
                    }
                    items.push(self.expr()?);
                    if let Some(close) = self.close_if(&Tok::RBracket) {
                        break close;
                    }
                    self.expect(&Tok::Comma, "`,` or `]`")?;
                };
                self.nest -= 1;
                let full = span.to(close);
                let children: Vec<&Expr> = items.iter().collect();
                let depth = 1 + children.iter().map(|c| c.depth).max().unwrap_or(0);
                if depth > MAX_AST_DEPTH {
                    return Err(too_deep(full));
                }
                Ok(Expr { kind: ExprKind::List(items), span: full, depth })
            }
            other => Err(Diagnostic::new(Code::Syntax, span, format!("unexpected {}", other.describe()))),
        }
    }
}

fn lit_ident(name: String, span: Span) -> PResult<Expr> {
    Ok(Expr { kind: ExprKind::Ident(name), span, depth: 1 })
}

fn parse_int(text: &str, span: Span) -> PResult<i64> {
    text.parse::<i64>().map_err(|_| {
        Diagnostic::new(Code::BadNumber, span, format!("integer `{text}` is outside the 64-bit range"))
    })
}

fn parse_dec(text: &str, span: Span) -> PResult<Decimal> {
    Decimal::from_str_exact(text).map_err(|_| {
        Diagnostic::new(
            Code::BadNumber,
            span,
            format!("decimal `{text}` has more than 28 significant digits"),
        )
    })
}
