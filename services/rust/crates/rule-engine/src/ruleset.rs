//! Ruleset evaluation and scoring (rule-dsl §7, architecture §3.1).
//!
//! Given the rulesets that apply to a project, [`evaluate_rulesets`]:
//! 1. evaluates every distinct `(rule, version)` once, concurrently, each with an optional time budget;
//! 2. computes each rule's contribution per ruleset (`weight × risk_score × fraction`, trapped scoring);
//! 3. aggregates contributions per ruleset (`sum` / `max` / `probabilistic_or` / `weighted_average`, capped);
//! 4. derives actions (`force_decline` > `force_approve` > `force_review`), reasons and trace items.
//!
//! Shadow semantics (champion/challenger):
//! * a **shadow rule** (rule status `shadow`) is evaluated and traced but excluded everywhere: its ruleset's score,
//!   the engine `rules_score`, actions and reasons;
//! * a **shadow ruleset** computes its *own* score exactly as if it were live (from its non-shadow rules) and
//!   reports it in `rulesets[]` with `shadow: true`, but is excluded from `rules_score`, actions and reasons.
//!
//! Every [`TraceItem`] carries the rule's would-be `contribution` (weight × risk × fraction, or the trapped score)
//! even when `shadow` is true; `shadow: true` means the value did not count toward the decision.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::{self, Either};
use serde::Serialize;
use serde_json::Value as Json;

use crate::context::EvalContext;
use crate::eval::{evaluate_rule, Outcome, RuleEvaluation};
use crate::model::{Action, Aggregation, OnTrapped, RuleEnvelope};
use crate::ports::{DataProvider, Timer};

/// A rule (at a specific version) inside a ruleset.
#[derive(Debug, Clone)]
pub struct RuleUnit {
    pub rule_id: String,
    pub version: i32,
    pub rule: Arc<RuleEnvelope>,
    pub weight: f64,
    /// Rule status is `shadow`.
    pub shadow: bool,
}

/// A ruleset ready for evaluation.
#[derive(Debug, Clone)]
pub struct RulesetUnit {
    pub ruleset_id: String,
    pub code: String,
    pub aggregation: Aggregation,
    pub max_score: f64,
    /// Empty = all event types.
    pub event_types: Vec<String>,
    /// Ruleset status is `shadow`.
    pub shadow: bool,
    pub rules: Vec<RuleUnit>,
}

/// Evaluation options.
#[derive(Clone, Copy, Default)]
pub struct EvalOptions<'a> {
    /// Per-rule time budget; requires `timer`.
    pub rule_timeout: Option<Duration>,
    pub timer: Option<&'a dyn Timer>,
}

impl std::fmt::Debug for EvalOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalOptions")
            .field("rule_timeout", &self.rule_timeout)
            .finish_non_exhaustive()
    }
}

/// One evaluated rule inside one ruleset (stored in `decisions.rule_results`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceItem {
    pub rule_id: String,
    pub rule_code: String,
    pub version: i32,
    pub ruleset_id: String,
    pub ruleset_code: String,
    pub kind: String,
    pub outcome: String,
    /// Would-be contribution (`weight × risk_score × fraction`, or `weight × trapped_score`), always filled —
    /// also for shadow items, so challengers can be compared with champions.
    pub contribution: f64,
    /// True when the rule or its ruleset is shadow: `contribution` did not count toward the decision.
    pub shadow: bool,
    pub action: Action,
    pub trapped_reason: Option<String>,
    pub trace: Json,
    pub duration_us: u64,
}

/// Score of one ruleset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulesetScore {
    pub ruleset_id: String,
    pub code: String,
    pub score: f64,
    pub shadow: bool,
}

/// Overrides derived from matched (or trapped with `on_trapped = review`) live rules of live rulesets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Actions {
    pub force_decline: bool,
    pub force_approve: bool,
    pub force_review: bool,
}

impl Actions {
    /// The winning override by precedence `force_decline > force_approve > force_review`.
    pub fn effective(&self) -> Option<Action> {
        if self.force_decline {
            Some(Action::ForceDecline)
        } else if self.force_approve {
            Some(Action::ForceApprove)
        } else if self.force_review {
            Some(Action::ForceReview)
        } else {
            None
        }
    }
}

/// A human-readable reason for the decision.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reason {
    pub code: String,
    pub engine: &'static str,
    pub contribution: f64,
    pub message: String,
}

/// Output of [`evaluate_rulesets`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineResult {
    /// Max over non-shadow ruleset scores (0 when none apply).
    pub rules_score: f64,
    pub rulesets: Vec<RulesetScore>,
    pub rule_results: Vec<TraceItem>,
    pub actions: Actions,
    /// Top 8 by contribution.
    pub reasons: Vec<Reason>,
    pub duration_us: u64,
}

/// Contribution of one rule in a ruleset (rule-dsl §7).
pub fn contribution(rule: &RuleEnvelope, weight: f64, evaluation: &RuleEvaluation) -> f64 {
    match &evaluation.outcome {
        Outcome::Match => weight * rule.risk_score * evaluation.fraction,
        Outcome::Trapped(_) if rule.on_trapped == OnTrapped::Score => weight * rule.trapped_score,
        _ => 0.0,
    }
}

/// Aggregates contributions. `max_possible` is `Σ weight × risk_score` (for `weighted_average`).
pub fn aggregate(aggregation: Aggregation, contributions: &[f64], max_possible: f64, max_score: f64) -> f64 {
    let raw = match aggregation {
        Aggregation::Sum => contributions.iter().sum(),
        Aggregation::Max => contributions.iter().copied().fold(0.0, f64::max),
        Aggregation::ProbabilisticOr => {
            let keep: f64 = contributions
                .iter()
                .map(|c| 1.0 - (c / 100.0).clamp(0.0, 1.0))
                .product();
            100.0 * (1.0 - keep)
        }
        Aggregation::WeightedAverage => {
            if max_possible > 0.0 {
                100.0 * contributions.iter().sum::<f64>() / max_possible
            } else {
                0.0
            }
        }
    };
    raw.clamp(0.0, max_score.clamp(0.0, 100.0))
}

async fn evaluate_with_budget(
    rule: &RuleEnvelope,
    ctx: &EvalContext,
    provider: &dyn DataProvider,
    options: &EvalOptions<'_>,
) -> (RuleEvaluation, u64) {
    let started = Instant::now();
    let evaluation = match (options.rule_timeout, options.timer) {
        (Some(budget), Some(timer)) => {
            let work = Box::pin(evaluate_rule(rule, ctx, provider));
            let sleep = Box::pin(timer.sleep(budget));
            match future::select(work, sleep).await {
                Either::Left((evaluation, _)) => evaluation,
                Either::Right(_) => RuleEvaluation {
                    outcome: Outcome::Trapped("timeout".into()),
                    fraction: 0.0,
                    trace: serde_json::json!({ "trapped_reason": "timeout" }),
                },
            }
        }
        _ => evaluate_rule(rule, ctx, provider).await,
    };
    let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    (evaluation, micros)
}

/// Evaluates rulesets against one event. Rulesets/rules whose `event_types` exclude the event are skipped.
pub async fn evaluate_rulesets(
    rulesets: &[RulesetUnit],
    ctx: &EvalContext,
    provider: &dyn DataProvider,
    options: &EvalOptions<'_>,
) -> EngineResult {
    let started = Instant::now();
    let applies = |types: &[String]| types.is_empty() || types.iter().any(|t| t == &ctx.event_type);

    // 1. Evaluate each distinct (rule_id, version) once, concurrently.
    let mut unique: Vec<&RuleUnit> = Vec::new();
    let mut index: HashMap<(String, i32), usize> = HashMap::new();
    for ruleset in rulesets.iter().filter(|rs| applies(&rs.event_types)) {
        for unit in ruleset
            .rules
            .iter()
            .filter(|u| u.rule.applies_to(&ctx.event_type))
        {
            if let Entry::Vacant(slot) = index.entry((unit.rule_id.clone(), unit.version)) {
                slot.insert(unique.len());
                unique.push(unit);
            }
        }
    }
    let evaluations: Vec<(RuleEvaluation, u64)> = future::join_all(
        unique
            .iter()
            .map(|unit| evaluate_with_budget(&unit.rule, ctx, provider, options)),
    )
    .await;

    // 2–4. Contributions, aggregation, actions, traces.
    let mut scores = Vec::new();
    let mut traces = Vec::new();
    let mut actions = Actions::default();
    let mut best_reason: HashMap<String, Reason> = HashMap::new();

    for ruleset in rulesets.iter().filter(|rs| applies(&rs.event_types)) {
        let mut contributions = Vec::new();
        let mut max_possible = 0.0;
        for unit in ruleset
            .rules
            .iter()
            .filter(|u| u.rule.applies_to(&ctx.event_type))
        {
            let Some(&i) = index.get(&(unit.rule_id.clone(), unit.version)) else {
                continue;
            };
            let Some((evaluation, duration_us)) = evaluations.get(i) else {
                continue;
            };
            let shadow = ruleset.shadow || unit.shadow;
            let rule = &unit.rule;
            let value = contribution(rule, unit.weight, evaluation);
            // Non-shadow rules count toward their ruleset's own score, even inside a shadow ruleset
            // (so a challenger ruleset's score equals what it would score live).
            if !unit.shadow {
                contributions.push(value);
                max_possible += unit.weight * rule.risk_score;
            }
            // Only live rules of live rulesets affect the decision.
            if !shadow {
                match &evaluation.outcome {
                    Outcome::Match => match rule.action {
                        Action::ForceDecline => actions.force_decline = true,
                        Action::ForceApprove => actions.force_approve = true,
                        Action::ForceReview => actions.force_review = true,
                        Action::Score => {}
                    },
                    Outcome::Trapped(_) if rule.on_trapped == OnTrapped::Review => {
                        actions.force_review = true
                    }
                    _ => {}
                }
                let is_action_hit = evaluation.outcome == Outcome::Match && rule.action != Action::Score;
                if value > 0.0 || is_action_hit {
                    let candidate = Reason {
                        code: rule.code.clone(),
                        engine: "rules",
                        contribution: value,
                        message: rule.description.clone().unwrap_or_else(|| rule.name.clone()),
                    };
                    let replace = best_reason
                        .get(&rule.code)
                        .is_none_or(|existing| existing.contribution < candidate.contribution);
                    if replace {
                        best_reason.insert(rule.code.clone(), candidate);
                    }
                }
            }
            traces.push(TraceItem {
                rule_id: unit.rule_id.clone(),
                rule_code: rule.code.clone(),
                version: unit.version,
                ruleset_id: ruleset.ruleset_id.clone(),
                ruleset_code: ruleset.code.clone(),
                kind: rule.kind.as_str().to_string(),
                outcome: evaluation.outcome.as_str().to_string(),
                contribution: value,
                shadow,
                action: rule.action,
                trapped_reason: evaluation.outcome.trapped_reason().map(str::to_string),
                trace: evaluation.trace.clone(),
                duration_us: *duration_us,
            });
        }
        let score = aggregate(
            ruleset.aggregation,
            &contributions,
            max_possible,
            ruleset.max_score,
        );
        scores.push(RulesetScore {
            ruleset_id: ruleset.ruleset_id.clone(),
            code: ruleset.code.clone(),
            score,
            shadow: ruleset.shadow,
        });
    }

    let rules_score = scores
        .iter()
        .filter(|s| !s.shadow)
        .map(|s| s.score)
        .fold(0.0, f64::max);
    let mut reasons: Vec<Reason> = best_reason.into_values().collect();
    reasons.sort_by(|a, b| {
        b.contribution
            .total_cmp(&a.contribution)
            .then_with(|| a.code.cmp(&b.code))
    });
    reasons.truncate(8);

    EngineResult {
        rules_score,
        rulesets: scores,
        rule_results: traces,
        actions,
        reasons,
        duration_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn aggregations() {
        let c = [40.0, 30.0, 0.0];
        assert!(close(aggregate(Aggregation::Sum, &c, 150.0, 100.0), 70.0));
        assert!(close(
            aggregate(Aggregation::Sum, &[80.0, 50.0], 150.0, 100.0),
            100.0
        ));
        assert!(close(
            aggregate(Aggregation::Sum, &[80.0, 50.0], 150.0, 90.0),
            90.0
        ));
        assert!(close(aggregate(Aggregation::Max, &c, 150.0, 100.0), 40.0));
        // 1 - 0.6*0.7 = 0.58
        assert!(close(
            aggregate(Aggregation::ProbabilisticOr, &c, 150.0, 100.0),
            58.0
        ));
        assert!(close(
            aggregate(Aggregation::WeightedAverage, &c, 140.0, 100.0),
            50.0
        ));
        assert!(close(
            aggregate(Aggregation::WeightedAverage, &[], 0.0, 100.0),
            0.0
        ));
        assert!(close(
            aggregate(Aggregation::ProbabilisticOr, &[150.0], 100.0, 100.0),
            100.0
        ));
    }

    #[test]
    fn action_precedence() {
        let a = Actions {
            force_decline: true,
            force_approve: true,
            force_review: true,
        };
        assert_eq!(a.effective(), Some(Action::ForceDecline));
        let a = Actions {
            force_decline: false,
            force_approve: true,
            force_review: true,
        };
        assert_eq!(a.effective(), Some(Action::ForceApprove));
        let a = Actions {
            force_review: true,
            ..Default::default()
        };
        assert_eq!(a.effective(), Some(Action::ForceReview));
        assert_eq!(Actions::default().effective(), None);
    }
}
