//! `/api/v1/projects/{pid}/rules/**` handlers. Thin: authorise, parse, delegate to `app::*`, map to DTOs.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use contracts::scoring::EvaluationContext;
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::pagination::{Page, PageParams};
use platform::telemetry::RequestId;
use platform::{AppError, AppResult, ProjectId};
use rule_engine::{evaluate_rule, EvalContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use super::dto::{call_ctx, project_scope, DecisionBody, RuleDetailOut, RuleOut, VersionOut};
use crate::adapters::data_provider::PgDataProvider;
use crate::adapters::event_context::{engine_data, load_event};
use crate::adapters::provider_cache::CachedProvider;
use crate::adapters::repo::{self, PerformanceRow, RuleFilter, RuleRow, RuleStats, VersionRow};
use crate::app::backtest::{backtest_rule, BacktestWindow};
use crate::app::workflow;
use crate::domain::backtest::BacktestReport;
use crate::domain::lifecycle::{serving_from_ledger, Action, ApprovedVersion, Serving, TargetStatus};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub typology: Option<String>,
    pub q: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

async fn servings(
    conn: &mut PgConnection,
    project: ProjectId,
    rules: &[RuleRow],
) -> AppResult<HashMap<Uuid, Serving>> {
    let ledger = repo::approved_ledger(&mut *conn, project).await?;
    let mut by_rule: HashMap<Uuid, Vec<ApprovedVersion>> = HashMap::new();
    for (t, id, v, target) in ledger {
        if let (true, Some(version)) = (t == "rule", v) {
            by_rule
                .entry(id)
                .or_default()
                .push(ApprovedVersion { version, target });
        }
    }
    Ok(rules
        .iter()
        .map(|r| {
            let l = by_rule.get(&r.id).map(Vec::as_slice).unwrap_or(&[]);
            (r.id, serving_from_ledger(l, r.status == "retired"))
        })
        .collect())
}

async fn current_versions(conn: &mut PgConnection, ids: &[Uuid]) -> AppResult<HashMap<Uuid, VersionRow>> {
    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT v.rule_id, v.version, v.definition, v.risk_score, v.trapped_score, v.action, v.on_trapped, \
         v.missing_as_no_match, v.change_note, v.created_by, v.created_at \
         FROM rules.rule_versions v JOIN rules.rules r ON r.id = v.rule_id AND v.version = r.current_version \
         WHERE r.id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|v| (v.rule_id, v)).collect())
}

pub async fn rule_out(conn: &mut PgConnection, project: ProjectId, rule: &RuleRow) -> AppResult<RuleOut> {
    let one = std::slice::from_ref(rule);
    let serving = servings(&mut *conn, project, one)
        .await?
        .remove(&rule.id)
        .unwrap_or_default();
    let current = current_versions(&mut *conn, &[rule.id]).await?;
    let stats = repo::stats_since(&mut *conn, &[rule.id], 7)
        .await?
        .remove(&rule.id)
        .unwrap_or_default();
    Ok(RuleOut::new(rule, current.get(&rule.id), serving, stats))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rules", params(("pid" = Uuid, Path), ListQuery),
    responses((status = 200, description = "Rules page")), tag = "rules")]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Page<RuleOut>>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let page = PageParams {
        page: q.page,
        page_size: q.page_size,
    };
    let filter = RuleFilter {
        kind: q.kind,
        status: q.status,
        typology: q.typology,
        q: q.q,
    };
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let (rules, total) = repo::list_rules(&mut tx, project, &filter, page.limit(), page.offset()).await?;
    let ids: Vec<Uuid> = rules.iter().map(|r| r.id).collect();
    let serving = servings(&mut tx, project, &rules).await?;
    let current = current_versions(&mut tx, &ids).await?;
    let stats = repo::stats_since(&mut tx, &ids, 7).await?;
    tx.commit().await?;
    let items = rules
        .iter()
        .map(|r| {
            RuleOut::new(
                r,
                current.get(&r.id),
                serving.get(&r.id).copied().unwrap_or_default(),
                stats.get(&r.id).cloned().unwrap_or_default(),
            )
        })
        .collect();
    Ok(Json(Page::new(items, total, &page)))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules", params(("pid" = Uuid, Path)),
    request_body(content = Object, description = "Rule envelope (rule-dsl §2), optional `change_note`"),
    responses((status = 201, body = RuleOut), (status = 422, description = "Invalid rule")), tag = "rules")]
pub async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(body): Json<Value>,
) -> AppResult<(StatusCode, Json<RuleOut>)> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let rule = workflow::create_rule(&state, &caller, tenant, project, &body).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = rule_out(&mut tx, project, &rule).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rules/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RuleDetailOut)), tag = "rules")]
pub async fn get(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<RuleDetailOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::get_rule(&mut tx, project, id).await?;
    let out = rule_out(&mut tx, project, &rule).await?;
    let versions = repo::list_versions(&mut tx, id).await?;
    let approvals = repo::approvals_of(&mut tx, project, id).await?;
    tx.commit().await?;
    Ok(Json(RuleDetailOut {
        rule: out,
        versions: versions.iter().map(|v| VersionOut::new(&rule, v)).collect(),
        approvals,
    }))
}

#[utoipa::path(put, path = "/api/v1/projects/{pid}/rules/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body(content = Object, description = "Rule envelope; creates a new version"),
    responses((status = 200, body = RuleOut)), tag = "rules")]
pub async fn update(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Value>,
) -> AppResult<Json<RuleOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let rule = workflow::update_rule(&state, &caller, tenant, project, id, &body).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = rule_out(&mut tx, project, &rule).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rules/{id}/versions/{v}",
    params(("pid" = Uuid, Path), ("id" = Uuid, Path), ("v" = i32, Path)),
    responses((status = 200, body = VersionOut)), tag = "rules")]
pub async fn get_version(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id, v)): Path<(Uuid, Uuid, i32)>,
) -> AppResult<Json<VersionOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::get_rule(&mut tx, project, id).await?;
    let version = repo::get_version(&mut tx, id, v).await?;
    tx.commit().await?;
    Ok(Json(VersionOut::new(&rule, &version)))
}

/// `POST …/rules/validate`: 200 when valid, 422 when not, the validation report as body in both cases.
#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/validate", params(("pid" = Uuid, Path)),
    request_body(content = Object, description = "Rule envelope"),
    responses((status = 200, description = "Valid"), (status = 422, description = "Invalid, same body")), tag = "rules")]
pub async fn validate(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(body): Json<Value>,
) -> AppResult<Response> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let (report, _, _) = workflow::validate(&state, tenant, project, &body).await?;
    let status = if report.valid {
        StatusCode::OK
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    Ok((status, Json(report)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestBody {
    #[schema(value_type = Object)]
    pub rule: Value,
    pub event_id: Option<Uuid>,
    #[schema(value_type = Object)]
    pub context: Option<EvaluationContext>,
    pub customer_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub occurred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TestOut {
    pub outcome: String,
    pub contribution: f64,
    pub trapped_reason: Option<String>,
    #[schema(value_type = Object)]
    pub trace: Value,
    pub duration_us: u64,
    #[schema(value_type = Object)]
    pub context: EvaluationContext,
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/test", params(("pid" = Uuid, Path)),
    request_body = TestBody, responses((status = 200, body = TestOut)), tag = "rules")]
pub async fn test(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path(pid): Path<Uuid>,
    Json(body): Json<TestBody>,
) -> AppResult<Json<TestOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let v = workflow::validated(&state, tenant, project, &body.rule).await?;
    let (context, customer_id, event_type, occurred_at, event_id) = match body.event_id {
        Some(event_id) => {
            let mut tx = TenantTx::begin(&state.pool, tenant).await?;
            let ev = load_event(&mut tx, project, event_id).await?;
            tx.commit().await?;
            (
                ev.context(),
                ev.customer_id,
                ev.event_type.clone(),
                ev.occurred_at,
                Some(ev.id),
            )
        }
        None => {
            let ctx = body
                .context
                .clone()
                .ok_or_else(|| AppError::field("context", "either event_id or context is required"))?;
            let customer_id = body
                .customer_id
                .or_else(|| {
                    ctx.event
                        .get("customer_id")
                        .and_then(Value::as_str)
                        .and_then(|s| Uuid::parse_str(s).ok())
                })
                .unwrap_or_else(Uuid::nil);
            let event_type = body
                .event_type
                .clone()
                .or_else(|| {
                    ctx.event
                        .get("event_type")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| v.envelope.event_types.first().cloned())
                .unwrap_or_else(|| "transaction".into());
            let occurred_at = body
                .occurred_at
                .or_else(|| {
                    ctx.event
                        .get("occurred_at")
                        .and_then(Value::as_str)
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|d| d.with_timezone(&Utc))
                })
                .unwrap_or_else(Utc::now);
            (ctx, customer_id, event_type, occurred_at, None)
        }
    };
    let catalog = state.catalogs.get(tenant, project).await?;
    let provider = CachedProvider::new(
        PgDataProvider::new(
            state.pool.clone(),
            tenant,
            project,
            catalog,
            state.graph.clone(),
            call_ctx(tenant, project, &caller, &rid),
        ),
        project,
        None,
    );
    let mut ctx = EvalContext::new(engine_data(&context, customer_id), event_type, occurred_at)
        .with_customer_id(customer_id.to_string());
    if let Some(id) = event_id {
        ctx = ctx.with_event_id(id.to_string());
    }
    let started = std::time::Instant::now();
    let evaluation = evaluate_rule(&v.envelope, &ctx, &provider).await;
    let contribution = rule_engine::ruleset::contribution(&v.envelope, 1.0, &evaluation);
    Ok(Json(TestOut {
        outcome: evaluation.outcome.as_str().to_string(),
        contribution,
        trapped_reason: evaluation.outcome.trapped_reason().map(str::to_string),
        trace: evaluation.trace,
        duration_us: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        context,
    }))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct StoredBacktestBody {
    #[serde(flatten)]
    pub window: BacktestWindow,
    pub version: Option<i32>,
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/{id}/backtest", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = StoredBacktestBody, responses((status = 200, body = BacktestReport)), tag = "rules")]
pub async fn backtest_stored(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<StoredBacktestBody>>,
) -> AppResult<Json<BacktestReport>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::get_rule(&mut tx, project, id).await?;
    let version = repo::get_version(&mut tx, id, body.version.unwrap_or(rule.current_version)).await?;
    tx.commit().await?;
    let envelope = crate::adapters::serving::parse_envelope(&rule, &version)
        .ok_or_else(|| AppError::Conflict("stored rule version no longer parses".into()))?;
    let report = backtest_rule(
        &state,
        tenant,
        project,
        Arc::new(envelope),
        &body.window,
        &call_ctx(tenant, project, &caller, &rid),
    )
    .await?;
    Ok(Json(report))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InlineBacktestBody {
    #[schema(value_type = Object)]
    pub rule: Value,
    #[serde(flatten)]
    pub window: BacktestWindow,
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/backtest", params(("pid" = Uuid, Path)),
    request_body = InlineBacktestBody, responses((status = 200, body = BacktestReport)), tag = "rules")]
pub async fn backtest_inline(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path(pid): Path<Uuid>,
    Json(body): Json<InlineBacktestBody>,
) -> AppResult<Json<BacktestReport>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let v = workflow::validated(&state, tenant, project, &body.rule).await?;
    let report = backtest_rule(
        &state,
        tenant,
        project,
        Arc::new(v.envelope),
        &body.window,
        &call_ctx(tenant, project, &caller, &rid),
    )
    .await?;
    Ok(Json(report))
}

async fn action(
    state: AppState,
    caller: Caller,
    pid: Uuid,
    id: Uuid,
    action: Action,
    role: ProjectRole,
    comment: Option<String>,
) -> AppResult<Json<RuleOut>> {
    let (tenant, project) = project_scope(&caller, pid, role).await?;
    let rule =
        workflow::rule_action(&state, &caller, tenant, project, id, action, comment.as_deref()).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = rule_out(&mut tx, project, &rule).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/{id}/submit", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RuleOut)), tag = "rules")]
pub async fn submit(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<RuleOut>> {
    action(state, caller, pid, id, Action::Submit, ProjectRole::Analyst, None).await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/{id}/approve", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = RuleOut)), tag = "rules")]
pub async fn approve(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RuleOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let target = body.target_status.unwrap_or(TargetStatus::Active);
    action(
        state,
        caller,
        pid,
        id,
        Action::Approve(target),
        ProjectRole::Approver,
        body.comment,
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/{id}/reject", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = RuleOut)), tag = "rules")]
pub async fn reject(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RuleOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    action(
        state,
        caller,
        pid,
        id,
        Action::Reject,
        ProjectRole::Approver,
        body.comment,
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rules/{id}/retire", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RuleOut)), tag = "rules")]
pub async fn retire(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RuleOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    action(
        state,
        caller,
        pid,
        id,
        Action::Retire,
        ProjectRole::Approver,
        body.comment,
    )
    .await
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PerformanceQuery {
    pub since_days: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PerformanceOut {
    pub since_days: i32,
    pub items: Vec<PerformanceRow>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rules/performance", params(("pid" = Uuid, Path), PerformanceQuery),
    responses((status = 200, body = PerformanceOut)), tag = "rules")]
pub async fn performance(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<PerformanceQuery>,
) -> AppResult<Json<PerformanceOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let days = q.since_days.unwrap_or(30);
    if !(1..=365).contains(&days) {
        return Err(AppError::field("since_days", "must be between 1 and 365"));
    }
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let items = repo::performance(&mut tx, project, days).await?;
    tx.commit().await?;
    Ok(Json(PerformanceOut {
        since_days: days,
        items,
    }))
}

/// Keeps `RuleStats` referenced for the OpenAPI schema list.
pub type RuleStatsSchema = RuleStats;
