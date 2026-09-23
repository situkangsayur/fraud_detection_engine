//! OpenAPI document (`/openapi.json`, Scalar UI at `/docs`), generated from the handler annotations.

use axum::Json;
use utoipa::OpenApi;

use super::{internal, public};

#[derive(Debug, OpenApi)]
#[openapi(
    info(title = "graph-service", description = "Graph engine: entity resolution, graph metrics and exploration."),
    paths(
        internal::links, internal::metrics, internal::metric, internal::label, internal::export,
        public::neighborhood, public::fraud_proximity, public::components, public::stats, public::search,
    ),
    components(schemas(
        contracts::graph::LinkKind, contracts::graph::GraphCustomer, contracts::graph::GraphEventLinks,
        contracts::graph::GraphLinksRequest, contracts::graph::GraphLinksResponse,
        contracts::graph::GraphMetricsRequest, contracts::graph::GraphMetrics,
        contracts::graph::GraphMetric, contracts::graph::GraphMetricRequest,
        contracts::graph::GraphMetricResponse, contracts::graph::CustomerLabelUpdate,
        crate::app::queries::NeighborhoodOut, crate::app::queries::GraphNode, crate::app::queries::GraphEdge,
        crate::app::queries::ProximityOut, crate::app::queries::PathNode, crate::app::queries::StatsOut,
        crate::app::queries::SupernodeOut, crate::app::queries::SearchOut, crate::app::queries::CustomerHit,
        crate::app::queries::EntityHit, crate::domain::components::Component,
    )),
    tags(
        (name = "internal", description = "Service-to-service (internal token + X-Tenant-Id/X-Project-Id)"),
        (name = "graph", description = "Graph explorer (JWT project viewer, or internal token)"),
    )
)]
pub struct ApiDoc;

pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
