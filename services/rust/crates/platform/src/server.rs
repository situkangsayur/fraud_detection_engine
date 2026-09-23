//! HTTP server bootstrap shared by all Rust services.
//!
//! A service builds its own `axum::Router` (its routes and state) and hands it to [`serve`], which adds:
//! * `/health/live`, `/health/ready` (pluggable [`ReadinessCheck`]s) and `/metrics`;
//! * the standard middleware stack: request id → tracing span → HTTP metrics → timeout → body
//!   limit → compression → CORS;
//! * graceful shutdown on SIGTERM/SIGINT (docker stop), which lets in-flight requests finish.
//!
//! This is the "template method" of a Java `AbstractServiceApplication`, done with a function that
//! takes the variable part (the router) as an argument: composition over inheritance.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::State;
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{middleware, Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use serde_json::json;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::BaseConfig;
use crate::telemetry::{request_id_header, track_http_metrics, REQUEST_ID_HEADER};

/// A dependency checked by `/health/ready`. Implement it for anything the service needs.
#[async_trait]
pub trait ReadinessCheck: Send + Sync + 'static {
    fn name(&self) -> &str;
    /// If a critical check fails, readiness returns 503 (load balancers stop routing traffic).
    /// Non-critical checks are reported but do not fail readiness (e.g. ML is optional for scoring).
    fn critical(&self) -> bool {
        true
    }
    async fn check(&self) -> Result<(), String>;
}

/// Postgres readiness (`SELECT 1`).
#[derive(Debug, Clone)]
pub struct PgReadiness(pub sqlx::PgPool);

#[async_trait]
impl ReadinessCheck for PgReadiness {
    fn name(&self) -> &str {
        "db"
    }
    async fn check(&self) -> Result<(), String> {
        crate::db::ping(&self.0).await.map_err(|e| e.to_string())
    }
}

/// Downstream HTTP service readiness (GET `<base>/health/live`), non-critical by default.
#[derive(Debug, Clone)]
pub struct HttpReadiness {
    pub name: String,
    pub url: String,
    pub critical: bool,
    client: reqwest::Client,
}

impl HttpReadiness {
    pub fn new(name: impl Into<String>, base_url: &str, critical: bool) -> Self {
        Self {
            name: name.into(),
            url: format!("{}/health/live", base_url.trim_end_matches('/')),
            critical,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ReadinessCheck for HttpReadiness {
    fn name(&self) -> &str {
        &self.name
    }
    fn critical(&self) -> bool {
        self.critical
    }
    async fn check(&self) -> Result<(), String> {
        let resp = self
            .client
            .get(&self.url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("status {}", resp.status()))
        }
    }
}

/// Everything [`serve`] needs besides the router.
#[derive(Clone)]
pub struct ServerOptions {
    pub bind_addr: String,
    pub cors_origins: Vec<String>,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
    pub readiness: Vec<Arc<dyn ReadinessCheck>>,
    pub metrics: Option<PrometheusHandle>,
}

impl std::fmt::Debug for ServerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerOptions")
            .field("bind_addr", &self.bind_addr)
            .finish_non_exhaustive()
    }
}

impl ServerOptions {
    pub fn from_config(cfg: &BaseConfig) -> Self {
        Self {
            bind_addr: cfg.bind_addr.clone(),
            cors_origins: cfg.cors_origin_list(),
            request_timeout: Duration::from_secs(cfg.request_timeout_secs),
            max_body_bytes: cfg.max_body_bytes,
            readiness: Vec::new(),
            metrics: None,
        }
    }

    pub fn with_readiness(mut self, check: impl ReadinessCheck) -> Self {
        self.readiness.push(Arc::new(check));
        self
    }

    pub fn with_metrics(mut self, handle: PrometheusHandle) -> Self {
        self.metrics = Some(handle);
        self
    }
}

#[derive(Clone)]
struct OpsState {
    readiness: Arc<Vec<Arc<dyn ReadinessCheck>>>,
    metrics: Option<PrometheusHandle>,
}

async fn live() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn ready(State(st): State<OpsState>) -> impl IntoResponse {
    let mut checks = serde_json::Map::new();
    let mut ok = true;
    for c in st.readiness.iter() {
        let res = tokio::time::timeout(Duration::from_secs(3), c.check()).await;
        let status = match res {
            Ok(Ok(())) => "ok".to_string(),
            Ok(Err(e)) => format!("error: {e}"),
            Err(_) => "error: timeout".to_string(),
        };
        if status != "ok" && c.critical() {
            ok = false;
        }
        checks.insert(c.name().to_string(), json!(status));
    }
    let code = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(json!({ "status": if ok { "ok" } else { "degraded" }, "checks": checks })),
    )
}

async fn metrics_route(State(st): State<OpsState>) -> impl IntoResponse {
    match &st.metrics {
        Some(h) => (StatusCode::OK, h.render()),
        None => (StatusCode::NOT_FOUND, String::new()),
    }
}

/// Applies the standard middleware stack to an application router. Exposed separately so
/// integration tests can exercise the same stack without binding a socket.
pub fn with_standard_layers(app: Router, opts: &ServerOptions) -> Router {
    let ops = OpsState {
        readiness: Arc::new(opts.readiness.clone()),
        metrics: opts.metrics.clone(),
    };
    let ops_router = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/metrics", get(metrics_route))
        .with_state(ops);

    let cors = if opts.cors_origins.is_empty() {
        CorsLayer::new()
    } else {
        let origins: Vec<HeaderValue> = opts.cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([
                axum::http::header::AUTHORIZATION,
                axum::http::header::CONTENT_TYPE,
                request_id_header(),
            ])
            .allow_credentials(true)
    };

    let trace = TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
        let rid = req
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-");
        tracing::info_span!("http", request_id = %rid, method = %req.method(), path = %req.uri().path())
    });

    // Layers wrap from bottom (innermost) to top (outermost).
    app.layer(middleware::from_fn(track_http_metrics))
        .merge(ops_router)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(opts.max_body_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            opts.request_timeout,
        ))
        .layer(cors)
        .layer(trace)
        .layer(PropagateRequestIdLayer::new(request_id_header()))
        .layer(SetRequestIdLayer::new(request_id_header(), MakeRequestUuid))
}

/// Binds, serves with the standard stack and shuts down gracefully.
pub async fn serve(app: Router, opts: ServerOptions) -> anyhow::Result<()> {
    let router = with_standard_layers(app, &opts);
    let listener = tokio::net::TcpListener::bind(&opts.bind_addr).await?;
    tracing::info!(addr = %opts.bind_addr, "listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("server stopped");
    Ok(())
}

/// Resolves on SIGINT or SIGTERM.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "failed to install Ctrl+C handler");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => tracing::error!(error = %e, "failed to install SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    struct Failing(bool);
    #[async_trait]
    impl ReadinessCheck for Failing {
        fn name(&self) -> &str {
            "dep"
        }
        fn critical(&self) -> bool {
            self.0
        }
        async fn check(&self) -> Result<(), String> {
            Err("down".into())
        }
    }

    fn opts(critical: bool) -> ServerOptions {
        ServerOptions {
            bind_addr: "127.0.0.1:0".into(),
            cors_origins: vec![],
            request_timeout: Duration::from_secs(5),
            max_body_bytes: 1024,
            readiness: vec![Arc::new(Failing(critical))],
            metrics: None,
        }
    }

    #[tokio::test]
    async fn live_ok_and_request_id_echoed() {
        let app = with_standard_layers(Router::new(), &opts(false));
        let resp = app
            .oneshot(Request::get("/health/live").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(REQUEST_ID_HEADER).is_some());
    }

    #[tokio::test]
    async fn readiness_respects_criticality() {
        let app = with_standard_layers(Router::new(), &opts(false));
        let resp = app
            .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let app = with_standard_layers(Router::new(), &opts(true));
        let resp = app
            .oneshot(Request::get("/health/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn incoming_request_id_is_propagated() {
        let app = with_standard_layers(Router::new(), &opts(false));
        let resp = app
            .oneshot(
                Request::get("/health/live")
                    .header(REQUEST_ID_HEADER, "abc-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.headers().get(REQUEST_ID_HEADER).unwrap(), "abc-123");
    }
}
