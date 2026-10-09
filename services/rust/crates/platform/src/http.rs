//! Outgoing service-to-service HTTP.
//!
//! [`ServiceClient`] wraps one downstream service (for example rule-service). Every call:
//! * authenticates with the internal token;
//! * forwards the tenant/project/actor context (`X-Tenant-Id`, `X-Project-Id`, `X-Actor`) and
//!   `x-request-id`;
//! * has its own timeout. The scoring pipeline uses tight per-engine budgets and must degrade
//!   instead of hanging (architecture.md §3);
//! * maps transport errors, timeouts and 5xx responses to [`AppError::Upstream`]. A 4xx response is
//!   passed through as the matching `AppError` (the downstream's problem detail is preserved).
//!
//! `reqwest::Client` is internally reference-counted and pools connections, so cloning a
//! `ServiceClient` is cheap and one should be created per downstream at startup.

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::auth::{HEADER_ACTOR, HEADER_PROJECT, HEADER_TENANT};
use crate::config::Secret;
use crate::error::{AppError, AppResult};
use crate::ids::{ProjectId, TenantId, UserId};
use crate::telemetry::REQUEST_ID_HEADER;

/// Per-call context forwarded to the downstream service.
#[derive(Debug, Clone)]
pub struct CallCtx {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
    pub actor: Option<UserId>,
    pub request_id: Option<String>,
}

impl CallCtx {
    pub fn new(tenant: TenantId, project: Option<ProjectId>) -> Self {
        Self {
            tenant,
            project,
            actor: None,
            request_id: None,
        }
    }
    pub fn with_actor(mut self, actor: Option<UserId>) -> Self {
        self.actor = actor;
        self
    }
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

#[derive(Clone)]
pub struct ServiceClient {
    name: &'static str,
    base_url: String,
    token: Secret,
    http: reqwest::Client,
    default_timeout: Duration,
}

impl std::fmt::Debug for ServiceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceClient")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl ServiceClient {
    /// `name` is used in errors/metrics, e.g. `"rule-service"`.
    pub fn new(
        name: &'static str,
        base_url: impl Into<String>,
        token: Secret,
        default_timeout: Duration,
    ) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| AppError::internal(format!("http client: {e}")))?;
        Ok(Self {
            name,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            http,
            default_timeout,
        })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        ctx: &CallCtx,
        timeout: Option<Duration>,
    ) -> reqwest::RequestBuilder {
        let mut rb = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(self.token.expose())
            .timeout(timeout.unwrap_or(self.default_timeout))
            .header(HEADER_TENANT, ctx.tenant.to_string());
        if let Some(p) = ctx.project {
            rb = rb.header(HEADER_PROJECT, p.to_string());
        }
        if let Some(a) = ctx.actor {
            rb = rb.header(HEADER_ACTOR, a.to_string());
        }
        if let Some(r) = &ctx.request_id {
            rb = rb.header(REQUEST_ID_HEADER, r);
        }
        rb
    }

    /// `POST path` with a JSON body, decoding a JSON response.
    pub async fn post_json<B: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        ctx: &CallCtx,
        timeout: Option<Duration>,
    ) -> AppResult<R> {
        let rb = self.request(Method::POST, path, ctx, timeout).json(body);
        self.send(rb).await
    }

    pub async fn put_json<B: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        ctx: &CallCtx,
        timeout: Option<Duration>,
    ) -> AppResult<R> {
        let rb = self.request(Method::PUT, path, ctx, timeout).json(body);
        self.send(rb).await
    }

    pub async fn get_json<R: DeserializeOwned>(
        &self,
        path: &str,
        ctx: &CallCtx,
        timeout: Option<Duration>,
    ) -> AppResult<R> {
        let rb = self.request(Method::GET, path, ctx, timeout);
        self.send(rb).await
    }

    /// Raw request for streaming/proxying use cases.
    pub fn raw(
        &self,
        method: Method,
        path: &str,
        ctx: &CallCtx,
        timeout: Option<Duration>,
    ) -> reqwest::RequestBuilder {
        self.request(method, path, ctx, timeout)
    }

    async fn send<R: DeserializeOwned>(&self, rb: reqwest::RequestBuilder) -> AppResult<R> {
        let started = std::time::Instant::now();
        let result = rb.send().await;
        let elapsed = started.elapsed().as_secs_f64();
        let resp = match result {
            Ok(r) => r,
            Err(e) => {
                let kind = if e.is_timeout() { "timeout" } else { "transport" };
                metrics::counter!("upstream_errors_total", "service" => self.name, "kind" => kind)
                    .increment(1);
                return Err(AppError::Upstream {
                    service: self.name.into(),
                    detail: format!("{kind}: {e}"),
                });
            }
        };
        metrics::histogram!("upstream_request_duration_seconds", "service" => self.name).record(elapsed);
        let status = resp.status();
        if status.is_success() {
            return resp.json::<R>().await.map_err(|e| AppError::Upstream {
                service: self.name.into(),
                detail: format!("invalid response body: {e}"),
            });
        }
        let body = resp.text().await.unwrap_or_default();
        Err(self.map_status(status, body))
    }

    fn map_status(&self, status: StatusCode, body: String) -> AppError {
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(String::from))
            .unwrap_or_else(|| body.chars().take(300).collect());
        match status {
            StatusCode::NOT_FOUND => AppError::NotFound(detail),
            StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => {
                // Preserve structured field errors when the downstream sent them.
                let errors = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("errors").cloned())
                    .and_then(|e| serde_json::from_value::<Vec<FieldErrorDto>>(e).ok())
                    .unwrap_or_default();
                if errors.is_empty() {
                    AppError::BadRequest(detail)
                } else {
                    AppError::Validation {
                        detail: Some(detail),
                        errors: errors
                            .into_iter()
                            .map(|e| crate::error::FieldError::new(e.field, e.message))
                            .collect(),
                    }
                }
            }
            StatusCode::CONFLICT => AppError::Conflict(detail),
            StatusCode::FORBIDDEN => AppError::Forbidden(detail),
            StatusCode::UNAUTHORIZED => AppError::Upstream {
                service: self.name.into(),
                detail: "unauthorized (check INTERNAL_API_TOKEN)".into(),
            },
            _ => {
                metrics::counter!("upstream_errors_total", "service" => self.name, "kind" => "status")
                    .increment(1);
                AppError::Upstream {
                    service: self.name.into(),
                    detail: format!("{status}: {detail}"),
                }
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct FieldErrorDto {
    #[serde(default)]
    field: String,
    #[serde(default)]
    message: String,
}
