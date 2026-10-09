//! Request/response DTOs of the HTTP API (the wire shapes of api-contract.md §B).
//!
//! DTOs are separate from repository rows on purpose: the database can evolve without breaking clients, and
//! each response is assembled explicitly (the Java equivalent of mapping entities to response records).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use platform::auth::Caller;
use platform::auth::ProjectRole;
use platform::http::CallCtx;
use platform::telemetry::RequestId;
use platform::{AppResult, ProjectId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::adapters::repo::{
    ApprovalRow, MemberRow, RuleRow, RuleStats, RulesetRow, RulesetVersionRow, VersionRow,
};
use crate::domain::lifecycle::{Serving, TargetStatus};

/// Authorises the caller for a project and returns the typed scope.
pub async fn project_scope(
    caller: &Caller,
    pid: Uuid,
    role: ProjectRole,
) -> AppResult<(TenantId, ProjectId)> {
    let project = ProjectId(pid);
    let tenant = caller.require_project_role(project, role).await?;
    Ok((tenant, project))
}

/// Context for outgoing internal calls (tenant/project/actor/request id forwarded).
pub fn call_ctx(tenant: TenantId, project: ProjectId, caller: &Caller, rid: &RequestId) -> CallCtx {
    CallCtx::new(tenant, Some(project))
        .with_actor(caller.actor_user_id())
        .with_request_id(rid.0.clone())
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RuleOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub typologies: Vec<String>,
    pub event_types: Vec<String>,
    pub status: String,
    pub current_version: i32,
    pub submitted_by: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Full envelope (rule-dsl §2) of the current version.
    #[schema(value_type = Object)]
    pub envelope: Value,
    /// Which approved versions are live / shadow right now.
    pub serving: Serving,
    pub stats_7d: RuleStats,
}

impl RuleOut {
    pub fn new(rule: &RuleRow, current: Option<&VersionRow>, serving: Serving, stats: RuleStats) -> Self {
        Self {
            id: rule.id,
            code: rule.code.clone(),
            name: rule.name.clone(),
            description: rule.description.clone(),
            kind: rule.kind.clone(),
            typologies: rule.typologies.clone(),
            event_types: rule.event_types.clone(),
            status: rule.status.clone(),
            current_version: rule.current_version,
            submitted_by: rule.submitted_by,
            submitted_at: rule.submitted_at,
            created_by: rule.created_by,
            created_at: rule.created_at,
            updated_at: rule.updated_at,
            envelope: current
                .map(|v| crate::adapters::repo::envelope(rule, v))
                .unwrap_or(Value::Null),
            serving,
            stats_7d: stats,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VersionOut {
    pub version: i32,
    pub risk_score: f64,
    pub action: String,
    pub change_note: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[schema(value_type = Object)]
    pub envelope: Value,
}

impl VersionOut {
    pub fn new(rule: &RuleRow, v: &VersionRow) -> Self {
        Self {
            version: v.version,
            risk_score: f64::from(v.risk_score),
            action: v.action.clone(),
            change_note: v.change_note.clone(),
            created_by: v.created_by,
            created_at: v.created_at,
            envelope: crate::adapters::repo::envelope(rule, v),
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RuleDetailOut {
    #[serde(flatten)]
    pub rule: RuleOut,
    pub versions: Vec<VersionOut>,
    #[schema(value_type = Vec<Object>)]
    pub approvals: Vec<ApprovalRow>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RulesetOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub event_types: Vec<String>,
    pub typologies: Vec<String>,
    pub aggregation: String,
    pub max_score: f64,
    pub version: i32,
    pub status: String,
    /// Which approved versions are live / shadow right now (independent of pending draft edits).
    pub serving: Serving,
    pub submitted_by: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub rules: Vec<MemberRow>,
}

impl RulesetOut {
    /// `members` are the current draft's members (`rules.ruleset_rules`).
    pub fn new(rs: &RulesetRow, members: Vec<MemberRow>, serving: Serving) -> Self {
        Self {
            id: rs.id,
            code: rs.code.clone(),
            name: rs.name.clone(),
            description: rs.description.clone(),
            event_types: rs.event_types.clone(),
            typologies: rs.typologies.clone(),
            aggregation: rs.aggregation.clone(),
            max_score: f64::from(rs.max_score),
            version: rs.version,
            status: rs.status.clone(),
            serving,
            submitted_by: rs.submitted_by,
            submitted_at: rs.submitted_at,
            created_by: rs.created_by,
            created_at: rs.created_at,
            updated_at: rs.updated_at,
            rules: members,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RulesetDetailOut {
    #[serde(flatten)]
    pub ruleset: RulesetOut,
    /// Immutable snapshots, newest first.
    pub versions: Vec<RulesetVersionRow>,
    #[schema(value_type = Vec<Object>)]
    pub approvals: Vec<ApprovalRow>,
}

/// Body of approve / reject endpoints.
#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct DecisionBody {
    pub target_status: Option<TargetStatus>,
    pub comment: Option<String>,
}

/// Groups members by ruleset id.
pub fn group_members(members: Vec<MemberRow>) -> HashMap<Uuid, Vec<MemberRow>> {
    let mut map: HashMap<Uuid, Vec<MemberRow>> = HashMap::new();
    for m in members {
        map.entry(m.ruleset_id).or_default().push(m);
    }
    map
}
