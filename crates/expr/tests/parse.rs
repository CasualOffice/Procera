//! Parser: grammar, precedence and syntax diagnostics with positions.
// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use procera_expr::*;

fn perr(src: &str) -> Diagnostic {
    parse(src).expect_err(src)
}

fn shape(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Lit(Literal::Int(i)) => i.to_string(),
        ExprKind::Lit(Literal::Decimal(d)) => d.to_string(),
        ExprKind::Lit(Literal::Bool(b)) => b.to_string(),
        ExprKind::Lit(Literal::Null) => "null".into(),
        ExprKind::Lit(Literal::String(s)) => format!("{s:?}"),
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Member { target, field } => format!("{}.{field}", shape(target)),
        ExprKind::Index { target, index } => format!("{}[{}]", shape(target), shape(index)),
        ExprKind::Unary { op, operand } => format!("({op:?} {})", shape(operand)),
        ExprKind::Binary { op, lhs, rhs } => format!("({} {} {})", shape(lhs), op.symbol(), shape(rhs)),
        ExprKind::Cond { cond, then, otherwise } => {
            format!("({} ? {} : {})", shape(cond), shape(then), shape(otherwise))
        }
        ExprKind::List(items) => format!("[{}]", items.iter().map(shape).collect::<Vec<_>>().join(", ")),
        ExprKind::Call { target, name, args, .. } => {
            let a = args.iter().map(shape).collect::<Vec<_>>().join(", ");
            match target {
                Some(t) => format!("{}.{name}({a})", shape(t)),
                None => format!("{name}({a})"),
            }
        }
        ExprKind::Macro { kind, target, var, body, .. } => {
            format!("{}.{kind:?}({var}, {})", shape(target), shape(body))
        }
        ExprKind::Widen { inner, .. } => shape(inner),
    }
}

fn p(src: &str) -> String {
    shape(&parse(src).unwrap_or_else(|d| panic!("{src}: {d}")))
}

#[test]
fn precedence_follows_cel() {
    assert_eq!(p("1 + 2 * 3"), "(1 + (2 * 3))");
    assert_eq!(p("a || b && c"), "(a || (b && c))");
    assert_eq!(p("a == b && c < d"), "((a == b) && (c < d))");
    assert_eq!(p("1 - 2 - 3"), "((1 - 2) - 3)");
    assert_eq!(p("!a.b"), "(Not a.b)");
    assert_eq!(p("-x * 2"), "((Neg x) * 2)");
    assert_eq!(p("a ? b : c ? d : e"), "(a ? b : (c ? d : e))");
    assert_eq!(p("x in [1, 2,]"), "(x in [1, 2])");
    assert_eq!(p("(1 + 2) * 3"), "((1 + 2) * 3)");
}

#[test]
fn members_calls_and_macros() {
    assert_eq!(p("initiator.manager.id"), "initiator.manager.id");
    assert_eq!(p("items[0].quantity"), "items[0].quantity");
    assert_eq!(p("name.startsWith(\"L\")"), "name.startsWith(\"L\")");
    assert_eq!(p("size(tags)"), "size(tags)");
    assert_eq!(p("items.map(i, i.quantity)"), "items.Map(i, i.quantity)");
    assert_eq!(p("items.exists_one(i, true)"), "items.ExistsOne(i, true)");
}

#[test]
fn literals() {
    assert_eq!(p("0.10"), "0.10");
    assert_eq!(p("-9223372036854775808"), "-9223372036854775808");
    assert_eq!(p("-1.5"), "-1.5");
    assert_eq!(p(r#"'it' + "é\n""#), r#"("it" + "é\n")"#);
    // Adjacent strings don't concatenate.
    assert_eq!(perr("'a''b'").code, Code::Syntax);
}

#[test]
fn string_escapes() {
    assert_eq!(p(r#""a\"b\\c\t""#), r#""a\"b\\c\t""#);
    assert_eq!(p(r"'A'"), "\"A\"");
}

#[test]
fn comments_are_ignored() {
    assert_eq!(p("1 + // one\n 2"), "(1 + 2)");
}

#[test]
fn syntax_errors_have_codes_and_positions() {
    let d = perr("amount > ");
    assert_eq!(d.code, Code::Syntax);
    assert_eq!((d.span.start, d.span.end), (9, 9));

    let d = perr("a b");
    assert_eq!(d.code, Code::Syntax);
    assert_eq!(d.span.start, 2);

    let d = perr("f(1, 2");
    assert_eq!(d.code, Code::Syntax);
    assert!(d.message.contains("`,` or `)`"), "{}", d.message);

    assert_eq!(perr("a ? b").code, Code::Syntax);
    assert_eq!(perr("a.").code, Code::Syntax);
    assert_eq!(perr(")").code, Code::Syntax);
}

#[test]
fn lexical_errors() {
    let d = perr("a = 1");
    assert_eq!(d.code, Code::Lexical);
    assert!(d.message.contains("=="));
    assert_eq!(perr("\"open").code, Code::Lexical);
    assert_eq!(perr(r#""\q""#).code, Code::Lexical);
    assert_eq!(perr(r#""\u12""#).code, Code::Lexical);
    assert_eq!(perr(r#""\uD800""#).code, Code::Lexical);
    assert_eq!(perr("a # b").code, Code::Lexical);
}

#[test]
fn bad_numbers() {
    assert_eq!(perr("9223372036854775808").code, Code::BadNumber);
    assert_eq!(perr("1e5").code, Code::BadNumber);
    assert_eq!(perr("12abc").code, Code::BadNumber);
    assert_eq!(perr("1.00000000000000000000000000001").code, Code::BadNumber);
}

#[test]
fn reserved_words_are_rejected() {
    let d = perr("if > 1");
    assert_eq!(d.code, Code::Reserved);
    assert_eq!((d.span.start, d.span.end), (0, 2));
}

#[test]
fn source_and_depth_limits() {
    let long = format!("\"{}\"", "a".repeat(MAX_SOURCE_LEN));
    assert_eq!(perr(&long).code, Code::SourceTooLong);

    let deep = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    assert_eq!(perr(&deep).code, Code::TooDeep);

    let chain = ["1"; 40].join(" + ");
    assert_eq!(perr(&chain).code, Code::TooDeep);

    let ok = ["1"; 20].join(" + ");
    assert!(parse(&ok).is_ok());

    let nots = format!("{}true", "!".repeat(1000));
    assert_eq!(perr(&nots).code, Code::TooDeep);
}

#[test]
fn macro_shape_errors() {
    assert_eq!(perr("items.all(1, true)").code, Code::MacroShape);
    assert_eq!(perr("items.map(i)").code, Code::MacroShape);
}

#[test]
fn line_and_column() {
    let src = "a &&\n  b ==";
    let d = perr(src);
    assert_eq!(d.span.line_col(src), (2, 7));
}

#[test]
fn language_tag() {
    assert_eq!(split_tagged("expr:cel: amount > 5"), Some(("cel", "amount > 5")));
    assert_eq!(split_tagged("amount"), None);
    assert_eq!(LANGUAGE_TAG, "cel");
}

#[test]
fn ast_round_trips_through_json() {
    let e = parse("items.filter(i, i.quantity > 1).map(i, i.unit_price)[0] + 1.5").unwrap();
    let json = serde_json::to_string(&e).unwrap();
    let back: Expr = serde_json::from_str(&json).unwrap();
    assert_eq!(e, back);
}
