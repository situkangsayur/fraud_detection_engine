//! Data sources, mappings, dead-letter errors and the field catalog.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use platform::auth::{Caller, ProjectRole};
use platform::error::AppResult;
use platform::pagination::{Page, PageParams};
use platform::ProjectId;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::data_sources::{self as ds, CatalogPatch, CatalogQuery, DataSourceIn};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/{pid}/data-sources", get(list).post(create))
        .route(
            "/api/v1/projects/{pid}/data-sources/{id}",
            get(get_one).patch(update).delete(remove),
        )
        .route(
            "/api/v1/projects/{pid}/data-sources/{id}/rotate-key",
            post(rotate),
        )
        .route(
            "/api/v1/projects/{pid}/data-sources/{id}/mappings",
            get(mappings).post(create_mapping),
        )
        .route(
            "/api/v1/projects/{pid}/data-sources/{id}/mappings/preview",
            post(preview),
        )
        .route(
            "/api/v1/projects/{pid}/data-sources/{id}/mappings/{version}/activate",
            post(activate),
        )
        .route("/api/v1/projects/{pid}/data-sources/{id}/errors", get(errors))
        .route("/api/v1/projects/{pid}/field-catalog", get(catalog))
        .route("/api/v1/projects/{pid}/field-catalog/{path}", patch(patch_field))
}

async fn list(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(ds::list(&st, &caller, ProjectId(pid), &p).await?))
}

async fn create(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<DataSourceIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(ds::create(&st, &caller, ProjectId(pid), &body).await?),
    ))
}

async fn get_one(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(ds::get(&st, &caller, ProjectId(pid), id).await?))
}

async fn update(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<DataSourceIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(ds::patch(&st, &caller, ProjectId(pid), id, &body).await?))
}

async fn remove(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    ds::delete(&st, &caller, ProjectId(pid), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn rotate(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(ds::rotate_key(&st, &caller, ProjectId(pid), id).await?))
}

async fn mappings(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(ds::list_mappings(&st, &caller, ProjectId(pid), id).await?))
}

#[derive(Debug, Deserialize)]
struct MappingIn {
    mapping: Value,
}

async fn create_mapping(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<MappingIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(ds::create_mapping(&st, &caller, ProjectId(pid), id, &body.mapping).await?),
    ))
}

#[derive(Debug, Deserialize)]
struct PreviewIn {
    mapping: Value,
    records: Vec<Value>,
}

async fn preview(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<PreviewIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        ds::preview(&st, &caller, ProjectId(pid), id, &body.mapping, &body.records).await?,
    ))
}

async fn activate(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id, version)): ApiPath<(Uuid, Uuid, i32)>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        ds::activate_mapping(&st, &caller, ProjectId(pid), id, version).await?,
    ))
}

async fn errors(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(ds::list_errors(&st, &caller, ProjectId(pid), id, &p).await?))
}

async fn catalog(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<CatalogQuery>,
) -> AppResult<Json<Value>> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    Ok(Json(ds::field_catalog(&st, tenant, project, &q).await?))
}

async fn patch_field(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, path)): ApiPath<(Uuid, String)>,
    ApiJson(body): ApiJson<CatalogPatch>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        ds::patch_field(&st, &caller, ProjectId(pid), &path, &body).await?,
    ))
}
