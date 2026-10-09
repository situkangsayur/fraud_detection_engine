//! Internal (service-to-service) endpoints: `/v1/internal/...`, never routed by the gateway.
//!
//! [`InternalCaller`] authenticates the internal token itself instead of `platform::auth::Caller`
//! because ingest-service sends a non-UUID `X-Actor` (e.g. `ingest-service:poller`) for scheduled
//! pulls; here a non-UUID actor is accepted and recorded as a service actor.

use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use platform::auth::{HEADER_ACTOR, HEADER_PROJECT, HEADER_TENANT};
use platform::error::{AppError, AppResult};
use platform::telemetry::RequestId;
use platform::{ProjectId, TenantId, UserId};
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::data_sources::{field_catalog, CatalogQuery};
use crate::application::ingest::{process_batch, BatchIn, BatchOut};
use crate::application::pipeline::IngestCtx;
use crate::state::AppState;

/// A verified internal caller.
#[derive(Debug, Clone)]
pub struct InternalCaller {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
    pub actor: Option<UserId>,
    pub actor_label: Option<String>,
}

impl InternalCaller {
    /// Checks that `X-Project-Id` (when sent) matches the path project.
    pub fn for_project(&self, project: ProjectId) -> AppResult<TenantId> {
        match self.project {
            Some(p) if p != project => Err(AppError::Forbidden(
                "X-Project-Id does not match the requested project".into(),
            )),
            _ => Ok(self.tenant),
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
}

impl<S> FromRequestParts<S> for InternalCaller
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let st = AppState::from_ref(state);
        let token = header(parts, "authorization")
            .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
            .ok_or_else(|| AppError::Unauthorized("missing internal token".into()))?;
        if !constant_time_eq(token.as_bytes(), st.cfg.internal_token.expose().as_bytes()) {
            return Err(AppError::Unauthorized("invalid internal token".into()));
        }
        let tenant = header(parts, HEADER_TENANT)
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or_else(|| AppError::BadRequest("internal call requires a valid X-Tenant-Id".into()))?;
        let project = match header(parts, HEADER_PROJECT) {
            None => None,
            Some(v) => {
                Some(Uuid::parse_str(v).map_err(|_| AppError::BadRequest("invalid X-Project-Id".into()))?)
            }
        };
        let actor_raw = header(parts, HEADER_ACTOR).map(String::from);
        Ok(Self {
            tenant: TenantId(tenant),
            project: project.map(ProjectId),
            actor: actor_raw
                .as_deref()
                .and_then(|a| Uuid::parse_str(a).ok())
                .map(UserId),
            actor_label: actor_raw,
        })
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/internal/projects/{pid}/sources/{source_id}/batch",
            post(internal_batch),
        )
        .route("/v1/internal/projects/{pid}/field-catalog", get(internal_catalog))
}

async fn internal_batch(
    State(st): State<AppState>,
    caller: InternalCaller,
    rid: RequestId,
    ApiPath((pid, source_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<BatchIn>,
) -> AppResult<(StatusCode, Json<BatchOut>)> {
    let project = ProjectId(pid);
    let tenant = caller.for_project(project)?;
    let ictx = IngestCtx {
        tenant,
        project,
        actor: caller.actor,
        request_id: Some(rid.0),
        job_id: body.job_id,
    };
    let out = process_batch(&st, &ictx, source_id, &body).await?;
    Ok((StatusCode::OK, Json(out)))
}

async fn internal_catalog(
    State(st): State<AppState>,
    caller: InternalCaller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<CatalogQuery>,
) -> AppResult<Json<Value>> {
    let project = ProjectId(pid);
    let tenant = caller.for_project(project)?;
    Ok(Json(field_catalog(&st, tenant, project, &q).await?))
}
