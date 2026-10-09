//! Internal endpoints (INT token only): `/v1/projects/{pid}/evaluate` and `/v1/projects/{pid}/bootstrap`.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::Json;
use contracts::scoring::{EvaluateRequest, EvaluateResponse};
use platform::auth::{Caller, ProjectRole};
use platform::telemetry::RequestId;
use platform::{AppError, AppResult};
use serde::Deserialize;
use uuid::Uuid;

use super::dto::{call_ctx, project_scope};
use crate::app::evaluation;
use crate::app::workflow::{self, BootstrapReport};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EvaluateQuery {
    /// Per-rule time budget override in milliseconds (1..=5000).
    pub rule_timeout_ms: Option<u64>,
}

#[utoipa::path(post, path = "/v1/projects/{pid}/evaluate", params(("pid" = Uuid, Path), EvaluateQuery),
    request_body = EvaluateRequest, responses((status = 200, body = EvaluateResponse)), tag = "internal")]
pub async fn evaluate(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path(pid): Path<Uuid>,
    Query(q): Query<EvaluateQuery>,
    Json(req): Json<EvaluateRequest>,
) -> AppResult<Json<EvaluateResponse>> {
    caller.require_service()?;
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let timeout = match q.rule_timeout_ms {
        Some(ms) if (1..=5_000).contains(&ms) => Some(Duration::from_millis(ms)),
        Some(_) => return Err(AppError::field("rule_timeout_ms", "must be between 1 and 5000")),
        None => None,
    };
    let response = evaluation::evaluate(
        &state,
        tenant,
        project,
        &req,
        call_ctx(tenant, project, &caller, &rid),
        timeout,
    )
    .await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BootstrapBody {
    /// Stage template: pre_payment, post_payment, returns, promo, account_security, payout.
    pub template: String,
}

#[utoipa::path(post, path = "/v1/projects/{pid}/bootstrap", params(("pid" = Uuid, Path)),
    request_body = BootstrapBody, responses((status = 200, body = BootstrapReport)), tag = "internal")]
pub async fn bootstrap(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(body): Json<BootstrapBody>,
) -> AppResult<Json<BootstrapReport>> {
    caller.require_service()?;
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::ProjectAdmin).await?;
    Ok(Json(
        workflow::bootstrap(&state, &caller, tenant, project, &body.template).await?,
    ))
}
