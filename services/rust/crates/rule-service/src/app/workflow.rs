//! Use cases that change rules, rulesets and proposals (maker–checker governance, architecture.md §4).
//!
//! Each function is one transaction: load (with `FOR UPDATE` where a status changes), ask the pure domain
//! whether the transition is allowed, persist, write the audit row, commit, invalidate the serving cache.

use std::collections::HashMap;

use platform::audit::{self, AuditEntry};
use platform::auth::Caller;
use platform::db::TenantTx;
use platform::error::FieldError;
use platform::{AppError, AppResult, ProjectId, TenantId};
use rule_engine::model::RuleEnvelope;
use rule_engine::{validate_rule_json, ValidationReport};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::adapters::repo::{self, ListScope, NewVersion, RuleRow, RulesetFields, RulesetRow};
use crate::domain::csv_import::EntryRow;
use crate::domain::lifecycle::{self, Action, LifecycleError, Status, TargetStatus};
use crate::domain::proposal::{self, ProposalType};
use crate::domain::templates;
use crate::state::AppState;

fn lifecycle_err(e: LifecycleError) -> AppError {
    match e {
        LifecycleError::SelfApproval | LifecycleError::ActorRequired => AppError::Forbidden(e.to_string()),
        LifecycleError::InvalidTransition { .. } => AppError::Conflict(e.to_string()),
    }
}

fn enum_str(v: impl Serialize) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Validates an envelope JSON against the project catalogue. `change_note` is accepted and removed.
pub async fn validate(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    body: &Value,
) -> AppResult<(ValidationReport, Option<RuleEnvelope>, Option<String>)> {
    let mut envelope = body.clone();
    let change_note = envelope
        .as_object_mut()
        .and_then(|o| o.remove("change_note"))
        .and_then(|v| v.as_str().map(str::to_string));
    let catalog = state.catalogs.get(tenant, project).await?;
    let (report, parsed) = validate_rule_json(&envelope, catalog.as_ref());
    Ok((report, parsed, change_note))
}

pub fn report_to_error(report: &ValidationReport) -> AppError {
    AppError::Validation {
        detail: Some("rule definition is invalid".into()),
        errors: report
            .errors
            .iter()
            .map(|e| FieldError::new(e.path.clone(), e.message.clone()))
            .collect(),
    }
}

/// A validated envelope ready to persist (owns the strings `NewVersion` borrows).
#[derive(Debug)]
pub struct Validated {
    pub envelope: RuleEnvelope,
    pub definition: Value,
    pub kind: String,
    pub action: String,
    pub on_trapped: String,
    pub change_note: Option<String>,
}

impl Validated {
    pub fn new_version(&self) -> NewVersion<'_> {
        NewVersion {
            code: &self.envelope.code,
            name: &self.envelope.name,
            description: self.envelope.description.as_deref(),
            kind: &self.kind,
            typologies: &self.envelope.typologies,
            event_types: &self.envelope.event_types,
            definition: &self.definition,
            risk_score: self.envelope.risk_score,
            trapped_score: self.envelope.trapped_score,
            action: &self.action,
            on_trapped: &self.on_trapped,
            missing_as_no_match: self.envelope.missing_as_no_match,
            change_note: self.change_note.as_deref(),
        }
    }
}

pub async fn validated(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    body: &Value,
) -> AppResult<Validated> {
    let (report, parsed, change_note) = validate(state, tenant, project, body).await?;
    match parsed {
        Some(envelope) if report.valid => Ok(Validated {
            definition: serde_json::to_value(&envelope.definition)
                .map_err(|e| AppError::internal(format!("serialize definition: {e}")))?,
            kind: envelope.kind.as_str().to_string(),
            action: enum_str(envelope.action),
            on_trapped: enum_str(envelope.on_trapped),
            envelope,
            change_note,
        }),
        _ => Err(report_to_error(&report)),
    }
}

// -------------------------------------------------------------------------------------------------------------
// Rules
// -------------------------------------------------------------------------------------------------------------

pub async fn create_rule(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    body: &Value,
) -> AppResult<RuleRow> {
    let v = validated(state, tenant, project, body).await?;
    let actor = caller.actor_user_id().map(|u| u.0);
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::insert_rule(&mut tx, tenant, project, &v.new_version(), Status::Draft, actor).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "rule.create")
            .scope(tenant, Some(project))
            .subject("rule", rule.id)
            .after(&v.envelope),
    )
    .await?;
    tx.commit().await?;
    Ok(rule)
}

pub async fn update_rule(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    body: &Value,
) -> AppResult<RuleRow> {
    let v = validated(state, tenant, project, body).await?;
    let actor = caller.actor_user_id().map(|u| u.0);
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::get_rule_for_update(&mut tx, project, id).await?;
    if v.envelope.code != rule.code {
        return Err(AppError::field("code", "the rule code is immutable"));
    }
    if v.kind != rule.kind {
        return Err(AppError::field(
            "kind",
            "the rule kind is immutable; create a new rule instead",
        ));
    }
    let next = lifecycle::transition(rule.status()?, Action::Edit, rule.submitted_by, actor)
        .map_err(lifecycle_err)?;
    let before = rule.clone();
    let updated = repo::add_version(&mut tx, tenant, &rule, &v.new_version(), next, actor).await?;
    supersede_pending(&mut tx, project, "rule", id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "rule.update")
            .scope(tenant, Some(project))
            .subject("rule", id)
            .before(&before)
            .after(&v.envelope)
            .metadata(serde_json::json!({ "version": updated.current_version })),
    )
    .await?;
    tx.commit().await?;
    Ok(updated)
}

async fn supersede_pending(
    conn: &mut sqlx::PgConnection,
    project: ProjectId,
    subject_type: &str,
    id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE core.approvals SET decision = 'rejected', decided_at = now(), comment = 'superseded by a new version' \
         WHERE project_id = $1 AND subject_type = $2 AND subject_id = $3 AND decision IS NULL",
    )
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Workflow action on a rule (submit / approve / reject / retire).
pub async fn rule_action(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    action: Action,
    comment: Option<&str>,
) -> AppResult<RuleRow> {
    let actor = caller.actor_user_id().map(|u| u.0);
    // Decisions must come from a real user (a service token may not approve on anyone's behalf).
    if !matches!(action, Action::Submit) && caller.user().is_none() {
        return Err(AppError::Forbidden(
            "approval decisions require a user token".into(),
        ));
    }
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rule = repo::get_rule_for_update(&mut tx, project, id).await?;
    let next =
        lifecycle::transition(rule.status()?, action, rule.submitted_by, actor).map_err(lifecycle_err)?;
    let (verb, updated) = match action {
        Action::Submit => {
            let r = repo::set_rule_status(&mut tx, id, next, Some(actor)).await?;
            repo::open_approval(
                &mut tx,
                tenant,
                project,
                "rule",
                id,
                Some(rule.current_version),
                actor,
            )
            .await?;
            ("rule.submit", r)
        }
        Action::Approve(target) => {
            // A newly approved live version replaces the live one; a shadow approval of a live rule keeps the
            // rule's status "active" (champion/challenger), so status reflects what matters most.
            let ledger = repo::rule_ledger(&mut tx, project, id).await?;
            let before = lifecycle::serving_from_ledger(&ledger, false);
            let status = lifecycle::status_after_approval(target, rule.current_version, before);
            let r = repo::set_rule_status(&mut tx, id, status, None).await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "rule",
                id,
                Some(rule.current_version),
                rule.submitted_by,
                actor,
                "approved",
                Some(target),
                comment,
            )
            .await?;
            ("rule.approve", r)
        }
        Action::Reject => {
            let r = repo::set_rule_status(&mut tx, id, next, None).await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "rule",
                id,
                Some(rule.current_version),
                rule.submitted_by,
                actor,
                "rejected",
                None,
                comment,
            )
            .await?;
            ("rule.reject", r)
        }
        Action::Retire => {
            let r = repo::set_rule_status(&mut tx, id, next, None).await?;
            supersede_pending(&mut tx, project, "rule", id).await?;
            ("rule.retire", r)
        }
        Action::Edit => return Err(AppError::BadRequest("use PUT to edit a rule".into())),
    };
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, verb)
            .scope(tenant, Some(project))
            .subject("rule", id)
            .before(serde_json::json!({ "status": rule.status }))
            .after(serde_json::json!({ "status": updated.status, "version": updated.current_version }))
            .metadata(serde_json::json!({ "comment": comment })),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(updated)
}

// -------------------------------------------------------------------------------------------------------------
// Rulesets
// -------------------------------------------------------------------------------------------------------------

/// Ruleset create/update body (rule-dsl §7 without members).
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RulesetBody {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub typologies: Vec<String>,
    #[serde(default = "default_aggregation")]
    pub aggregation: String,
    #[serde(default = "default_max_score")]
    pub max_score: f64,
    /// Stored on the new ruleset version.
    #[serde(default)]
    pub change_note: Option<String>,
}

fn default_aggregation() -> String {
    "probabilistic_or".into()
}
fn default_max_score() -> f64 {
    100.0
}

impl RulesetBody {
    fn check(&self) -> AppResult<()> {
        let spec = serde_json::json!({
            "code": self.code, "name": self.name, "event_types": self.event_types, "typologies": self.typologies,
            "aggregation": self.aggregation, "max_score": self.max_score, "rules": []
        });
        let spec: rule_engine::RulesetSpec = serde_json::from_value(spec)
            .map_err(|e| AppError::field("aggregation", format!("invalid ruleset: {e}")))?;
        let report = rule_engine::validate::validate_ruleset(&spec);
        if report.valid {
            Ok(())
        } else {
            Err(report_to_error(&report))
        }
    }

    fn fields(&self) -> RulesetFields<'_> {
        RulesetFields {
            code: &self.code,
            name: &self.name,
            description: self.description.as_deref(),
            event_types: &self.event_types,
            typologies: &self.typologies,
            aggregation: &self.aggregation,
            max_score: self.max_score,
        }
    }
}

pub async fn create_ruleset(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    body: &RulesetBody,
) -> AppResult<RulesetRow> {
    body.check()?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rs = repo::insert_ruleset(
        &mut tx,
        tenant,
        project,
        &body.fields(),
        Status::Draft,
        caller.actor_user_id().map(|u| u.0),
    )
    .await?;
    repo::snapshot_ruleset(
        &mut tx,
        rs.id,
        body.change_note.as_deref(),
        caller.actor_user_id().map(|u| u.0),
    )
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "ruleset.create")
            .scope(tenant, Some(project))
            .subject("ruleset", rs.id)
            .after(&rs),
    )
    .await?;
    tx.commit().await?;
    Ok(rs)
}

pub async fn update_ruleset(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    body: &RulesetBody,
) -> AppResult<RulesetRow> {
    body.check()?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let before = repo::get_ruleset(&mut tx, project, id).await?;
    if before.code != body.code {
        return Err(AppError::field("code", "the ruleset code is immutable"));
    }
    lifecycle::transition(before.status()?, Action::Edit, None, None).map_err(lifecycle_err)?;
    // New draft version; the approved snapshot(s) keep serving until this version is approved.
    let rs = repo::update_ruleset(&mut tx, id, &body.fields()).await?;
    repo::snapshot_ruleset(
        &mut tx,
        id,
        body.change_note.as_deref(),
        caller.actor_user_id().map(|u| u.0),
    )
    .await?;
    supersede_pending(&mut tx, project, "ruleset", id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "ruleset.update")
            .scope(tenant, Some(project))
            .subject("ruleset", id)
            .before(&before)
            .after(&rs)
            .metadata(serde_json::json!({ "version": rs.version })),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(rs)
}

/// One member in `PUT /rulesets/{id}/rules`.
#[derive(Debug, Clone, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberIn {
    pub rule_id: Uuid,
    #[serde(default = "default_weight")]
    pub weight: f64,
    #[serde(default)]
    pub pinned_version: Option<i32>,
}

fn default_weight() -> f64 {
    1.0
}

pub async fn set_members(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    members: &[MemberIn],
) -> AppResult<RulesetRow> {
    let mut errors = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, m) in members.iter().enumerate() {
        if !(0.0..=10.0).contains(&m.weight) {
            errors.push(FieldError::new(
                format!("[{i}].weight"),
                "must be between 0 and 10",
            ));
        }
        if !seen.insert(m.rule_id) {
            errors.push(FieldError::new(format!("[{i}].rule_id"), "duplicate rule"));
        }
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let before = repo::get_ruleset(&mut tx, project, id).await?;
    lifecycle::transition(before.status()?, Action::Edit, None, None).map_err(lifecycle_err)?;
    for (i, m) in members.iter().enumerate() {
        let rule = repo::get_rule(&mut tx, project, m.rule_id)
            .await
            .map_err(|_| AppError::field(format!("[{i}].rule_id"), "rule not found in this project"))?;
        if rule.status == "retired" {
            return Err(AppError::field(format!("[{i}].rule_id"), "rule is retired"));
        }
        if let Some(v) = m.pinned_version {
            repo::get_version(&mut tx, m.rule_id, v)
                .await
                .map_err(|_| AppError::field(format!("[{i}].pinned_version"), "version does not exist"))?;
        }
    }
    let old = repo::members(&mut tx, project, &[id]).await?;
    let triples: Vec<_> = members
        .iter()
        .map(|m| (m.rule_id, m.weight, m.pinned_version))
        .collect();
    repo::replace_members(&mut tx, tenant, id, &triples).await?;
    let rs = sqlx::query_as::<_, RulesetRow>(
        "UPDATE rules.rulesets SET version = version + 1, status = 'draft', submitted_by = NULL, submitted_at = NULL \
         WHERE id = $1 RETURNING id, tenant_id, project_id, code, name, description, event_types, typologies, \
         aggregation, max_score, version, status, submitted_by, submitted_at, created_by, created_at, updated_at",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    repo::snapshot_ruleset(
        &mut tx,
        id,
        Some("membership change"),
        caller.actor_user_id().map(|u| u.0),
    )
    .await?;
    supersede_pending(&mut tx, project, "ruleset", id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "ruleset.members.update")
            .scope(tenant, Some(project))
            .subject("ruleset", id)
            .before(&old)
            .after(serde_json::json!(members
                .iter()
                .map(|m| serde_json::json!({"rule_id": m.rule_id, "weight": m.weight, "pinned_version": m.pinned_version}))
                .collect::<Vec<_>>())),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(rs)
}

pub async fn ruleset_action(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    action: Action,
    comment: Option<&str>,
) -> AppResult<RulesetRow> {
    let actor = caller.actor_user_id().map(|u| u.0);
    if !matches!(action, Action::Submit) && caller.user().is_none() {
        return Err(AppError::Forbidden(
            "approval decisions require a user token".into(),
        ));
    }
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let rs: RulesetRow = sqlx::query_as(
        "SELECT id, tenant_id, project_id, code, name, description, event_types, typologies, aggregation, max_score, \
         version, status, submitted_by, submitted_at, created_by, created_at, updated_at FROM rules.rulesets \
         WHERE project_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::not_found("ruleset not found"))?;
    let next = lifecycle::transition(rs.status()?, action, rs.submitted_by, actor).map_err(lifecycle_err)?;
    let (verb, updated) = match action {
        Action::Submit => {
            let r = repo::set_ruleset_status(&mut tx, id, next, Some(actor)).await?;
            repo::open_approval(&mut tx, tenant, project, "ruleset", id, Some(rs.version), actor).await?;
            ("ruleset.submit", r)
        }
        Action::Approve(target) => {
            let ledger = repo::subject_ledger(&mut tx, project, "ruleset", id).await?;
            let before = lifecycle::serving_from_ledger(&ledger, false);
            let status = lifecycle::status_after_approval(target, rs.version, before);
            let r = repo::set_ruleset_status(&mut tx, id, status, None).await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "ruleset",
                id,
                Some(rs.version),
                rs.submitted_by,
                actor,
                "approved",
                Some(target),
                comment,
            )
            .await?;
            ("ruleset.approve", r)
        }
        Action::Reject => {
            let r = repo::set_ruleset_status(&mut tx, id, next, None).await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "ruleset",
                id,
                Some(rs.version),
                rs.submitted_by,
                actor,
                "rejected",
                None,
                comment,
            )
            .await?;
            ("ruleset.reject", r)
        }
        Action::Retire => {
            let r = repo::set_ruleset_status(&mut tx, id, next, None).await?;
            supersede_pending(&mut tx, project, "ruleset", id).await?;
            ("ruleset.retire", r)
        }
        Action::Edit => return Err(AppError::BadRequest("use PUT to edit a ruleset".into())),
    };
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, verb)
            .scope(tenant, Some(project))
            .subject("ruleset", id)
            .before(serde_json::json!({ "status": rs.status }))
            .after(serde_json::json!({ "status": updated.status, "version": rs.version }))
            .metadata(serde_json::json!({ "comment": comment })),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(updated)
}

// -------------------------------------------------------------------------------------------------------------
// Proposals
// -------------------------------------------------------------------------------------------------------------

/// Approves a pending proposal: applies it as a **shadow** rule/version (or retires the target).
pub async fn approve_proposal(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    comment: Option<&str>,
) -> AppResult<repo::ProposalRow> {
    let reviewer = caller
        .user()
        .map(|u| u.user_id().0)
        .ok_or_else(|| AppError::Forbidden("approval decisions require a user token".into()))?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let p = repo::get_proposal(&mut tx, project, id, true).await?;
    proposal::review(&p.status, p.created_by, reviewer).map_err(|m| AppError::Conflict(m.into()))?;
    let ptype = ProposalType::parse(&p.proposal_type)
        .ok_or_else(|| AppError::internal(format!("bad proposal type {}", p.proposal_type)))?;

    let applied_rule_id = match ptype {
        ProposalType::NewRule => {
            let def = p
                .definition
                .as_ref()
                .ok_or_else(|| AppError::Conflict("proposal has no definition".into()))?;
            let v = validated(state, tenant, project, def).await?;
            let rule = repo::insert_rule(
                &mut tx,
                tenant,
                project,
                &v.new_version(),
                Status::Shadow,
                p.created_by,
            )
            .await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "rule",
                rule.id,
                Some(1),
                p.created_by,
                Some(reviewer),
                "approved",
                Some(TargetStatus::Shadow),
                Some("applied from proposal"),
            )
            .await?;
            Some(rule.id)
        }
        ProposalType::ModifyRule | ProposalType::TuneThreshold => {
            let target = p
                .target_rule_id
                .ok_or_else(|| AppError::Conflict("proposal has no target rule".into()))?;
            let rule = repo::get_rule_for_update(&mut tx, project, target).await?;
            if rule.status == "retired" {
                return Err(AppError::Conflict("target rule is retired".into()));
            }
            let mut def = p
                .definition
                .clone()
                .ok_or_else(|| AppError::Conflict("proposal has no definition".into()))?;
            if let Some(obj) = def.as_object_mut() {
                obj.insert("code".into(), Value::String(rule.code.clone()));
            }
            let v = validated(state, tenant, project, &def).await?;
            if v.kind != rule.kind {
                return Err(AppError::Conflict(
                    "proposal changes the rule kind; propose a new rule instead".into(),
                ));
            }
            let ledger = repo::rule_ledger(&mut tx, project, target).await?;
            let has_live = lifecycle::serving_from_ledger(&ledger, false)
                .live_version
                .is_some();
            let status = if has_live { Status::Active } else { Status::Shadow };
            let updated =
                repo::add_version(&mut tx, tenant, &rule, &v.new_version(), status, p.created_by).await?;
            supersede_pending(&mut tx, project, "rule", target).await?;
            repo::decide_approval(
                &mut tx,
                tenant,
                project,
                "rule",
                target,
                Some(updated.current_version),
                p.created_by,
                Some(reviewer),
                "approved",
                Some(TargetStatus::Shadow),
                Some("applied from proposal"),
            )
            .await?;
            Some(target)
        }
        ProposalType::RetireRule => {
            let target = p
                .target_rule_id
                .ok_or_else(|| AppError::Conflict("proposal has no target rule".into()))?;
            let rule = repo::get_rule_for_update(&mut tx, project, target).await?;
            let next = lifecycle::transition(rule.status()?, Action::Retire, None, Some(reviewer))
                .map_err(lifecycle_err)?;
            repo::set_rule_status(&mut tx, target, next, None).await?;
            Some(target)
        }
    };
    let row = repo::review_proposal(&mut tx, id, "applied", reviewer, comment, applied_rule_id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "proposal.approve")
            .scope(tenant, Some(project))
            .subject("proposal", id)
            .after(serde_json::json!({ "status": "applied", "applied_rule_id": applied_rule_id, "type": p.proposal_type })),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(row)
}

pub async fn reject_proposal(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    id: Uuid,
    comment: Option<&str>,
) -> AppResult<repo::ProposalRow> {
    let reviewer = caller
        .user()
        .map(|u| u.user_id().0)
        .ok_or_else(|| AppError::Forbidden("approval decisions require a user token".into()))?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let p = repo::get_proposal(&mut tx, project, id, true).await?;
    proposal::review(&p.status, p.created_by, reviewer).map_err(|m| AppError::Conflict(m.into()))?;
    let row = repo::review_proposal(&mut tx, id, "rejected", reviewer, comment, None).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "proposal.reject")
            .scope(tenant, Some(project))
            .subject("proposal", id)
            .metadata(serde_json::json!({ "comment": comment })),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

// -------------------------------------------------------------------------------------------------------------
// Bootstrap
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct BootstrapReport {
    pub template: String,
    pub lists_created: u32,
    pub entries_upserted: u64,
    pub rules_created: u32,
    pub rulesets_created: u32,
    /// Codes/names that already existed (idempotent re-run).
    pub skipped: Vec<String>,
}

/// Creates the template's lists, rules and rulesets (idempotent by code / name). Template rules go live
/// immediately: they are platform-reviewed content, recorded in the ledger as approved by the system.
pub async fn bootstrap(
    state: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    template_name: &str,
) -> AppResult<BootstrapReport> {
    let template = templates::load(template_name).map_err(|m| AppError::field("template", m))?;
    let catalog = state.catalogs.get(tenant, project).await?;
    let mut report = BootstrapReport {
        template: template_name.into(),
        ..Default::default()
    };
    let scope = ListScope::Project(tenant, project);
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;

    for list in &template.reference_lists {
        let list_id = match repo::find_list_by_name(&mut tx, scope, &list.name).await? {
            Some(id) => {
                report.skipped.push(format!("list:{}", list.name));
                id
            }
            None => {
                report.lists_created += 1;
                repo::insert_list(
                    &mut tx,
                    scope,
                    &list.name,
                    Some(&list.description),
                    &list.list_type,
                    &list.key_kind,
                    &list.columns,
                    None,
                )
                .await?
            }
        };
        let entries: Vec<EntryRow> = list
            .entries
            .iter()
            .map(|e| EntryRow {
                key: e.key.clone(),
                attributes: if e.attributes.is_object() {
                    e.attributes.clone()
                } else {
                    serde_json::json!({})
                },
                valid_from: None,
                valid_until: None,
                reason: e.reason.clone(),
            })
            .collect();
        report.entries_upserted += repo::upsert_entries(&mut tx, tenant, list_id, &entries, None).await?;
    }

    let mut rule_ids: HashMap<String, Uuid> = HashMap::new();
    for raw in &template.rules {
        let (validation, parsed) = validate_rule_json(raw, catalog.as_ref());
        let envelope = match parsed {
            Some(e) if validation.valid => e,
            _ => return Err(report_to_error(&validation)),
        };
        if let Some(existing) = repo::find_rule_by_code(&mut tx, project, &envelope.code).await? {
            report.skipped.push(format!("rule:{}", envelope.code));
            rule_ids.insert(envelope.code.clone(), existing.id);
            continue;
        }
        let v = Validated {
            definition: serde_json::to_value(&envelope.definition)
                .map_err(|e| AppError::internal(format!("serialize definition: {e}")))?,
            kind: envelope.kind.as_str().to_string(),
            action: enum_str(envelope.action),
            on_trapped: enum_str(envelope.on_trapped),
            change_note: Some(format!("bootstrap template {template_name}")),
            envelope,
        };
        let rule =
            repo::insert_rule(&mut tx, tenant, project, &v.new_version(), Status::Active, None).await?;
        repo::decide_approval(
            &mut tx,
            tenant,
            project,
            "rule",
            rule.id,
            Some(1),
            None,
            None,
            "approved",
            Some(TargetStatus::Active),
            Some(&format!("bootstrap template {template_name}")),
        )
        .await?;
        rule_ids.insert(v.envelope.code.clone(), rule.id);
        report.rules_created += 1;
    }

    for t in &template.rulesets {
        if repo::find_ruleset_by_code(&mut tx, project, &t.code)
            .await?
            .is_some()
        {
            report.skipped.push(format!("ruleset:{}", t.code));
            continue;
        }
        let body = RulesetBody {
            code: t.code.clone(),
            name: t.name.clone(),
            description: Some(t.description.clone()),
            event_types: t.event_types.clone(),
            typologies: t.typologies.clone(),
            aggregation: t.aggregation.clone(),
            max_score: t.max_score,
            change_note: Some(format!("bootstrap template {template_name}")),
        };
        body.check()?;
        let rs = repo::insert_ruleset(&mut tx, tenant, project, &body.fields(), Status::Active, None).await?;
        let members: Vec<(Uuid, f64, Option<i32>)> = t
            .members
            .iter()
            .filter_map(|m| rule_ids.get(&m.rule).map(|id| (*id, m.weight, None)))
            .collect();
        repo::replace_members(&mut tx, tenant, rs.id, &members).await?;
        // Version 1 snapshot, approved by the system so template rulesets serve immediately.
        repo::snapshot_ruleset(&mut tx, rs.id, body.change_note.as_deref(), None).await?;
        repo::decide_approval(
            &mut tx,
            tenant,
            project,
            "ruleset",
            rs.id,
            Some(rs.version),
            None,
            None,
            "approved",
            Some(TargetStatus::Active),
            Some(&format!("bootstrap template {template_name}")),
        )
        .await?;
        report.rulesets_created += 1;
    }

    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "rules.bootstrap")
            .scope(tenant, Some(project))
            .subject("project", project)
            .after(&report),
    )
    .await?;
    tx.commit().await?;
    state.serving.invalidate(project).await;
    Ok(report)
}
