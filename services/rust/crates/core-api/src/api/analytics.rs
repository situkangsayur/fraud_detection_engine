//! Analytics and project audit log.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use platform::auth::{Caller, ProjectRole};
use platform::error::AppResult;
use platform::pagination::{Page, PageParams};
use platform::ProjectId;
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiPath, ApiQuery};
use crate::application::analytics::{self, Range};
use crate::application::queries::{list_audit, AuditFilter};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/{pid}/analytics/overview", get(overview))
        .route("/api/v1/projects/{pid}/analytics/drift", get(drift))
        .route("/api/v1/projects/{pid}/analytics/typologies", get(typologies))
        .route("/api/v1/projects/{pid}/audit", get(audit))
}

async fn overview(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(r): ApiQuery<Range>,
) -> AppResult<Json<Value>> {
    Ok(Json(analytics::overview(&st, &caller, ProjectId(pid), &r).await?))
}

async fn drift(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(analytics::drift(&st, &caller, ProjectId(pid)).await?))
}

async fn typologies(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(r): ApiQuery<Range>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        analytics::typologies(&st, &caller, ProjectId(pid), &r).await?,
    ))
}

async fn audit(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<AuditFilter>,
) -> AppResult<Json<Page<Value>>> {
    let project = ProjectId(pid);
    let tenant = caller
        .require_project_role(project, ProjectRole::Approver)
        .await?;
    Ok(Json(list_audit(&st, tenant, Some(project), &f, &p).await?))
}
