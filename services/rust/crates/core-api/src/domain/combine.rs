//! Decision combiner (architecture.md §3.1).
//!
//! Pure function from per-engine results to the final decision and explanation. Keeping it pure
//! (no IO, no clock) means the most business-critical arithmetic of the platform is covered by
//! plain unit tests and cannot be affected by timeouts or DB state.
//!
//! * Only **available** engines take part. A degraded (failed) engine or an engine without an
//!   active model is dropped and its weight renormalised away.
//! * `engine_combination = noisy_or` (default): `final = 100·(1 − Π(1 − sᵢ/100)^(wᵢ/w_max))`.
//!   Independent evidence accumulates, and an engine with no evidence (score 0) contributes a factor
//!   of 1, so it never dilutes a strong signal from another engine. A single engine at its full weight
//!   passes through unchanged (one rule hit of 60 → final 60).
//! * `engine_combination = weighted_average`: `Σ wᵢ·sᵢ / Σ wᵢ`, a calibrated blend (quiet engines pull
//!   the score down). Kept for projects that prefer it.
//! * Reason contributions are an **attribution of `final_score`**: engine shares sum to the final
//!   score (proportional to `wᵢ·sᵢ` for the average, to `−eᵢ·ln(1 − pᵢ)` for noisy-OR), and a rule's
//!   share of the rules engine is proportional to its rule-level contribution.
//! * Rule actions override thresholds: `force_decline` > `force_approve` > `force_review` > thresholds.
//! * If rule-service itself is unavailable, the decision is at least
//!   `settings.rules_unavailable_decision` (default `review`). We take the **more severe** of that
//!   and the score-based decision, so an ML/graph "decline" is never weakened by the fallback.

use contracts::events::{EngineScores, MlSummary};
use contracts::graph::GraphMetrics;
use contracts::scoring::{Actions, Reason};
use contracts::Decision;

use super::settings::{EngineCombination, ProjectSettings};

/// Everything the combiner needs.
#[derive(Debug, Clone, Default)]
pub struct CombineInput<'a> {
    /// Rules engine score; `None` when rule-service was unavailable.
    pub rules_score: Option<f64>,
    pub actions: Actions,
    pub rule_reasons: &'a [Reason],
    pub ml: Option<&'a MlSummary>,
    pub graph: Option<&'a GraphMetrics>,
    /// Engines that failed (`rules`, `supervised`, `unsupervised`, `graph`).
    pub degraded: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub struct CombineOutput {
    pub final_score: f64,
    pub decision: Decision,
    pub engine_scores: EngineScores,
    pub reasons: Vec<Reason>,
}

fn severity(d: Decision) -> u8 {
    match d {
        Decision::Approve => 0,
        Decision::Review => 1,
        Decision::Decline => 2,
    }
}

/// Graph engine score from metrics and settings.
pub fn graph_score(g: &GraphMetrics, settings: &ProjectSettings) -> f64 {
    let by_distance = g
        .distance_to_fraud
        .and_then(|d| {
            settings
                .graph_scores
                .fraud_distance_scores
                .get(&d.to_string())
                .copied()
        })
        .unwrap_or(0.0);
    let shared = if g.shared_with_fraud_kinds.is_empty() {
        0.0
    } else {
        settings.graph_scores.shared_fraud_entity_score
    };
    by_distance.max(shared).clamp(0.0, 100.0)
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn combine(input: &CombineInput<'_>, settings: &ProjectSettings) -> CombineOutput {
    let w = settings.engine_weights;
    let supervised = input
        .ml
        .and_then(|m| m.fraud_probability)
        .map(|p| (p * 100.0).clamp(0.0, 100.0));
    let unsupervised = input
        .ml
        .and_then(|m| m.anomaly_score)
        .map(|a| (a * 100.0).clamp(0.0, 100.0));
    let graph = input.graph.map(|g| graph_score(g, settings));
    let rules = input.rules_score.map(|s| s.clamp(0.0, 100.0));

    let engines = [
        ("rules", rules, w.rules),
        ("supervised", supervised, w.supervised),
        ("unsupervised", unsupervised, w.unsupervised),
        ("graph", graph, w.graph),
    ];
    let (final_score, shares) = combine_scores(&engines, settings.engine_combination);
    let final_score = round2(final_score);
    let share_of = |name: &str| {
        shares
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| *c)
            .unwrap_or(0.0)
    };

    // --- decision
    let t = settings.decision_thresholds;
    let by_threshold = if final_score >= t.decline {
        Decision::Decline
    } else if final_score >= t.review {
        Decision::Review
    } else {
        Decision::Approve
    };
    let mut decision = if input.actions.force_decline {
        Decision::Decline
    } else if input.actions.force_approve {
        Decision::Approve
    } else if input.actions.force_review {
        if severity(by_threshold) > severity(Decision::Review) {
            by_threshold
        } else {
            Decision::Review
        }
    } else {
        by_threshold
    };
    if rules.is_none() && input.degraded.iter().any(|d| d == "rules") {
        let fallback = settings.rules_unavailable_decision;
        if severity(fallback) > severity(decision) {
            decision = fallback;
        }
    }

    // --- reasons: attribution of the final score
    let mut reasons: Vec<Reason> = Vec::new();
    if let Some(rs) = rules {
        let rules_share = share_of("rules");
        let rule_total: f64 = input.rule_reasons.iter().map(|r| r.contribution.max(0.0)).sum();
        for r in input.rule_reasons {
            let portion = if rule_total > 0.0 && rs > 0.0 {
                // A rule's share of the rules engine, capped by the engine's own score.
                (r.contribution.max(0.0) / rule_total.max(rs)) * rules_share
            } else {
                0.0
            };
            reasons.push(Reason {
                contribution: round2(portion),
                ..r.clone()
            });
        }
    }
    if let (Some(s), Some(m)) = (supervised, input.ml) {
        if s >= 50.0 {
            reasons.push(Reason {
                code: "ML_SUPERVISED_HIGH".into(),
                engine: "supervised".into(),
                contribution: round2(share_of("supervised")),
                message: format!(
                    "Supervised model P(fraud) = {:.2}",
                    m.fraud_probability.unwrap_or_default()
                ),
            });
        }
    }
    if let (Some(s), Some(m)) = (unsupervised, input.ml) {
        if s >= 60.0 {
            let code = match m.cluster_id {
                Some(c) if c >= 0 => format!("ML_ANOMALY_CLUSTER_{c}"),
                _ => "ML_ANOMALY_HIGH".into(),
            };
            reasons.push(Reason {
                code,
                engine: "unsupervised".into(),
                contribution: round2(share_of("unsupervised")),
                message: format!("Anomaly score {:.2}", m.anomaly_score.unwrap_or_default()),
            });
        }
    }
    if let (Some(s), Some(g)) = (graph, input.graph) {
        if s > 0.0 {
            let graph_share = share_of("graph");
            let portion = |score: f64| graph_share * (score / s).min(1.0);
            if let Some(d) = g.distance_to_fraud {
                reasons.push(Reason {
                    code: format!("GRAPH_FRAUD_DISTANCE_{d}"),
                    engine: "graph".into(),
                    contribution: round2(portion(
                        settings
                            .graph_scores
                            .fraud_distance_scores
                            .get(&d.to_string())
                            .copied()
                            .unwrap_or(0.0),
                    )),
                    message: format!("{d} hop(s) from a confirmed fraud customer"),
                });
            }
            for k in &g.shared_with_fraud_kinds {
                reasons.push(Reason {
                    code: format!("GRAPH_SHARED_{}", k.as_str().to_uppercase()),
                    engine: "graph".into(),
                    contribution: round2(portion(settings.graph_scores.shared_fraud_entity_score)),
                    message: format!("Shares a {} with a fraud customer", k.as_str().replace('_', " ")),
                });
            }
        }
    }
    reasons.sort_by(|a, b| b.contribution.total_cmp(&a.contribution));
    reasons.truncate(8);

    CombineOutput {
        final_score,
        decision,
        engine_scores: EngineScores {
            rules: rules.map(round2),
            supervised: supervised.map(round2),
            unsupervised: unsupervised.map(round2),
            graph: graph.map(round2),
        },
        reasons,
    }
}

/// Combines available engine scores. Returns the final score (unrounded) and each available
/// engine's attributed share of it (shares sum to the final score).
fn combine_scores<'a>(
    engines: &[(&'a str, Option<f64>, f64)],
    method: EngineCombination,
) -> (f64, Vec<(&'a str, f64)>) {
    let available: Vec<(&str, f64, f64)> = engines
        .iter()
        .filter_map(|(n, s, w)| s.filter(|_| *w > 0.0).map(|s| (*n, s.clamp(0.0, 100.0), *w)))
        .collect();
    if available.is_empty() {
        return (0.0, Vec::new());
    }
    match method {
        EngineCombination::WeightedAverage => {
            let den: f64 = available.iter().map(|(_, _, w)| w).sum();
            let parts: Vec<(&str, f64)> = available.iter().map(|(n, s, w)| (*n, s * w / den)).collect();
            (parts.iter().map(|(_, c)| c).sum(), parts)
        }
        EngineCombination::NoisyOr => {
            let w_max = available.iter().map(|(_, _, w)| *w).fold(0.0_f64, f64::max);
            // Evidence of each engine in log space: −eᵢ·ln(1 − pᵢ), with p capped below 1 so a
            // 100-score engine stays finite (it still drives the final score to ~100).
            let evidence: Vec<(&str, f64)> = available
                .iter()
                .map(|(n, s, w)| {
                    let p = (s / 100.0).min(0.999_999);
                    (*n, -(w / w_max) * (1.0 - p).ln())
                })
                .collect();
            let total: f64 = evidence.iter().map(|(_, e)| e).sum();
            let final_score = 100.0 * (1.0 - (-total).exp());
            let parts = evidence
                .iter()
                .map(|(n, e)| (*n, if total > 0.0 { final_score * e / total } else { 0.0 }))
                .collect();
            (final_score, parts)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::graph::LinkKind;

    fn ml(p: f64, a: f64) -> MlSummary {
        MlSummary {
            fraud_probability: Some(p),
            anomaly_score: Some(a),
            cluster_id: Some(4),
            ..Default::default()
        }
    }

    #[test]
    fn weighted_average_all_engines() {
        let s = ProjectSettings {
            engine_combination: EngineCombination::WeightedAverage,
            ..ProjectSettings::default()
        };
        let m = ml(0.5, 0.2);
        let g = GraphMetrics {
            distance_to_fraud: Some(2),
            ..Default::default()
        };
        let out = combine(
            &CombineInput {
                rules_score: Some(80.0),
                ml: Some(&m),
                graph: Some(&g),
                ..Default::default()
            },
            &s,
        );
        // 0.45*80 + 0.30*50 + 0.10*20 + 0.15*70 = 36 + 15 + 2 + 10.5 = 63.5
        assert_eq!(out.final_score, 63.5);
        assert_eq!(out.decision, Decision::Review);
        assert_eq!(out.engine_scores.graph, Some(70.0));
    }

    fn all_engines(s: &ProjectSettings) -> CombineOutput {
        let m = ml(0.5, 0.2);
        let g = GraphMetrics {
            distance_to_fraud: Some(2),
            ..Default::default()
        };
        combine(
            &CombineInput {
                rules_score: Some(80.0),
                ml: Some(&m),
                graph: Some(&g),
                ..Default::default()
            },
            s,
        )
    }

    #[test]
    fn noisy_or_is_default_and_accumulates_evidence() {
        let s = ProjectSettings::default();
        assert_eq!(s.engine_combination, EngineCombination::NoisyOr);
        // e = w/w_max = (1, 2/3, 2/9, 1/3); evidence = −e·ln(1−p)
        // = 1.6094 + 0.4621 + 0.0496 + 0.4013 = 2.5224 → 100·(1 − e^−2.5224) ≈ 91.97
        let out = all_engines(&s);
        assert!((out.final_score - 91.97).abs() < 0.02, "{}", out.final_score);
        assert_eq!(out.decision, Decision::Decline);
    }

    #[test]
    fn noisy_or_quiet_engines_do_not_dilute_a_strong_signal() {
        let s = ProjectSettings::default();
        let m = ml(0.0, 0.0);
        let g = GraphMetrics::default();
        let out = combine(
            &CombineInput {
                rules_score: Some(60.0),
                ml: Some(&m),
                graph: Some(&g),
                ..Default::default()
            },
            &s,
        );
        // weighted average would give 0.45·60 = 27 → approve; noisy-OR keeps the rule's 60 → review
        assert_eq!(out.final_score, 60.0);
        assert_eq!(out.decision, Decision::Review);
    }

    #[test]
    fn noisy_or_never_exceeds_100_and_handles_certainty() {
        let s = ProjectSettings::default();
        let m = ml(1.0, 1.0);
        let out = combine(
            &CombineInput {
                rules_score: Some(100.0),
                ml: Some(&m),
                ..Default::default()
            },
            &s,
        );
        assert!(
            out.final_score <= 100.0 && out.final_score >= 99.99,
            "{}",
            out.final_score
        );
    }

    #[test]
    fn engine_shares_sum_to_final_score() {
        for method in [EngineCombination::NoisyOr, EngineCombination::WeightedAverage] {
            let engines = [
                ("rules", Some(80.0), 0.45),
                ("supervised", Some(50.0), 0.30),
                ("unsupervised", None, 0.10),
                ("graph", Some(70.0), 0.15),
            ];
            let (final_score, shares) = combine_scores(&engines, method);
            let sum: f64 = shares.iter().map(|(_, c)| c).sum();
            assert!((sum - final_score).abs() < 1e-9, "{method:?}");
            assert!(shares.iter().all(|(n, _)| *n != "unsupervised"));
        }
    }

    #[test]
    fn rule_reasons_are_attributed_within_the_rules_share() {
        let s = ProjectSettings::default();
        let reasons = vec![
            Reason {
                code: "RL-A".into(),
                engine: "rules".into(),
                contribution: 30.0,
                message: "a".into(),
            },
            Reason {
                code: "RL-B".into(),
                engine: "rules".into(),
                contribution: 30.0,
                message: "b".into(),
            },
        ];
        let out = combine(
            &CombineInput {
                rules_score: Some(51.0), // probabilistic_or of two 30s
                rule_reasons: &reasons,
                ..Default::default()
            },
            &s,
        );
        let total: f64 = out.reasons.iter().map(|r| r.contribution).sum();
        assert!(
            (total - out.final_score).abs() < 0.05,
            "{total} vs {}",
            out.final_score
        );
        assert_eq!(out.reasons[0].contribution, out.reasons[1].contribution);
    }

    #[test]
    fn degraded_engines_are_renormalised() {
        let s = ProjectSettings::default();
        let degraded = vec![
            "supervised".to_string(),
            "unsupervised".to_string(),
            "graph".to_string(),
        ];
        let out = combine(
            &CombineInput {
                rules_score: Some(90.0),
                degraded: &degraded,
                ..Default::default()
            },
            &s,
        );
        assert_eq!(out.final_score, 90.0);
        assert_eq!(out.decision, Decision::Decline);
        assert_eq!(out.engine_scores.supervised, None);
    }

    #[test]
    fn action_precedence() {
        let s = ProjectSettings::default();
        let run = |a: Actions, score: f64| {
            combine(
                &CombineInput {
                    rules_score: Some(score),
                    actions: a,
                    ..Default::default()
                },
                &s,
            )
            .decision
        };
        let all = Actions {
            force_decline: true,
            force_approve: true,
            force_review: true,
        };
        assert_eq!(run(all, 0.0), Decision::Decline);
        let approve_review = Actions {
            force_approve: true,
            force_review: true,
            ..Default::default()
        };
        assert_eq!(run(approve_review, 95.0), Decision::Approve);
        let review = Actions {
            force_review: true,
            ..Default::default()
        };
        assert_eq!(run(review, 0.0), Decision::Review);
        // force_review never weakens a threshold decline
        assert_eq!(run(review, 95.0), Decision::Decline);
        assert_eq!(run(Actions::default(), 10.0), Decision::Approve);
    }

    #[test]
    fn rules_down_fallback_is_at_least_review() {
        let s = ProjectSettings::default();
        let degraded = vec!["rules".to_string()];
        let m = ml(0.1, 0.1);
        let out = combine(
            &CombineInput {
                rules_score: None,
                ml: Some(&m),
                degraded: &degraded,
                ..Default::default()
            },
            &s,
        );
        assert_eq!(out.decision, Decision::Review);
        let m = ml(0.99, 0.99);
        let out = combine(
            &CombineInput {
                rules_score: None,
                ml: Some(&m),
                degraded: &degraded,
                ..Default::default()
            },
            &s,
        );
        assert_eq!(out.decision, Decision::Decline);
    }

    #[test]
    fn reasons_sorted_and_capped() {
        let s = ProjectSettings::default();
        let rule_reasons: Vec<Reason> = (0..7)
            .map(|i| Reason {
                code: format!("RL-{i}"),
                engine: "rules".into(),
                contribution: f64::from(i) * 5.0,
                message: "x".into(),
            })
            .collect();
        let m = ml(0.9, 0.9);
        let g = GraphMetrics {
            distance_to_fraud: Some(1),
            shared_with_fraud_kinds: vec![LinkKind::Card],
            ..Default::default()
        };
        let out = combine(
            &CombineInput {
                rules_score: Some(100.0),
                rule_reasons: &rule_reasons,
                ml: Some(&m),
                graph: Some(&g),
                ..Default::default()
            },
            &s,
        );
        assert_eq!(out.reasons.len(), 8);
        assert!(out
            .reasons
            .windows(2)
            .all(|w| w[0].contribution >= w[1].contribution));
        assert!(out.reasons.iter().any(|r| r.code == "ML_SUPERVISED_HIGH"));
        assert!(out.reasons.iter().any(|r| r.code == "GRAPH_FRAUD_DISTANCE_1"));
    }

    #[test]
    fn no_engines_scores_zero() {
        let out = combine(&CombineInput::default(), &ProjectSettings::default());
        assert_eq!(out.final_score, 0.0);
        assert_eq!(out.decision, Decision::Approve);
    }
}
