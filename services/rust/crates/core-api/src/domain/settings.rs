//! Project settings (`core.project_settings`): typed model, defaults and validation.
//!
//! Settings are stored as one JSON value per key so that new keys can be added without a
//! migration. Reading merges the stored rows over [`ProjectSettings::default`]; an invalid stored
//! value (should never happen, writes are validated) falls back to the default instead of
//! breaking scoring.

use std::collections::BTreeMap;

use contracts::Decision;
use platform::error::FieldError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const KEYS: &[&str] = &[
    "decision_thresholds",
    "engine_weights",
    "graph_scores",
    "timeouts",
    "cases",
    "rules_unavailable_decision",
    "engine_combination",
];

/// Internal bookkeeping key (template bootstrap status); not user-editable.
pub const TEMPLATE_BOOTSTRAP_KEY: &str = "template_bootstrap";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionThresholds {
    pub review: f64,
    pub decline: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineWeights {
    pub rules: f64,
    pub supervised: f64,
    pub unsupervised: f64,
    pub graph: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphScores {
    /// Hop distance ("1", "2", ...) → score.
    pub fraud_distance_scores: BTreeMap<String, f64>,
    /// Score when the customer shares an entity directly with a fraud customer.
    #[serde(default = "default_shared_score")]
    pub shared_fraud_entity_score: f64,
}

fn default_shared_score() -> f64 {
    80.0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeouts {
    pub graph_ms: u64,
    pub ml_ms: u64,
    pub rules_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseSettings {
    pub auto_create_on: Vec<Decision>,
    pub dedupe_open_per_customer: bool,
}

/// How per-engine scores are combined into `final_score` (architecture.md §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EngineCombination {
    /// `100·(1 − Π(1 − sᵢ/100)^(wᵢ/w_max))`: independent evidence accumulates and an engine with
    /// no evidence (score 0) never dilutes a strong signal from another engine. Default.
    #[default]
    NoisyOr,
    /// `Σ wᵢ·sᵢ / Σ wᵢ`: a calibrated blend; a single strong engine is pulled down by quiet ones.
    WeightedAverage,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectSettings {
    pub decision_thresholds: DecisionThresholds,
    pub engine_weights: EngineWeights,
    pub graph_scores: GraphScores,
    pub timeouts: Timeouts,
    pub cases: CaseSettings,
    pub rules_unavailable_decision: Decision,
    pub engine_combination: EngineCombination,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            decision_thresholds: DecisionThresholds {
                review: 50.0,
                decline: 80.0,
            },
            engine_weights: EngineWeights {
                rules: 0.45,
                supervised: 0.30,
                unsupervised: 0.10,
                graph: 0.15,
            },
            graph_scores: GraphScores {
                fraud_distance_scores: [("1", 90.0), ("2", 70.0), ("3", 40.0)]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                shared_fraud_entity_score: 80.0,
            },
            timeouts: Timeouts {
                graph_ms: 100,
                ml_ms: 200,
                rules_ms: 300,
            },
            cases: CaseSettings {
                auto_create_on: vec![Decision::Review, Decision::Decline],
                dedupe_open_per_customer: true,
            },
            rules_unavailable_decision: Decision::Review,
            engine_combination: EngineCombination::NoisyOr,
        }
    }
}

impl ProjectSettings {
    /// Default value of every key as JSON (inserted on project creation).
    pub fn default_rows() -> Vec<(&'static str, Value)> {
        let d = Self::default();
        KEYS.iter().filter_map(|k| d.get(k).map(|v| (*k, v))).collect()
    }

    /// JSON value of one key.
    pub fn get(&self, key: &str) -> Option<Value> {
        let v = match key {
            "decision_thresholds" => serde_json::to_value(self.decision_thresholds),
            "engine_weights" => serde_json::to_value(self.engine_weights),
            "graph_scores" => serde_json::to_value(&self.graph_scores),
            "timeouts" => serde_json::to_value(self.timeouts),
            "cases" => serde_json::to_value(&self.cases),
            "rules_unavailable_decision" => serde_json::to_value(self.rules_unavailable_decision),
            "engine_combination" => serde_json::to_value(self.engine_combination),
            _ => return None,
        };
        v.ok()
    }

    /// All keys as a JSON object (API response).
    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for k in KEYS {
            if let Some(v) = self.get(k) {
                m.insert((*k).to_string(), v);
            }
        }
        Value::Object(m)
    }

    /// Merges stored rows over the defaults. Invalid rows are ignored (logged by the caller).
    pub fn from_rows<'a>(rows: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Self {
        let mut s = Self::default();
        for (k, v) in rows {
            if validate(k, v).is_ok() {
                s.apply(k, v);
            }
        }
        s
    }

    fn apply(&mut self, key: &str, v: &Value) {
        match key {
            "decision_thresholds" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.decision_thresholds = x;
                }
            }
            "engine_weights" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.engine_weights = x;
                }
            }
            "graph_scores" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.graph_scores = x;
                }
            }
            "timeouts" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.timeouts = x;
                }
            }
            "cases" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.cases = x;
                }
            }
            "engine_combination" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.engine_combination = x;
                }
            }
            "rules_unavailable_decision" => {
                if let Ok(x) = serde_json::from_value(v.clone()) {
                    self.rules_unavailable_decision = x;
                }
            }
            _ => {}
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(key: &str, v: &Value) -> Result<T, Vec<FieldError>> {
    serde_json::from_value(v.clone()).map_err(|e| vec![FieldError::new(key, e.to_string())])
}

fn in_range(errors: &mut Vec<FieldError>, field: &str, v: f64, lo: f64, hi: f64) {
    if !v.is_finite() || v < lo || v > hi {
        errors.push(FieldError::new(field, format!("must be between {lo} and {hi}")));
    }
}

/// Validates a settings value for `key` (PUT /settings/{key}).
pub fn validate(key: &str, v: &Value) -> Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();
    match key {
        "decision_thresholds" => {
            let t: DecisionThresholds = parse(key, v)?;
            in_range(&mut errors, "decision_thresholds.review", t.review, 0.0, 100.0);
            in_range(&mut errors, "decision_thresholds.decline", t.decline, 0.0, 100.0);
            if t.review >= t.decline {
                errors.push(FieldError::new(
                    "decision_thresholds",
                    "review must be lower than decline",
                ));
            }
        }
        "engine_weights" => {
            let w: EngineWeights = parse(key, v)?;
            for (n, x) in [
                ("rules", w.rules),
                ("supervised", w.supervised),
                ("unsupervised", w.unsupervised),
                ("graph", w.graph),
            ] {
                in_range(&mut errors, &format!("engine_weights.{n}"), x, 0.0, 1.0);
            }
            if w.rules + w.supervised + w.unsupervised + w.graph <= 0.0 {
                errors.push(FieldError::new(
                    "engine_weights",
                    "at least one weight must be > 0",
                ));
            }
        }
        "graph_scores" => {
            let g: GraphScores = parse(key, v)?;
            for (d, s) in &g.fraud_distance_scores {
                match d.parse::<u32>() {
                    Ok(n) if (1..=6).contains(&n) => {}
                    _ => errors.push(FieldError::new(
                        format!("graph_scores.fraud_distance_scores.{d}"),
                        "distance keys must be 1..6",
                    )),
                }
                in_range(
                    &mut errors,
                    &format!("graph_scores.fraud_distance_scores.{d}"),
                    *s,
                    0.0,
                    100.0,
                );
            }
            in_range(
                &mut errors,
                "graph_scores.shared_fraud_entity_score",
                g.shared_fraud_entity_score,
                0.0,
                100.0,
            );
        }
        "timeouts" => {
            let t: Timeouts = parse(key, v)?;
            for (n, x) in [
                ("graph_ms", t.graph_ms),
                ("ml_ms", t.ml_ms),
                ("rules_ms", t.rules_ms),
            ] {
                if !(10..=5000).contains(&x) {
                    errors.push(FieldError::new(
                        format!("timeouts.{n}"),
                        "must be between 10 and 5000",
                    ));
                }
            }
        }
        "cases" => {
            let c: CaseSettings = parse(key, v)?;
            if c.auto_create_on.contains(&Decision::Approve) {
                errors.push(FieldError::new(
                    "cases.auto_create_on",
                    "approve cannot open cases",
                ));
            }
        }
        "rules_unavailable_decision" => {
            let _: Decision = parse(key, v)?;
        }
        "engine_combination" => {
            let _: EngineCombination = parse(key, v)?;
        }
        other => {
            errors.push(FieldError::new(other, "unknown settings key"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_roundtrip_and_validate() {
        for (k, v) in ProjectSettings::default_rows() {
            assert!(validate(k, &v).is_ok(), "{k}");
        }
        let s = ProjectSettings::from_rows(ProjectSettings::default_rows().iter().map(|(k, v)| (*k, v)));
        assert_eq!(s, ProjectSettings::default());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(validate("decision_thresholds", &json!({"review": 80, "decline": 50})).is_err());
        assert!(validate(
            "engine_weights",
            &json!({"rules": 0, "supervised": 0, "unsupervised": 0, "graph": 0})
        )
        .is_err());
        assert!(validate(
            "engine_weights",
            &json!({"rules": 2, "supervised": 0, "unsupervised": 0, "graph": 0})
        )
        .is_err());
        assert!(validate("timeouts", &json!({"graph_ms": 1, "ml_ms": 200, "rules_ms": 300})).is_err());
        assert!(validate(
            "cases",
            &json!({"auto_create_on": ["approve"], "dedupe_open_per_customer": true})
        )
        .is_err());
        assert!(validate("rules_unavailable_decision", &json!("maybe")).is_err());
        assert!(validate("nope", &json!(1)).is_err());
        assert!(validate(
            "decision_thresholds",
            &json!({"review": 10, "decline": 20, "x": 1})
        )
        .is_err());
    }

    #[test]
    fn stored_rows_override_defaults() {
        let v = json!({"review": 40, "decline": 70});
        let s = ProjectSettings::from_rows([("decision_thresholds", &v)]);
        assert_eq!(s.decision_thresholds.review, 40.0);
        let bad = json!("garbage");
        let s2 = ProjectSettings::from_rows([("decision_thresholds", &bad)]);
        assert_eq!(s2.decision_thresholds.review, 50.0);
    }
}
