//! `/api/v1/tenants/*` (platform admin / tenant admin).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use platform::auth::Caller;
use platform::error::AppResult;
use platform::pagination::{Page, PageParams};
use platform::TenantId;
use serde_json::Value;
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::queries::{list_audit, AuditFilter};
use crate::application::tenants::{self, NewTenantIn, NewUserIn, PatchTenantIn, PatchUserIn};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tenants", get(list).post(create))
        .route("/api/v1/tenants/{tid}", get(get_one).patch(update))
        .route("/api/v1/tenants/{tid}/users", get(list_users).post(create_user))
        .route("/api/v1/tenants/{tid}/users/{uid}", patch(update_user))
        .route("/api/v1/tenants/{tid}/audit", get(audit))
}

async fn list(
    State(st): State<AppState>,
    caller: Caller,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(tenants::list(&st, &caller, &p).await?))
}

async fn create(
    State(st): State<AppState>,
    caller: Caller,
    ApiJson(body): ApiJson<NewTenantIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(tenants::create(&st, &caller, &body).await?),
    ))
}

async fn get_one(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(tid): ApiPath<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(tenants::get(&st, &caller, TenantId(tid)).await?))
}

async fn update(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(tid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PatchTenantIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(tenants::patch(&st, &caller, TenantId(tid), &body).await?))
}

async fn list_users(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(tid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(tenants::list_users(&st, &caller, TenantId(tid), &p).await?))
}

async fn create_user(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(tid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<NewUserIn>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(tenants::create_user(&st, &caller, TenantId(tid), &body).await?),
    ))
}

async fn update_user(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((tid, uid)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<PatchUserIn>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        tenants::patch_user(&st, &caller, TenantId(tid), uid, &body).await?,
    ))
}

async fn audit(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(tid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<AuditFilter>,
) -> AppResult<Json<Page<Value>>> {
    let tenant = TenantId(tid);
    caller.require_tenant_admin(tenant)?;
    Ok(Json(list_audit(&st, tenant, None, &f, &p).await?))
}
