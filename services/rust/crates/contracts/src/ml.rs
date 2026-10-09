//! core-api ⇄ ml-service (api-contract.md §D). ml-service is Python; these structs mirror its
//! Pydantic models.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

/// `POST /v1/projects/{pid}/supervised/predict` and `/unsupervised/score` request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MlPredictRequest {
    #[serde(default)]
    pub event_id: Option<Uuid>,
    /// Feature-set v1 values (feature-catalog.md §2); missing keys are imputed by ml-service.
    #[schema(value_type = Object)]
    pub features: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FeatureContribution {
    pub name: String,
    pub contribution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MlPredictResponse {
    pub model_id: Uuid,
    pub model_version: i32,
    #[serde(default)]
    pub algorithm: Option<String>,
    pub fraud_probability: f64,
    #[serde(default)]
    pub top_features: Vec<FeatureContribution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MlScoreResponse {
    pub model_id: Uuid,
    pub model_version: i32,
    /// 0..1, higher = more anomalous.
    pub anomaly_score: f64,
    #[serde(default)]
    pub cluster_id: Option<i32>,
    #[serde(default)]
    pub cluster_fraud_rate: Option<f64>,
}
