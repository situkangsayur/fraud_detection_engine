//! HTTP adapters implementing the engine ports with [`platform::http::ServiceClient`].
//!
//! Exact downstream calls (keep in sync with api-contract.md §B–§D):
//! * rule-service  `POST /v1/projects/{pid}/evaluate`          `EvaluateRequest` → `EvaluateResponse`
//! * rule-service  `POST /v1/projects/{pid}/bootstrap`         `{template}` → any
//! * graph-service `POST /v1/projects/{pid}/links`             `GraphLinksRequest` → `GraphLinksResponse`
//! * graph-service `POST /v1/projects/{pid}/metrics`           `GraphMetricsRequest` → `GraphMetrics`
//! * graph-service `PUT  /v1/projects/{pid}/customers/{cid}/label` `{risk_label}` → any
//! * ml-service    `POST /v1/projects/{pid}/supervised/predict` `{event_id, features, source, explain}` → `MlPredictResponse` (404 = no active model)
//! * ml-service    `POST /v1/projects/{pid}/unsupervised/score` `{event_id, features, source}` → `MlScoreResponse` (404 = no active model)
//! * ml-service    `POST /v1/algorithms/validate-config`        `{ml_config}` → `{valid, errors}`

use std::time::Duration;

use async_trait::async_trait;
use contracts::graph::{
    CustomerLabelUpdate, GraphLinksRequest, GraphLinksResponse, GraphMetrics, GraphMetricsRequest,
};
use contracts::ml::{MlPredictResponse, MlScoreResponse};
use contracts::scoring::{EvaluateRequest, EvaluateResponse};
use platform::error::{AppError, AppResult};
use platform::http::{CallCtx, ServiceClient};
use platform::ProjectId;
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::application::ports::{ConfigValidation, GraphClient, MlClient, MlRequest, RuleEngineClient};

/// Sends a request whose response body we do not need (2xx with any/empty body).
async fn send_ignore_body<B: Serialize + ?Sized>(
    client: &ServiceClient,
    method: Method,
    path: &str,
    body: &B,
    ctx: &CallCtx,
    timeout: Option<Duration>,
) -> AppResult<()> {
    let resp = client
        .raw(method, path, ctx, timeout)
        .json(body)
        .send()
        .await
        .map_err(|e| AppError::Upstream {
            service: client.name().into(),
            detail: e.to_string(),
        })?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        404 => AppError::NotFound(text.chars().take(300).collect()),
        409 => AppError::Conflict(text.chars().take(300).collect()),
        _ => AppError::Upstream {
            service: client.name().into(),
            detail: format!("{status}: {}", text.chars().take(300).collect::<String>()),
        },
    })
}

#[derive(Debug, Clone)]
pub struct HttpRuleEngine(pub ServiceClient);

#[async_trait]
impl RuleEngineClient for HttpRuleEngine {
    async fn evaluate(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &EvaluateRequest,
        timeout: Duration,
    ) -> AppResult<EvaluateResponse> {
        self.0
            .post_json(
                &format!("/v1/projects/{project}/evaluate"),
                req,
                ctx,
                Some(timeout),
            )
            .await
    }

    async fn bootstrap(&self, ctx: &CallCtx, project: ProjectId, template: &str) -> AppResult<()> {
        send_ignore_body(
            &self.0,
            Method::POST,
            &format!("/v1/projects/{project}/bootstrap"),
            &json!({ "template": template }),
            ctx,
            Some(Duration::from_secs(30)),
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub struct HttpGraph(pub ServiceClient);

#[async_trait]
impl GraphClient for HttpGraph {
    async fn links(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &GraphLinksRequest,
        timeout: Duration,
    ) -> AppResult<GraphLinksResponse> {
        self.0
            .post_json(&format!("/v1/projects/{project}/links"), req, ctx, Some(timeout))
            .await
    }

    async fn metrics(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &GraphMetricsRequest,
        timeout: Duration,
    ) -> AppResult<GraphMetrics> {
        self.0
            .post_json(
                &format!("/v1/projects/{project}/metrics"),
                req,
                ctx,
                Some(timeout),
            )
            .await
    }

    async fn set_label(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        customer: Uuid,
        risk_label: &str,
    ) -> AppResult<()> {
        send_ignore_body(
            &self.0,
            Method::PUT,
            &format!("/v1/projects/{project}/customers/{customer}/label"),
            &CustomerLabelUpdate {
                risk_label: risk_label.to_string(),
            },
            ctx,
            Some(Duration::from_secs(5)),
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub struct HttpMl(pub ServiceClient);

fn no_model<T>(r: AppResult<T>) -> AppResult<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

#[async_trait]
impl MlClient for HttpMl {
    async fn predict(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &MlRequest,
        timeout: Duration,
    ) -> AppResult<Option<MlPredictResponse>> {
        no_model(
            self.0
                .post_json(
                    &format!("/v1/projects/{project}/supervised/predict"),
                    req,
                    ctx,
                    Some(timeout),
                )
                .await,
        )
    }

    async fn score(
        &self,
        ctx: &CallCtx,
        project: ProjectId,
        req: &MlRequest,
        timeout: Duration,
    ) -> AppResult<Option<MlScoreResponse>> {
        #[derive(Serialize)]
        struct ScoreBody<'a> {
            event_id: Option<Uuid>,
            features: &'a serde_json::Map<String, Value>,
            source: &'a Option<serde_json::Map<String, Value>>,
        }
        let body = ScoreBody {
            event_id: req.event_id,
            features: &req.features,
            source: &req.source,
        };
        no_model(
            self.0
                .post_json(
                    &format!("/v1/projects/{project}/unsupervised/score"),
                    &body,
                    ctx,
                    Some(timeout),
                )
                .await,
        )
    }

    async fn validate_config(&self, ctx: &CallCtx, ml_config: &Value) -> AppResult<ConfigValidation> {
        self.0
            .post_json(
                "/v1/algorithms/validate-config",
                &json!({ "ml_config": ml_config }),
                ctx,
                Some(Duration::from_secs(5)),
            )
            .await
    }
}
