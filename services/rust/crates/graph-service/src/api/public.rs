//! Read endpoints for the UI graph explorer (JWT with project role ≥ viewer) and for internal
//! service callers such as llm-service tools (internal token + X-Tenant-Id/X-Project-Id).

use axum::extract::{Path, Query, State};
use axum::Json;
use platform::auth::Caller;
use platform::AppResult;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::{authorize_viewer, parse_kinds};
use crate::app::project_config::Overrides;
use crate::app::queries::{self, NeighborhoodOut, ProximityOut, SearchOut, StatsOut};
use crate::app::state::AppState;
use crate::domain::components::Component;
use crate::domain::model::limits;

#[derive(Debug, Deserialize, IntoParams)]
pub struct NeighborhoodQuery {
    /// Customer hops (1–3, default 2).
    pub depth: Option<u32>,
    /// Comma-separated link kinds (default: project config).
    pub link_kinds: Option<String>,
    pub include_similar: Option<bool>,
    /// Maximum nodes returned (≤ 300, default 150).
    pub limit_nodes: Option<usize>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/graph/customers/{cid}/neighborhood", tag = "graph",
    params(("pid" = Uuid, Path), ("cid" = Uuid, Path), NeighborhoodQuery),
    responses((status = 200, body = NeighborhoodOut), (status = 404, description = "customer not in graph")))]
pub async fn neighborhood(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, cid)): Path<(Uuid, Uuid)>,
    Query(q): Query<NeighborhoodQuery>,
) -> AppResult<Json<NeighborhoodOut>> {
    let (tenant, project) = authorize_viewer(&caller, pid).await?;
    let o = Overrides {
        link_kinds: parse_kinds(q.link_kinds.as_deref())?,
        include_similar: q.include_similar,
        max_depth: Some(q.depth.unwrap_or(2).clamp(1, 3)),
    };
    let limit = q.limit_nodes.unwrap_or(150).min(limits::NEIGHBORHOOD_MAX_NODES);
    Ok(Json(
        queries::neighborhood(&state, tenant, project, cid, &o, limit).await?,
    ))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ProximityQuery {
    /// Customer hops (1–4, default project config).
    pub max_depth: Option<u32>,
    pub link_kinds: Option<String>,
    pub include_similar: Option<bool>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/graph/customers/{cid}/fraud-proximity", tag = "graph",
    params(("pid" = Uuid, Path), ("cid" = Uuid, Path), ProximityQuery),
    responses((status = 200, body = ProximityOut), (status = 404, description = "customer not in graph")))]
pub async fn fraud_proximity(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, cid)): Path<(Uuid, Uuid)>,
    Query(q): Query<ProximityQuery>,
) -> AppResult<Json<ProximityOut>> {
    let (tenant, project) = authorize_viewer(&caller, pid).await?;
    let o = Overrides {
        link_kinds: parse_kinds(q.link_kinds.as_deref())?,
        include_similar: q.include_similar,
        max_depth: q.max_depth,
    };
    Ok(Json(
        queries::fraud_proximity(&state, tenant, project, cid, &o).await?,
    ))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ComponentsQuery {
    /// Minimum component size (default 3).
    pub min_size: Option<u32>,
    /// Only components containing at least one fraud customer.
    pub only_with_fraud: Option<bool>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/graph/components", tag = "graph",
    params(("pid" = Uuid, Path), ComponentsQuery),
    responses((status = 200, body = [Component])))]
pub async fn components(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<ComponentsQuery>,
) -> AppResult<Json<Vec<Component>>> {
    let (tenant, project) = authorize_viewer(&caller, pid).await?;
    Ok(Json(
        queries::components(
            &state,
            tenant,
            project,
            q.min_size.unwrap_or(3),
            q.only_with_fraud.unwrap_or(false),
        )
        .await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/graph/stats", tag = "graph",
    params(("pid" = Uuid, Path)),
    responses((status = 200, body = StatsOut)))]
pub async fn stats(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
) -> AppResult<Json<StatsOut>> {
    let (tenant, project) = authorize_viewer(&caller, pid).await?;
    Ok(Json(queries::stats(&state, tenant, project).await?))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Customer external-id prefix, or an exact email / phone / IP / device id / ref id.
    pub q: String,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/graph/search", tag = "graph",
    params(("pid" = Uuid, Path), SearchQuery),
    responses((status = 200, body = SearchOut)))]
pub async fn search(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<SearchOut>> {
    let (tenant, project) = authorize_viewer(&caller, pid).await?;
    Ok(Json(queries::search(&state, tenant, project, &q.q).await?))
}
