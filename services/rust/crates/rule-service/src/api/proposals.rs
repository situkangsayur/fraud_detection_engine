//! `/api/v1/projects/{pid}/proposals/**`: rule proposals from the LLM (INT token) or analysts.
//!
//! Creation always stores the proposal, even an invalid one (`validation.valid = false`), so reviewers can see what
//! the LLM tried. Valid proposals are backtested over the last 30 days at creation time.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::pagination::{Page, PageParams};
use platform::telemetry::RequestId;
use platform::{AppError, AppResult};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::dto::{call_ctx, project_scope, DecisionBody};
use crate::adapters::repo::{self, NewProposal, ProposalRow};
use crate::app::backtest::{backtest_rule, BacktestWindow};
use crate::app::workflow;
use crate::domain::proposal::{self, ProposalSource, ProposalType};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProposalBody {
    pub source: ProposalSource,
    pub proposal_type: ProposalType,
    #[serde(default)]
    pub target_rule_id: Option<Uuid>,
    /// Full rule envelope (rule-dsl §2); omitted for `retire_rule`.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub definition: Option<Value>,
    pub rationale: String,
    /// `[{code, section, excerpt?, regulation_id?, chunk_id?, version?}]`
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    pub citations: Option<Value>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub evidence: Option<Value>,
    #[serde(default)]
    pub report_id: Option<Uuid>,
    #[serde(default)]
    pub llm_model: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/proposals", params(("pid" = Uuid, Path)),
    request_body = CreateProposalBody, responses((status = 201, body = ProposalRow)), tag = "proposals")]
pub async fn create(
    State(state): State<AppState>,
    caller: Caller,
    rid: RequestId,
    Path(pid): Path<Uuid>,
    Json(body): Json<CreateProposalBody>,
) -> AppResult<(StatusCode, Json<ProposalRow>)> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Analyst).await?;
    proposal::check_new(body.proposal_type, body.definition.is_some(), body.target_rule_id)
        .map_err(|m| AppError::field("proposal_type", m))?;
    if body.rationale.trim().is_empty() {
        return Err(AppError::field("rationale", "must not be empty"));
    }
    let citations = body.citations.clone().unwrap_or_else(|| json!([]));
    if !citations.is_array() {
        return Err(AppError::field("citations", "must be an array"));
    }
    let evidence = body.evidence.clone().unwrap_or_else(|| json!({}));

    // Target rule must exist; modify/tune proposals always keep the target's code.
    let mut definition = body.definition.clone();
    let mut code_conflict = None;
    {
        let mut tx = TenantTx::begin(&state.pool, tenant).await?;
        if let Some(target) = body.target_rule_id {
            let rule = repo::get_rule(&mut tx, project, target)
                .await
                .map_err(|_| AppError::field("target_rule_id", "rule not found in this project"))?;
            if let Some(obj) = definition.as_mut().and_then(Value::as_object_mut) {
                obj.insert("code".into(), Value::String(rule.code.clone()));
            }
        } else if let Some(code) = definition
            .as_ref()
            .and_then(|d| d.get("code"))
            .and_then(Value::as_str)
        {
            if repo::find_rule_by_code(&mut tx, project, code).await?.is_some() {
                code_conflict = Some(code.to_string());
            }
        }
        tx.commit().await?;
    }

    let (validation, backtest) = match &definition {
        None => (json!({ "valid": true, "errors": [] }), None),
        Some(def) => {
            let (mut report, parsed, _) = workflow::validate(&state, tenant, project, def).await?;
            if let Some(code) = &code_conflict {
                report.valid = false;
                report.errors.push(rule_engine::ValidationError {
                    path: "code".into(),
                    message: format!("a rule with code '{code}' already exists"),
                });
            }
            let backtest = match parsed {
                Some(envelope) if report.valid => {
                    let window = BacktestWindow {
                        since_days: Some(30),
                        ..Default::default()
                    };
                    match backtest_rule(
                        &state,
                        tenant,
                        project,
                        Arc::new(envelope),
                        &window,
                        &call_ctx(tenant, project, &caller, &rid),
                    )
                    .await
                    {
                        Ok(r) => serde_json::to_value(r).ok(),
                        Err(e) => Some(json!({ "error": e.to_string() })),
                    }
                }
                _ => None,
            };
            (
                serde_json::to_value(&report).unwrap_or_else(|_| json!({})),
                backtest,
            )
        }
    };

    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let row = repo::insert_proposal(
        &mut tx,
        tenant,
        project,
        &NewProposal {
            source: body.source.as_str(),
            proposal_type: body.proposal_type.as_str(),
            target_rule_id: body.target_rule_id,
            definition: definition.as_ref(),
            rationale: &body.rationale,
            citations: &citations,
            evidence: &evidence,
            validation: &validation,
            backtest: backtest.as_ref(),
            report_id: body.report_id,
            llm_model: body.llm_model.as_deref(),
            created_by: caller.actor_user_id().map(|u| u.0),
        },
    )
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(&caller, "proposal.create")
            .scope(tenant, Some(project))
            .subject("proposal", row.id)
            .metadata(
                json!({ "source": row.source, "type": row.proposal_type, "valid": validation["valid"] }),
            ),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub status: Option<String>,
    pub source: Option<String>,
    pub report_id: Option<Uuid>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/proposals", params(("pid" = Uuid, Path), ListQuery),
    responses((status = 200, description = "Proposals page")), tag = "proposals")]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Page<ProposalRow>>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let page = PageParams {
        page: q.page,
        page_size: q.page_size,
    };
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let (rows, total) = repo::list_proposals(
        &mut tx,
        project,
        q.status.as_deref(),
        q.source.as_deref(),
        q.report_id,
        page.limit(),
        page.offset(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(Page::new(rows, total, &page)))
}

#[utoipa::path(get, path = "/api/v1/projects/{pid}/proposals/{id}", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    responses((status = 200, body = ProposalRow)), tag = "proposals")]
pub async fn get(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ProposalRow>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let row = repo::get_proposal(&mut tx, project, id, false).await?;
    tx.commit().await?;
    Ok(Json(row))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/proposals/{id}/approve", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = ProposalRow)), tag = "proposals")]
pub async fn approve(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<ProposalRow>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Approver).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    Ok(Json(
        workflow::approve_proposal(&state, &caller, tenant, project, id, body.comment.as_deref()).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/proposals/{id}/reject", params(("pid" = Uuid, Path), ("id" = Uuid, Path)),
    request_body = DecisionBody, responses((status = 200, body = ProposalRow)), tag = "proposals")]
pub async fn reject(
    State(state): State<AppState>,
    caller: Caller,
    Path((pid, id)): Path<(Uuid, Uuid)>,
    body: Option<Json<DecisionBody>>,
) -> AppResult<Json<ProposalRow>> {
    let (tenant, project) = project_scope(&caller, pid, ProjectRole::Approver).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    Ok(Json(
        workflow::reject_proposal(&state, &caller, tenant, project, id, body.comment.as_deref()).await?,
    ))
}
