//! Property tests (docs/15 §5): evaluation is deterministic, budgets always terminate with a
//! typed result, nothing panics on arbitrary input, and well-typed expressions don't go wrong
//! (no runtime type errors, and the value has exactly the checked type).
// Test helpers outside #[test] fns panic on purpose.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::*;
use procera_expr::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use time::{Date, Duration};

#[derive(Clone, Copy, Debug)]
enum T {
    I,
    D,
    B,
    S,
    LI,
    Dt,
    Du,
    M,
}

fn leaves(t: T) -> Vec<&'static str> {
    match t {
        T::I => vec![
            "qty",
            "size(tags)",
            "name.size()",
            "start_date.day()",
            "wait.inHours()",
            "(maybe_qty != null ? maybe_qty : 7)",
            "scores.size()",
            "9223372036854775807",
            "-3",
            "0",
        ],
        T::D => vec!["total", "0.1", "12.50", "amount.amount", "avg(scores)", "sum(prices)", "-0.005"],
        T::B => vec![
            "flag",
            "true",
            "maybe_flag == true",
            "nickname == null",
            "maybe_qty != null && maybe_qty > 3",
        ],
        T::S => vec!["name", "\"ab\"", "\"\"", "tags.join(\",\")", "(severity + \"\")", "initiator.id"],
        T::LI => vec!["scores", "[1, 2, 3]", "scores.filter(x, false)"],
        T::Dt => vec!["start_date", "today()", "date(now())", "date(\"2024-02-29\")"],
        T::Du => vec!["wait", "duration(\"P1D\")", "duration(\"PT90M\")", "-duration(\"PT1S\")"],
        T::M => vec!["amount", "money(5, \"INR\")", "money(-0.01, \"INR\")"],
    }
}

/// Expressions of type `t` built from typed combinators, so nearly all are well typed.
fn typed(t: T, depth: u32) -> BoxedStrategy<String> {
    let leaf = proptest::sample::select(leaves(t)).prop_map(str::to_string).boxed();
    let lit = match t {
        T::I => any::<i16>().prop_map(|i| i.to_string()).boxed(),
        T::D => (any::<i32>(), 0u32..6).prop_map(|(m, s)| Decimal::new(m.into(), s).to_string()).boxed(),
        _ => leaf.clone(),
    };
    if depth == 0 {
        return prop_oneof![leaf, lit].boxed();
    }
    let g = |t| typed(t, depth - 1);
    let f2 = |a: BoxedStrategy<String>, b: BoxedStrategy<String>, fmt: &'static str| {
        (a, b).prop_map(move |(a, b)| fmt.replacen("{}", &a, 1).replacen("{}", &b, 1)).boxed()
    };
    let f1 = |a: BoxedStrategy<String>, fmt: &'static str| a.prop_map(move |a| fmt.replace("{}", &a)).boxed();
    let cond = |x: T| (g(T::B), g(x), g(x)).prop_map(|(c, a, b)| format!("({c} ? {a} : {b})")).boxed();
    let combos: Vec<BoxedStrategy<String>> = match t {
        T::I => vec![
            (g(T::I), proptest::sample::select(vec!["+", "-", "*", "/", "%"]), g(T::I))
                .prop_map(|(a, o, b)| format!("({a} {o} {b})"))
                .boxed(),
            f1(g(T::I), "-({})"),
            f1(g(T::I), "abs({})"),
            f2(g(T::I), g(T::I), "max({}, {})"),
            f1(g(T::LI), "sum({})"),
            f1(g(T::LI), "min({})"),
            f2(g(T::LI), g(T::I), "{}[{}]"),
            f1(g(T::D), "int({})"),
            f1(g(T::I), "int(string({}))"),
            f2(g(T::Dt), g(T::Dt), "({} - {}).inDays()"),
            f1(g(T::S), "{}.size()"),
            cond(T::I),
        ],
        T::D => vec![
            (g(T::D), proptest::sample::select(vec!["+", "-", "*", "/", "%"]), g(T::I))
                .prop_map(|(a, o, b)| format!("({a} {o} {b})"))
                .boxed(),
            (g(T::I), proptest::sample::select(vec!["+", "-", "*", "/"]), g(T::D))
                .prop_map(|(a, o, b)| format!("({a} {o} {b})"))
                .boxed(),
            f1(g(T::D), "round({}, 2)"),
            f1(g(T::D), "floor({})"),
            f1(g(T::I), "decimal({})"),
            f2(g(T::I), g(T::D), "[{}, {}][0]"),
            f2(g(T::D), g(T::I), "min({}, {})"),
            f2(g(T::M), g(T::M), "({} / {})"),
            f1(g(T::LI), "avg({})"),
            (g(T::B), g(T::D), g(T::I)).prop_map(|(c, a, b)| format!("({c} ? {b} : {a})")).boxed(),
        ],
        T::B => vec![
            (g(T::I), proptest::sample::select(vec!["<", "<=", ">", ">=", "==", "!="]), g(T::D))
                .prop_map(|(a, o, b)| format!("({a} {o} {b})"))
                .boxed(),
            f2(g(T::S), g(T::S), "({} == {})"),
            f2(g(T::B), g(T::B), "({} && {})"),
            f2(g(T::B), g(T::B), "({} || {})"),
            f1(g(T::B), "!({})"),
            f2(g(T::LI), g(T::I), "{}.all(x, x > {})"),
            f2(g(T::LI), g(T::I), "{}.exists_one(x, x == {})"),
            f2(g(T::I), g(T::LI), "({} in {})"),
            f2(g(T::S), g(T::S), "{}.startsWith({})"),
            f2(g(T::Dt), g(T::Dt), "({} < {})"),
            f2(g(T::Du), g(T::Du), "({} >= {})"),
            f2(g(T::M), g(T::M), "({} > {})"),
            f2(g(T::LI), g(T::LI), "({} == {})"),
            cond(T::B),
        ],
        T::S => vec![
            f2(g(T::S), g(T::S), "({} + {})"),
            f1(g(T::I), "string({})"),
            f1(g(T::D), "string({})"),
            f1(g(T::S), "{}.upper()"),
            f2(g(T::S), g(T::S), "{}.replace(\"a\", {})"),
            f1(g(T::S), "{}.substring(0, 1)"),
            f1(g(T::Dt), "string({})"),
            f1(g(T::Du), "string({})"),
            f1(g(T::M), "string({})"),
            f1(g(T::S), "{}.split(\"b\").join(\"-\")"),
            cond(T::S),
        ],
        T::LI => vec![
            f2(g(T::LI), g(T::LI), "({} + {})"),
            f2(g(T::LI), g(T::I), "{}.map(x, x * {})"),
            f2(g(T::LI), g(T::I), "{}.filter(x, x > {})"),
            f2(g(T::I), g(T::I), "[{}, {}]"),
            f1(g(T::S), "{}.split(\"a\").map(x, x.size())"),
            cond(T::LI),
        ],
        T::Dt => vec![
            f2(g(T::Dt), g(T::Du), "({} + {})"),
            f2(g(T::Dt), g(T::Du), "({} - {})"),
            f1(g(T::Du), "date(now() + {})"),
            cond(T::Dt),
        ],
        T::Du => vec![
            f2(g(T::Du), g(T::Du), "({} + {})"),
            f2(g(T::Du), g(T::I), "({} * {})"),
            f1(g(T::Du), "abs({})"),
            f2(g(T::Dt), g(T::Dt), "({} - {})"),
            f1(g(T::Du), "-({})"),
            cond(T::Du),
        ],
        T::M => vec![
            f2(g(T::M), g(T::M), "({} + {})"),
            f2(g(T::M), g(T::M), "({} - {})"),
            f2(g(T::M), g(T::I), "({} * {})"),
            f2(g(T::M), g(T::D), "({} / {})"),
            f1(g(T::M), "round({}, 2)"),
            f2(g(T::M), g(T::M), "sum([{}, {}])"),
            cond(T::M),
        ],
    };
    let mut all = vec![leaf, lit];
    all.extend(combos);
    proptest::strategy::Union::new(all).boxed()
}

fn any_typed_expr() -> BoxedStrategy<String> {
    prop_oneof![
        typed(T::I, 3),
        typed(T::D, 3),
        typed(T::B, 3),
        typed(T::S, 2),
        typed(T::LI, 2),
        typed(T::Dt, 2),
        typed(T::Du, 2),
        typed(T::M, 2)
    ]
    .boxed()
}

fn dec() -> impl Strategy<Value = Decimal> {
    prop_oneof![
        (any::<i64>(), 0u32..10).prop_map(|(m, s)| Decimal::new(m, s)),
        (-10_000i64..10_000, 0u32..3).prop_map(|(m, s)| Decimal::new(m, s)),
    ]
}

fn small_str() -> impl Strategy<Value = String> {
    "[ab, ]{0,12}"
}

fn int() -> impl Strategy<Value = i64> {
    prop_oneof![-50i64..50, any::<i64>()]
}

/// Random values for every variable, always conforming to the schema.
fn random_bindings() -> impl Strategy<Value = Bindings> {
    (
        (int(), dec(), proptest::option::of(int()), small_str(), proptest::option::of(small_str())),
        (
            any::<bool>(),
            proptest::option::of(any::<bool>()),
            -1_000_000i32..1_000_000,
            -10_000_000i64..10_000_000,
        ),
        (
            proptest::collection::vec(small_str(), 0..5),
            proptest::collection::vec(int(), 0..8),
            proptest::collection::vec(dec(), 0..5),
            dec(),
            0usize..3,
        ),
    )
        .prop_map(
            |((qty, total, mq, name, nick), (flag, mf, day, secs), (tags, scores, prices, amt, sev))| {
                let base = bindings();
                let s = schema();
                let opt = |o: Option<Value>| o.unwrap_or(Value::Null);
                let date = Date::from_julian_day(2_460_000 + day).unwrap_or(Date::MIN);
                let mut v: BTreeMap<String, Value> = BTreeMap::new();
                for k in ["initiator", "instance", "items", "employee", "due", "salary", "notes"] {
                    v.insert(k.into(), base.get(k).cloned().unwrap_or(Value::Null));
                }
                v.insert("qty".into(), Value::Int(qty));
                v.insert("total".into(), Value::Decimal(total));
                v.insert("maybe_qty".into(), opt(mq.map(Value::Int)));
                v.insert("name".into(), Value::string(name));
                v.insert("nickname".into(), opt(nick.map(Value::string)));
                v.insert("flag".into(), Value::Bool(flag));
                v.insert("maybe_flag".into(), opt(mf.map(Value::Bool)));
                v.insert("start_date".into(), Value::Date(date));
                v.insert("wait".into(), Value::Duration(Duration::seconds(secs)));
                v.insert("tags".into(), Value::list(tags.into_iter().map(Value::string).collect()));
                v.insert("scores".into(), Value::list(scores.into_iter().map(Value::Int).collect()));
                v.insert("prices".into(), Value::list(prices.into_iter().map(Value::Decimal).collect()));
                v.insert("amount".into(), Value::Money { amount: amt, currency: "INR".into() });
                v.insert("severity".into(), Value::string(["minor", "major", "critical"][sev]));
                Bindings::new(&s, v).expect("generated bindings conform")
            },
        )
}

fn arb_budget() -> impl Strategy<Value = Budget> {
    (0u64..400, 0u64..20_000, 0usize..200, 0usize..20, 1u32..40).prop_map(|(s, a, sl, ll, d)| Budget {
        max_steps: s,
        max_alloc_bytes: a,
        max_string_len: sl,
        max_list_len: ll,
        max_depth: d,
    })
}

fn is_limit_or_data_error(e: &EvalError) -> bool {
    !matches!(e, EvalError::Type(_) | EvalError::Unbound(_))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// Well-typed expressions don't go wrong: the generator only builds typed expressions, the
    /// checker accepts them, and evaluation either yields a value of exactly the checked type
    /// or a budget/data error, never a runtime type error.
    #[test]
    fn well_typed_expressions_dont_go_wrong(src in any_typed_expr(), b in random_bindings()) {
        let c = match compile(&src, &schema(), &CheckOptions::default()) {
            Ok(c) => c,
            Err(d) if d.iter().all(|d| matches!(d.code, Code::TooDeep | Code::SourceTooLong)) => return Ok(()),
            Err(d) => return Err(TestCaseError::fail(format!("{src}: {d:?}"))),
        };
        let big = Budget { max_steps: 1_000_000, ..Budget::default() };
        match evaluate(&c, &b, &env(), &big) {
            Ok(v) => prop_assert!(v.has_exact_type(&c.ty), "{src}: {v:?} is not {}", c.ty),
            Err(e) => prop_assert!(is_limit_or_data_error(&e), "{src}: {e}"),
        }
    }

    /// Same expression, bindings and environment → same value and same resource usage.
    #[test]
    fn evaluation_is_deterministic(src in any_typed_expr(), b in random_bindings()) {
        if let Ok(c) = compile(&src, &schema(), &CheckOptions::default()) {
            let r1 = evaluate_metered(&c, &b, &env(), &Budget::default());
            let r2 = evaluate_metered(&c, &b.clone(), &env(), &Budget::default());
            prop_assert_eq!(r1, r2);
        }
    }

    /// Any budget, however small, ends in a value or a typed error; a value under a small
    /// budget is the same value as under a generous one.
    #[test]
    fn budgets_always_terminate(src in any_typed_expr(), b in random_bindings(), budget in arb_budget()) {
        if let Ok(c) = compile(&src, &schema(), &CheckOptions::default()) {
            let small = evaluate_metered(&c, &b, &env(), &budget);
            match &small {
                Ok((_, usage)) => {
                    prop_assert!(usage.steps <= budget.max_steps);
                    prop_assert!(usage.alloc_bytes <= budget.max_alloc_bytes);
                    let big = Budget { max_steps: 1_000_000, ..Budget::default() };
                    let full = evaluate(&c, &b, &env(), &big);
                    prop_assert_eq!(small.map(|(v, _)| v), full);
                }
                Err(e) => prop_assert!(is_limit_or_data_error(e), "{src}: {e}"),
            }
        }
    }

    /// The parser and checker never panic on arbitrary text.
    #[test]
    fn arbitrary_text_never_panics(src in "\\PC{0,80}") {
        let _ = compile(&src, &schema(), &CheckOptions::condition());
    }

    /// Token soup reaches deeper into the parser and checker than random characters do.
    #[test]
    fn token_soup_never_panics(
        toks in proptest::collection::vec(proptest::sample::select(vec![
            "qty", "total", "amount", "name", "flag", "maybe_qty", "items", "tags", "scores", "initiator",
            ".", "manager", "id", "(", ")", "[", "]", ",", "+", "-", "*", "/", "%", "==", "!=", "<", ">=",
            "&&", "||", "!", "?", ":", "in", "null", "1", "2.5", "\"s\"", "size", "map", "x", "all",
            "sum", "now", "money", "\"INR\"", "duration", "\"P1D\"", "-9223372036854775808",
        ]), 0..30),
        b in random_bindings(),
    ) {
        let src = toks.join(" ");
        if let Ok(c) = compile(&src, &schema(), &CheckOptions::default()) {
            match evaluate(&c, &b, &env(), &Budget::default()) {
                Ok(v) => prop_assert!(v.has_exact_type(&c.ty), "{src}: {v:?} is not {}", c.ty),
                Err(e) => prop_assert!(is_limit_or_data_error(&e), "{src}: {e}"),
            }
        }
    }
}

/// Guards against a vacuous generator: most generated expressions must compile, and a good
/// share must evaluate to a value rather than an error.
#[test]
fn generator_exercises_evaluation() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &[7; 32]);
    let mut runner = TestRunner::new_with_rng(Config::default(), rng);
    let (exprs, binds) = (any_typed_expr(), random_bindings());
    let (mut compiled, mut ok) = (0, 0);
    for _ in 0..400 {
        let src = exprs.new_tree(&mut runner).unwrap().current();
        let b = binds.new_tree(&mut runner).unwrap().current();
        if let Ok(c) = compile(&src, &schema(), &CheckOptions::default()) {
            compiled += 1;
            ok += usize::from(evaluate(&c, &b, &env(), &Budget::default()).is_ok());
        }
    }
    assert!(compiled >= 395, "compiled {compiled}/400");
    assert!(ok >= 250, "evaluated {ok}/400");
}
