//! `/api/v1/projects/*`: projects, members, settings.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use platform::auth::Caller;
use platform::error::AppResult;
use platform::pagination::{Page, PageParams};
use platform::ProjectId;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::projects::{self, ProjectIn};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/projects", get(list).post(create))
        .route("/api/v1/projects/{pid}", get(get_one).patch(update))
        .route("/api/v1/projects/{pid}/archive", post(archive))
        .route("/api/v1/projects/{pid}/members", get(members).post(add_member))
        .route(
            "/api/v1/projects/{pid}/members/{uid}",
            put(put_member).delete(remove_member),
        )
        .route("/api/v1/projects/{pid}/settings", get(settings))
        .route("/api/v1/projects/{pid}/settings/{key}", put(put_setting))
}

async fn list(
    State(st): State<AppState>,
    caller: Caller,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(projects::list(&st, &caller, &p).await?))
}

async fn create(
    State(st): State<AppState>,
    caller: Caller,
    ApiJson(body): ApiJson<ProjectIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(projects::create(&st, &caller, &body).await?),
    ))
}

async fn get_one(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(projects::get(&st, &caller, ProjectId(pid)).await?))
}

async fn update(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ProjectIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(projects::patch(&st, &caller, ProjectId(pid), &body).await?))
}

async fn archive(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(projects::archive(&st, &caller, ProjectId(pid)).await?))
}

async fn members(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(projects::list_members(&st, &caller, ProjectId(pid)).await?))
}

#[derive(Debug, Deserialize)]
struct MemberIn {
    user_id: Uuid,
    role: String,
}

#[derive(Debug, Deserialize)]
struct RoleIn {
    role: String,
}

async fn add_member(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<MemberIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        projects::upsert_member(&st, &caller, ProjectId(pid), body.user_id, &body.role).await?,
    ))
}

async fn put_member(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, uid)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<RoleIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        projects::upsert_member(&st, &caller, ProjectId(pid), uid, &body.role).await?,
    ))
}

async fn remove_member(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, uid)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    projects::remove_member(&st, &caller, ProjectId(pid), uid).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn settings(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(projects::get_settings(&st, &caller, ProjectId(pid)).await?))
}

async fn put_setting(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, key)): ApiPath<(Uuid, String)>,
    ApiJson(value): ApiJson<Value>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        projects::put_setting(&st, &caller, ProjectId(pid), &key, &value).await?,
    ))
}
