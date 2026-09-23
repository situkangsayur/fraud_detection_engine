//! Behaviour of every rule kind against an in-memory data provider.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::Duration;
use common::{block_on, ctx, rule, t0, MockProvider};
use rule_engine::model::AggFn;
use rule_engine::ports::{
    BucketPoint, HistFilter, HistPredicate, ProviderError, QueryWindow, SeriesRequest, VelocityData,
};
use rule_engine::{evaluate_rule, Outcome};
use serde_json::json;

fn envelope(kind: &str, definition: serde_json::Value) -> serde_json::Value {
    json!({ "code": "RL-TEST-001", "name": "test", "kind": kind, "risk_score": 40, "definition": definition })
}

fn simple(when: serde_json::Value) -> serde_json::Value {
    envelope("simple", json!({ "kind": "simple", "when": when }))
}

// ------------------------------------------------------------------------------------------------ simple

#[test]
fn simple_spec_example_matches_field_vs_field() {
    let r = rule(simple(json!({ "all": [
        { "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"const","value": 5000000} },
        { "left": {"type":"field","path":"event.issuer_country"}, "op": "ne", "right": {"type":"field","path":"event.geo_country"} }
    ]})));
    let result = block_on(evaluate_rule(&r, &ctx(), &MockProvider::default()));
    assert_eq!(result.outcome, Outcome::Match);
    assert_eq!(result.fraction, 1.0);
    assert_eq!(result.trace["when"]["all"][1]["right_value"], json!("ID"));
}

#[test]
fn simple_no_match_and_trapped() {
    let no = rule(simple(
        json!({ "left": {"type":"field","path":"event.amount"}, "op": "lt", "right": {"type":"const","value": 10} }),
    ));
    assert_eq!(
        block_on(evaluate_rule(&no, &ctx(), &MockProvider::default())).outcome,
        Outcome::NoMatch
    );

    let missing = rule(simple(
        json!({ "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} }),
    ));
    let result = block_on(evaluate_rule(&missing, &ctx(), &MockProvider::default()));
    assert_eq!(
        result.outcome,
        Outcome::Trapped("null_operand: event.nope".into())
    );

    let null_field = rule(simple(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "eq", "right": {"type":"const","value": "X"} }),
    ));
    assert!(matches!(
        block_on(evaluate_rule(&null_field, &ctx(), &MockProvider::default())).outcome,
        Outcome::Trapped(_)
    ));
}

#[test]
fn missing_as_no_match_turns_missing_fields_into_no_match_only() {
    let mut v = simple(
        json!({ "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} }),
    );
    v["missing_as_no_match"] = json!(true);
    assert_eq!(
        block_on(evaluate_rule(&rule(v), &ctx(), &MockProvider::default())).outcome,
        Outcome::NoMatch
    );

    // a type mismatch still traps
    let mut v = simple(
        json!({ "left": {"type":"field","path":"event.channel"}, "op": "gt", "right": {"type":"const","value": 1} }),
    );
    v["missing_as_no_match"] = json!(true);
    assert!(matches!(
        block_on(evaluate_rule(&rule(v), &ctx(), &MockProvider::default())).outcome,
        Outcome::Trapped(_)
    ));
}

#[test]
fn kleene_any_with_trapped_member() {
    // any(trapped, true) = match ; any(trapped, false) = trapped
    let hit = rule(simple(json!({ "any": [
        { "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} },
        { "left": {"type":"field","path":"event.channel"}, "op": "eq", "right": {"type":"const","value": "web"} }
    ]})));
    assert_eq!(
        block_on(evaluate_rule(&hit, &ctx(), &MockProvider::default())).outcome,
        Outcome::Match
    );
    let unknown = rule(simple(json!({ "any": [
        { "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} },
        { "left": {"type":"field","path":"event.channel"}, "op": "eq", "right": {"type":"const","value": "app"} }
    ]})));
    assert!(matches!(
        block_on(evaluate_rule(&unknown, &ctx(), &MockProvider::default())).outcome,
        Outcome::Trapped(_)
    ));
    // all(trapped, false) = no_match
    let no = rule(simple(json!({ "all": [
        { "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} },
        { "left": {"type":"field","path":"event.channel"}, "op": "eq", "right": {"type":"const","value": "app"} }
    ]})));
    assert_eq!(
        block_on(evaluate_rule(&no, &ctx(), &MockProvider::default())).outcome,
        Outcome::NoMatch
    );
}

#[test]
fn weighted_scoring_gives_partial_fraction() {
    let mut v = simple(json!({ "all": [
        { "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"const","value": 1000000}, "weight": 3 },
        { "left": {"type":"field","path":"features.is_new_device"}, "op": "eq", "right": {"type":"const","value": 1}, "weight": 1 },
        { "left": {"type":"field","path":"customer.kyc_level"}, "op": "gte", "right": {"type":"const","value": 3}, "weight": 2 }
    ]}));
    v["definition"]["scoring"] = json!("weighted");
    let result = block_on(evaluate_rule(&rule(v), &ctx(), &MockProvider::default()));
    assert_eq!(result.outcome, Outcome::Match);
    assert!((result.fraction - 4.0 / 6.0).abs() < 1e-9, "{}", result.fraction);

    let mut none = simple(
        json!({ "left": {"type":"field","path":"event.amount"}, "op": "lt", "right": {"type":"const","value": 1}, "weight": 2 }),
    );
    none["definition"]["scoring"] = json!("weighted");
    assert_eq!(
        block_on(evaluate_rule(&rule(none), &ctx(), &MockProvider::default())).outcome,
        Outcome::NoMatch
    );
}

#[test]
fn formula_operand_with_header_and_traps() {
    // F(x,y,z) = 2x + 2^y / z^2 with x = features.cust_cnt_24h (4), y = 3, z = 2 → 8 + 2 = 10
    let r = rule(simple(json!({
        "left": { "type": "formula", "expr": "F(x,y,z) = 2x + 2^y / z^2", "args": {
            "x": {"type":"field","path":"features.cust_cnt_24h"},
            "y": {"type":"const","value": 3},
            "z": {"type":"const","value": 2} } },
        "op": "eq", "right": {"type":"const","value": 10}
    })));
    assert_eq!(
        block_on(evaluate_rule(&r, &ctx(), &MockProvider::default())).outcome,
        Outcome::Match
    );

    let div0 = rule(simple(json!({
        "left": { "type": "formula", "expr": "x / y", "args": {
            "x": {"type":"field","path":"event.amount"}, "y": {"type":"const","value": 0} } },
        "op": "gt", "right": {"type":"const","value": 1}
    })));
    assert_eq!(
        block_on(evaluate_rule(&div0, &ctx(), &MockProvider::default())).outcome,
        Outcome::Trapped("division_by_zero".into())
    );

    let null_arg = rule(simple(json!({
        "left": { "type": "formula", "expr": "x * 2", "args": { "x": {"type":"field","path":"event.nope"} } },
        "op": "gt", "right": {"type":"const","value": 1}
    })));
    let out = block_on(evaluate_rule(&null_arg, &ctx(), &MockProvider::default())).outcome;
    assert!(
        matches!(&out, Outcome::Trapped(r) if r.starts_with("null_operand: formula arg 'x'")),
        "{out:?}"
    );

    // formula compared against a customer attribute (income ratio)
    let ratio = rule(simple(json!({
        "left": { "type": "formula", "expr": "amount / income", "args": {
            "amount": {"type":"field","path":"event.amount"},
            "income": {"type":"field","path":"customer.attributes.monthly_income"} } },
        "op": "gt", "right": {"type":"const","value": 1.2}
    })));
    assert_eq!(
        block_on(evaluate_rule(&ratio, &ctx(), &MockProvider::default())).outcome,
        Outcome::Match
    );
}

#[test]
fn source_paths_and_array_indexes() {
    let r = rule(simple(json!({ "all": [
        { "left": {"type":"field","path":"source.order.items[0].sku"}, "op": "eq", "right": {"type":"const","value": "A"} },
        { "left": {"type":"field","path":"source.order.coupon"}, "op": "regex", "right": {"type":"const","value": "^HEMAT\\d+$"} }
    ]})));
    assert_eq!(
        block_on(evaluate_rule(&r, &ctx(), &MockProvider::default())).outcome,
        Outcome::Match
    );
}

// ------------------------------------------------------------------------------------------------ velocity

fn velocity_rule(body: serde_json::Value) -> rule_engine::RuleEnvelope {
    let mut def = body;
    def["kind"] = json!("velocity");
    rule(envelope("velocity", def))
}

#[test]
fn velocity_builds_resolved_query_and_compares() {
    let r = velocity_rule(json!({
        "history_event_types": ["transaction"],
        "group_by": ["instrument_fingerprint"],
        "window": { "duration": "30d" },
        "aggregate": { "fn": "distinct_count", "field": "customer_id" },
        "statistic": null, "include_current": true, "min_samples": 1,
        "compare": { "op": "gte", "right": {"type":"const","value": 3} }
    }));
    let provider = MockProvider::aggregate(4.0, 4);
    let result = block_on(evaluate_rule(&r, &ctx(), &provider));
    assert_eq!(result.outcome, Outcome::Match);
    assert_eq!(result.trace["value"], json!(4));
    let q = provider.last_velocity_query();
    assert_eq!(q.group_by[0].field, "instrument_fingerprint");
    assert_eq!(q.group_by[0].value, json!("card-abc"));
    assert_eq!(q.window, QueryWindow::Duration { seconds: 30 * 86_400 });
    assert_eq!(q.aggregate_fn, AggFn::DistinctCount);
    assert_eq!(q.aggregate_field.as_deref(), Some("customer_id"));
    assert!(q.include_current);
    assert_eq!(q.event_id.as_deref(), Some("evt-1"));
    assert_eq!(q.anchor, t0());
    assert_eq!(q.series, SeriesRequest::None);
    assert!(q.filter.is_none());
}

#[test]
fn velocity_defaults_and_compare_against_field() {
    // sum(amount) 24h per customer vs customer.attributes.monthly_income; history types default to current type
    let r = velocity_rule(json!({
        "group_by": ["customer_id"], "window": { "duration": "24h" },
        "aggregate": { "fn": "sum", "field": "amount" },
        "compare": { "op": "gt", "right": {"type":"field","path":"customer.attributes.monthly_income"} }
    }));
    let provider = MockProvider::aggregate(6_000_000.0, 3);
    assert_eq!(
        block_on(evaluate_rule(&r, &ctx(), &provider)).outcome,
        Outcome::Match
    );
    let q = provider.last_velocity_query();
    assert_eq!(q.history_event_types, vec!["transaction".to_string()]);
    assert_eq!(q.group_by[0].value, json!("cust-1"));
}

#[test]
fn velocity_customer_id_falls_back_to_context_id() {
    let mut c = ctx();
    c.data["event"].as_object_mut().unwrap().remove("customer_id");
    let r = velocity_rule(json!({
        "group_by": ["customer_id"], "window": { "last_n": 10 },
        "aggregate": { "fn": "count" }, "compare": { "op": "gt", "right": {"type":"const","value": 5} }
    }));
    let provider = MockProvider::aggregate(2.0, 2);
    assert_eq!(
        block_on(evaluate_rule(&r, &c, &provider)).outcome,
        Outcome::NoMatch
    );
    let q = provider.last_velocity_query();
    assert_eq!(q.group_by[0].value, json!("cust-1"));
    assert_eq!(q.window, QueryWindow::LastN { n: 10 });
}

#[test]
fn velocity_traps() {
    let base = json!({
        "group_by": ["device_id"], "window": { "duration": "1h" },
        "aggregate": { "fn": "avg", "field": "amount" }, "min_samples": 3,
        "compare": { "op": "gt", "right": {"type":"const","value": 5} }
    });
    // insufficient samples
    let out = block_on(evaluate_rule(
        &velocity_rule(base.clone()),
        &ctx(),
        &MockProvider::aggregate(10.0, 2),
    ))
    .outcome;
    assert!(
        matches!(&out, Outcome::Trapped(r) if r.starts_with("insufficient_history")),
        "{out:?}"
    );
    // avg over empty history
    let mut b = base.clone();
    b["min_samples"] = json!(0);
    let empty = MockProvider::with_velocity(|_| Ok(VelocityData::default()));
    let out = block_on(evaluate_rule(&velocity_rule(b), &ctx(), &empty)).outcome;
    assert!(
        matches!(&out, Outcome::Trapped(r) if r.starts_with("insufficient_history")),
        "{out:?}"
    );
    // count over empty history is 0, not a trap
    let count = velocity_rule(json!({
        "group_by": ["device_id"], "window": { "duration": "1h" }, "aggregate": { "fn": "count" },
        "compare": { "op": "gte", "right": {"type":"const","value": 1} }
    }));
    assert_eq!(
        block_on(evaluate_rule(&count, &ctx(), &empty)).outcome,
        Outcome::NoMatch
    );
    // provider error
    let failing = MockProvider::with_velocity(|_| Err(ProviderError::Timeout));
    assert_eq!(
        block_on(evaluate_rule(&velocity_rule(base.clone()), &ctx(), &failing)).outcome,
        Outcome::Trapped("timeout".into())
    );
    // missing group-by value: trapped, or no_match with missing_as_no_match
    let mut missing = base;
    missing["group_by"] = json!(["promo_code"]);
    let r = velocity_rule(missing.clone());
    let out = block_on(evaluate_rule(&r, &ctx(), &MockProvider::aggregate(1.0, 5))).outcome;
    assert_eq!(out, Outcome::Trapped("null_group_by_value: promo_code".into()));
    let mut env = envelope("velocity", {
        let mut d = missing;
        d["kind"] = json!("velocity");
        d
    });
    env["missing_as_no_match"] = json!(true);
    let provider = MockProvider::aggregate(1.0, 5);
    assert_eq!(
        block_on(evaluate_rule(&rule(env), &ctx(), &provider)).outcome,
        Outcome::NoMatch
    );
    assert_eq!(
        provider.velocity_call_count(),
        0,
        "no query when group-by value is missing"
    );
}

#[test]
fn velocity_zscore_and_gaussian_tail() {
    // history 3..7 (mean 5, sd 1.5811); current amount 7_500_000 → huge z; use `of` to control input
    let zscore = velocity_rule(json!({
        "group_by": ["customer_id"], "window": { "duration": "30d" },
        "aggregate": { "fn": "avg", "field": "amount" },
        "statistic": { "fn": "zscore", "of": {"type":"const","value": 10} },
        "compare": { "op": "gt", "right": {"type":"const","value": 3} }
    }));
    let provider = MockProvider::with_velocity(|_| {
        Ok(VelocityData {
            aggregate: Some(5.0),
            samples: 5,
            values: vec![3.0, 4.0, 5.0, 6.0, 7.0],
            buckets: vec![],
        })
    });
    let result = block_on(evaluate_rule(&zscore, &ctx(), &provider));
    assert_eq!(result.outcome, Outcome::Match);
    let z = result.trace["value"].as_f64().unwrap();
    assert!((z - 5.0 / 1.581_138_830_084_19).abs() < 1e-9, "{z}");
    let q = provider.last_velocity_query();
    assert_eq!(q.series, SeriesRequest::Values);
    assert!(
        !q.include_current,
        "statistics exclude the current event by default"
    );

    let tail = velocity_rule(json!({
        "group_by": ["customer_id"], "window": { "duration": "30d" },
        "aggregate": { "fn": "avg", "field": "amount" },
        "statistic": { "fn": "gaussian_tail", "tail": "upper" },
        "compare": { "op": "lt", "right": {"type":"const","value": 0.01} }
    }));
    let provider = MockProvider::with_velocity(|_| {
        Ok(VelocityData {
            aggregate: None,
            samples: 6,
            values: vec![100_000.0, 150_000.0, 120_000.0, 90_000.0, 110_000.0, 130_000.0],
            buckets: vec![],
        })
    });
    // default input = current event amount (7.5M) → far upper tail
    assert_eq!(
        block_on(evaluate_rule(&tail, &ctx(), &provider)).outcome,
        Outcome::Match
    );

    // zero variance traps
    let flat = MockProvider::with_velocity(|_| {
        Ok(VelocityData {
            aggregate: None,
            samples: 5,
            values: vec![1.0; 5],
            buckets: vec![],
        })
    });
    assert_eq!(
        block_on(evaluate_rule(&tail, &ctx(), &flat)).outcome,
        Outcome::Trapped("zero_variance".into())
    );
}

#[test]
fn velocity_percentile_rank_linear_trend_poisson() {
    let pr = velocity_rule(json!({
        "group_by": ["customer_id"], "window": { "duration": "30d" },
        "aggregate": { "fn": "avg", "field": "amount" },
        "statistic": { "fn": "percentile_rank" }, "min_samples": 4,
        "compare": { "op": "gte", "right": {"type":"const","value": 0.99} }
    }));
    let provider = MockProvider::with_velocity(|_| {
        Ok(VelocityData {
            aggregate: None,
            samples: 4,
            values: vec![1.0, 2.0, 3.0, 4.0],
            buckets: vec![],
        })
    });
    assert_eq!(
        block_on(evaluate_rule(&pr, &ctx(), &provider)).outcome,
        Outcome::Match
    );

    let buckets = |values: &[f64]| -> Vec<BucketPoint> {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| BucketPoint {
                start: t0() - Duration::days(5 - i as i64),
                value: *v,
            })
            .collect()
    };
    let trend = velocity_rule(json!({
        "group_by": ["merchant_id"], "window": { "duration": "7d" },
        "aggregate": { "fn": "count" },
        "statistic": { "fn": "linear_trend", "bucket": "1d", "output": "slope" }, "min_samples": 3,
        "compare": { "op": "gte", "right": {"type":"const","value": 1} }
    }));
    let b = buckets(&[1.0, 2.0, 3.0, 4.0, 9.0]);
    let provider = MockProvider::with_velocity(move |_| {
        Ok(VelocityData {
            samples: 19,
            buckets: b.clone(),
            ..Default::default()
        })
    });
    let result = block_on(evaluate_rule(&trend, &ctx(), &provider));
    assert_eq!(result.outcome, Outcome::Match);
    assert_eq!(result.trace["value"], json!(1));
    assert_eq!(
        provider.last_velocity_query().series,
        SeriesRequest::Buckets {
            bucket_seconds: 86_400,
            func: AggFn::Count
        }
    );

    let poisson = velocity_rule(json!({
        "group_by": ["api_client_id"], "window": { "duration": "5h" },
        "aggregate": { "fn": "count" },
        "statistic": { "fn": "poisson_tail", "bucket": "1h" }, "min_samples": 4,
        "compare": { "op": "lt", "right": {"type":"const","value": 0.06} }
    }));
    let pb = buckets(&[1.0, 3.0, 2.0, 2.0, 5.0]);
    let mut c = ctx();
    c.data["event"]["api_client_id"] = json!("client-7");
    let provider = MockProvider::with_velocity(move |_| {
        Ok(VelocityData {
            samples: 13,
            buckets: pb.clone(),
            ..Default::default()
        })
    });
    assert_eq!(
        block_on(evaluate_rule(&poisson, &c, &provider)).outcome,
        Outcome::Match
    );
}

// ------------------------------------------------------------------------------------------------ composite

fn composite_rule(gate: serde_json::Value, filter: serde_json::Value) -> rule_engine::RuleEnvelope {
    rule(envelope(
        "composite",
        json!({
            "kind": "composite",
            "gate": gate,
            "history_filter": filter,
            "velocity": {
                "history_event_types": ["promo_redemption", "transaction"],
                "group_by": ["device_id"], "window": { "duration": "7d" },
                "aggregate": { "fn": "distinct_count", "field": "customer_id" },
                "compare": { "op": "gte", "right": {"type":"const","value": 3} }
            }
        }),
    ))
}

fn promo_ctx() -> rule_engine::EvalContext {
    let mut c = ctx();
    c.data["event"]["promo_code"] = json!("FLASH50");
    c
}

#[test]
fn composite_gate_filter_and_velocity() {
    let r = composite_rule(
        json!({ "all": [ { "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" } ] }),
        json!({ "all": [
            { "left": {"type":"hist","path":"promo_code"}, "op": "eq", "right": {"type":"field","path":"event.promo_code"} },
            { "left": {"type":"hist","path":"discount_amount"}, "op": "gt", "right": {"type":"const","value": 0} }
        ]}),
    );
    let provider = MockProvider::aggregate(4.0, 6);
    let result = block_on(evaluate_rule(&r, &promo_ctx(), &provider));
    assert_eq!(result.outcome, Outcome::Match);
    let q = provider.last_velocity_query();
    assert_eq!(
        q.filter,
        Some(HistFilter::And(vec![
            HistFilter::Pred(HistPredicate {
                field: "promo_code".into(),
                op: rule_engine::model::Op::Eq,
                value: json!("FLASH50")
            }),
            HistFilter::Pred(HistPredicate {
                field: "discount_amount".into(),
                op: rule_engine::model::Op::Gt,
                value: json!(0)
            }),
        ]))
    );
    assert_eq!(
        q.history_event_types,
        vec!["promo_redemption".to_string(), "transaction".to_string()]
    );
    assert!(result.trace["history_filter"].is_object());
}

#[test]
fn composite_gate_short_circuits() {
    let r = composite_rule(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" }),
        json!({ "left": {"type":"hist","path":"promo_code"}, "op": "eq", "right": {"type":"field","path":"event.promo_code"} }),
    );
    let provider = MockProvider::aggregate(10.0, 10);
    // promo_code is null in the default context → gate no_match → no query
    assert_eq!(
        block_on(evaluate_rule(&r, &ctx(), &provider)).outcome,
        Outcome::NoMatch
    );
    assert_eq!(provider.velocity_call_count(), 0);

    let trapped_gate = composite_rule(
        json!({ "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} }),
        json!({ "left": {"type":"hist","path":"promo_code"}, "op": "is_not_null" }),
    );
    assert!(matches!(
        block_on(evaluate_rule(&trapped_gate, &ctx(), &provider)).outcome,
        Outcome::Trapped(_)
    ));
    assert_eq!(provider.velocity_call_count(), 0);
}

#[test]
fn composite_invalid_or_null_filter_traps() {
    let provider = MockProvider::aggregate(10.0, 10);
    let not_hist = composite_rule(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" }),
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "eq", "right": {"type":"const","value": "X"} }),
    );
    let out = block_on(evaluate_rule(&not_hist, &promo_ctx(), &provider)).outcome;
    assert!(
        matches!(&out, Outcome::Trapped(r) if r.starts_with("invalid_history_filter")),
        "{out:?}"
    );

    let bad_op = composite_rule(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" }),
        json!({ "left": {"type":"hist","path":"promo_code"}, "op": "regex", "right": {"type":"const","value": "^F"} }),
    );
    let out = block_on(evaluate_rule(&bad_op, &promo_ctx(), &provider)).outcome;
    assert!(
        matches!(&out, Outcome::Trapped(r) if r.contains("not allowed")),
        "{out:?}"
    );

    let null_right = composite_rule(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" }),
        json!({ "left": {"type":"hist","path":"merchant_category"}, "op": "eq", "right": {"type":"field","path":"event.merchant_category"} }),
    );
    let out = block_on(evaluate_rule(&null_right, &promo_ctx(), &provider)).outcome;
    assert_eq!(
        out,
        Outcome::Trapped("null_operand: event.merchant_category".into())
    );
    assert_eq!(provider.velocity_call_count(), 0);
}

#[test]
fn composite_filter_supports_any_not_in() {
    let r = composite_rule(
        json!({ "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" }),
        json!({ "any": [
            { "left": {"type":"hist","path":"channel"}, "op": "in", "right": {"type":"const","value": ["web", "app"]} },
            { "not": { "left": {"type":"hist","path":"status"}, "op": "is_null" } }
        ]}),
    );
    let provider = MockProvider::aggregate(1.0, 1);
    assert_eq!(
        block_on(evaluate_rule(&r, &promo_ctx(), &provider)).outcome,
        Outcome::NoMatch
    );
    let q = provider.last_velocity_query();
    assert!(
        matches!(q.filter, Some(HistFilter::Or(ref items)) if items.len() == 2 && matches!(items[1], HistFilter::Not(_)))
    );
}

// ------------------------------------------------------------------------------------------------ reference

fn reference_rule(def: serde_json::Value) -> rule_engine::RuleEnvelope {
    let mut d = def;
    d["kind"] = json!("reference");
    rule(envelope("reference", d))
}

#[test]
fn reference_modes() {
    let provider = MockProvider::default()
        .with_list(
            "card_blacklist",
            &[
                ("card-abc", json!({"reason": "chargeback"}), true),
                ("card-old", json!({}), false),
            ],
        )
        .with_list(
            "merchant_limits",
            &[("m-1", json!({"max_amount": 5_000_000}), true)],
        );
    let exists = reference_rule(
        json!({ "list": "card_blacklist", "key": {"type":"field","path":"event.instrument_fingerprint"}, "mode": "exists" }),
    );
    let result = block_on(evaluate_rule(&exists, &ctx(), &provider));
    assert_eq!(result.outcome, Outcome::Match);
    assert_eq!(result.trace["key"], json!("card-abc"));

    let not_exists = reference_rule(
        json!({ "list": "card_blacklist", "key": {"type":"field","path":"event.device_id"}, "mode": "not_exists" }),
    );
    assert_eq!(
        block_on(evaluate_rule(&not_exists, &ctx(), &provider)).outcome,
        Outcome::Match
    );

    // entry outside its validity window counts as not found
    let mut c = ctx();
    c.data["event"]["instrument_fingerprint"] = json!("card-old");
    assert_eq!(
        block_on(evaluate_rule(&exists, &c, &provider)).outcome,
        Outcome::NoMatch
    );

    let attribute = reference_rule(json!({
        "list": "merchant_limits", "key": {"type":"field","path":"event.merchant_id"}, "mode": "attribute",
        "attribute_condition": { "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"ref","path":"max_amount"} }
    }));
    assert_eq!(
        block_on(evaluate_rule(&attribute, &ctx(), &provider)).outcome,
        Outcome::Match
    );
    let mut c = ctx();
    c.data["event"]["merchant_id"] = json!("m-unknown");
    assert_eq!(
        block_on(evaluate_rule(&attribute, &c, &provider)).outcome,
        Outcome::NoMatch
    );

    // numeric keys are rendered as text
    let numeric = reference_rule(
        json!({ "list": "card_blacklist", "key": {"type":"field","path":"customer.kyc_level"}, "mode": "exists" }),
    );
    block_on(evaluate_rule(&numeric, &ctx(), &provider));
    assert_eq!(provider.reference_calls.lock().unwrap().last().unwrap().1, "1");
}

#[test]
fn reference_traps() {
    let provider = MockProvider::default();
    let unknown = reference_rule(
        json!({ "list": "nope", "key": {"type":"field","path":"event.device_id"}, "mode": "exists" }),
    );
    assert_eq!(
        block_on(evaluate_rule(&unknown, &ctx(), &provider)).outcome,
        Outcome::Trapped("unknown_reference_list".into())
    );
    let null_key = reference_rule(
        json!({ "list": "nope", "key": {"type":"field","path":"event.promo_code"}, "mode": "exists" }),
    );
    assert_eq!(
        block_on(evaluate_rule(&null_key, &ctx(), &provider)).outcome,
        Outcome::Trapped("null_operand: event.promo_code".into())
    );
    assert!(provider
        .reference_calls
        .lock()
        .unwrap()
        .iter()
        .all(|(_, k)| k != "null"));
}

// ------------------------------------------------------------------------------------------------ graph

fn graph_rule(metric: &str, op: &str, value: serde_json::Value) -> rule_engine::RuleEnvelope {
    rule(envelope(
        "graph",
        json!({
            "kind": "graph", "metric": metric,
            "link_kinds": ["phone", "card", "device"], "include_similar": true, "max_depth": 3,
            "compare": { "op": op, "right": {"type":"const","value": value} }
        }),
    ))
}

#[test]
fn graph_metrics() {
    let near = graph_rule("distance_to_fraud", "lte", json!(2));
    let provider = MockProvider::default().with_graph("distance_to_fraud", Some(2.0));
    assert_eq!(
        block_on(evaluate_rule(&near, &ctx(), &provider)).outcome,
        Outcome::Match
    );
    let q = provider.graph_calls.lock().unwrap()[0].clone();
    assert_eq!(q.customer_id, "cust-1");
    assert_eq!(q.max_depth, 3);
    assert_eq!(q.link_kinds, vec!["phone", "card", "device"]);

    // no fraud within depth → +∞ → never ≤ 2, and never trapped
    let provider = MockProvider::default().with_graph("distance_to_fraud", None);
    let result = block_on(evaluate_rule(&near, &ctx(), &provider));
    assert_eq!(result.outcome, Outcome::NoMatch);
    assert_eq!(result.trace["value"], json!("Infinity"));

    let community = graph_rule("community_fraud_rate", "gt", json!(0.3));
    let provider = MockProvider::default().with_graph("community_fraud_rate", None);
    assert_eq!(
        block_on(evaluate_rule(&community, &ctx(), &provider)).outcome,
        Outcome::Trapped("no_community".into())
    );

    let neighbors = graph_rule("fraud_neighbors", "gte", json!(1));
    let provider = MockProvider {
        graph_error: true,
        ..Default::default()
    };
    assert!(matches!(
        block_on(evaluate_rule(&neighbors, &ctx(), &provider)).outcome,
        Outcome::Trapped(_)
    ));

    let mut c = ctx();
    c.customer_id = None;
    c.data["event"].as_object_mut().unwrap().remove("customer_id");
    let provider = MockProvider::default().with_graph("distance_to_fraud", Some(1.0));
    assert_eq!(
        block_on(evaluate_rule(&near, &c, &provider)).outcome,
        Outcome::Trapped("null_operand: customer_id".into())
    );
}
