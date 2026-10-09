//! Logging, request ids and Prometheus metrics.
//!
//! * Logs are JSON lines (`tracing` + `tracing-subscriber`). Every request runs inside a span that
//!   carries `request_id`, `method` and `path`, so every log line of that request can be correlated.
//! * `x-request-id` is generated when absent, echoed in the response, and forwarded to downstream
//!   services by [`crate::http::ServiceClient`].
//! * Metrics: `http_requests_total{method,route,status}` and
//!   `http_request_duration_seconds{method,route,status}` use the **route template**
//!   (`/api/v1/projects/{pid}/rules`), not the raw path, to keep label cardinality bounded.

use std::time::Instant;

use axum::extract::{FromRequestParts, MatchedPath, Request};
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

pub fn request_id_header() -> HeaderName {
    HeaderName::from_static(REQUEST_ID_HEADER)
}

/// Initialises JSON logging. Safe to call more than once (later calls are no-ops).
pub fn init_tracing(default_level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_target(true),
        )
        .try_init();
}

/// Installs the global Prometheus recorder and returns the handle used by the `/metrics` route.
pub fn init_metrics() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            metrics_exporter_prometheus::Matcher::Full("http_request_duration_seconds".into()),
            &[
                0.005, 0.01, 0.025, 0.05, 0.1, 0.15, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        )?
        .install_recorder()?;
    Ok(handle)
}

/// axum middleware recording request count and latency per route template.
pub async fn track_http_metrics(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".to_owned());
    let response = next.run(req).await;
    let status = response.status().as_u16().to_string();
    let labels = [("method", method), ("route", route), ("status", status)];
    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels).record(start.elapsed().as_secs_f64());
    response
}

/// The current request id (extractor). Falls back to a fresh UUID if the layer is not installed.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl<S: Send + Sync> FromRequestParts<S> for RequestId {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let id = parts
            .headers
            .get(REQUEST_ID_HEADER)
            .and_then(|v: &HeaderValue| v.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(Self(id))
    }
}
