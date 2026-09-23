//! Cases and labels.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use platform::auth::Caller;
use platform::error::AppResult;
use platform::pagination::{Page, PageParams};
use platform::ProjectId;
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::cases::{self, CaseFilter, CasePatch, LabelFilter, LabelIn, ResolveIn};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects/{pid}/cases", get(list))
        .route("/api/v1/projects/{pid}/cases/{id}", get(get_one).patch(update))
        .route("/api/v1/projects/{pid}/cases/{id}/resolve", post(resolve))
        .route(
            "/api/v1/projects/{pid}/labels",
            get(list_labels).post(create_label),
        )
}

async fn list(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<CaseFilter>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(cases::list(&st, &caller, ProjectId(pid), &f, &p).await?))
}

async fn get_one(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(cases::get(&st, &caller, ProjectId(pid), id).await?))
}

async fn update(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<CasePatch>,
) -> AppResult<Json<Value>> {
    Ok(Json(cases::patch(&st, &caller, ProjectId(pid), id, &body).await?))
}

async fn resolve(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<ResolveIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        cases::resolve(&st, &caller, ProjectId(pid), id, &body).await?,
    ))
}

async fn create_label(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<LabelIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(cases::create_label(&st, &caller, ProjectId(pid), &body).await?),
    ))
}

async fn list_labels(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<LabelFilter>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(
        cases::list_labels(&st, &caller, ProjectId(pid), &f, &p).await?,
    ))
}
