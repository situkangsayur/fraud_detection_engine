//! Error handling: one error type per service boundary, rendered as RFC 7807 `problem+json`.
//!
//! ## Why `Result<T, AppError>` instead of exceptions
//!
//! Rust has no exceptions. A function that can fail returns `Result<T, E>`, and the `?` operator
//! propagates the error to the caller — like a checked exception, but explicit at every call site
//! and with zero hidden control flow. `AppError` is the error type used by HTTP handlers.
//! Because it implements axum's `IntoResponse`, a handler can return `AppResult<Json<T>>`, and the
//! failure branch automatically becomes a proper problem+json response. This works like a Java
//! `@ControllerAdvice`, except that it is resolved at compile time.
//!
//! Internal errors are **logged with detail but returned without detail**, so SQL or stack
//! information never leaks to clients.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A single field-level validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// RFC 7807 problem document.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("validation failed")]
    Validation {
        detail: Option<String>,
        errors: Vec<FieldError>,
    },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    /// A downstream service failed or timed out.
    #[error("upstream service `{service}` failed: {detail}")]
    Upstream { service: String, detail: String },
    #[error("service unavailable: {0}")]
    Unavailable(String),
    /// Anything unexpected. The inner error is logged, never returned to the client.
    #[error("internal error: {0:#}")]
    Internal(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn validation(errors: Vec<FieldError>) -> Self {
        Self::Validation { detail: None, errors }
    }

    pub fn field(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::validation(vec![FieldError::new(field, message)])
    }

    pub fn not_found(what: impl Into<String>) -> Self {
        Self::NotFound(what.into())
    }

    pub fn internal(msg: impl std::fmt::Display) -> Self {
        Self::Internal(anyhow::anyhow!("{msg}"))
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Validation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Upstream { .. } => StatusCode::BAD_GATEWAY,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable problem type slug.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not-found",
            Self::Validation { .. } => "validation-error",
            Self::BadRequest(_) => "bad-request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Conflict(_) => "conflict",
            Self::Upstream { .. } => "upstream-error",
            Self::Unavailable(_) => "unavailable",
            Self::Internal(_) => "internal-error",
        }
    }

    /// Converts to a problem document. Internal details are replaced by a generic message.
    pub fn to_problem(&self) -> Problem {
        let status = self.status();
        let (detail, errors) = match self {
            Self::NotFound(m)
            | Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::Conflict(m)
            | Self::Unavailable(m) => (Some(m.clone()), vec![]),
            Self::Validation { detail, errors } => (detail.clone(), errors.clone()),
            Self::Upstream { service, .. } => {
                (Some(format!("dependency `{service}` is unavailable")), vec![])
            }
            Self::Internal(_) => (Some("an unexpected error occurred".to_string()), vec![]),
        };
        Problem {
            type_: format!("https://fraud-platform.local/problems/{}", self.kind()),
            title: status.canonical_reason().unwrap_or("Error").to_string(),
            status: status.as_u16(),
            detail,
            errors,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            Self::Internal(e) => tracing::error!(error = ?e, "internal error"),
            Self::Upstream { service, detail } => tracing::warn!(%service, %detail, "upstream error"),
            _ => tracing::debug!(error = %self, "request failed"),
        }
        let problem = self.to_problem();
        let status = self.status();
        let body = serde_json::to_vec(&problem).unwrap_or_default();
        (status, [(header::CONTENT_TYPE, "application/problem+json")], body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => Self::NotFound("resource not found".into()),
            sqlx::Error::Database(db) => match db.code().as_deref() {
                // unique_violation
                Some("23505") => Self::Conflict(
                    db.constraint()
                        .map(|c| format!("duplicate value violates `{c}`"))
                        .unwrap_or_else(|| "duplicate value".into()),
                ),
                // foreign_key_violation
                Some("23503") => {
                    Self::Conflict("referenced resource does not exist or is still in use".into())
                }
                // check_violation
                Some("23514") => Self::BadRequest(
                    db.constraint()
                        .map(|c| format!("value violates constraint `{c}`"))
                        .unwrap_or_else(|| "value violates a constraint".into()),
                ),
                // insufficient_privilege / RLS violation
                Some("42501") => Self::Forbidden("operation not permitted".into()),
                _ => Self::Internal(e.into()),
            },
            sqlx::Error::PoolTimedOut => Self::Unavailable("database pool exhausted".into()),
            _ => Self::Internal(e.into()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::BadRequest(format!("invalid JSON: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn validation_maps_to_422_with_field_errors() {
        let err = AppError::field("definition.when", "must not be empty");
        let p = err.to_problem();
        assert_eq!(p.status, 422);
        assert_eq!(
            p.errors,
            vec![FieldError::new("definition.when", "must not be empty")]
        );
        assert!(p.type_.ends_with("/validation-error"));
    }

    #[test]
    fn internal_errors_do_not_leak_detail() {
        let err = AppError::Internal(anyhow::anyhow!("password=secret in SQL"));
        let p = err.to_problem();
        assert_eq!(p.status, 500);
        assert_eq!(p.detail.as_deref(), Some("an unexpected error occurred"));
    }

    #[test]
    fn row_not_found_is_404() {
        let err: AppError = sqlx::Error::RowNotFound.into();
        assert_eq!(err.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn upstream_is_502_and_names_service_only() {
        let err = AppError::Upstream {
            service: "ml-service".into(),
            detail: "connect refused 10.0.0.3".into(),
        };
        let p = err.to_problem();
        assert_eq!(p.status, 502);
        assert_eq!(
            p.detail.as_deref(),
            Some("dependency `ml-service` is unavailable")
        );
    }

    #[test]
    fn response_has_problem_content_type() {
        let resp = AppError::NotFound("rule".into()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json")
        );
    }
}
