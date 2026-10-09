//! Ports: what the application layer needs from the other engines.
//!
//! These traits are the Rust equivalent of Java interfaces used for dependency inversion
//! (hexagonal architecture). The scoring pipeline depends on `dyn RuleEngineClient`, not on
//! reqwest. Production wires the HTTP adapters (`adapters::engines`), and tests can wire fakes.
//! That is the whole point: the orchestration logic (timeouts, degraded mode) is tested without
//! running rule-, graph- or ml-service.
//!
//! Database access is deliberately **not** hidden behind repository traits: SQL is the
//! domain-specific language of persistence here, the queries rely on RLS and Postgres features,
//! and the integration tests run against a real Postgres. A mock repository would test the mock,
//! not the query.

use std::time::Duration;

use async_trait::async_trait;
use contracts::graph::{GraphLinksRequest, GraphLinksResponse, GraphMetrics, GraphMetricsRequest};
use contracts::ml::{MlPredictResponse, MlScoreResponse};
use contracts::scoring::{EvaluateRequest, EvaluateResponse};
use platform::error::AppResult;
use platform::http::CallCtx;
use platform::ProjectId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// rule-service.
#[async_trait]
pub trait RuleEngineClient: Send + Sync + 'static {
    async fn evaluate(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &EvaluateRequest,
        timeout: Duration,
    ) -> AppResult<EvaluateResponse>;

    /// Creates the template rules/rulesets/lists of a project stage.
    async fn bootstrap(&self, ctx: &CallCtx, project: ProjectId, template: &str) -> AppResult<()>;
}

/// graph-service.
#[async_trait]
pub trait GraphClient: Send + Sync + 'static {
    async fn links(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &GraphLinksRequest,
        timeout: Duration,
    ) -> AppResult<GraphLinksResponse>;

    async fn metrics(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &GraphMetricsRequest,
        timeout: Duration,
    ) -> AppResult<GraphMetrics>;

    async fn set_label(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        customer: Uuid,
        risk_label: &str,
    ) -> AppResult<()>;
}

/// Body of `supervised/predict` and `unsupervised/score` (ml-service Pydantic model).
#[derive(Debug, Clone, Default, Serialize)]
pub struct MlRequest {
    pub event_id: Option<Uuid>,
    pub features: Map<String, Value>,
    /// Raw source payload (for `ml_config.features.extra_source_fields`).
    pub source: Option<Map<String, Value>>,
    pub explain: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigError {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigValidation {
    pub valid: bool,
    #[serde(default)]
    pub errors: Vec<ConfigError>,
}

/// ml-service. `Ok(None)` = the project has no active model of that kind (not an error).
#[async_trait]
pub trait MlClient: Send + Sync + 'static {
    async fn predict(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &MlRequest,
        timeout: Duration,
    ) -> AppResult<Option<MlPredictResponse>>;

    async fn score(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &MlRequest,
        timeout: Duration,
    ) -> AppResult<Option<MlScoreResponse>>;

    async fn validate_config(&self, ctx: &CallCtx, ml_config: &Value) -> AppResult<ConfigValidation>;
}
