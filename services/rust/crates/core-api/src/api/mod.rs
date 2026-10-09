//! # HTTP API (axum routers, DTO extraction, OpenAPI)
//!
//! Handlers are thin: extract → authorise (via the `Caller` extractor + `require_project_role`) →
//! call one application function → return JSON. Business logic never lives here.
//!
//! [`ApiJson`], [`ApiQuery`] and [`ApiPath`] wrap axum's extractors so that malformed input yields
//! an RFC 7807 problem document instead of axum's plain-text rejection.

pub mod analytics;
pub mod auth;
pub mod cases;
pub mod data_sources;
pub mod events;
pub mod internal;
pub mod openapi;
pub mod projects;
pub mod tenants;

use axum::extract::{FromRequest, FromRequestParts, Path, Query, Request};
use axum::http::request::Parts;
use axum::Router;
use platform::error::AppError;
use serde::de::DeserializeOwned;

use crate::state::AppState;

/// JSON body with problem+json rejections.
#[derive(Debug, Clone)]
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Self(v)),
            Err(rej) => Err(AppError::BadRequest(rej.body_text())),
        }
    }
}

/// Query string with problem+json rejections.
#[derive(Debug, Clone)]
pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(v)) => Ok(Self(v)),
            Err(rej) => Err(AppError::BadRequest(rej.body_text())),
        }
    }
}

/// Path parameters with problem+json rejections (e.g. a malformed UUID → 400).
#[derive(Debug, Clone)]
pub struct ApiPath<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(v)) => Ok(Self(v)),
            Err(rej) => Err(AppError::BadRequest(rej.body_text())),
        }
    }
}

/// All routes of core-api (health/metrics are added by `platform::server`).
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(auth::routes())
        .merge(tenants::routes())
        .merge(projects::routes())
        .merge(events::routes())
        .merge(cases::routes())
        .merge(data_sources::routes())
        .merge(analytics::routes())
        .merge(internal::routes())
        .with_state(state)
        .merge(openapi::routes())
}
