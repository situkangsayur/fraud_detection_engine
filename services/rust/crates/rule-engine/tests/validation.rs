//! Save-time validation (rule-dsl §8): shape errors with JSON paths and semantic errors with rule paths.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rule_engine::model::RulesetSpec;
use rule_engine::validate::{validate_rule_json, validate_ruleset, StaticCatalog};
use serde_json::{json, Value};

fn catalog() -> StaticCatalog {
    StaticCatalog::default()
        .with_paths([
            "event.amount",
            "event.issuer_country",
            "event.geo_country",
            "event.promo_code",
            "event.merchant_id",
            "event.instrument_fingerprint",
            "event.device_id",
            "features.cust_cnt_24h",
            "customer.kyc_level",
        ])
        .with_velocity_fields([
            "customer_id",
            "amount",
            "device_id",
            "instrument_fingerprint",
            "promo_code",
            "discount_amount",
        ])
}

fn envelope(kind: &str, definition: Value) -> Value {
    json!({ "code": "RL-TEST-001", "name": "test", "kind": kind, "risk_score": 40,
            "typologies": ["carding"], "event_types": ["transaction"], "definition": definition })
}

fn errors(v: &Value) -> Vec<(String, String)> {
    let (report, _) = validate_rule_json(v, &catalog());
    report.errors.into_iter().map(|e| (e.path, e.message)).collect()
}

fn assert_error(v: &Value, path: &str, contains: &str) {
    let errs = errors(v);
    assert!(
        errs.iter().any(|(p, m)| p == path && m.contains(contains)),
        "expected error at `{path}` containing `{contains}`, got {errs:#?}"
    );
}

#[test]
fn valid_rule_reports_references() {
    let v = envelope(
        "reference",
        json!({
            "kind": "reference", "list": "merchant_limits", "key": {"type":"field","path":"event.merchant_id"},
            "mode": "attribute",
            "attribute_condition": { "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"ref","path":"max_amount"} }
        }),
    );
    let (report, rule) = validate_rule_json(&v, &catalog());
    assert!(report.valid, "{:#?}", report.errors);
    assert!(rule.is_some());
    assert_eq!(report.referenced_lists, vec!["merchant_limits"]);
    assert_eq!(
        report.referenced_fields,
        vec!["event.amount", "event.merchant_id"]
    );
}

#[test]
fn shape_errors_carry_json_paths() {
    let v = envelope(
        "simple",
        json!({ "kind": "simple", "when": {
        "left": {"type":"field","path":"event.amount"}, "op": "greater_than", "right": {"type":"const","value": 1} } }),
    );
    let errs = errors(&v);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].0.starts_with("definition"), "{errs:?}");
    assert!(errs[0].1.contains("greater_than"), "{errs:?}");

    let v = json!({ "code": "RL-X-001", "name": "x", "kind": "simple", "risk_score": 1, "definiton": {} });
    let errs = errors(&v);
    assert!(errs[0].1.contains("definiton"), "{errs:?}");

    let v = envelope(
        "velocity",
        json!({ "kind": "velocity", "group_by": ["amount"], "window": {"duration": "30x"},
        "aggregate": {"fn": "count"}, "compare": {"op": "gt", "right": {"type":"const","value": 1}} }),
    );
    let errs = errors(&v);
    assert!(errs[0].0.starts_with("definition"), "{errs:?}");

    let v = envelope(
        "velocity",
        json!({ "kind": "velocity", "group_by": ["amount"], "window": {"duration": "1d"},
        "aggregate": {"fn": "count"}, "statistic": {"fn": "median_absolute"},
        "compare": {"op": "gt", "right": {"type":"const","value": 1}} }),
    );
    assert!(errors(&v)[0].1.contains("median_absolute"));
}

#[test]
fn envelope_errors() {
    let mut v = envelope(
        "velocity",
        json!({ "kind": "simple", "when": {
        "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"const","value": 1} } }),
    );
    v["code"] = json!("bad code");
    v["risk_score"] = json!(150);
    v["typologies"] = json!(["carding", "phishing"]);
    assert_error(&v, "kind", "does not match definition.kind");
    assert_error(&v, "code", "code must match");
    assert_error(&v, "risk_score", "0..100");
    assert_error(&v, "typologies[1]", "unknown typology 'phishing'");
}

#[test]
fn field_paths_and_positions() {
    let v = envelope(
        "simple",
        json!({ "kind": "simple", "when": { "all": [
            { "left": {"type":"field","path":"event.unknown"}, "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"field","path":"source.any.thing[0]"}, "op": "eq", "right": {"type":"const","value": 1} },
            { "left": {"type":"field","path":"customer.attributes.income"}, "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"field","path":"payload.x"}, "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"ref","path":"x"}, "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"hist","path":"amount"}, "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"field","path":"event.amount"}, "op": "is_null", "right": {"type":"const","value": 1} },
            { "left": {"type":"field","path":"event.amount"}, "op": "gt" }
        ]}}),
    );
    let errs = errors(&v);
    assert_error(
        &v,
        "definition.when.all[0].left.path",
        "unknown field 'event.unknown'",
    );
    assert!(
        !errs.iter().any(|(p, _)| p.starts_with("definition.when.all[1]")),
        "source.* is always allowed"
    );
    assert!(
        !errs.iter().any(|(p, _)| p.starts_with("definition.when.all[2]")),
        "customer.attributes.* is allowed"
    );
    assert_error(&v, "definition.when.all[3].left.path", "must start with one of");
    assert_error(
        &v,
        "definition.when.all[4].left",
        "`ref` operands are only allowed",
    );
    assert_error(
        &v,
        "definition.when.all[5].left",
        "`hist` operands are only allowed",
    );
    assert_error(&v, "definition.when.all[6].right", "takes no right operand");
    assert_error(&v, "definition.when.all[7].right", "needs a right operand");
}

#[test]
fn operator_constant_shapes_and_formulas() {
    let v = envelope(
        "simple",
        json!({ "kind": "simple", "when": { "all": [
            { "left": {"type":"field","path":"event.amount"}, "op": "between", "right": {"type":"const","value": [1]} },
            { "left": {"type":"field","path":"event.geo_country"}, "op": "in", "right": {"type":"const","value": "ID"} },
            { "left": {"type":"field","path":"event.geo_country"}, "op": "regex", "right": {"type":"const","value": "("} },
            { "left": {"type":"field","path":"event.geo_country"}, "op": "similar", "right": {"type":"const","value": "ID"}, "options": {"threshold": 1.5} },
            { "left": {"type":"formula","expr":"F(x,y) = x + y + z","args":{"x":{"type":"field","path":"event.amount"},"y":{"type":"const","value":1}}},
              "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"formula","expr":"x + ","args":{"x":{"type":"field","path":"event.amount"}}},
              "op": "gt", "right": {"type":"const","value": 1} },
            { "left": {"type":"formula","expr":"F(x) = x","args":{"x":{"type":"field","path":"event.amount"},"w":{"type":"field","path":"event.nope"}}},
              "op": "gt", "right": {"type":"const","value": 1}, "weight": -1 }
        ]}}),
    );
    assert_error(
        &v,
        "definition.when.all[0].right.value",
        "between expects an array",
    );
    assert_error(&v, "definition.when.all[1].right.value", "expects an array");
    assert_error(&v, "definition.when.all[2].right.value", "invalid regex");
    assert_error(&v, "definition.when.all[3].options.threshold", "0..1");
    assert_error(
        &v,
        "definition.when.all[4].left.expr",
        "variable 'z' is not declared in the header (position 17)",
    );
    assert_error(&v, "definition.when.all[5].left.expr", "end of formula");
    assert_error(&v, "definition.when.all[6].left.expr", "unexpected args [w]");
    assert_error(
        &v,
        "definition.when.all[6].left.args.w.path",
        "unknown field 'event.nope'",
    );
    assert_error(&v, "definition.when.all[6].weight", ">= 0");
}

#[test]
fn velocity_semantics() {
    let v = envelope(
        "velocity",
        json!({
            "kind": "velocity", "group_by": ["merchant_category"], "window": {"last_n": 20},
            "aggregate": {"fn": "percentile", "field": "amount"},
            "statistic": {"fn": "poisson_tail", "bucket": "1h"},
            "compare": {"op": "regex", "right": {"type":"hist","path":"amount"}}
        }),
    );
    assert_error(&v, "definition.group_by[0]", "not velocity-enabled");
    assert_error(&v, "definition.aggregate.p", "percentile needs p");
    assert_error(&v, "definition.window", "needs a duration window");
    assert_error(&v, "definition.compare.op", "is not allowed in compare");
    assert_error(&v, "definition.compare.right", "`hist` operands are only allowed");

    let v = envelope(
        "velocity",
        json!({
            "kind": "velocity", "group_by": [], "window": {"duration": "1h"},
            "aggregate": {"fn": "sum", "p": 0.5},
            "statistic": {"fn": "linear_trend", "bucket": "2h", "output": "slope"},
            "compare": {"op": "gt", "right": {"type":"const","value": 1}}
        }),
    );
    assert_error(&v, "definition.group_by", "at least one field");
    assert_error(&v, "definition.aggregate.field", "needs a field");
    assert_error(&v, "definition.aggregate.p", "only allowed with fn = percentile");
    assert_error(&v, "definition.statistic.bucket", "shorter than the window");

    let v = envelope(
        "velocity",
        json!({
            "kind": "velocity", "group_by": ["customer_id"], "window": {"duration": "30d"},
            "aggregate": {"fn": "count"}, "statistic": {"fn": "zscore"},
            "compare": {"op": "gt", "right": {"type":"const","value": 3}}
        }),
    );
    assert_error(&v, "definition.aggregate.field", "needs aggregate.field");
}

#[test]
fn composite_semantics() {
    let v = envelope(
        "composite",
        json!({
            "kind": "composite",
            "gate": { "left": {"type":"hist","path":"promo_code"}, "op": "is_not_null" },
            "history_filter": { "all": [
                { "left": {"type":"field","path":"event.promo_code"}, "op": "eq", "right": {"type":"const","value": "X"} },
                { "left": {"type":"hist","path":"promo_code"}, "op": "regex", "right": {"type":"const","value": "^X"} },
                { "left": {"type":"hist","path":"merchant_category"}, "op": "eq", "right": {"type":"hist","path":"promo_code"} },
                { "at_least": { "n": 1, "of": [ { "left": {"type":"hist","path":"promo_code"}, "op": "is_null" } ] } }
            ]},
            "velocity": { "group_by": ["device_id"], "window": {"duration": "7d"},
                          "aggregate": {"fn": "distinct_count", "field": "customer_id"},
                          "compare": {"op": "gte", "right": {"type":"const","value": 3}} }
        }),
    );
    assert_error(&v, "definition.gate.left", "`hist` operands are only allowed");
    assert_error(
        &v,
        "definition.history_filter.all[0].left",
        "must be a `hist` operand",
    );
    assert_error(
        &v,
        "definition.history_filter.all[1].op",
        "not allowed in history_filter",
    );
    assert_error(
        &v,
        "definition.history_filter.all[2].left.path",
        "not velocity-enabled",
    );
    assert_error(
        &v,
        "definition.history_filter.all[2].right",
        "must be const, field or formula",
    );
    assert_error(
        &v,
        "definition.history_filter.all[3].at_least",
        "not supported in history_filter",
    );
}

#[test]
fn reference_and_graph_semantics() {
    let v = envelope(
        "reference",
        json!({ "kind": "reference", "list": "Bad-List", "key": {"type":"ref","path":"x"}, "mode": "attribute" }),
    );
    assert_error(&v, "definition.list", "list name must match");
    assert_error(&v, "definition.key", "`ref` operands are only allowed");
    assert_error(&v, "definition.attribute_condition", "needs attribute_condition");

    let v = envelope(
        "reference",
        json!({ "kind": "reference", "list": "blacklist", "key": {"type":"field","path":"event.device_id"},
        "mode": "exists", "attribute_condition": { "left": {"type":"ref","path":"x"}, "op": "is_null" } }),
    );
    assert_error(
        &v,
        "definition.attribute_condition",
        "only allowed with mode 'attribute'",
    );

    let v = envelope(
        "graph",
        json!({ "kind": "graph", "metric": "distance_to_fraud", "link_kinds": ["phone", "fax"],
        "max_depth": 5, "compare": {"op": "lte", "right": {"type":"const","value": 2}} }),
    );
    assert_error(&v, "definition.max_depth", "1..3");
    assert_error(&v, "definition.link_kinds[1]", "unknown link kind 'fax'");
}

#[test]
fn ruleset_validation() {
    let spec: RulesetSpec = serde_json::from_value(json!({
        "code": "rs-bad", "name": " ", "typologies": ["x"], "max_score": 120,
        "rules": [ {"rule_id": "a", "weight": 11}, {"rule_id": "a"} ]
    }))
    .unwrap();
    let report = validate_ruleset(&spec);
    let paths: Vec<_> = report.errors.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "code",
            "name",
            "max_score",
            "typologies[0]",
            "rules[0].weight",
            "rules[1].rule_id"
        ]
    );
    assert_eq!(spec.aggregation, rule_engine::model::Aggregation::ProbabilisticOr);
    assert_eq!(spec.rules[1].weight, 1.0);
}
