//! `/api/v1/projects/{pid}/rulesets/**` handlers.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::pagination::{Page, PageParams};
use platform::telemetry::RequestId;
use platform::{AppError, AppResult, ProjectId};
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::dto::{call_ctx, group_members, project_scope, DecisionBody, RulesetDetailOut, RulesetOut};
use crate::adapters::repo::{self, RulesetRow};
use crate::adapters::serving::candidate_unit;
use crate::app::backtest::{backtest_ruleset, BacktestWindow};
use crate::app::workflow::{self, MemberIn, RulesetBody};
use crate::domain::backtest::{BacktestReport, Thresholds};
use crate::domain::lifecycle::{serving_from_ledger, Action, ApprovedVersion, Serving, TargetStatus};
use crate::state::AppState;

/// Serving state (live / shadow version) of every ruleset of a project, from the approval ledger.
async fn servings(
    conn: &mut PgConnection,
    project: ProjectId,
) -> AppResult<HashMap<Uuid, Vec<ApprovedVersion>>> {
    let mut map: HashMap<Uuid, Vec<ApprovedVersion>> = HashMap::new();
    for (t, id, version, target) in repo::approved_ledger(conn, project).await? {
        if let (true, Some(version)) = (t == "ruleset", version) {
            map.entry(id)
                .or_default()
                .push(ApprovedVersion { version, target });
        }
    }
    Ok(map)
}

fn serving_of(ledgers: &HashMap<Uuid, Vec<ApprovedVersion>>, rs: &RulesetRow) -> Serving {
    serving_from_ledger(
        ledgers.get(&rs.id).map(Vec::as_slice).unwrap_or(&[]),
        rs.status == "retired",
    )
}

async fn ruleset_out(conn: &mut PgConnection, project: ProjectId, rs: &RulesetRow) -> AppResult<RulesetOut> {
    let members = repo::members(&mut *conn, project, &[rs.id]).await?;
    let ledgers = servings(&mut *conn, project).await?;
    Ok(RulesetOut::new(rs, members, serving_of(&ledgers, rs)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub status: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rulesets", params(("pid" = Uuid, Path), ListQuery),
    responses((status = 200, description = "Rulesets page")), tag = "rulesets")]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Page<RulesetOut>>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let page = PageParams {
        page: q.page,
        page_size: q.page_size,
    };
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let (rows, total) =
        repo::list_rulesets(&mut tx, project, q.status.as_deref(), page.limit(), page.offset()).await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut members = group_members(repo::members(&mut tx, project, &ids).await?);
    let ledgers = servings(&mut tx, project).await?;
    tx.commit().await?;
    let items = rows
        .iter()
        .map(|rs| {
            RulesetOut::new(
                rs,
                members.remove(&rs.id).unwrap_or_default(),
                serving_of(&ledgers, rs),
            )
        })
        .collect();
    Ok(Json(Page::new(items, total, &page)))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets", params(("pid" = Uuid, Path)),
    request_body = RulesetBody, responses((status = 201, body = RulesetOut)), tag = "rulesets")]
pub async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(body): Json<RulesetBody>,
) -> AppResult<(StatusCode, Json<RulesetOut>)> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let rs = workflow::create_ruleset(&state, &caller, tenant, project, &body).await?;
    Ok((
        StatusCode::CREATED,
        Json(RulesetOut::new(&rs, vec![], Serving::default())),
    ))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/rulesets/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RulesetDetailOut)), tag = "rulesets")]
pub async fn get(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<RulesetDetailOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rs = repo::get_ruleset(&mut tx, project, id).await?;
    let out = ruleset_out(&mut tx, project, &rs).await?;
    let versions = repo::ruleset_versions(&mut tx, id).await?;
    let approvals = repo::approvals_of(&mut tx, project, id).await?;
    tx.commit().await?;
    Ok(Json(RulesetDetailOut {
        ruleset: out,
        versions,
        approvals,
    }))
}

#[utoipa::path(put, path = "/api/v1/projects/{pid}/rulesets/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = RulesetBody, responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn update(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<RulesetBody>,
) -> AppResult<Json<RulesetOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let rs = workflow::update_ruleset(&state, &caller, tenant, project, id, &body).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = ruleset_out(&mut tx, project, &rs).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(delete, path = "/api/v1/projects/{pid}/rulesets/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 204), (status = 409, description = "Ruleset was approved; retire it instead")), tag = "rulesets")]
pub async fn delete(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rs = repo::get_ruleset(&mut tx, project, id).await?;
    let ledgers = servings(&mut tx, project).await?;
    if ledgers.contains_key(&id) {
        return Err(AppError::Conflict(
            "an approved ruleset cannot be deleted; retire it instead".into(),
        ));
    }
    sqlx::query("DELETE FROM rules.rulesets WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    platform::audit::record(
        &mut **tx,
        &platform::audit::AuditEntry::by(&caller, "ruleset.delete")
            .scope(tenant, Some(project))
            .subject("ruleset", id)
            .before(&rs),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(put, path = "/api/v1/projects/{pid}/rulesets/{id}/rules", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = Vec<MemberIn>, responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn set_members(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    Json(members): Json<Vec<MemberIn>>,
) -> AppResult<Json<RulesetOut>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let rs = workflow::set_members(&state, &caller, tenant, project, id, &members).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = ruleset_out(&mut tx, project, &rs).await?;
    tx.commit().await?;
    Ok(Json(out))
}

async fn action(
    state: AppState,
    caller: Caller,
    pid: Uuid,
    id: Uuid,
    action: Action,
    role: ProjectRole,
    comment: Option<String>,
) -> AppResult<Json<RulesetOut>> {
    let (tenant, project) = project_scope(&caller, pid, role).await?;
    let rs =
        workflow::ruleset_action(&state, &caller, tenant, project, id, action, comment.as_deref()).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let out = ruleset_out(&mut tx, project, &rs).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets/{id}/submit", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn submit(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<RulesetOut>> {
    action(s, c, pid, id, Action::Submit, ProjectRole::Analyst, None).await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets/{id}/approve", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn approve(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RulesetOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let target = body.target_status.unwrap_or(TargetStatus::Active);
    action(
        s,
        c,
        pid,
        id,
        Action::Approve(target),
        ProjectRole::Approver,
        body.comment,
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets/{id}/reject", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn reject(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RulesetOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    action(s, c, pid, id, Action::Reject, ProjectRole::Approver, body.comment).await
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets/{id}/retire", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = RulesetOut)), tag = "rulesets")]
pub async fn retire(
    State(s): State<AppState>,
    c: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<RulesetOut>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    action(s, c, pid, id, Action::Retire, ProjectRole::Approver, body.comment).await
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct RulesetBacktestBody {
    #[serde(flatten)]
    pub window: BacktestWindow,
    /// Ruleset version to replay (default: the current draft version).
    #[serde(default)]
    pub version: Option<i32>,
    /// Overrides the project's `decision_thresholds` setting (defaults 50 / 80 when unset).
    #[serde(default)]
    pub thresholds: Option<Thresholds>,
}

/// The project's decision thresholds; a malformed setting falls back to the defaults (logged).
async fn project_thresholds(conn: &mut PgConnection, project: ProjectId) -> AppResult<Thresholds> {
    Ok(match repo::project_thresholds(conn, project).await? {
        Some(v) => serde_json::from_value(v).unwrap_or_else(|e| {
            tracing::warn!(%project, error = %e, "invalid decision_thresholds setting; using defaults");
            Thresholds::default()
        }),
        None => Thresholds::default(),
    })
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/rulesets/{id}/backtest", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = RulesetBacktestBody, responses((status = 200, body = BacktestReport)), tag = "rulesets")]
pub async fn backtest(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<RulesetBacktestBody>>,
) -> AppResult<Json<BacktestReport>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rs = repo::get_ruleset(&mut tx, project, id).await?;
    let unit = candidate_unit(
        &mut tx,
        project,
        rs.id,
        &rs.code,
        body.version.unwrap_or(rs.version),
    )
    .await?;
    let thresholds = match body.thresholds {
        Some(t) => t,
        None => project_thresholds(&mut tx, project).await?,
    };
    tx.commit().await?;
    let report = backtest_ruleset(
        &state,
        tenant,
        project,
        unit,
        &body.window,
        thresholds,
        &call_ctx(tenant, project, &caller, &rid),
    )
    .await?;
    Ok(Json(report))
}
