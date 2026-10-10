//! SP-08: FEEL (dsntk) vs CEL (cel-rust) for Procera's IR expressions (ADR-0010).
//! Runs the expressions used by the reference processes through both engines and
//! reports correctness, semantics differences, and timing. Throwaway spike code.

use std::sync::Arc;
use std::time::Instant;

use dsntk_feel::context::FeelContext;
use dsntk_feel::values::Value as FV;
use dsntk_feel::{FeelNumber, FeelScope, Name};

fn n(s: &str) -> Name {
    Name::new(&[s])
}

fn feel_ctx() -> FeelContext {
    let mut manager = FeelContext::default();
    manager.set_entry(&n("id"), FV::String("u-17".into()));
    let mut initiator = FeelContext::default();
    initiator.set_entry(&n("id"), FV::String("u-42".into()));
    initiator.set_entry(&n("manager"), FV::Context(manager));
    let mut instance = FeelContext::default();
    instance.set_entry(&n("id"), FV::String("inst-123".into()));
    let item = |q: i64, p: i64, s: i32| {
        let mut c = FeelContext::default();
        c.set_entry(&n("quantity"), FV::Number(FeelNumber::from(q)));
        c.set_entry(&n("unit_price"), FV::Number(FeelNumber::new(p, s)));
        FV::Context(c)
    };
    let mut ctx = FeelContext::default();
    ctx.set_entry(&n("sensitivity"), FV::String("high".into()));
    ctx.set_entry(&n("needs_doc_change"), FV::Boolean(true));
    ctx.set_entry(&n("amount"), FV::Number(FeelNumber::new(5_000_001, 2))); // 50000.01
    ctx.set_entry(&n("severity"), FV::String("critical".into()));
    ctx.set_entry(&n("occurrence"), FV::Number(FeelNumber::from(3)));
    ctx.set_entry(&n("initiator"), FV::Context(initiator));
    ctx.set_entry(&n("instance"), FV::Context(instance));
    ctx.set_entry(&n("items"), FV::List(vec![item(2, 1999, 2), item(1, 10, 1)].into()));
    ctx
}

fn feel_eval(scope: &FeelScope, src: &str) -> Result<String, String> {
    let ast = dsntk_feel_parser::parse_expression(scope, src, false).map_err(|e| format!("parse: {e}"))?;
    Ok(format!("{}", dsntk_feel_evaluator::evaluate(scope, &ast)))
}

fn cel_ctx<'a>() -> cel::Context<'a, 'a> {
    let mut ctx = cel::Context::default();
    let json = serde_json::json!({
        "sensitivity": "high", "needs_doc_change": true, "amount": 50000.01, "severity": "critical", "occurrence": 3,
        "initiator": {"id": "u-42", "manager": {"id": "u-17"}}, "instance": {"id": "inst-123"},
        "items": [{"quantity": 2, "unit_price": 19.99}, {"quantity": 1, "unit_price": 1.0}]
    });
    if let serde_json::Value::Object(m) = json {
        for (k, v) in m {
            ctx.add_variable(k, v).unwrap_or_else(|e| panic!("cel var: {e:?}"));
        }
    }
    ctx
}

fn cel_eval(ctx: &cel::Context, src: &str) -> Result<String, String> {
    let p = cel::Program::compile(src).map_err(|e| format!("parse: {e}"))?;
    p.execute(ctx).map(|v| format!("{v:?}")).map_err(|e| format!("eval: {e}"))
}

struct Case {
    id: &'static str,
    what: &'static str,
    feel: &'static str,
    cel: &'static str,
}

const CASES: &[Case] = &[
    Case { id: "C01", what: "string equality (it.access-request)", feel: r#"sensitivity = "high""#, cel: r#"sensitivity == "high""# },
    Case { id: "C02", what: "boolean variable (qms.capa)", feel: "needs_doc_change", cel: "needs_doc_change" },
    Case { id: "C03", what: "decimal exactness 0.1+0.2=0.3", feel: "0.1 + 0.2 = 0.3", cel: "0.1 + 0.2 == 0.3" },
    Case { id: "C04", what: "money threshold 50000.01 > 50000", feel: "amount > 50000", cel: "amount > 50000.0" },
    Case { id: "C05", what: "money with int literal", feel: "amount > 50000", cel: "amount > 50000" },
    Case { id: "C06", what: "principal navigation", feel: "initiator.manager.id", cel: "initiator.manager.id" },
    Case { id: "C07", what: "string concat (idempotency key)", feel: r#"instance.id + ":grant""#, cel: r#"instance.id + ":grant""# },
    Case { id: "C08", what: "conditional (risk score)", feel: r#"(if severity = "critical" then 5 else 1) * occurrence"#, cel: r#"(severity == "critical" ? 5 : 1) * occurrence"# },
    Case { id: "C09", what: "list total (line items)", feel: "sum(for i in items return i.quantity * i.unit_price)", cel: "items.map(i, double(i.quantity) * i.unit_price)" },
    Case { id: "C10", what: "missing variable", feel: "missing_var > 5", cel: "missing_var > 5" },
    Case { id: "C11", what: "date + duration", feel: r#"date("2026-11-02") + duration("P30D")"#, cel: r#"timestamp("2026-11-02T00:00:00Z") + duration("720h")"# },
    Case { id: "C12", what: "type error (string > number)", feel: r#"sensitivity > 5"#, cel: r#"sensitivity > 5"# },
];

fn main() {
    let scope = FeelScope::from(feel_ctx());
    let env_ctx = cel_ctx();
    let _ = Arc::new(());
    println!("| Case | What | FEEL (dsntk) | CEL (cel-rust) |\n|---|---|---|---|");
    for c in CASES {
        let f = feel_eval(&scope, c.feel).unwrap_or_else(|e| format!("ERR {e}"));
        let g = cel_eval(&env_ctx, c.cel).unwrap_or_else(|e| format!("ERR {e}"));
        println!("| {} | {} | `{}` → {} | `{}` → {} |", c.id, c.what, c.feel, f.replace('|', "\\|"), c.cel, g.replace('|', "\\|"));
    }

    // Runaway expression: FEEL has ranges; CEL cannot construct unbounded collections.
    for size in [1_000u32, 10_000, 50_000] {
        let src = format!("count(for i in 1..{size} return i)");
        let t = Instant::now();
        let r = feel_eval(&scope, &src);
        println!("Runaway FEEL `{src}` → {:?} in {:?}", r, t.elapsed());
    }

    // Throughput: parse once, evaluate N times.
    const N: u32 = 100_000;
    let ast = dsntk_feel_parser::parse_expression(&scope, "amount > 50000 and sensitivity = \"high\"", false).unwrap_or_else(|e| panic!("{e}"));
    let t = Instant::now();
    for _ in 0..N {
        std::hint::black_box(dsntk_feel_evaluator::evaluate(&scope, &ast));
    }
    let feel_ns = t.elapsed().as_nanos() / N as u128;
    let prog = cel::Program::compile("amount > 50000.0 && sensitivity == \"high\"").unwrap_or_else(|e| panic!("{e}"));
    let t = Instant::now();
    for _ in 0..N {
        std::hint::black_box(prog.execute(&env_ctx).ok());
    }
    let cel_ns = t.elapsed().as_nanos() / N as u128;
    let t = Instant::now();
    for _ in 0..10_000 {
        std::hint::black_box(dsntk_feel_parser::parse_expression(&scope, "amount > 50000 and sensitivity = \"high\"", false).ok());
    }
    let feel_parse = t.elapsed().as_nanos() / 10_000;
    let t = Instant::now();
    for _ in 0..10_000 {
        std::hint::black_box(cel::Program::compile("amount > 50000.0 && sensitivity == \"high\"").ok());
    }
    let cel_parse = t.elapsed().as_nanos() / 10_000;
    println!("\nThroughput (release build): evaluate FEEL {feel_ns} ns/op, CEL {cel_ns} ns/op; parse FEEL {feel_parse} ns, CEL {cel_parse} ns");
}
