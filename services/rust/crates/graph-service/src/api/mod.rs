//! # API layer — HTTP adapter (driving side of the hexagon)
//!
//! Handlers are thin: extract → authorise → call a use case → serialise. Two audiences:
//!
//! * `/v1/projects/{pid}/…`: **internal** endpoints for core-api, rule-service and ml-service,
//!   authenticated with the internal token plus tenant/project headers. The gateway never routes
//!   `/v1/...`.
//! * `/api/v1/projects/{pid}/graph/…`: **user** endpoints for the UI, authorised by the
//!   project role in the JWT (viewer or above).
//!
//! Authentication is an axum *extractor* (`Caller`): a handler that declares it cannot run
//! unauthenticated. This is the compile-time equivalent of a Spring security filter chain.

pub mod internal;
pub mod openapi;
pub mod public;

use axum::routing::{get, post, put};
use axum::Router;
use contracts::graph::LinkKind;
use platform::auth::{Caller, ProjectRole};
use platform::{AppError, AppResult, ProjectId, TenantId};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};
use uuid::Uuid;

use crate::app::state::AppState;

/// Builds the service router (without the platform's standard layers; see `platform::server`).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/projects/{pid}/links", post(internal::links))
        .route("/v1/projects/{pid}/metrics", post(internal::metrics))
        .route("/v1/projects/{pid}/metric", post(internal::metric))
        .route("/v1/projects/{pid}/customers/{cid}/label", put(internal::label))
        .route("/v1/projects/{pid}/export", get(internal::export))
        .route(
            "/api/v1/projects/{pid}/graph/customers/{cid}/neighborhood",
            get(public::neighborhood),
        )
        .route(
            "/api/v1/projects/{pid}/graph/customers/{cid}/fraud-proximity",
            get(public::fraud_proximity),
        )
        .route("/api/v1/projects/{pid}/graph/components", get(public::components))
        .route("/api/v1/projects/{pid}/graph/stats", get(public::stats))
        .route("/api/v1/projects/{pid}/graph/search", get(public::search))
        .route("/openapi.json", get(openapi::openapi_json))
        .merge(Scalar::with_url("/docs", openapi::ApiDoc::openapi()))
        .with_state(state)
}

/// Internal endpoints: service token only; `X-Project-Id` (if sent) must match the path.
async fn authorize_service(caller: &Caller, pid: Uuid) -> AppResult<(TenantId, ProjectId)> {
    caller.require_service()?;
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    Ok((tenant, project))
}

/// Read endpoints: a user with at least `viewer` on the project, or an internal service caller
/// (e.g. llm-service tools) whose `X-Project-Id`, if sent, matches the path.
async fn authorize_viewer(caller: &Caller, pid: Uuid) -> AppResult<(TenantId, ProjectId)> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    Ok((tenant, project))
}

/// Parses a comma-separated `link_kinds` query value.
fn parse_kinds(raw: Option<&str>) -> AppResult<Option<Vec<LinkKind>>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    raw.split(',')
        .map(|k| {
            LinkKind::parse(k.trim())
                .ok_or_else(|| AppError::field("link_kinds", format!("unknown link kind `{}`", k.trim())))
        })
        .collect::<AppResult<Vec<_>>>()
        .map(Some)
}

/// Validates an explicit (non-empty) list of kinds from a JSON body.
fn non_empty_kinds(kinds: Vec<LinkKind>) -> AppResult<Vec<LinkKind>> {
    if kinds.is_empty() {
        Err(AppError::field("link_kinds", "must not be empty"))
    } else {
        Ok(kinds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_parsing() {
        assert_eq!(parse_kinds(None).ok(), Some(None));
        assert_eq!(
            parse_kinds(Some("card, phone")).ok(),
            Some(Some(vec![LinkKind::Card, LinkKind::Phone]))
        );
        assert!(parse_kinds(Some("card,bogus")).is_err());
        assert!(non_empty_kinds(vec![]).is_err());
    }
}
