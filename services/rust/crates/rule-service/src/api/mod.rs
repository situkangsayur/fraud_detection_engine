//! HTTP layer: routes, OpenAPI document, request/response mapping.
//!
//! Handlers are deliberately thin: *authorise → parse → call a use case → map to a DTO*. Business rules live in
//! `domain` / `rule-engine`, orchestration in `app`. This is the "controller" of a layered Java service, but the
//! wiring is plain functions registered on an axum `Router` instead of annotations and reflection.

pub mod dto;
pub mod formulas;
pub mod internal;
pub mod lists;
pub mod proposals;
pub mod rules;
pub mod rulesets;

use axum::routing::{get, post, put};
use axum::{Json, Router};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

use crate::state::AppState;

#[derive(Debug, OpenApi)]
#[openapi(
    info(title = "rule-service", version = "1.0", description = "Rules, rulesets, reference lists, evaluation, backtests and proposals (api-contract.md §B)."),
    paths(
        internal::evaluate, internal::bootstrap,
        rules::list, rules::create, rules::get, rules::update, rules::get_version, rules::validate, rules::test,
        rules::backtest_stored, rules::backtest_inline, rules::submit, rules::approve, rules::reject, rules::retire,
        rules::performance,
        rulesets::list, rulesets::create, rulesets::get, rulesets::update, rulesets::delete, rulesets::set_members,
        rulesets::submit, rulesets::approve, rulesets::reject, rulesets::retire, rulesets::backtest,
        lists::project_list, lists::project_create, lists::project_get, lists::project_patch, lists::project_delete,
        lists::project_entries, lists::project_upsert, lists::project_import, lists::project_delete_entry,
        lists::tenant_list, lists::tenant_create, lists::tenant_get, lists::tenant_patch, lists::tenant_delete,
        lists::tenant_entries, lists::tenant_upsert, lists::tenant_import, lists::tenant_delete_entry,
        formulas::evaluate,
        proposals::create, proposals::list, proposals::get, proposals::approve, proposals::reject,
    ),
    tags(
        (name = "internal", description = "Service-to-service (INT token)"),
        (name = "rules"), (name = "rulesets"), (name = "reference-lists"), (name = "formulas"), (name = "proposals")
    )
)]
pub struct ApiDoc;

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// The application router (without the standard ops/middleware stack added by `platform::server`).
pub fn router(state: AppState) -> Router {
    let p = "/api/v1/projects/{pid}";
    let t = "/api/v1/tenants/{tid}";
    Router::new()
        // internal
        .route("/v1/projects/{pid}/evaluate", post(internal::evaluate))
        .route("/v1/projects/{pid}/bootstrap", post(internal::bootstrap))
        // rules (static segments before `{id}` for clarity; the router prefers static matches anyway)
        .route(&format!("{p}/rules"), get(rules::list).post(rules::create))
        .route(&format!("{p}/rules/validate"), post(rules::validate))
        .route(&format!("{p}/rules/test"), post(rules::test))
        .route(&format!("{p}/rules/backtest"), post(rules::backtest_inline))
        .route(&format!("{p}/rules/performance"), get(rules::performance))
        .route(&format!("{p}/rules/{{id}}"), get(rules::get).put(rules::update))
        .route(
            &format!("{p}/rules/{{id}}/versions/{{v}}"),
            get(rules::get_version),
        )
        .route(
            &format!("{p}/rules/{{id}}/backtest"),
            post(rules::backtest_stored),
        )
        .route(&format!("{p}/rules/{{id}}/submit"), post(rules::submit))
        .route(&format!("{p}/rules/{{id}}/approve"), post(rules::approve))
        .route(&format!("{p}/rules/{{id}}/reject"), post(rules::reject))
        .route(&format!("{p}/rules/{{id}}/retire"), post(rules::retire))
        // rulesets
        .route(
            &format!("{p}/rulesets"),
            get(rulesets::list).post(rulesets::create),
        )
        .route(
            &format!("{p}/rulesets/{{id}}"),
            get(rulesets::get).put(rulesets::update).delete(rulesets::delete),
        )
        .route(&format!("{p}/rulesets/{{id}}/rules"), put(rulesets::set_members))
        .route(&format!("{p}/rulesets/{{id}}/submit"), post(rulesets::submit))
        .route(&format!("{p}/rulesets/{{id}}/approve"), post(rulesets::approve))
        .route(&format!("{p}/rulesets/{{id}}/reject"), post(rulesets::reject))
        .route(&format!("{p}/rulesets/{{id}}/retire"), post(rulesets::retire))
        .route(&format!("{p}/rulesets/{{id}}/backtest"), post(rulesets::backtest))
        // reference lists (project)
        .route(
            &format!("{p}/reference-lists"),
            get(lists::project_list).post(lists::project_create),
        )
        .route(
            &format!("{p}/reference-lists/{{id}}"),
            get(lists::project_get)
                .patch(lists::project_patch)
                .delete(lists::project_delete),
        )
        .route(
            &format!("{p}/reference-lists/{{id}}/entries"),
            get(lists::project_entries).post(lists::project_upsert),
        )
        .route(
            &format!("{p}/reference-lists/{{id}}/import"),
            post(lists::project_import),
        )
        .route(
            &format!("{p}/reference-lists/{{id}}/entries/{{entry_id}}"),
            axum::routing::delete(lists::project_delete_entry),
        )
        // reference lists (tenant-wide)
        .route(
            &format!("{t}/reference-lists"),
            get(lists::tenant_list).post(lists::tenant_create),
        )
        .route(
            &format!("{t}/reference-lists/{{id}}"),
            get(lists::tenant_get)
                .patch(lists::tenant_patch)
                .delete(lists::tenant_delete),
        )
        .route(
            &format!("{t}/reference-lists/{{id}}/entries"),
            get(lists::tenant_entries).post(lists::tenant_upsert),
        )
        .route(
            &format!("{t}/reference-lists/{{id}}/import"),
            post(lists::tenant_import),
        )
        .route(
            &format!("{t}/reference-lists/{{id}}/entries/{{entry_id}}"),
            axum::routing::delete(lists::tenant_delete_entry),
        )
        // formulas & proposals
        .route(&format!("{p}/formulas/evaluate"), post(formulas::evaluate))
        .route(
            &format!("{p}/proposals"),
            get(proposals::list).post(proposals::create),
        )
        .route(&format!("{p}/proposals/{{id}}"), get(proposals::get))
        .route(&format!("{p}/proposals/{{id}}/approve"), post(proposals::approve))
        .route(&format!("{p}/proposals/{{id}}/reject"), post(proposals::reject))
        // docs
        .route("/openapi.json", get(openapi_json))
        .merge(Scalar::with_url("/docs", ApiDoc::openapi()))
        .with_state(state)
}
