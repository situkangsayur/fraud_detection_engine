//! OpenAPI document (`/openapi.json`) and Scalar UI (`/docs`).
//!
//! Component schemas are derived from the Rust types (`utoipa::ToSchema`) so they cannot drift from
//! the code. Paths are listed in one table below (summary + tag + auth); per-handler annotations
//! for request/response bodies are on the backlog.

use axum::routing::get;
use axum::{Json, Router};
use utoipa::openapi::path::{HttpMethod, OperationBuilder};
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::{OpenApi as OpenApiDoc, Paths};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "core-api",
        description = "Fraud platform core API: tenants, projects, identity, data sources & \
        mapping, scoring orchestration, cases, labels, analytics, audit (docs/technical/api-contract.md §A)."
    ),
    components(schemas(
        contracts::events::CanonicalEventIn,
        contracts::events::DecisionOut,
        contracts::scoring::EvaluateRequest,
        contracts::scoring::EvaluateResponse,
        contracts::graph::GraphMetrics,
        platform::error::Problem,
        crate::application::auth::LoginIn,
        crate::application::auth::TokenPair,
        crate::application::tenants::NewTenantIn,
        crate::application::tenants::NewUserIn,
        crate::application::projects::ProjectIn,
        crate::application::data_sources::DataSourceIn,
        crate::application::ingest::BatchIn,
        crate::application::ingest::BatchOut,
        crate::application::cases::CasePatch,
        crate::application::cases::ResolveIn,
        crate::application::cases::LabelIn,
    ))
)]
struct ApiDoc;

/// (method, path, tag, summary, auth)
const ROUTES: &[(&str, &str, &str, &str, &str)] = &[
    (
        "POST",
        "/api/v1/auth/login",
        "auth",
        "Log in, returns access + refresh token",
        "none",
    ),
    (
        "POST",
        "/api/v1/auth/refresh",
        "auth",
        "Rotate refresh token",
        "refresh token",
    ),
    (
        "POST",
        "/api/v1/auth/logout",
        "auth",
        "Revoke refresh-token family",
        "JWT",
    ),
    (
        "GET",
        "/api/v1/me",
        "auth",
        "Current user, tenant and projects",
        "JWT",
    ),
    (
        "GET",
        "/api/v1/tenants",
        "tenants",
        "List tenants",
        "platform admin",
    ),
    (
        "POST",
        "/api/v1/tenants",
        "tenants",
        "Create tenant + tenant admin",
        "platform admin",
    ),
    (
        "GET",
        "/api/v1/tenants/{tid}",
        "tenants",
        "Get tenant",
        "PA or TA",
    ),
    (
        "PATCH",
        "/api/v1/tenants/{tid}",
        "tenants",
        "Update tenant",
        "PA or TA",
    ),
    (
        "GET",
        "/api/v1/tenants/{tid}/users",
        "tenants",
        "List tenant users",
        "PA or TA",
    ),
    (
        "POST",
        "/api/v1/tenants/{tid}/users",
        "tenants",
        "Create tenant user",
        "PA or TA",
    ),
    (
        "PATCH",
        "/api/v1/tenants/{tid}/users/{uid}",
        "tenants",
        "Update tenant user",
        "PA or TA",
    ),
    (
        "GET",
        "/api/v1/tenants/{tid}/audit",
        "audit",
        "Tenant audit log",
        "TA",
    ),
    (
        "GET",
        "/api/v1/projects",
        "projects",
        "Projects visible to the caller",
        "JWT",
    ),
    (
        "POST",
        "/api/v1/projects",
        "projects",
        "Create project (+settings, canonical source, template)",
        "TA",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}",
        "projects",
        "Project with configs and summary",
        "viewer or INT",
    ),
    (
        "PATCH",
        "/api/v1/projects/{pid}",
        "projects",
        "Update project",
        "project_admin",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/archive",
        "projects",
        "Archive project",
        "TA",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/members",
        "projects",
        "List members",
        "viewer",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/members",
        "projects",
        "Add/replace member {user_id, role}",
        "project_admin",
    ),
    (
        "PUT",
        "/api/v1/projects/{pid}/members/{uid}",
        "projects",
        "Set member role",
        "project_admin",
    ),
    (
        "DELETE",
        "/api/v1/projects/{pid}/members/{uid}",
        "projects",
        "Remove member",
        "project_admin",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/settings",
        "settings",
        "All settings (defaults merged)",
        "viewer",
    ),
    (
        "PUT",
        "/api/v1/projects/{pid}/settings/{key}",
        "settings",
        "Update one settings key",
        "project_admin",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/events",
        "ingest",
        "Score a canonical event",
        "analyst or INT",
    ),
    (
        "POST",
        "/api/v1/ingest/{slug}",
        "ingest",
        "Webhook: score one raw record",
        "X-Api-Key",
    ),
    (
        "POST",
        "/api/v1/ingest/{slug}/batch",
        "ingest",
        "Webhook: batch of raw records (≤1000)",
        "X-Api-Key",
    ),
    (
        "POST",
        "/v1/internal/projects/{pid}/sources/{source_id}/batch",
        "internal",
        "Batch from ingest-service",
        "INT",
    ),
    (
        "GET",
        "/v1/internal/projects/{pid}/field-catalog",
        "internal",
        "Field catalog for rule validation",
        "INT",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/score/simulate",
        "ingest",
        "Dry-run the pipeline (nothing persisted)",
        "analyst",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/events/{id}/rescore",
        "ingest",
        "Re-run engines for a stored event",
        "analyst",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/events",
        "events",
        "List events",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/events/{id}",
        "events",
        "Event detail",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/decisions/{event_id}",
        "events",
        "Decision of an event",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/customers",
        "customers",
        "List customers",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/customers/{id}",
        "customers",
        "Customer with stats",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/customers/{id}/events",
        "customers",
        "Customer events",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/cases",
        "cases",
        "List cases",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/cases/{id}",
        "cases",
        "Case detail",
        "viewer",
    ),
    (
        "PATCH",
        "/api/v1/projects/{pid}/cases/{id}",
        "cases",
        "Update case / add note",
        "analyst",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/cases/{id}/resolve",
        "cases",
        "Resolve case with a label",
        "analyst",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/labels",
        "labels",
        "List labels",
        "viewer",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/labels",
        "labels",
        "Create label",
        "analyst or INT",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/data-sources",
        "data-sources",
        "List data sources",
        "viewer",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/data-sources",
        "data-sources",
        "Create data source (API key shown once)",
        "analyst",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/data-sources/{id}",
        "data-sources",
        "Get data source",
        "viewer",
    ),
    (
        "PATCH",
        "/api/v1/projects/{pid}/data-sources/{id}",
        "data-sources",
        "Update data source",
        "analyst",
    ),
    (
        "DELETE",
        "/api/v1/projects/{pid}/data-sources/{id}",
        "data-sources",
        "Delete data source without events",
        "project_admin",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/data-sources/{id}/rotate-key",
        "data-sources",
        "Rotate webhook key",
        "project_admin",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/data-sources/{id}/mappings",
        "data-sources",
        "Mapping versions",
        "viewer",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/data-sources/{id}/mappings",
        "data-sources",
        "Create draft mapping",
        "analyst",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/data-sources/{id}/mappings/preview",
        "data-sources",
        "Preview mapping on records",
        "analyst",
    ),
    (
        "POST",
        "/api/v1/projects/{pid}/data-sources/{id}/mappings/{version}/activate",
        "data-sources",
        "Activate mapping",
        "analyst",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/data-sources/{id}/errors",
        "data-sources",
        "Dead-letter rows",
        "viewer",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/field-catalog",
        "data-sources",
        "Field catalog (built-in + source)",
        "viewer",
    ),
    (
        "PATCH",
        "/api/v1/projects/{pid}/field-catalog/{path}",
        "data-sources",
        "Update a source field",
        "project_admin",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/analytics/overview",
        "analytics",
        "Dashboard overview",
        "viewer or INT",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/analytics/drift",
        "analytics",
        "Feature drift (PSI)",
        "viewer or INT",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/analytics/typologies",
        "analytics",
        "Labelled fraud per typology/week",
        "viewer or INT",
    ),
    (
        "GET",
        "/api/v1/projects/{pid}/audit",
        "audit",
        "Project audit log",
        "approver",
    ),
];

pub fn document() -> OpenApiDoc {
    let mut doc = ApiDoc::openapi();
    doc.info.version = env!("CARGO_PKG_VERSION").to_string();
    let mut paths = Paths::new();
    for (method, path, tag, summary, auth) in ROUTES {
        let m = match *method {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            _ => HttpMethod::Delete,
        };
        let op = OperationBuilder::new()
            .tag(*tag)
            .summary(Some(*summary))
            .description(Some(format!("Auth: {auth}")))
            .build();
        paths.add_path_operation(*path, vec![m], op);
    }
    doc.paths = paths;
    if let Some(c) = doc.components.as_mut() {
        c.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
    doc
}

pub fn routes() -> Router {
    let doc = document();
    let json = doc.clone();
    Router::new()
        .route("/openapi.json", get(move || async move { Json(json) }))
        .merge(Scalar::with_url("/docs", doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_lists_all_routes() {
        let d = document();
        assert!(d.paths.paths.contains_key("/api/v1/ingest/{slug}/batch"));
        assert!(d.paths.paths.len() >= 40);
        let v = serde_json::to_value(&d).unwrap_or_default();
        assert!(v["components"]["schemas"].get("DecisionOut").is_some());
    }
}
