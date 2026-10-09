//! Public ingest/decision DTOs of core-api (api-contract.md §A.3).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::Decision;
use crate::graph::GraphMetrics;
use crate::scoring::{Reason, RuleResultTrace};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CustomerIn {
    pub external_id: String,
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub registered_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub kyc_level: Option<i16>,
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: Option<Map<String, Value>>,
}

/// Event in canonical shape (feature-catalog.md §1). Also the output of a data-source mapping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CanonicalEventIn {
    pub external_id: String,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub customer: CustomerIn,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub merchant_id: Option<String>,
    #[serde(default)]
    pub merchant_category: Option<String>,
    #[serde(default)]
    pub payment_method: Option<String>,
    #[serde(default)]
    pub instrument_fingerprint: Option<String>,
    #[serde(default)]
    pub card_bin: Option<String>,
    #[serde(default)]
    pub card_last4: Option<String>,
    #[serde(default)]
    pub issuer_country: Option<String>,
    #[serde(default)]
    pub recipient_fingerprint: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub geo_country: Option<String>,
    #[serde(default)]
    pub geo_city: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub promo_code: Option<String>,
    #[serde(default)]
    pub discount_amount: Option<f64>,
    #[serde(default)]
    pub cashback_amount: Option<f64>,
    #[serde(default)]
    pub ref_transaction_id: Option<String>,
    #[serde(default)]
    pub shipping_address: Option<String>,
    #[serde(default)]
    pub billing_address: Option<String>,
    #[serde(default)]
    pub account_change_type: Option<String>,
    #[serde(default)]
    pub login_success: Option<bool>,
    #[serde(default)]
    pub api_client_id: Option<String>,
    /// Raw PAN — hashed immediately by core-api into `instrument_fingerprint` (+ bin/last4), never stored.
    #[serde(default, skip_serializing)]
    pub card_number: Option<String>,
    /// Raw beneficiary/bank account number — hashed into `recipient_fingerprint`, never stored.
    #[serde(default, skip_serializing)]
    pub account_number: Option<String>,
    /// Extra fields, exposed to rules as `source.*`.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub payload: Option<Map<String, Value>>,
}

/// Reference to a model used for a decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelRef {
    pub id: Uuid,
    pub version: i32,
    #[serde(default)]
    pub algorithm: Option<String>,
}

/// ML part of a decision (`decisions.ml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MlSummary {
    #[serde(default)]
    pub fraud_probability: Option<f64>,
    #[serde(default)]
    pub anomaly_score: Option<f64>,
    #[serde(default)]
    pub cluster_id: Option<i32>,
    #[serde(default)]
    pub cluster_fraud_rate: Option<f64>,
    #[serde(default)]
    pub supervised_model: Option<ModelRef>,
    #[serde(default)]
    pub unsupervised_model: Option<ModelRef>,
}

/// Per-engine scores (0..100). `None` = engine unavailable/degraded for this decision.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EngineScores {
    pub rules: Option<f64>,
    pub supervised: Option<f64>,
    pub unsupervised: Option<f64>,
    pub graph: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DecisionOut {
    pub event_id: Uuid,
    pub external_id: String,
    pub project_id: Uuid,
    pub decision: Decision,
    pub final_score: f64,
    pub engine_scores: EngineScores,
    pub reasons: Vec<Reason>,
    pub rule_results: Vec<RuleResultTrace>,
    pub ml: MlSummary,
    #[serde(default)]
    pub graph: Option<GraphMetrics>,
    /// Engines dropped from this decision: `rules` | `supervised` | `unsupervised` | `graph`.
    pub degraded: Vec<String>,
    #[serde(default)]
    pub case_id: Option<Uuid>,
    pub latency_ms: u64,
    pub persisted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_card_number_is_never_serialised() {
        let ev = CanonicalEventIn {
            external_id: "T-1".into(),
            event_type: "transaction".into(),
            card_number: Some("4111111111111111".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&ev).unwrap_or_default();
        assert!(!json.contains("4111111111111111"));
    }

    #[test]
    fn canonical_event_minimal_json() {
        let v = serde_json::json!({
            "external_id": "T-1", "event_type": "login", "occurred_at": "2026-09-23T01:02:03Z",
            "customer": { "external_id": "C-1" }, "login_success": false
        });
        let ev: CanonicalEventIn = serde_json::from_value(v).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ev.login_success, Some(false));
    }
}
