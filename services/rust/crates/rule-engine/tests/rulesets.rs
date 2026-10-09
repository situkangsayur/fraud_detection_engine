//! Ruleset evaluation: contributions, aggregation, shadow, actions, reasons, dedupe, event types, timeouts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{block_on, ctx, rule, InstantTimer, MockProvider, NeverTimer};
use rule_engine::model::{Action, Aggregation};
use rule_engine::{evaluate_rulesets, EvalOptions, RuleUnit, RulesetUnit};
use serde_json::json;

fn simple_rule(code: &str, risk: f64, hit: bool, extra: serde_json::Value) -> Arc<rule_engine::RuleEnvelope> {
    let op = if hit { "gt" } else { "lt" };
    let mut v = json!({
        "code": code, "name": format!("rule {code}"), "kind": "simple", "risk_score": risk,
        "definition": { "kind": "simple", "when": {
            "left": {"type":"field","path":"event.amount"}, "op": op, "right": {"type":"const","value": 1000} } }
    });
    if let (Some(target), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in extra {
            target.insert(k.clone(), val.clone());
        }
    }
    Arc::new(rule(v))
}

fn trapping_rule(code: &str, extra: serde_json::Value) -> Arc<rule_engine::RuleEnvelope> {
    let mut v = json!({
        "code": code, "name": code, "kind": "simple", "risk_score": 50,
        "definition": { "kind": "simple", "when": {
            "left": {"type":"field","path":"event.nope"}, "op": "gt", "right": {"type":"const","value": 1} } }
    });
    for (k, val) in extra.as_object().unwrap() {
        v[k] = val.clone();
    }
    Arc::new(rule(v))
}

fn unit(id: &str, rule: Arc<rule_engine::RuleEnvelope>, weight: f64) -> RuleUnit {
    RuleUnit {
        rule_id: id.into(),
        version: 1,
        rule,
        weight,
        shadow: false,
    }
}

fn ruleset(code: &str, aggregation: Aggregation, rules: Vec<RuleUnit>) -> RulesetUnit {
    RulesetUnit {
        ruleset_id: format!("id-{code}"),
        code: code.into(),
        aggregation,
        max_score: 100.0,
        event_types: vec![],
        shadow: false,
        rules,
    }
}

#[test]
fn probabilistic_or_scoring_and_reasons() {
    let rs = ruleset(
        "RS-CARDING",
        Aggregation::ProbabilisticOr,
        vec![
            unit("r1", simple_rule("RL-A", 40.0, true, json!({})), 1.0),
            unit(
                "r2",
                simple_rule("RL-B", 50.0, true, json!({"description": "big amount"})),
                0.6,
            ),
            unit("r3", simple_rule("RL-C", 90.0, false, json!({})), 1.0),
        ],
    );
    let result = block_on(evaluate_rulesets(
        &[rs],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    // contributions 40, 30, 0 → 100 × (1 − 0.6 × 0.7) = 58
    assert!((result.rules_score - 58.0).abs() < 1e-9, "{}", result.rules_score);
    assert_eq!(result.rule_results.len(), 3);
    assert_eq!(result.rule_results[1].contribution, 30.0);
    assert_eq!(result.rule_results[2].outcome, "no_match");
    assert_eq!(
        result.reasons.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
        vec!["RL-A", "RL-B"]
    );
    assert_eq!(result.reasons[1].message, "big amount");
    assert_eq!(result.actions.effective(), None);
    let trace = serde_json::to_value(&result.rule_results[0]).unwrap();
    for key in [
        "rule_id",
        "rule_code",
        "version",
        "ruleset_code",
        "kind",
        "outcome",
        "contribution",
        "shadow",
        "action",
        "trapped_reason",
        "trace",
        "duration_us",
    ] {
        assert!(trace.get(key).is_some(), "trace item misses {key}");
    }
}

#[test]
fn rules_score_is_max_over_active_rulesets_and_shadow_is_excluded() {
    let a = ruleset(
        "RS-A",
        Aggregation::Sum,
        vec![unit("r1", simple_rule("RL-A", 30.0, true, json!({})), 1.0)],
    );
    let b = ruleset(
        "RS-B",
        Aggregation::Sum,
        vec![unit("r2", simple_rule("RL-B", 70.0, true, json!({})), 1.0)],
    );
    let mut shadow = ruleset(
        "RS-S",
        Aggregation::Sum,
        vec![unit(
            "r3",
            simple_rule("RL-S", 100.0, true, json!({"action": "force_decline"})),
            1.0,
        )],
    );
    shadow.shadow = true;
    let mut shadow_rule = unit(
        "r4",
        simple_rule("RL-SR", 100.0, true, json!({"action": "force_decline"})),
        1.0,
    );
    shadow_rule.shadow = true;
    let c = ruleset("RS-C", Aggregation::Sum, vec![shadow_rule]);
    let result = block_on(evaluate_rulesets(
        &[a, b, shadow, c],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert_eq!(result.rules_score, 70.0);
    assert_eq!(result.rulesets.len(), 4);
    assert!(result.rulesets[2].shadow);
    assert_eq!(
        result.rulesets[2].score, 100.0,
        "a shadow ruleset reports its own would-be score"
    );
    assert_eq!(
        result.rulesets[3].score, 0.0,
        "shadow rules do not count inside an active ruleset"
    );
    assert!(
        !result.actions.force_decline,
        "shadow rules never trigger actions"
    );
    assert!(result.rule_results.iter().filter(|t| t.shadow).count() == 2);
    assert!(result
        .reasons
        .iter()
        .all(|r| r.code != "RL-S" && r.code != "RL-SR"));
}

#[test]
fn actions_and_trapped_handling() {
    let rules = vec![
        unit(
            "r1",
            simple_rule("RL-WL", 0.0, true, json!({"action": "force_approve"})),
            1.0,
        ),
        unit(
            "r2",
            simple_rule("RL-BL", 0.0, true, json!({"action": "force_decline"})),
            1.0,
        ),
        unit(
            "r3",
            trapping_rule("RL-TR-SCORE", json!({"on_trapped": "score", "trapped_score": 20})),
            1.5,
        ),
        unit(
            "r4",
            trapping_rule("RL-TR-REVIEW", json!({"on_trapped": "review"})),
            1.0,
        ),
        unit("r5", trapping_rule("RL-TR-IGNORE", json!({})), 1.0),
    ];
    let result = block_on(evaluate_rulesets(
        &[ruleset("RS", Aggregation::Sum, rules)],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert!(result.actions.force_approve && result.actions.force_decline && result.actions.force_review);
    assert_eq!(result.actions.effective(), Some(Action::ForceDecline));
    assert_eq!(
        result.rules_score, 30.0,
        "only the on_trapped=score rule contributes (1.5 × 20)"
    );
    let trapped: Vec<_> = result
        .rule_results
        .iter()
        .filter(|t| t.outcome == "trapped")
        .collect();
    assert_eq!(trapped.len(), 3);
    assert!(trapped
        .iter()
        .all(|t| t.trapped_reason.as_deref() == Some("null_operand: event.nope")));
    // action-only rules (score 0) still appear as reasons
    assert!(result.reasons.iter().any(|r| r.code == "RL-BL"));
}

#[test]
fn weighted_average_and_caps() {
    let rules = vec![
        unit("r1", simple_rule("RL-A", 40.0, true, json!({})), 1.0),
        unit("r2", simple_rule("RL-B", 60.0, false, json!({})), 1.0),
    ];
    let result = block_on(evaluate_rulesets(
        &[ruleset("RS", Aggregation::WeightedAverage, rules)],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert!((result.rules_score - 40.0).abs() < 1e-9);

    let mut capped = ruleset(
        "RS",
        Aggregation::Sum,
        vec![
            unit("r1", simple_rule("RL-A", 80.0, true, json!({})), 1.0),
            unit("r2", simple_rule("RL-B", 80.0, true, json!({})), 1.0),
        ],
    );
    capped.max_score = 75.0;
    let result = block_on(evaluate_rulesets(
        &[capped],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert_eq!(result.rules_score, 75.0);
}

#[test]
fn same_rule_in_two_rulesets_is_evaluated_once() {
    let velocity = Arc::new(rule(json!({
        "code": "RL-VEL", "name": "velocity", "kind": "velocity", "risk_score": 50,
        "definition": { "kind": "velocity", "group_by": ["customer_id"], "window": {"duration": "1h"},
                        "aggregate": {"fn": "count"}, "compare": {"op": "gte", "right": {"type":"const","value": 3}} }
    })));
    let provider = MockProvider::aggregate(5.0, 5);
    let a = ruleset(
        "RS-A",
        Aggregation::Max,
        vec![unit("v", Arc::clone(&velocity), 1.0)],
    );
    let b = ruleset("RS-B", Aggregation::Max, vec![unit("v", velocity, 0.5)]);
    let result = block_on(evaluate_rulesets(
        &[a, b],
        &ctx(),
        &provider,
        &EvalOptions::default(),
    ));
    assert_eq!(provider.velocity_call_count(), 1);
    assert_eq!(result.rulesets[0].score, 50.0);
    assert_eq!(result.rulesets[1].score, 25.0);
    assert_eq!(
        result.reasons.len(),
        1,
        "one reason per rule code, keeping the best contribution"
    );
    assert_eq!(result.reasons[0].contribution, 50.0);
}

#[test]
fn event_type_filtering() {
    let login_only = simple_rule("RL-LOGIN", 50.0, true, json!({"event_types": ["login"]}));
    let any = simple_rule("RL-ANY", 10.0, true, json!({}));
    let mut rs_login = ruleset(
        "RS-LOGIN",
        Aggregation::Sum,
        vec![unit("x", simple_rule("RL-X", 99.0, true, json!({})), 1.0)],
    );
    rs_login.event_types = vec!["login".into()];
    let rs = ruleset(
        "RS",
        Aggregation::Sum,
        vec![unit("a", login_only, 1.0), unit("b", any, 1.0)],
    );
    let result = block_on(evaluate_rulesets(
        &[rs, rs_login],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert_eq!(result.rules_score, 10.0);
    assert_eq!(result.rulesets.len(), 1);
    assert_eq!(result.rule_results.len(), 1);
    assert_eq!(result.rule_results[0].rule_code, "RL-ANY");
}

#[test]
fn per_rule_timeout() {
    let velocity = Arc::new(rule(json!({
        "code": "RL-SLOW", "name": "slow", "kind": "velocity", "risk_score": 50,
        "definition": { "kind": "velocity", "group_by": ["customer_id"], "window": {"duration": "1h"},
                        "aggregate": {"fn": "count"}, "compare": {"op": "gte", "right": {"type":"const","value": 3}} }
    })));
    let provider = MockProvider {
        hang_velocity: true,
        ..Default::default()
    };
    let fast = simple_rule("RL-FAST", 20.0, true, json!({}));
    let rs = ruleset(
        "RS",
        Aggregation::Sum,
        vec![unit("slow", velocity, 1.0), unit("fast", fast, 1.0)],
    );
    let timer = InstantTimer;
    let options = EvalOptions {
        rule_timeout: Some(Duration::from_millis(50)),
        timer: Some(&timer),
    };
    let result = block_on(evaluate_rulesets(
        std::slice::from_ref(&rs),
        &ctx(),
        &provider,
        &options,
    ));
    let slow = result
        .rule_results
        .iter()
        .find(|t| t.rule_code == "RL-SLOW")
        .unwrap();
    assert_eq!(slow.trapped_reason.as_deref(), Some("timeout"));
    // the fast rule still completes (its future wins the race only if it is ready immediately — it is)
    let fast = result
        .rule_results
        .iter()
        .find(|t| t.rule_code == "RL-FAST")
        .unwrap();
    assert_eq!(fast.outcome, "match");

    // a timer that never fires does not interfere
    let never = NeverTimer;
    let provider = MockProvider::aggregate(5.0, 5);
    let options = EvalOptions {
        rule_timeout: Some(Duration::from_secs(1)),
        timer: Some(&never),
    };
    let result = block_on(evaluate_rulesets(&[rs], &ctx(), &provider, &options));
    assert!(result.rule_results.iter().all(|t| t.outcome == "match"));
}

fn challenger_rules() -> Vec<RuleUnit> {
    vec![
        unit(
            "c1",
            simple_rule("RL-C1", 40.0, true, json!({"action": "force_decline"})),
            1.0,
        ),
        unit(
            "c2",
            simple_rule("RL-C2", 50.0, true, json!({"description": "challenger"})),
            0.6,
        ),
        unit("c3", trapping_rule("RL-C3", json!({"on_trapped": "review"})), 1.0),
        unit(
            "c4",
            trapping_rule("RL-C4", json!({"on_trapped": "score", "trapped_score": 10})),
            2.0,
        ),
        unit("c5", simple_rule("RL-C5", 90.0, false, json!({})), 1.0),
    ]
}

#[test]
fn shadow_ruleset_scores_like_live_but_does_not_affect_decision() {
    for aggregation in [
        Aggregation::Sum,
        Aggregation::Max,
        Aggregation::ProbabilisticOr,
        Aggregation::WeightedAverage,
    ] {
        // Live evaluation of the challenger alone.
        let live = ruleset("RS-CHAL", aggregation, challenger_rules());
        let live_result = block_on(evaluate_rulesets(
            &[live],
            &ctx(),
            &MockProvider::default(),
            &EvalOptions::default(),
        ));

        // Champion (live) + challenger (shadow).
        let champion = ruleset(
            "RS-CHAMP",
            aggregation,
            vec![unit("k1", simple_rule("RL-K1", 25.0, true, json!({})), 1.0)],
        );
        let champion_only = block_on(evaluate_rulesets(
            std::slice::from_ref(&champion),
            &ctx(),
            &MockProvider::default(),
            &EvalOptions::default(),
        ));
        let mut challenger = ruleset("RS-CHAL", aggregation, challenger_rules());
        challenger.shadow = true;
        let both = block_on(evaluate_rulesets(
            &[champion, challenger],
            &ctx(),
            &MockProvider::default(),
            &EvalOptions::default(),
        ));

        let shadow_score = both.rulesets.iter().find(|r| r.code == "RS-CHAL").unwrap();
        assert!(shadow_score.shadow);
        assert!(
            (shadow_score.score - live_result.rulesets[0].score).abs() < 1e-9,
            "{aggregation:?}: shadow {} vs live {}",
            shadow_score.score,
            live_result.rulesets[0].score
        );
        assert!(live_result.rulesets[0].score > 0.0);

        // Engine-level results are exactly the champion's.
        assert_eq!(both.rules_score, champion_only.rules_score, "{aggregation:?}");
        assert_eq!(both.actions, champion_only.actions, "{aggregation:?}");
        assert_eq!(both.reasons, champion_only.reasons, "{aggregation:?}");
        assert!(live_result.actions.force_decline && live_result.actions.force_review);

        // Shadow trace items carry would-be contributions equal to the live ones.
        for live_item in &live_result.rule_results {
            let shadow_item = both
                .rule_results
                .iter()
                .find(|t| t.ruleset_code == "RS-CHAL" && t.rule_code == live_item.rule_code)
                .unwrap();
            assert!(shadow_item.shadow && !live_item.shadow);
            assert_eq!(shadow_item.contribution, live_item.contribution);
            assert_eq!(shadow_item.outcome, live_item.outcome);
        }
    }
}

#[test]
fn shadow_rule_inside_shadow_ruleset_is_excluded_from_its_score() {
    let mut members = challenger_rules();
    let mut extra = unit("c6", simple_rule("RL-C6", 100.0, true, json!({})), 1.0);
    extra.shadow = true;
    members.push(extra);
    let mut challenger = ruleset("RS-CHAL", Aggregation::Sum, members);
    challenger.shadow = true;
    let live = ruleset("RS-CHAL", Aggregation::Sum, challenger_rules());
    let with_extra = block_on(evaluate_rulesets(
        &[challenger],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    let reference = block_on(evaluate_rulesets(
        &[live],
        &ctx(),
        &MockProvider::default(),
        &EvalOptions::default(),
    ));
    assert_eq!(with_extra.rulesets[0].score, reference.rulesets[0].score);
    let item = with_extra
        .rule_results
        .iter()
        .find(|t| t.rule_code == "RL-C6")
        .unwrap();
    assert_eq!(item.contribution, 100.0, "would-be contribution is still traced");
    assert!(item.shadow);
    assert_eq!(with_extra.rules_score, 0.0);
}
