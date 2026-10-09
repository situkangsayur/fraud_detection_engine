//! Property tests for the formula language.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use proptest::prelude::*;
use rule_engine::formula::{BinOp, EvalError, Expr, Formula, Func};

fn var_name() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["x", "y", "z", "amount", "cnt_24h", "e", "pi"]).prop_map(str::to_string)
}

fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        (0u32..10_000).prop_map(f64::from),
        (0.0f64..1e6),
        Just(1e-7),
        Just(2.5e20)
    ]
}

fn expr() -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![number().prop_map(Expr::Num), var_name().prop_map(Expr::Var)];
    leaf.prop_recursive(5, 48, 4, |inner| {
        let op = prop::sample::select(vec![
            BinOp::Add,
            BinOp::Sub,
            BinOp::Mul,
            BinOp::Div,
            BinOp::Mod,
            BinOp::Pow,
        ]);
        prop_oneof![
            (op, inner.clone(), inner.clone()).prop_map(|(o, a, b)| Expr::Bin(o, Box::new(a), Box::new(b))),
            inner.clone().prop_map(|a| Expr::Neg(Box::new(a))),
            inner.clone().prop_map(|a| Expr::Call(Func::Abs, vec![a])),
            prop::collection::vec(inner.clone(), 1..4).prop_map(|args| Expr::Call(Func::Max, args)),
            (inner.clone(), inner.clone(), inner).prop_map(|(c, a, b)| Expr::Call(Func::If, vec![c, a, b])),
        ]
    })
}

proptest! {
    /// Rendering an AST and parsing it back yields the same AST.
    #[test]
    fn display_parse_round_trip(e in expr()) {
        let rendered = e.to_string();
        let parsed = Formula::parse(&rendered).unwrap();
        prop_assert_eq!(parsed.expr(), &e, "rendered: {}", rendered);
    }

    /// The spec formula matches the direct computation whenever z ≠ 0.
    #[test]
    fn spec_formula_matches_direct_computation(x in -1e6f64..1e6, y in -10.0f64..10.0, z in 0.001f64..1e3) {
        let f = Formula::parse("F(x,y,z) = 2x + 2^y / z^2").unwrap();
        let vars: BTreeMap<String, f64> = [("x".into(), x), ("y".into(), y), ("z".into(), z)].into();
        let got = f.eval(&vars).unwrap();
        let want = 2.0 * x + 2f64.powf(y) / z.powi(2);
        prop_assert!((got - want).abs() <= 1e-9 * want.abs().max(1.0), "{} vs {}", got, want);
    }

    /// Evaluation never panics and never returns a non-finite number.
    #[test]
    fn evaluation_is_total_and_finite(e in expr(), x in -1e3f64..1e3, y in -1e3f64..1e3, z in -1e3f64..1e3) {
        let f = Formula::parse(&e.to_string()).unwrap();
        let vars: BTreeMap<String, f64> = [
            ("x".into(), x), ("y".into(), y), ("z".into(), z), ("amount".into(), 1.0), ("cnt_24h".into(), 2.0),
        ].into();
        match f.eval(&vars) {
            Ok(v) => prop_assert!(v.is_finite()),
            Err(EvalError::DivisionByZero | EvalError::NonFinite(_) | EvalError::Domain(_)) => {}
            Err(other) => prop_assert!(false, "unexpected error {other}"),
        }
    }
}
