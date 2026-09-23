//! Internal (service-to-service) endpoints.

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use contracts::graph::{
    CustomerLabelUpdate, GraphLinksRequest, GraphLinksResponse, GraphMetricRequest, GraphMetricResponse,
    GraphMetrics, GraphMetricsRequest,
};
use futures::StreamExt;
use platform::auth::Caller;
use platform::db::TenantTx;
use platform::AppResult;
use uuid::Uuid;

use super::{authorize_service, non_empty_kinds};
use crate::adapters::postgres::EXPORT_SQL;
use crate::app::project_config::Overrides;
use crate::app::state::AppState;
use crate::app::{links, queries};

/// Entity resolution for one event (idempotent).
#[utoipa::path(post, path = "/v1/projects/{pid}/links", tag = "internal",
    params(("pid" = Uuid, Path, description = "Project id")),
    request_body = GraphLinksRequest,
    responses((status = 200, body = GraphLinksResponse), (status = 422, description = "validation error")))]
pub async fn links(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(req): Json<GraphLinksRequest>,
) -> AppResult<Json<GraphLinksResponse>> {
    let (tenant, project) = authorize_service(&caller, pid).await?;
    Ok(Json(links::ingest_links(&state, tenant, project, &req).await?))
}

/// All pipeline graph metrics of a customer.
#[utoipa::path(post, path = "/v1/projects/{pid}/metrics", tag = "internal",
    params(("pid" = Uuid, Path, description = "Project id")),
    request_body = GraphMetricsRequest,
    responses((status = 200, body = GraphMetrics)))]
pub async fn metrics(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(req): Json<GraphMetricsRequest>,
) -> AppResult<Json<GraphMetrics>> {
    let (tenant, project) = authorize_service(&caller, pid).await?;
    let o = Overrides {
        link_kinds: req.link_kinds.map(non_empty_kinds).transpose()?,
        include_similar: req.include_similar,
        max_depth: req.max_depth.map(u32::from),
    };
    Ok(Json(
        queries::metrics(&state, tenant, project, req.customer_id, &o).await?,
    ))
}

/// One graph-rule metric with the rule's own parameters (rule-service `graph` rules).
#[utoipa::path(post, path = "/v1/projects/{pid}/metric", tag = "internal",
    params(("pid" = Uuid, Path, description = "Project id")),
    request_body = GraphMetricRequest,
    responses((status = 200, body = GraphMetricResponse)))]
pub async fn metric(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(req): Json<GraphMetricRequest>,
) -> AppResult<Json<GraphMetricResponse>> {
    let (tenant, project) = authorize_service(&caller, pid).await?;
    let o = Overrides {
        link_kinds: Some(non_empty_kinds(req.link_kinds)?),
        include_similar: Some(req.include_similar),
        max_depth: Some(u32::from(req.max_depth)),
    };
    let value = queries::single_metric(&state, tenant, project, req.customer_id, req.metric, &o).await?;
    Ok(Json(GraphMetricResponse { value }))
}

/// Mirrors a customer's risk label (204; 404 when the customer has no graph node yet).
#[utoipa::path(put, path = "/v1/projects/{pid}/customers/{cid}/label", tag = "internal",
    params(("pid" = Uuid, Path, description = "Project id"), ("cid" = Uuid, Path, description = "Customer id")),
    request_body = CustomerLabelUpdate,
    responses((status = 204), (status = 404, description = "customer has no graph node")))]
pub async fn label(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, cid)): Path<(Uuid, Uuid)>,
    Json(req): Json<CustomerLabelUpdate>,
) -> AppResult<StatusCode> {
    let (tenant, project) = authorize_service(&caller, pid).await?;
    queries::set_label(&state, tenant, project, cid, &req.risk_label).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// NDJSON stream of customer–customer weighted edges `{"source","target","weight"}` for Louvain.
///
/// weight = number of shared (non-supernode) entities + max similarity score of similar-entity
/// pairs (only when the project's `include_similar` is on).
#[utoipa::path(get, path = "/v1/projects/{pid}/export", tag = "internal",
    params(("pid" = Uuid, Path, description = "Project id")),
    responses((status = 200, content_type = "application/x-ndjson", description = "one JSON edge per line")))]
pub async fn export(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
) -> AppResult<Response> {
    let (tenant, project) = authorize_service(&caller, pid).await?;
    // Resolve config (and 404 for an unknown project) before streaming starts.
    let cfg = {
        let mut tx = TenantTx::begin(&state.pool, tenant).await?;
        let cfg = state.configs.get(&mut tx, tenant, project).await?;
        tx.commit().await?;
        cfg
    };
    let kinds: Vec<String> = cfg.kinds().iter().map(|k| k.as_str().to_string()).collect();
    let cap = i32::try_from(cfg.supernode_cap()).unwrap_or(i32::MAX);
    let min_sim: f32 = if cfg.include_similar { cfg.threshold() } else { 2.0 };

    // A producer task owns the transaction and streams rows through a bounded channel, so memory
    // stays flat for large graphs and back-pressure comes from the HTTP client.
    let (tx_lines, rx_lines) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(256);
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let result: AppResult<()> = async {
            let mut tx = TenantTx::begin(&pool, tenant).await?;
            {
                let mut rows = sqlx::query_as::<_, (Uuid, Uuid, f64)>(EXPORT_SQL)
                    .bind(project.as_uuid())
                    .bind(cap)
                    .bind(&kinds)
                    .bind(min_sim)
                    .fetch(&mut **tx);
                while let Some(row) = rows.next().await {
                    let (s, t, w) = row?;
                    let line = format!("{{\"source\":\"{s}\",\"target\":\"{t}\",\"weight\":{w}}}\n");
                    if tx_lines.send(Ok(Bytes::from(line))).await.is_err() {
                        break; // client went away
                    }
                }
            }
            tx.commit().await?;
            Ok(())
        }
        .await;
        if let Err(e) = result {
            tracing::error!(error = %e, "graph export failed");
            let _ = tx_lines
                .send(Err(std::io::Error::other("graph export failed")))
                .await;
        }
    });
    let stream = futures::stream::unfold(rx_lines, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Ok((
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        Body::from_stream(stream),
    )
        .into_response())
}
