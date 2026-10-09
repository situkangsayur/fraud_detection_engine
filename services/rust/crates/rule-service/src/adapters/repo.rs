//! Postgres repositories for the `rules` schema (+ `core.approvals` for the maker–checker ledger).
//!
//! Functions take `&mut PgConnection`, so the **caller owns the transaction** (a [`platform::db::TenantTx`]):
//! a rule insert, its first version and the audit row commit or roll back together. This is the Rust
//! equivalent of Spring's `@Transactional` service method, but explicit: you can see where the transaction
//! begins and ends. Every query filters by `project_id` (or `tenant_id` for tenant-wide lists) even though RLS
//! already isolates tenants.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use platform::{AppError, AppResult, ProjectId, TenantId};

use crate::domain::lifecycle::{ApprovedVersion, Status, TargetStatus};

// -------------------------------------------------------------------------------------------------------------
// Rules
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RuleRow {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub typologies: Vec<String>,
    pub event_types: Vec<String>,
    pub current_version: i32,
    pub status: String,
    pub submitted_by: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RuleRow {
    pub fn status(&self) -> AppResult<Status> {
        Status::parse(&self.status)
            .ok_or_else(|| AppError::internal(format!("bad rule status {}", self.status)))
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct VersionRow {
    pub rule_id: Uuid,
    pub version: i32,
    pub definition: Value,
    pub risk_score: f32,
    pub trapped_score: f32,
    pub action: String,
    pub on_trapped: String,
    pub missing_as_no_match: bool,
    pub change_note: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Reserved key inside `rule_versions.definition` holding the version's envelope metadata.
///
/// `rules.rules` only has columns for the *latest* name/typologies/event_types, but those fields change what a
/// rule does (e.g. `event_types` decides where it runs), so they must be versioned with the definition: an
/// edit may only take effect once approved. Each version therefore stores its metadata here; the key is
/// stripped before the definition reaches the engine.
pub const META_KEY: &str = "_envelope";

/// Stored form of a definition: the kind-specific body plus [`META_KEY`].
pub fn stored_definition(nv: &NewVersion<'_>) -> Value {
    let mut def = nv.definition.clone();
    if let Some(obj) = def.as_object_mut() {
        obj.insert(
            META_KEY.into(),
            json!({
                "name": nv.name,
                "description": nv.description,
                "typologies": nv.typologies,
                "event_types": nv.event_types,
            }),
        );
    }
    def
}

/// The kind-specific body without the metadata key.
pub fn plain_definition(stored: &Value) -> Value {
    let mut def = stored.clone();
    if let Some(obj) = def.as_object_mut() {
        obj.remove(META_KEY);
    }
    def
}

/// Rebuilds the rule-dsl §2 envelope of a stored version (versioned metadata wins over the rule row).
pub fn envelope(rule: &RuleRow, v: &VersionRow) -> Value {
    let meta = v.definition.get(META_KEY).cloned().unwrap_or(Value::Null);
    let pick = |key: &str, fallback: Value| match meta.get(key) {
        Some(val) if !val.is_null() => val.clone(),
        _ => fallback,
    };
    let mut e = json!({
        "code": rule.code,
        "name": pick("name", json!(rule.name)),
        "kind": rule.kind,
        "typologies": pick("typologies", json!(rule.typologies)),
        "event_types": pick("event_types", json!(rule.event_types)),
        "risk_score": f64::from(v.risk_score),
        "trapped_score": f64::from(v.trapped_score),
        "action": v.action,
        "on_trapped": v.on_trapped,
        "missing_as_no_match": v.missing_as_no_match,
        "definition": plain_definition(&v.definition),
    });
    let description = pick("description", json!(rule.description));
    if let (false, Some(obj)) = (description.is_null(), e.as_object_mut()) {
        obj.insert("description".into(), description);
    }
    e
}

const RULE_COLS: &str = "id, tenant_id, project_id, code, name, description, kind, typologies, event_types, \
                         current_version, status, submitted_by, submitted_at, created_by, created_at, updated_at";

pub async fn get_rule(conn: &mut PgConnection, project: ProjectId, id: Uuid) -> AppResult<RuleRow> {
    sqlx::query_as::<_, RuleRow>(&format!(
        "SELECT {RULE_COLS} FROM rules.rules WHERE project_id = $1 AND id = $2"
    ))
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("rule not found"))
}

pub async fn get_rule_for_update(
    conn: &mut PgConnection,
    project: ProjectId,
    id: Uuid,
) -> AppResult<RuleRow> {
    sqlx::query_as::<_, RuleRow>(&format!(
        "SELECT {RULE_COLS} FROM rules.rules WHERE project_id = $1 AND id = $2 FOR UPDATE"
    ))
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("rule not found"))
}

pub async fn find_rule_by_code(
    conn: &mut PgConnection,
    project: ProjectId,
    code: &str,
) -> AppResult<Option<RuleRow>> {
    Ok(sqlx::query_as::<_, RuleRow>(&format!(
        "SELECT {RULE_COLS} FROM rules.rules WHERE project_id = $1 AND code = $2"
    ))
    .bind(project.as_uuid())
    .bind(code)
    .fetch_optional(conn)
    .await?)
}

#[derive(Debug, Clone, Default)]
pub struct RuleFilter {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub typology: Option<String>,
    pub q: Option<String>,
}

pub async fn list_rules(
    conn: &mut PgConnection,
    project: ProjectId,
    f: &RuleFilter,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<RuleRow>, i64)> {
    let q =
        f.q.as_ref()
            .map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
    let where_sql =
        "project_id = $1 AND ($2::text IS NULL OR kind = $2) AND ($3::text IS NULL OR status = $3) \
                     AND ($4::text IS NULL OR $4 = ANY(typologies)) \
                     AND ($5::text IS NULL OR code ILIKE $5 OR name ILIKE $5 OR description ILIKE $5)";
    let rows = sqlx::query_as::<_, RuleRow>(&format!(
        "SELECT {RULE_COLS} FROM rules.rules WHERE {where_sql} ORDER BY code LIMIT $6 OFFSET $7"
    ))
    .bind(project.as_uuid())
    .bind(&f.kind)
    .bind(&f.status)
    .bind(&f.typology)
    .bind(&q)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM rules.rules WHERE {where_sql}"))
        .bind(project.as_uuid())
        .bind(&f.kind)
        .bind(&f.status)
        .bind(&f.typology)
        .bind(&q)
        .fetch_one(conn)
        .await?;
    Ok((rows, total))
}

pub async fn get_version(conn: &mut PgConnection, rule_id: Uuid, version: i32) -> AppResult<VersionRow> {
    sqlx::query_as::<_, VersionRow>(
        "SELECT rule_id, version, definition, risk_score, trapped_score, action, on_trapped, missing_as_no_match, \
         change_note, created_by, created_at FROM rules.rule_versions WHERE rule_id = $1 AND version = $2",
    )
    .bind(rule_id)
    .bind(version)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("rule version not found"))
}

pub async fn list_versions(conn: &mut PgConnection, rule_id: Uuid) -> AppResult<Vec<VersionRow>> {
    Ok(sqlx::query_as::<_, VersionRow>(
        "SELECT rule_id, version, definition, risk_score, trapped_score, action, on_trapped, missing_as_no_match, \
         change_note, created_by, created_at FROM rules.rule_versions WHERE rule_id = $1 ORDER BY version DESC",
    )
    .bind(rule_id)
    .fetch_all(conn)
    .await?)
}

/// Fields of a validated envelope needed for persistence.
#[derive(Debug, Clone)]
pub struct NewVersion<'a> {
    pub code: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub kind: &'a str,
    pub typologies: &'a [String],
    pub event_types: &'a [String],
    pub definition: &'a Value,
    pub risk_score: f64,
    pub trapped_score: f64,
    pub action: &'a str,
    pub on_trapped: &'a str,
    pub missing_as_no_match: bool,
    pub change_note: Option<&'a str>,
}

pub async fn insert_rule(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    nv: &NewVersion<'_>,
    status: Status,
    created_by: Option<Uuid>,
) -> AppResult<RuleRow> {
    let rule = sqlx::query_as::<_, RuleRow>(&format!(
        "INSERT INTO rules.rules (tenant_id, project_id, code, name, description, kind, typologies, event_types, \
         current_version, status, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1,$9,$10) RETURNING {RULE_COLS}"
    ))
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(nv.code)
    .bind(nv.name)
    .bind(nv.description)
    .bind(nv.kind)
    .bind(nv.typologies)
    .bind(nv.event_types)
    .bind(status.as_str())
    .bind(created_by)
    .fetch_one(&mut *conn)
    .await?;
    insert_version(conn, tenant, rule.id, 1, nv, created_by).await?;
    Ok(rule)
}

pub async fn insert_version(
    conn: &mut PgConnection,
    tenant: TenantId,
    rule_id: Uuid,
    version: i32,
    nv: &NewVersion<'_>,
    created_by: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO rules.rule_versions (rule_id, tenant_id, version, definition, risk_score, trapped_score, action, \
         on_trapped, missing_as_no_match, change_note, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(rule_id)
    .bind(tenant.as_uuid())
    .bind(version)
    .bind(stored_definition(nv))
    .bind(nv.risk_score as f32)
    .bind(nv.trapped_score as f32)
    .bind(nv.action)
    .bind(nv.on_trapped)
    .bind(nv.missing_as_no_match)
    .bind(nv.change_note)
    .bind(created_by)
    .execute(conn)
    .await?;
    Ok(())
}

/// New version of an existing rule: bumps `current_version`, updates envelope metadata, sets `status`.
pub async fn add_version(
    conn: &mut PgConnection,
    tenant: TenantId,
    rule: &RuleRow,
    nv: &NewVersion<'_>,
    status: Status,
    created_by: Option<Uuid>,
) -> AppResult<RuleRow> {
    let next = rule.current_version + 1;
    insert_version(conn, tenant, rule.id, next, nv, created_by).await?;
    Ok(sqlx::query_as::<_, RuleRow>(&format!(
        "UPDATE rules.rules SET name = $2, description = $3, typologies = $4, event_types = $5, current_version = $6, \
         status = $7, submitted_by = NULL, submitted_at = NULL WHERE id = $1 RETURNING {RULE_COLS}"
    ))
    .bind(rule.id)
    .bind(nv.name)
    .bind(nv.description)
    .bind(nv.typologies)
    .bind(nv.event_types)
    .bind(next)
    .bind(status.as_str())
    .fetch_one(conn)
    .await?)
}

pub async fn set_rule_status(
    conn: &mut PgConnection,
    id: Uuid,
    status: Status,
    submitted_by: Option<Option<Uuid>>,
) -> AppResult<RuleRow> {
    // `submitted_by = Some(x)` sets (or clears, for `Some(None)`) the maker; `None` leaves it untouched.
    let sql = format!(
        "UPDATE rules.rules SET status = $2, \
         submitted_by = CASE WHEN $4 THEN $3 ELSE submitted_by END, \
         submitted_at = CASE WHEN $4 THEN (CASE WHEN $3 IS NULL THEN NULL ELSE now() END) ELSE submitted_at END \
         WHERE id = $1 RETURNING {RULE_COLS}"
    );
    Ok(sqlx::query_as::<_, RuleRow>(&sql)
        .bind(id)
        .bind(status.as_str())
        .bind(submitted_by.flatten())
        .bind(submitted_by.is_some())
        .fetch_one(conn)
        .await?)
}

// -------------------------------------------------------------------------------------------------------------
// Approval ledger (core.approvals)
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApprovalRow {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub subject_version: Option<i32>,
    pub requested_by: Option<Uuid>,
    pub requested_at: DateTime<Utc>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision: Option<String>,
    pub target_status: Option<String>,
    pub comment: Option<String>,
}

const APPROVAL_COLS: &str =
    "id, subject_type, subject_id, subject_version, requested_by, requested_at, decided_by, \
                             decided_at, decision, target_status, comment";

pub async fn open_approval(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    subject_type: &str,
    subject_id: Uuid,
    version: Option<i32>,
    requested_by: Option<Uuid>,
) -> AppResult<()> {
    // Close any stale pending request of the same subject first (resubmission).
    sqlx::query(
        "UPDATE core.approvals SET decision = 'rejected', decided_at = now(), comment = 'superseded by resubmission' \
         WHERE project_id = $1 AND subject_type = $2 AND subject_id = $3 AND decision IS NULL",
    )
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO core.approvals (tenant_id, project_id, subject_type, subject_id, subject_version, requested_by) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .bind(version)
    .bind(requested_by)
    .execute(conn)
    .await?;
    Ok(())
}

/// Decides the pending approval of a subject (or records a decided one if none is pending, e.g. bootstrap).
#[allow(clippy::too_many_arguments)]
pub async fn decide_approval(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    subject_type: &str,
    subject_id: Uuid,
    version: Option<i32>,
    requested_by: Option<Uuid>,
    decided_by: Option<Uuid>,
    decision: &str,
    target: Option<TargetStatus>,
    comment: Option<&str>,
) -> AppResult<ApprovalRow> {
    let updated = sqlx::query_as::<_, ApprovalRow>(&format!(
        "UPDATE core.approvals SET decided_by = $4, decided_at = now(), decision = $5, target_status = $6, comment = $7 \
         WHERE id = (SELECT id FROM core.approvals WHERE project_id = $1 AND subject_type = $2 AND subject_id = $3 \
                     AND decision IS NULL ORDER BY requested_at DESC LIMIT 1) RETURNING {APPROVAL_COLS}"
    ))
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .bind(decided_by)
    .bind(decision)
    .bind(target.map(TargetStatus::as_str))
    .bind(comment)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(row) = updated {
        return Ok(row);
    }
    Ok(sqlx::query_as::<_, ApprovalRow>(&format!(
        "INSERT INTO core.approvals (tenant_id, project_id, subject_type, subject_id, subject_version, requested_by, \
         decided_by, decided_at, decision, target_status, comment) VALUES ($1,$2,$3,$4,$5,$6,$7,now(),$8,$9,$10) \
         RETURNING {APPROVAL_COLS}"
    ))
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .bind(version)
    .bind(requested_by)
    .bind(decided_by)
    .bind(decision)
    .bind(target.map(TargetStatus::as_str))
    .bind(comment)
    .fetch_one(conn)
    .await?)
}

pub async fn approvals_of(
    conn: &mut PgConnection,
    project: ProjectId,
    subject_id: Uuid,
) -> AppResult<Vec<ApprovalRow>> {
    Ok(sqlx::query_as::<_, ApprovalRow>(&format!(
        "SELECT {APPROVAL_COLS} FROM core.approvals WHERE project_id = $1 AND subject_id = $2 ORDER BY requested_at"
    ))
    .bind(project.as_uuid())
    .bind(subject_id)
    .fetch_all(conn)
    .await?)
}

/// Approved decisions of all rules/rulesets of a project, chronological (input of the serving computation).
pub async fn approved_ledger(
    conn: &mut PgConnection,
    project: ProjectId,
) -> AppResult<Vec<(String, Uuid, Option<i32>, TargetStatus)>> {
    let rows: Vec<(String, Uuid, Option<i32>, String)> = sqlx::query_as(
        "SELECT subject_type, subject_id, subject_version, target_status FROM core.approvals \
         WHERE project_id = $1 AND decision = 'approved' AND subject_type IN ('rule','ruleset') \
         AND target_status IN ('active','shadow') ORDER BY decided_at, requested_at, id",
    )
    .bind(project.as_uuid())
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(t, id, v, target)| TargetStatus::parse(&target).map(|ts| (t, id, v, ts)))
        .collect())
}

/// Approved ledger of one subject (`rule` or `ruleset`), chronological, as the pure serving function expects it.
pub async fn subject_ledger(
    conn: &mut PgConnection,
    project: ProjectId,
    subject_type: &str,
    subject_id: Uuid,
) -> AppResult<Vec<ApprovedVersion>> {
    let rows: Vec<(Option<i32>, String)> = sqlx::query_as(
        "SELECT subject_version, target_status FROM core.approvals WHERE project_id = $1 AND subject_type = $2 \
         AND subject_id = $3 AND decision = 'approved' AND target_status IN ('active','shadow') \
         ORDER BY decided_at, requested_at, id",
    )
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(v, t)| {
            Some(ApprovedVersion {
                version: v?,
                target: TargetStatus::parse(&t)?,
            })
        })
        .collect())
}

/// Ledger of one rule.
pub async fn rule_ledger(
    conn: &mut PgConnection,
    project: ProjectId,
    rule_id: Uuid,
) -> AppResult<Vec<ApprovedVersion>> {
    subject_ledger(conn, project, "rule", rule_id).await
}

// -------------------------------------------------------------------------------------------------------------
// Rulesets
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RulesetRow {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub event_types: Vec<String>,
    pub typologies: Vec<String>,
    pub aggregation: String,
    pub max_score: f32,
    pub version: i32,
    pub status: String,
    pub submitted_by: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RulesetRow {
    pub fn status(&self) -> AppResult<Status> {
        Status::parse(&self.status)
            .ok_or_else(|| AppError::internal(format!("bad ruleset status {}", self.status)))
    }
}

const RULESET_COLS: &str = "id, tenant_id, project_id, code, name, description, event_types, typologies, aggregation, \
                            max_score, version, status, submitted_by, submitted_at, created_by, created_at, updated_at";

pub async fn get_ruleset(conn: &mut PgConnection, project: ProjectId, id: Uuid) -> AppResult<RulesetRow> {
    sqlx::query_as::<_, RulesetRow>(&format!(
        "SELECT {RULESET_COLS} FROM rules.rulesets WHERE project_id = $1 AND id = $2"
    ))
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("ruleset not found"))
}

pub async fn find_ruleset_by_code(
    conn: &mut PgConnection,
    project: ProjectId,
    code: &str,
) -> AppResult<Option<RulesetRow>> {
    Ok(sqlx::query_as::<_, RulesetRow>(&format!(
        "SELECT {RULESET_COLS} FROM rules.rulesets WHERE project_id = $1 AND code = $2"
    ))
    .bind(project.as_uuid())
    .bind(code)
    .fetch_optional(conn)
    .await?)
}

pub async fn list_rulesets(
    conn: &mut PgConnection,
    project: ProjectId,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<RulesetRow>, i64)> {
    let rows = sqlx::query_as::<_, RulesetRow>(&format!(
        "SELECT {RULESET_COLS} FROM rules.rulesets WHERE project_id = $1 AND ($2::text IS NULL OR status = $2) \
         ORDER BY code LIMIT $3 OFFSET $4"
    ))
    .bind(project.as_uuid())
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rules.rulesets WHERE project_id = $1 AND ($2::text IS NULL OR status = $2)",
    )
    .bind(project.as_uuid())
    .bind(status)
    .fetch_one(conn)
    .await?;
    Ok((rows, total))
}

#[derive(Debug, Clone)]
pub struct RulesetFields<'a> {
    pub code: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub event_types: &'a [String],
    pub typologies: &'a [String],
    pub aggregation: &'a str,
    pub max_score: f64,
}

pub async fn insert_ruleset(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    f: &RulesetFields<'_>,
    status: Status,
    created_by: Option<Uuid>,
) -> AppResult<RulesetRow> {
    Ok(sqlx::query_as::<_, RulesetRow>(&format!(
        "INSERT INTO rules.rulesets (tenant_id, project_id, code, name, description, event_types, typologies, \
         aggregation, max_score, status, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING {RULESET_COLS}"
    ))
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(f.code)
    .bind(f.name)
    .bind(f.description)
    .bind(f.event_types)
    .bind(f.typologies)
    .bind(f.aggregation)
    .bind(f.max_score as f32)
    .bind(status.as_str())
    .bind(created_by)
    .fetch_one(conn)
    .await?)
}

pub async fn update_ruleset(
    conn: &mut PgConnection,
    id: Uuid,
    f: &RulesetFields<'_>,
) -> AppResult<RulesetRow> {
    Ok(sqlx::query_as::<_, RulesetRow>(&format!(
        "UPDATE rules.rulesets SET name = $2, description = $3, event_types = $4, typologies = $5, aggregation = $6, \
         max_score = $7, version = version + 1, status = 'draft', submitted_by = NULL, submitted_at = NULL \
         WHERE id = $1 RETURNING {RULESET_COLS}"
    ))
    .bind(id)
    .bind(f.name)
    .bind(f.description)
    .bind(f.event_types)
    .bind(f.typologies)
    .bind(f.aggregation)
    .bind(f.max_score as f32)
    .fetch_one(conn)
    .await?)
}

pub async fn set_ruleset_status(
    conn: &mut PgConnection,
    id: Uuid,
    status: Status,
    submitted_by: Option<Option<Uuid>>,
) -> AppResult<RulesetRow> {
    // `submitted_by = Some(x)` sets (or clears, for `Some(None)`) the maker; `None` leaves it untouched.
    let sql = format!(
        "UPDATE rules.rulesets SET status = $2, \
         submitted_by = CASE WHEN $4 THEN $3 ELSE submitted_by END, \
         submitted_at = CASE WHEN $4 THEN (CASE WHEN $3 IS NULL THEN NULL ELSE now() END) ELSE submitted_at END \
         WHERE id = $1 RETURNING {RULESET_COLS}"
    );
    Ok(sqlx::query_as::<_, RulesetRow>(&sql)
        .bind(id)
        .bind(status.as_str())
        .bind(submitted_by.flatten())
        .bind(submitted_by.is_some())
        .fetch_one(conn)
        .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct MemberRow {
    pub ruleset_id: Uuid,
    pub rule_id: Uuid,
    pub rule_code: String,
    pub rule_status: String,
    pub weight: f32,
    pub pinned_version: Option<i32>,
    pub position: i32,
}

pub async fn members(
    conn: &mut PgConnection,
    project: ProjectId,
    ruleset_ids: &[Uuid],
) -> AppResult<Vec<MemberRow>> {
    Ok(sqlx::query_as::<_, MemberRow>(
        "SELECT rr.ruleset_id, rr.rule_id, r.code AS rule_code, r.status AS rule_status, rr.weight, rr.pinned_version, \
         rr.position FROM rules.ruleset_rules rr JOIN rules.rules r ON r.id = rr.rule_id \
         WHERE r.project_id = $1 AND rr.ruleset_id = ANY($2) ORDER BY rr.ruleset_id, rr.position, r.code",
    )
    .bind(project.as_uuid())
    .bind(ruleset_ids)
    .fetch_all(conn)
    .await?)
}

pub async fn replace_members(
    conn: &mut PgConnection,
    tenant: TenantId,
    ruleset_id: Uuid,
    members: &[(Uuid, f64, Option<i32>)],
) -> AppResult<()> {
    sqlx::query("DELETE FROM rules.ruleset_rules WHERE ruleset_id = $1")
        .bind(ruleset_id)
        .execute(&mut *conn)
        .await?;
    for (pos, (rule_id, weight, pinned)) in members.iter().enumerate() {
        sqlx::query(
            "INSERT INTO rules.ruleset_rules (ruleset_id, rule_id, tenant_id, weight, pinned_version, position) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(ruleset_id)
        .bind(rule_id)
        .bind(tenant.as_uuid())
        .bind(*weight as f32)
        .bind(*pinned)
        .bind(i32::try_from(pos).unwrap_or(i32::MAX))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------------------
// Ruleset versions (immutable snapshots of config + membership, db/migrations/0011)
// -------------------------------------------------------------------------------------------------------------

/// One member inside a ruleset version snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SnapshotMember {
    pub rule_id: Uuid,
    pub weight: f64,
    #[serde(default)]
    pub pinned_version: Option<i32>,
    #[serde(default)]
    pub position: i32,
}

/// `rules.ruleset_versions.config`: everything that decides how a ruleset scores.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct RulesetConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub typologies: Vec<String>,
    pub aggregation: String,
    pub max_score: f64,
    #[serde(default)]
    pub members: Vec<SnapshotMember>,
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct RulesetVersionRow {
    pub ruleset_id: Uuid,
    pub version: i32,
    #[schema(value_type = Object)]
    pub config: Value,
    pub change_note: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl RulesetVersionRow {
    pub fn parsed(&self) -> AppResult<RulesetConfig> {
        serde_json::from_value(self.config.clone()).map_err(|e| {
            AppError::internal(format!(
                "ruleset {} v{} snapshot does not parse: {e}",
                self.ruleset_id, self.version
            ))
        })
    }
}

/// Freezes the current draft (header row + `ruleset_rules`) as version `rulesets.version`.
///
/// Called after every mutation that bumped `rulesets.version`. Snapshots are append-only: a duplicate version
/// is a programming error and surfaces as a conflict instead of silently rewriting an approved snapshot.
pub async fn snapshot_ruleset(
    conn: &mut PgConnection,
    ruleset_id: Uuid,
    change_note: Option<&str>,
    created_by: Option<Uuid>,
) -> AppResult<i32> {
    Ok(sqlx::query_scalar(
        "INSERT INTO rules.ruleset_versions (ruleset_id, tenant_id, version, config, change_note, created_by) \
         SELECT rs.id, rs.tenant_id, rs.version, jsonb_build_object( \
             'name', rs.name, 'description', rs.description, 'event_types', to_jsonb(rs.event_types), \
             'typologies', to_jsonb(rs.typologies), 'aggregation', rs.aggregation, 'max_score', rs.max_score, \
             'members', COALESCE((SELECT jsonb_agg(jsonb_build_object('rule_id', m.rule_id, 'weight', m.weight, \
                         'pinned_version', m.pinned_version, 'position', m.position) ORDER BY m.position) \
                         FROM rules.ruleset_rules m WHERE m.ruleset_id = rs.id), '[]'::jsonb)), $2, $3 \
         FROM rules.rulesets rs WHERE rs.id = $1 RETURNING version",
    )
    .bind(ruleset_id)
    .bind(change_note)
    .bind(created_by)
    .fetch_one(conn)
    .await?)
}

pub async fn ruleset_versions(
    conn: &mut PgConnection,
    ruleset_id: Uuid,
) -> AppResult<Vec<RulesetVersionRow>> {
    Ok(sqlx::query_as::<_, RulesetVersionRow>(
        "SELECT ruleset_id, version, config, change_note, created_by, created_at FROM rules.ruleset_versions \
         WHERE ruleset_id = $1 ORDER BY version DESC",
    )
    .bind(ruleset_id)
    .fetch_all(conn)
    .await?)
}

pub async fn ruleset_version(
    conn: &mut PgConnection,
    ruleset_id: Uuid,
    version: i32,
) -> AppResult<RulesetVersionRow> {
    sqlx::query_as::<_, RulesetVersionRow>(
        "SELECT ruleset_id, version, config, change_note, created_by, created_at FROM rules.ruleset_versions \
         WHERE ruleset_id = $1 AND version = $2",
    )
    .bind(ruleset_id)
    .bind(version)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("ruleset version not found"))
}

/// Project decision thresholds (`core.project_settings` key `decision_thresholds`), if configured.
pub async fn project_thresholds(conn: &mut PgConnection, project: ProjectId) -> AppResult<Option<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT value FROM core.project_settings WHERE project_id = $1 AND key = 'decision_thresholds'",
    )
    .bind(project.as_uuid())
    .fetch_optional(conn)
    .await?)
}

// -------------------------------------------------------------------------------------------------------------
// Reference lists
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct ListRow {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub list_type: String,
    pub key_kind: String,
    #[schema(value_type = Object)]
    pub columns: Value,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub entry_count: i64,
}

impl ListRow {
    pub fn scope(&self) -> &'static str {
        if self.project_id.is_some() {
            "project"
        } else {
            "tenant"
        }
    }
}

const LIST_COLS: &str = "l.id, l.tenant_id, l.project_id, l.name, l.description, l.list_type, l.key_kind, l.columns, \
                         l.created_by, l.created_at, l.updated_at, \
                         (SELECT count(*) FROM rules.reference_entries en WHERE en.list_id = l.id) AS entry_count";

/// Where a list lives: a project, or tenant-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListScope {
    Project(TenantId, ProjectId),
    Tenant(TenantId),
}

impl ListScope {
    pub fn tenant(&self) -> TenantId {
        match self {
            ListScope::Project(t, _) | ListScope::Tenant(t) => *t,
        }
    }
    pub fn project(&self) -> Option<ProjectId> {
        match self {
            ListScope::Project(_, p) => Some(*p),
            ListScope::Tenant(_) => None,
        }
    }
}

/// Lists visible in a scope: a project sees its own lists **and** the tenant-wide ones.
pub async fn list_lists(conn: &mut PgConnection, scope: ListScope) -> AppResult<Vec<ListRow>> {
    let rows = match scope {
        ListScope::Project(t, p) => sqlx::query_as::<_, ListRow>(&format!(
            "SELECT {LIST_COLS} FROM rules.reference_lists l WHERE l.tenant_id = $1 \
             AND (l.project_id = $2 OR l.project_id IS NULL) ORDER BY l.project_id IS NULL, l.name"
        ))
        .bind(t.as_uuid())
        .bind(p.as_uuid())
        .fetch_all(conn)
        .await?,
        ListScope::Tenant(t) => sqlx::query_as::<_, ListRow>(&format!(
            "SELECT {LIST_COLS} FROM rules.reference_lists l WHERE l.tenant_id = $1 AND l.project_id IS NULL ORDER BY l.name"
        ))
        .bind(t.as_uuid())
        .fetch_all(conn)
        .await?,
    };
    Ok(rows)
}

/// A list **owned** by the scope (project lists for a project scope, tenant lists for the tenant scope).
/// `readable` additionally allows a project to *read* tenant-wide lists.
pub async fn get_list(
    conn: &mut PgConnection,
    scope: ListScope,
    id: Uuid,
    readable: bool,
) -> AppResult<ListRow> {
    let row = sqlx::query_as::<_, ListRow>(&format!(
        "SELECT {LIST_COLS} FROM rules.reference_lists l WHERE l.tenant_id = $1 AND l.id = $2"
    ))
    .bind(scope.tenant().as_uuid())
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("reference list not found"))?;
    let owned = row.project_id == scope.project().map(|p| p.as_uuid());
    let tenant_readable = readable && row.project_id.is_none();
    if owned || tenant_readable {
        Ok(row)
    } else {
        Err(AppError::not_found("reference list not found"))
    }
}

pub async fn find_list_by_name(
    conn: &mut PgConnection,
    scope: ListScope,
    name: &str,
) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM rules.reference_lists WHERE tenant_id = $1 AND name = $2 \
         AND project_id IS NOT DISTINCT FROM $3",
    )
    .bind(scope.tenant().as_uuid())
    .bind(name)
    .bind(scope.project().map(|p| p.as_uuid()))
    .fetch_optional(conn)
    .await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_list(
    conn: &mut PgConnection,
    scope: ListScope,
    name: &str,
    description: Option<&str>,
    list_type: &str,
    key_kind: &str,
    columns: &Value,
    created_by: Option<Uuid>,
) -> AppResult<Uuid> {
    Ok(sqlx::query_scalar(
        "INSERT INTO rules.reference_lists (tenant_id, project_id, name, description, list_type, key_kind, columns, \
         created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
    )
    .bind(scope.tenant().as_uuid())
    .bind(scope.project().map(|p| p.as_uuid()))
    .bind(name)
    .bind(description)
    .bind(list_type)
    .bind(key_kind)
    .bind(columns)
    .bind(created_by)
    .fetch_one(conn)
    .await?)
}

pub async fn update_list(
    conn: &mut PgConnection,
    id: Uuid,
    description: Option<&str>,
    list_type: Option<&str>,
    key_kind: Option<&str>,
    columns: Option<&Value>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE rules.reference_lists SET description = COALESCE($2, description), list_type = COALESCE($3, list_type), \
         key_kind = COALESCE($4, key_kind), columns = COALESCE($5, columns) WHERE id = $1",
    )
    .bind(id)
    .bind(description)
    .bind(list_type)
    .bind(key_kind)
    .bind(columns)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn delete_list(conn: &mut PgConnection, id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM rules.reference_lists WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Codes of non-retired rules whose current version references a list by name (project scope, or every
/// project of the tenant for a tenant-wide list).
pub async fn rules_referencing_list(
    conn: &mut PgConnection,
    scope: ListScope,
    name: &str,
) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT r.code FROM rules.rules r JOIN rules.rule_versions v ON v.rule_id = r.id AND v.version = r.current_version \
         WHERE r.tenant_id = $1 AND ($2::uuid IS NULL OR r.project_id = $2) AND r.status <> 'retired' \
         AND r.kind = 'reference' AND v.definition->>'list' = $3 ORDER BY r.code",
    )
    .bind(scope.tenant().as_uuid())
    .bind(scope.project().map(|p| p.as_uuid()))
    .bind(name)
    .fetch_all(conn)
    .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct EntryRowDb {
    pub id: i64,
    pub key: String,
    #[schema(value_type = Object)]
    pub attributes: Value,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub reason: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_entries(
    conn: &mut PgConnection,
    list_id: Uuid,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<EntryRowDb>, i64)> {
    let pattern = q.map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
    let rows = sqlx::query_as::<_, EntryRowDb>(
        "SELECT id, key, attributes, valid_from, valid_until, reason, created_by, created_at FROM rules.reference_entries \
         WHERE list_id = $1 AND ($2::text IS NULL OR key ILIKE $2) ORDER BY key LIMIT $3 OFFSET $4",
    )
    .bind(list_id)
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rules.reference_entries WHERE list_id = $1 AND ($2::text IS NULL OR key ILIKE $2)",
    )
    .bind(list_id)
    .bind(&pattern)
    .fetch_one(conn)
    .await?;
    Ok((rows, total))
}

/// Bulk upsert on `(list_id, key)` in one statement (UNNEST), used by the API, CSV import and bootstrap.
pub async fn upsert_entries(
    conn: &mut PgConnection,
    tenant: TenantId,
    list_id: Uuid,
    entries: &[crate::domain::csv_import::EntryRow],
    created_by: Option<Uuid>,
) -> AppResult<u64> {
    if entries.is_empty() {
        return Ok(0);
    }
    let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    let attrs: Vec<Value> = entries.iter().map(|e| e.attributes.clone()).collect();
    let from: Vec<Option<DateTime<Utc>>> = entries.iter().map(|e| e.valid_from).collect();
    let until: Vec<Option<DateTime<Utc>>> = entries.iter().map(|e| e.valid_until).collect();
    let reasons: Vec<Option<&str>> = entries.iter().map(|e| e.reason.as_deref()).collect();
    let res = sqlx::query(
        "INSERT INTO rules.reference_entries (tenant_id, list_id, key, attributes, valid_from, valid_until, reason, created_by) \
         SELECT $1, $2, k, COALESCE(a, '{}'::jsonb), COALESCE(vf, now()), vu, r, $8 \
         FROM UNNEST($3::text[], $4::jsonb[], $5::timestamptz[], $6::timestamptz[], $7::text[]) AS t(k, a, vf, vu, r) \
         ON CONFLICT (list_id, key) DO UPDATE SET attributes = EXCLUDED.attributes, valid_from = EXCLUDED.valid_from, \
         valid_until = EXCLUDED.valid_until, reason = EXCLUDED.reason",
    )
    .bind(tenant.as_uuid())
    .bind(list_id)
    .bind(&keys)
    .bind(&attrs)
    .bind(&from)
    .bind(&until)
    .bind(&reasons)
    .bind(created_by)
    .execute(conn)
    .await?;
    Ok(res.rows_affected())
}

pub async fn delete_entry(conn: &mut PgConnection, list_id: Uuid, entry_id: i64) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM rules.reference_entries WHERE list_id = $1 AND id = $2")
        .bind(list_id)
        .bind(entry_id)
        .execute(conn)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("entry not found"));
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------------------
// Hits, counters, performance
// -------------------------------------------------------------------------------------------------------------

/// One persisted non-`no_match` rule outcome.
#[derive(Debug, Clone)]
pub struct HitRow {
    pub rule_id: Uuid,
    pub rule_version: i32,
    pub ruleset_id: Option<Uuid>,
    pub outcome: &'static str,
    pub contribution: f64,
    pub shadow: bool,
}

/// Per-rule evaluation counters for one event.
#[derive(Debug, Clone, Copy)]
pub struct CounterDelta {
    pub rule_id: Uuid,
    pub matched: bool,
    pub trapped: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn record_evaluation(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    event_id: Uuid,
    occurred_at: DateTime<Utc>,
    hits: &[HitRow],
    counters: &[CounterDelta],
) -> AppResult<()> {
    if !hits.is_empty() {
        let rule_ids: Vec<Uuid> = hits.iter().map(|h| h.rule_id).collect();
        let versions: Vec<i32> = hits.iter().map(|h| h.rule_version).collect();
        let rulesets: Vec<Option<Uuid>> = hits.iter().map(|h| h.ruleset_id).collect();
        let outcomes: Vec<&str> = hits.iter().map(|h| h.outcome).collect();
        let contributions: Vec<f32> = hits.iter().map(|h| h.contribution as f32).collect();
        let shadows: Vec<bool> = hits.iter().map(|h| h.shadow).collect();
        sqlx::query(
            "INSERT INTO rules.rule_hits (tenant_id, project_id, event_id, rule_id, rule_version, ruleset_id, outcome, \
             contribution, shadow, occurred_at) \
             SELECT $1, $2, $3, r, v, rs, o, c, s, $10 \
             FROM UNNEST($4::uuid[], $5::int4[], $6::uuid[], $7::text[], $8::float4[], $9::bool[]) AS t(r, v, rs, o, c, s)",
        )
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(event_id)
        .bind(&rule_ids)
        .bind(&versions)
        .bind(&rulesets)
        .bind(&outcomes)
        .bind(&contributions)
        .bind(&shadows)
        .bind(occurred_at)
        .execute(&mut *conn)
        .await?;
    }
    if !counters.is_empty() {
        let ids: Vec<Uuid> = counters.iter().map(|c| c.rule_id).collect();
        let matched: Vec<i64> = counters.iter().map(|c| i64::from(c.matched)).collect();
        let trapped: Vec<i64> = counters.iter().map(|c| i64::from(c.trapped)).collect();
        sqlx::query(
            "INSERT INTO rules.rule_eval_counters (tenant_id, rule_id, day, evaluated, matched, trapped) \
             SELECT $1, r, ($5 AT TIME ZONE 'UTC')::date, 1, m, t \
             FROM UNNEST($2::uuid[], $3::int8[], $4::int8[]) AS u(r, m, t) \
             ON CONFLICT (rule_id, day) DO UPDATE SET evaluated = rules.rule_eval_counters.evaluated + 1, \
             matched = rules.rule_eval_counters.matched + EXCLUDED.matched, \
             trapped = rules.rule_eval_counters.trapped + EXCLUDED.trapped",
        )
        .bind(tenant.as_uuid())
        .bind(&ids)
        .bind(&matched)
        .bind(&trapped)
        .bind(occurred_at)
        .execute(conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema, Default)]
pub struct RuleStats {
    pub evaluated: i64,
    pub matched: i64,
    pub trapped: i64,
}

pub async fn stats_since(
    conn: &mut PgConnection,
    rule_ids: &[Uuid],
    days: i32,
) -> AppResult<std::collections::HashMap<Uuid, RuleStats>> {
    let rows: Vec<(Uuid, i64, i64, i64)> = sqlx::query_as(
        "SELECT rule_id, COALESCE(sum(evaluated),0)::int8, COALESCE(sum(matched),0)::int8, COALESCE(sum(trapped),0)::int8 \
         FROM rules.rule_eval_counters WHERE rule_id = ANY($1) AND day > (now() AT TIME ZONE 'UTC')::date - $2 \
         GROUP BY rule_id",
    )
    .bind(rule_ids)
    .bind(days)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, evaluated, matched, trapped)| {
            (
                id,
                RuleStats {
                    evaluated,
                    matched,
                    trapped,
                },
            )
        })
        .collect())
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct PerformanceRow {
    pub rule_id: Uuid,
    pub code: String,
    pub name: String,
    pub status: String,
    pub evaluated: i64,
    pub matched: i64,
    pub trapped: i64,
    pub hit_rate: Option<f64>,
    pub labeled_fraud_hits: i64,
    pub labeled_legit_hits: i64,
    pub precision: Option<f64>,
    pub last_hit_at: Option<DateTime<Utc>>,
}

pub async fn performance(
    conn: &mut PgConnection,
    project: ProjectId,
    days: i32,
) -> AppResult<Vec<PerformanceRow>> {
    Ok(sqlx::query_as::<_, PerformanceRow>(
        "WITH c AS (SELECT rule_id, sum(evaluated)::int8 AS evaluated, sum(matched)::int8 AS matched, \
                           sum(trapped)::int8 AS trapped \
                    FROM rules.rule_eval_counters WHERE day > (now() AT TIME ZONE 'UTC')::date - $2 GROUP BY rule_id), \
              h AS (SELECT rh.rule_id, count(*) FILTER (WHERE l.label = 'fraud')::int8 AS fraud_hits, \
                           count(*) FILTER (WHERE l.label = 'legit')::int8 AS legit_hits, max(rh.occurred_at) AS last_hit_at \
                    FROM rules.rule_hits rh LEFT JOIN core.event_labels l ON l.event_id = rh.event_id \
                    WHERE rh.project_id = $1 AND rh.outcome = 'match' AND NOT rh.shadow \
                      AND rh.occurred_at > now() - make_interval(days => $2) GROUP BY rh.rule_id) \
         SELECT r.id AS rule_id, r.code, r.name, r.status, COALESCE(c.evaluated,0) AS evaluated, \
                COALESCE(c.matched,0) AS matched, COALESCE(c.trapped,0) AS trapped, \
                CASE WHEN COALESCE(c.evaluated,0) > 0 THEN c.matched::float8 / c.evaluated END AS hit_rate, \
                COALESCE(h.fraud_hits,0) AS labeled_fraud_hits, COALESCE(h.legit_hits,0) AS labeled_legit_hits, \
                CASE WHEN COALESCE(h.fraud_hits,0) + COALESCE(h.legit_hits,0) > 0 \
                     THEN h.fraud_hits::float8 / (h.fraud_hits + h.legit_hits) END AS precision, \
                h.last_hit_at \
         FROM rules.rules r LEFT JOIN c ON c.rule_id = r.id LEFT JOIN h ON h.rule_id = r.id \
         WHERE r.project_id = $1 ORDER BY r.code",
    )
    .bind(project.as_uuid())
    .bind(days)
    .fetch_all(conn)
    .await?)
}

// -------------------------------------------------------------------------------------------------------------
// Proposals
// -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct ProposalRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source: String,
    pub proposal_type: String,
    pub target_rule_id: Option<Uuid>,
    #[schema(value_type = Object)]
    pub definition: Option<Value>,
    pub rationale: String,
    #[schema(value_type = Object)]
    pub citations: Value,
    #[schema(value_type = Object)]
    pub evidence: Value,
    #[schema(value_type = Object)]
    pub validation: Value,
    #[schema(value_type = Object)]
    pub backtest: Option<Value>,
    pub report_id: Option<Uuid>,
    pub llm_model: Option<String>,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub review_comment: Option<String>,
    pub applied_rule_id: Option<Uuid>,
}

const PROPOSAL_COLS: &str = "id, project_id, source, proposal_type, target_rule_id, definition, rationale, citations, \
                             evidence, validation, backtest, report_id, llm_model, status, created_by, created_at, \
                             reviewed_by, reviewed_at, review_comment, applied_rule_id";

#[derive(Debug, Clone)]
pub struct NewProposal<'a> {
    pub source: &'a str,
    pub proposal_type: &'a str,
    pub target_rule_id: Option<Uuid>,
    pub definition: Option<&'a Value>,
    pub rationale: &'a str,
    pub citations: &'a Value,
    pub evidence: &'a Value,
    pub validation: &'a Value,
    pub backtest: Option<&'a Value>,
    pub report_id: Option<Uuid>,
    pub llm_model: Option<&'a str>,
    pub created_by: Option<Uuid>,
}

pub async fn insert_proposal(
    conn: &mut PgConnection,
    tenant: TenantId,
    project: ProjectId,
    p: &NewProposal<'_>,
) -> AppResult<ProposalRow> {
    Ok(sqlx::query_as::<_, ProposalRow>(&format!(
        "INSERT INTO rules.proposals (tenant_id, project_id, source, proposal_type, target_rule_id, definition, rationale, \
         citations, evidence, validation, backtest, report_id, llm_model, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING {PROPOSAL_COLS}"
    ))
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(p.source)
    .bind(p.proposal_type)
    .bind(p.target_rule_id)
    .bind(p.definition)
    .bind(p.rationale)
    .bind(p.citations)
    .bind(p.evidence)
    .bind(p.validation)
    .bind(p.backtest)
    .bind(p.report_id)
    .bind(p.llm_model)
    .bind(p.created_by)
    .fetch_one(conn)
    .await?)
}

pub async fn get_proposal(
    conn: &mut PgConnection,
    project: ProjectId,
    id: Uuid,
    lock: bool,
) -> AppResult<ProposalRow> {
    let lock_sql = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as::<_, ProposalRow>(&format!(
        "SELECT {PROPOSAL_COLS} FROM rules.proposals WHERE project_id = $1 AND id = $2{lock_sql}"
    ))
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| AppError::not_found("proposal not found"))
}

pub async fn list_proposals(
    conn: &mut PgConnection,
    project: ProjectId,
    status: Option<&str>,
    source: Option<&str>,
    report_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<ProposalRow>, i64)> {
    let where_sql =
        "project_id = $1 AND ($2::text IS NULL OR status = $2) AND ($3::text IS NULL OR source = $3) \
                     AND ($4::uuid IS NULL OR report_id = $4)";
    let rows = sqlx::query_as::<_, ProposalRow>(&format!(
        "SELECT {PROPOSAL_COLS} FROM rules.proposals WHERE {where_sql} ORDER BY created_at DESC LIMIT $5 OFFSET $6"
    ))
    .bind(project.as_uuid())
    .bind(status)
    .bind(source)
    .bind(report_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *conn)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM rules.proposals WHERE {where_sql}"))
        .bind(project.as_uuid())
        .bind(status)
        .bind(source)
        .bind(report_id)
        .fetch_one(conn)
        .await?;
    Ok((rows, total))
}

pub async fn review_proposal(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    reviewer: Uuid,
    comment: Option<&str>,
    applied_rule_id: Option<Uuid>,
) -> AppResult<ProposalRow> {
    Ok(sqlx::query_as::<_, ProposalRow>(&format!(
        "UPDATE rules.proposals SET status = $2, reviewed_by = $3, reviewed_at = now(), review_comment = $4, \
         applied_rule_id = $5 WHERE id = $1 RETURNING {PROPOSAL_COLS}"
    ))
    .bind(id)
    .bind(status)
    .bind(reviewer)
    .bind(comment)
    .bind(applied_rule_id)
    .fetch_one(conn)
    .await?)
}
