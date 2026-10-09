//! core-api ⇄ rule-service: `POST /v1/projects/{pid}/evaluate` (api-contract.md §B).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::{RuleAction, RuleKind, RuleOutcome};

/// The evaluation context seen by rules (rule-dsl §5). Each part is a JSON object;
/// `source` is the raw source record, `ml`/`graph` may be empty objects when those engines are
/// degraded (rules referencing them then trap or no-match per `missing_as_no_match`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EvaluationContext {
    #[schema(value_type = Object)]
    pub event: Value,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub source: Value,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub customer: Value,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub features: Value,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub ml: Value,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub graph: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EvaluateRequest {
    pub event_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub event_type: String,
    pub customer_id: Uuid,
    pub context: EvaluationContext,
    /// When true nothing is persisted (`rule_hits`, counters) — simulation / tests.
    #[serde(default)]
    pub dry_run: bool,
}

/// Score of one ruleset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RulesetScore {
    pub ruleset_id: Uuid,
    pub code: String,
    pub score: f64,
    pub shadow: bool,
}

/// One evaluated rule (rule-dsl §7). Stored verbatim in `core.decisions.rule_results`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RuleResultTrace {
    pub rule_id: Uuid,
    pub rule_code: String,
    pub version: i32,
    #[serde(default)]
    pub ruleset_id: Option<Uuid>,
    #[serde(default)]
    pub ruleset_code: Option<String>,
    pub kind: RuleKind,
    pub outcome: RuleOutcome,
    pub contribution: f64,
    pub shadow: bool,
    pub action: RuleAction,
    #[serde(default)]
    pub trapped_reason: Option<String>,
    /// Kind-specific evaluation trace (values compared, samples, window, ...).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub trace: Value,
    pub duration_us: u64,
}

/// Explanation item (architecture.md §3.1): up to 8, sorted by contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Reason {
    /// Rule code (`RL-CARD-001`) or engine code (`ML_SUPERVISED_HIGH`, `GRAPH_FRAUD_DISTANCE_2`, ...).
    pub code: String,
    /// `rules` | `supervised` | `unsupervised` | `graph`
    pub engine: String,
    pub contribution: f64,
    pub message: String,
}

/// Overrides requested by matched (non-shadow) rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Actions {
    pub force_decline: bool,
    pub force_approve: bool,
    pub force_review: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EvaluateResponse {
    /// Max over active (non-shadow) rulesets, 0..=100.
    pub rules_score: f64,
    pub rulesets: Vec<RulesetScore>,
    pub rule_results: Vec<RuleResultTrace>,
    pub actions: Actions,
    pub reasons: Vec<Reason>,
    pub duration_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_request_accepts_minimal_context() {
        let json = serde_json::json!({
            "event_id": Uuid::nil(), "occurred_at": "2026-09-23T10:00:00Z", "event_type": "transaction",
            "customer_id": Uuid::nil(), "context": { "event": { "amount": 10 } }
        });
        let req: EvaluateRequest = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
        assert!(!req.dry_run);
        assert_eq!(req.context.ml, Value::Null);
    }
}
