//! Ingest (webhook + canonical), simulate, rescore, events, decisions, customers.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use contracts::events::DecisionOut;
use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, ProjectRole};
use platform::error::{AppError, AppResult};
use platform::pagination::{Page, PageParams};
use platform::telemetry::RequestId;
use platform::ProjectId;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::{ApiJson, ApiPath, ApiQuery};
use crate::application::context::source_cfg;
use crate::application::ingest::{authenticate_key, process_batch, resolve_mode, BatchIn, BatchOut};
use crate::application::pipeline::{
    process_record, rescore, resolve_source, simulate, IngestCtx, IngestMode, RecordError, RecordOutcome,
};
use crate::application::queries::{self, CustomerFilter, EventFilter};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ingest/{slug}", post(ingest_one))
        .route("/api/v1/ingest/{slug}/batch", post(ingest_batch))
        .route("/api/v1/projects/{pid}/events", get(list_events).post(post_event))
        .route("/api/v1/projects/{pid}/events/{id}", get(get_event))
        .route("/api/v1/projects/{pid}/events/{id}/rescore", post(rescore_event))
        .route("/api/v1/projects/{pid}/score/simulate", post(simulate_event))
        .route("/api/v1/projects/{pid}/decisions/{event_id}", get(get_decision))
        .route("/api/v1/projects/{pid}/customers", get(list_customers))
        .route("/api/v1/projects/{pid}/customers/{id}", get(get_customer))
        .route(
            "/api/v1/projects/{pid}/customers/{id}/events",
            get(customer_events),
        )
}

fn api_key(h: &HeaderMap) -> AppResult<&str> {
    h.get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Unauthorized("missing X-Api-Key".into()))
}

fn outcome_response(o: RecordOutcome) -> Response {
    match o {
        RecordOutcome::Decided(d) => (StatusCode::CREATED, Json(*d)).into_response(),
        RecordOutcome::Loaded { event_id, external_id } => (
            StatusCode::ACCEPTED,
            Json(json!({ "event_id": event_id, "external_id": external_id, "decision": null, "persisted": true })),
        )
            .into_response(),
    }
}

fn record_error(e: RecordError) -> AppError {
    match e {
        RecordError::Invalid(errors) => AppError::Validation {
            detail: Some("record rejected (mapping/validation)".into()),
            errors,
        },
        RecordError::Failed(e) => e,
    }
}

#[derive(Debug, Default, Deserialize)]
struct ModeQuery {
    mode: Option<String>,
}

async fn ingest_one(
    State(st): State<AppState>,
    headers: HeaderMap,
    rid: RequestId,
    ApiPath(slug): ApiPath<String>,
    ApiQuery(q): ApiQuery<ModeQuery>,
    ApiJson(raw): ApiJson<Value>,
) -> AppResult<Response> {
    let auth = authenticate_key(&st, &slug, api_key(&headers)?).await?;
    let source = source_cfg(&st, auth.tenant, auth.project, auth.source_id).await?;
    let mode = resolve_mode(q.mode.as_deref(), &source)?;
    let ictx = IngestCtx {
        tenant: auth.tenant,
        project: auth.project,
        actor: None,
        request_id: Some(rid.0),
        job_id: None,
    };
    let out = process_record(&st, &ictx, &source, &raw, mode)
        .await
        .map_err(record_error)?;
    Ok(outcome_response(out))
}

async fn ingest_batch(
    State(st): State<AppState>,
    headers: HeaderMap,
    rid: RequestId,
    ApiPath(slug): ApiPath<String>,
    ApiJson(body): ApiJson<BatchIn>,
) -> AppResult<Json<BatchOut>> {
    let auth = authenticate_key(&st, &slug, api_key(&headers)?).await?;
    let ictx = IngestCtx {
        tenant: auth.tenant,
        project: auth.project,
        actor: None,
        request_id: Some(rid.0),
        job_id: body.job_id,
    };
    Ok(Json(process_batch(&st, &ictx, auth.source_id, &body).await?))
}

async fn post_event(
    State(st): State<AppState>,
    caller: Caller,
    rid: RequestId,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(raw): ApiJson<Value>,
) -> AppResult<Response> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let source = resolve_source(&st, tenant, project, None).await?;
    let ictx = IngestCtx {
        tenant,
        project,
        actor: caller.actor_user_id(),
        request_id: Some(rid.0),
        job_id: None,
    };
    let out = process_record(&st, &ictx, &source, &raw, IngestMode::Score)
        .await
        .map_err(record_error)?;
    Ok(outcome_response(out))
}

#[derive(Debug, Deserialize)]
struct SimulateIn {
    event: Option<Value>,
    source_id: Option<Uuid>,
    record: Option<Value>,
}

async fn simulate_event(
    State(st): State<AppState>,
    caller: Caller,
    rid: RequestId,
    ApiPath(pid): ApiPath<Uuid>,
    ApiJson(body): ApiJson<SimulateIn>,
) -> AppResult<Json<DecisionOut>> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let (source_id, raw) = match (body.event, body.source_id, body.record) {
        (Some(ev), _, None) => (None, ev),
        (None, Some(s), Some(r)) => (Some(s), r),
        _ => {
            return Err(AppError::BadRequest(
                "send either {event} or {source_id, record}".into(),
            ))
        }
    };
    let source = resolve_source(&st, tenant, project, source_id).await?;
    let ictx = IngestCtx {
        tenant,
        project,
        actor: caller.actor_user_id(),
        request_id: Some(rid.0),
        job_id: None,
    };
    Ok(Json(simulate(&st, &ictx, &source, &raw).await?))
}

async fn rescore_event(
    State(st): State<AppState>,
    caller: Caller,
    rid: RequestId,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<DecisionOut>> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let ictx = IngestCtx {
        tenant,
        project,
        actor: caller.actor_user_id(),
        request_id: Some(rid.0.clone()),
        job_id: None,
    };
    let (before, after) = rescore(&st, &ictx, id).await?;
    audit::record(
        &st.pool,
        &AuditEntry::by(&caller, "decision.rescore")
            .scope(tenant, Some(project))
            .subject("event", id)
            .before(&before)
            .after(&after)
            .request_id(rid.0),
    )
    .await?;
    Ok(Json(after))
}

async fn list_events(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<EventFilter>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(
        queries::list_events(&st, &caller, ProjectId(pid), &f, &p).await?,
    ))
}

async fn get_event(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(queries::get_event(&st, &caller, ProjectId(pid), id).await?))
}

async fn get_decision(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        queries::get_decision(&st, &caller, ProjectId(pid), id).await?,
    ))
}

async fn list_customers(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath(pid): ApiPath<Uuid>,
    ApiQuery(p): ApiQuery<PageParams>,
    ApiQuery(f): ApiQuery<CustomerFilter>,
) -> AppResult<Json<Page<Value>>> {
    Ok(Json(
        queries::list_customers(&st, &caller, ProjectId(pid), &f, &p).await?,
    ))
}

async fn get_customer(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        queries::get_customer(&st, &caller, ProjectId(pid), id).await?,
    ))
}

async fn customer_events(
    State(st): State<AppState>,
    caller: Caller,
    ApiPath((pid, id)): ApiPath<(Uuid, Uuid)>,
    ApiQuery(p): ApiQuery<PageParams>,
) -> AppResult<Json<Page<Value>>> {
    let f = EventFilter {
        customer_id: Some(id),
        ..Default::default()
    };
    Ok(Json(
        queries::list_events(&st, &caller, ProjectId(pid), &f, &p).await?,
    ))
}
