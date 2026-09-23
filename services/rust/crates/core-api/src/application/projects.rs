//! Projects, members and settings.
//!
//! Creating a project also creates its default settings rows, the internal `canonical` data source,
//! the creator's membership and, if requested, a background job that asks rule-service to
//! bootstrap the stage template (retried with backoff, idempotent, survives restarts via the
//! `template_bootstrap` settings row).

use std::time::Duration;

use chrono_tz::Tz;
use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, CallerKind, ProjectRole, TenantRole};
use platform::db::TenantTx;
use platform::error::{AppError, AppResult, FieldError};
use platform::http::CallCtx;
use platform::pagination::{Page, PageParams};
use platform::{ProjectId, TenantId};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::application::context::invalidate_project;
use crate::domain::settings::{self, ProjectSettings, TEMPLATE_BOOTSTRAP_KEY};
use crate::state::AppState;

use super::util::{collect_page, valid_slug};

pub const STAGES: &[&str] = &[
    "pre_payment",
    "post_payment",
    "returns",
    "promo",
    "account_security",
    "payout",
    "custom",
];

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct ProjectIn {
    pub slug: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub stage: Option<String>,
    pub business_context: Option<String>,
    pub timezone: Option<String>,
    pub currency: Option<String>,
    /// Stage template to bootstrap (`none` = no template). Default: the stage (except `custom`).
    pub template: Option<String>,
    #[schema(value_type = Object)]
    pub ml_config: Option<Value>,
    #[schema(value_type = Object)]
    pub llm_config: Option<Value>,
    #[schema(value_type = Object)]
    pub graph_config: Option<Value>,
}

fn validate_fields(p: &ProjectIn, creating: bool) -> Vec<FieldError> {
    let mut e = Vec::new();
    if creating {
        match &p.slug {
            Some(s) if valid_slug(s) => {}
            _ => e.push(FieldError::new(
                "slug",
                "lower-case letters, digits and '-', 2-63 characters",
            )),
        }
        if p.name.as_deref().map(str::trim).unwrap_or("").is_empty() {
            e.push(FieldError::new("name", "required"));
        }
    }
    if let Some(n) = &p.name {
        if n.len() > 200 || n.trim().is_empty() {
            e.push(FieldError::new("name", "1-200 characters"));
        }
    }
    if let Some(s) = &p.stage {
        if !STAGES.contains(&s.as_str()) {
            e.push(FieldError::new("stage", format!("one of {}", STAGES.join(", "))));
        }
    }
    if let Some(t) = &p.template {
        if t != "none" && !STAGES.contains(&t.as_str()) {
            e.push(FieldError::new("template", "a stage name or `none`"));
        }
    }
    if let Some(tz) = &p.timezone {
        if tz.parse::<Tz>().is_err() {
            e.push(FieldError::new("timezone", "unknown IANA timezone"));
        }
    }
    if let Some(c) = &p.currency {
        if c.len() != 3 || !c.chars().all(|x| x.is_ascii_alphabetic()) {
            e.push(FieldError::new("currency", "ISO 4217 code"));
        }
    }
    if p.description.as_ref().is_some_and(|d| d.len() > 2000) {
        e.push(FieldError::new("description", "max 2000 characters"));
    }
    if p.business_context.as_ref().is_some_and(|d| d.len() > 10_000) {
        e.push(FieldError::new("business_context", "max 10000 characters"));
    }
    for (name, v) in [
        ("ml_config", &p.ml_config),
        ("llm_config", &p.llm_config),
        ("graph_config", &p.graph_config),
    ] {
        if v.as_ref().is_some_and(|v| !v.is_object()) {
            e.push(FieldError::new(name, "must be a JSON object"));
        }
    }
    if let Some(g) = &p.graph_config {
        if let Some(kinds) = g.get("link_kinds").and_then(Value::as_array) {
            for k in kinds {
                if k.as_str().and_then(contracts::graph::LinkKind::parse).is_none() {
                    e.push(FieldError::new(
                        "graph_config.link_kinds",
                        format!("unknown link kind {k}"),
                    ));
                }
            }
        }
        if let Some(d) = g.get("max_depth") {
            if !d.as_u64().is_some_and(|d| (1..=4).contains(&d)) {
                e.push(FieldError::new("graph_config.max_depth", "1..4"));
            }
        }
    }
    e
}

/// Validates `ml_config` with ml-service. Returns warnings when ml-service is unreachable.
async fn validate_ml_config(st: &AppState, tenant: TenantId, ml_config: &Value) -> AppResult<Vec<String>> {
    let ctx = CallCtx::new(tenant, None);
    match st.engines.ml.validate_config(&ctx, ml_config).await {
        Ok(v) if v.valid => Ok(vec![]),
        Ok(v) => Err(AppError::validation(
            v.errors
                .into_iter()
                .map(|e| FieldError::new(format!("ml_config.{}", e.path), e.message))
                .collect(),
        )),
        Err(e) => {
            tracing::warn!(error = %e, "ml-service unavailable; ml_config accepted without validation");
            Ok(vec![
                "ml_config was not validated: ml-service is unavailable".to_string()
            ])
        }
    }
}

pub async fn list(st: &AppState, caller: &Caller, page: &PageParams) -> AppResult<Page<Value>> {
    let (tenant, filter_user): (TenantId, Option<Uuid>) = match &caller.kind {
        CallerKind::Service { tenant_id, .. } => (*tenant_id, None),
        CallerKind::User(u) => match u.tenant_id() {
            None => return Ok(Page::new(vec![], 0, page)), // platform admins see no project data
            Some(t) if u.claims.trole == TenantRole::TenantAdmin => (t, None),
            Some(t) => (t, Some(u.user_id().as_uuid())),
        },
    };
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM \
           (SELECT p.id, p.slug, p.name, p.description, p.stage, p.status, p.timezone, p.currency::text AS currency, \
                   p.created_at, m.role \
            FROM core.projects p \
            LEFT JOIN core.project_members m ON m.project_id = p.id AND m.user_id = $1 \
            WHERE ($1::uuid IS NULL OR m.user_id IS NOT NULL) ORDER BY p.name) t \
         LIMIT $2 OFFSET $3",
    )
    .bind(filter_user)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}

pub async fn create(st: &AppState, caller: &Caller, input: &ProjectIn) -> AppResult<Value> {
    let user = caller
        .user()
        .ok_or_else(|| AppError::Forbidden("users only".into()))?;
    let tenant = user
        .tenant_id()
        .ok_or_else(|| AppError::Forbidden("requires tenant admin".into()))?;
    caller.require_tenant_admin(tenant)?;
    let errors = validate_fields(input, true);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut warnings = Vec::new();
    if let Some(ml) = &input.ml_config {
        warnings.extend(validate_ml_config(st, tenant, ml).await?);
    }
    let stage = input.stage.clone().unwrap_or_else(|| "custom".into());
    let template = match input.template.as_deref() {
        Some("none") => None,
        Some(t) => Some(t.to_string()),
        None if stage != "custom" => Some(stage.clone()),
        None => None,
    };

    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let (pid,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.projects (tenant_id, slug, name, description, stage, business_context, timezone, currency, \
                                    ml_config, llm_config, graph_config, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6, COALESCE($7,'Asia/Jakarta'), COALESCE(upper($8),'IDR'), \
                 $9, \
                 COALESCE($10, '{\"chat_model\": null, \"temperature\": 0.1, \"language\": \"id\", \"system_prompt_extra\": \"\"}'::jsonb), \
                 COALESCE($11, '{\"link_kinds\": [\"email\",\"phone\",\"device\",\"card\",\"bank_account\",\"address\",\"ref_transaction\"], \"include_similar\": true, \"max_depth\": 3, \"supernode_degree_cap\": 50, \"similarity_threshold\": 0.85}'::jsonb), \
                 $12) RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(&input.slug)
    .bind(input.name.as_deref().map(str::trim))
    .bind(&input.description)
    .bind(&stage)
    .bind(&input.business_context)
    .bind(&input.timezone)
    .bind(&input.currency)
    .bind(input.ml_config.clone().or_else(|| Some(default_ml_config())))
    .bind(&input.llm_config)
    .bind(&input.graph_config)
    .bind(user.user_id().as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("a project with this slug already exists".into()),
        other => other,
    })?;
    let project = ProjectId(pid);
    init_project(
        &mut tx,
        tenant,
        project,
        Some(user.user_id().as_uuid()),
        template.as_deref(),
    )
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "project.create")
            .scope(tenant, Some(project))
            .subject("project", pid)
            .after(json!({ "slug": input.slug, "name": input.name, "stage": stage, "template": template })),
    )
    .await?;
    tx.commit().await?;

    if let Some(t) = template {
        spawn_template_bootstrap(st.clone(), tenant, project, t);
    }
    let mut out = get(st, caller, project).await?;
    if let Some(m) = out.as_object_mut() {
        m.insert("warnings".into(), json!(warnings));
    }
    Ok(out)
}

pub fn default_ml_config() -> Value {
    json!({
        "supervised":   {"algorithm": "mlp_backprop", "params": {}},
        "unsupervised": {"anomaly_algorithm": "isolation_forest", "anomaly_params": {},
                         "clustering_algorithm": "hdbscan", "clustering_params": {}},
        "features":     {"include": ["*"], "exclude": [], "extra_source_fields": []}
    })
}

/// Default settings, canonical data source, creator membership, template bookkeeping.
pub async fn init_project(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    creator: Option<Uuid>,
    template: Option<&str>,
) -> AppResult<()> {
    for (k, v) in ProjectSettings::default_rows() {
        sqlx::query(
            "INSERT INTO core.project_settings (tenant_id, project_id, key, value) VALUES ($1,$2,$3,$4) \
             ON CONFLICT (project_id, key) DO NOTHING",
        )
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(k)
        .bind(v)
        .execute(&mut ***tx)
        .await?;
    }
    if let Some(t) = template {
        sqlx::query(
            "INSERT INTO core.project_settings (tenant_id, project_id, key, value) VALUES ($1,$2,$3,$4) \
             ON CONFLICT (project_id, key) DO NOTHING",
        )
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(TEMPLATE_BOOTSTRAP_KEY)
        .bind(json!({ "template": t, "status": "pending" }))
        .execute(&mut ***tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO core.data_sources (tenant_id, project_id, slug, name, kind, description) \
         VALUES ($1, $2, 'canonical', 'Canonical API', 'internal', 'Events posted in canonical shape to /events') \
         ON CONFLICT (project_id, slug) DO NOTHING",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .execute(&mut ***tx)
    .await?;
    if let Some(u) = creator {
        sqlx::query(
            "INSERT INTO core.project_members (tenant_id, project_id, user_id, role) VALUES ($1,$2,$3,'project_admin') \
             ON CONFLICT (project_id, user_id) DO NOTHING",
        )
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(u)
        .execute(&mut ***tx)
        .await?;
    }
    Ok(())
}

/// Asks rule-service to bootstrap the template, retrying with exponential backoff (≤ ~15 min).
pub fn spawn_template_bootstrap(st: AppState, tenant: TenantId, project: ProjectId, template: String) {
    tokio::spawn(async move {
        let mut delay = Duration::from_secs(2);
        for attempt in 1..=12 {
            let ctx = CallCtx::new(tenant, Some(project));
            match st.engines.rules.bootstrap(&ctx, project, &template).await {
                Ok(()) => {
                    let res: AppResult<()> = async {
                        let mut tx = TenantTx::begin(&st.pool, tenant).await?;
                        sqlx::query(
                            "UPDATE core.project_settings SET value = jsonb_set(value, '{status}', '\"done\"'), \
                             updated_at = now() WHERE project_id = $1 AND key = $2",
                        )
                        .bind(project.as_uuid())
                        .bind(TEMPLATE_BOOTSTRAP_KEY)
                        .execute(&mut **tx)
                        .await?;
                        tx.commit().await
                    }
                    .await;
                    if let Err(e) = res {
                        tracing::warn!(error = %e, "could not record template bootstrap status");
                    }
                    tracing::info!(%project, %template, "project template bootstrapped");
                    return;
                }
                Err(e) => {
                    tracing::warn!(%project, attempt, error = %e, "template bootstrap failed; retrying");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(120));
                }
            }
        }
        tracing::error!(%project, %template, "template bootstrap gave up; will retry on next start");
    });
}

/// On startup: retry template bootstraps that never completed (across tenants).
pub async fn resume_pending_bootstraps(st: &AppState) -> AppResult<()> {
    let tenants: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM core.tenants WHERE status = 'active'")
        .fetch_all(&st.pool)
        .await?;
    for (tid,) in tenants {
        let tenant = TenantId(tid);
        let mut tx = TenantTx::begin(&st.pool, tenant).await?;
        let rows: Vec<(Uuid, Value)> =
            sqlx::query_as("SELECT project_id, value FROM core.project_settings WHERE key = $1")
                .bind(TEMPLATE_BOOTSTRAP_KEY)
                .fetch_all(&mut **tx)
                .await?;
        tx.commit().await?;
        for (pid, v) in rows {
            if v.get("status").and_then(Value::as_str) == Some("pending") {
                if let Some(t) = v.get("template").and_then(Value::as_str) {
                    spawn_template_bootstrap(st.clone(), tenant, ProjectId(pid), t.to_string());
                }
            }
        }
    }
    Ok(())
}

pub async fn get(st: &AppState, caller: &Caller, project: ProjectId) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value, Value)> = sqlx::query_as(
        "SELECT to_jsonb(p) - 'created_by', jsonb_build_object( \
            'events_30d', (SELECT count(*) FROM core.events e WHERE e.project_id = p.id \
                            AND e.occurred_at > now() - interval '30 days'), \
            'decisions_30d', (SELECT count(*) FROM core.decisions d WHERE d.project_id = p.id \
                            AND d.created_at > now() - interval '30 days'), \
            'declines_30d', (SELECT count(*) FROM core.decisions d WHERE d.project_id = p.id \
                            AND d.decision = 'decline' AND d.created_at > now() - interval '30 days'), \
            'open_cases', (SELECT count(*) FROM core.cases c WHERE c.project_id = p.id \
                            AND c.status IN ('open', 'in_review')), \
            'active_rules', NULL, 'active_models', NULL) \
         FROM core.projects p WHERE p.id = $1",
    )
    .bind(project.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    tx.commit().await?;
    let (mut p, summary) = row.ok_or_else(|| AppError::not_found("project not found"))?;
    if let Some(m) = p.as_object_mut() {
        m.insert("summary".into(), summary);
    }
    Ok(p)
}

pub async fn patch(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    input: &ProjectIn,
) -> AppResult<Value> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    if input.slug.is_some() {
        return Err(AppError::field("slug", "cannot be changed"));
    }
    let errors = validate_fields(input, false);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut warnings = Vec::new();
    if let Some(ml) = &input.ml_config {
        warnings.extend(validate_ml_config(st, tenant, ml).await?);
    }
    let before = get(st, caller, project).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    sqlx::query(
        "UPDATE core.projects SET name = COALESCE($2, name), description = COALESCE($3, description), \
            stage = COALESCE($4, stage), business_context = COALESCE($5, business_context), \
            timezone = COALESCE($6, timezone), currency = COALESCE(upper($7), currency), \
            ml_config = COALESCE($8, ml_config), llm_config = COALESCE($9, llm_config), \
            graph_config = COALESCE($10, graph_config) \
         WHERE id = $1",
    )
    .bind(project.as_uuid())
    .bind(input.name.as_deref().map(str::trim))
    .bind(&input.description)
    .bind(&input.stage)
    .bind(&input.business_context)
    .bind(&input.timezone)
    .bind(&input.currency)
    .bind(&input.ml_config)
    .bind(&input.llm_config)
    .bind(&input.graph_config)
    .execute(&mut **tx)
    .await?;
    tx.commit().await?;
    invalidate_project(st, project).await;
    let mut after = get(st, caller, project).await?;
    audit::record(
        &st.pool,
        &AuditEntry::by(caller, "project.update")
            .scope(tenant, Some(project))
            .subject("project", project)
            .before(&before)
            .after(&after),
    )
    .await?;
    if let Some(m) = after.as_object_mut() {
        m.insert("warnings".into(), json!(warnings));
    }
    Ok(after)
}

pub async fn archive(st: &AppState, caller: &Caller, project: ProjectId) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    caller.require_tenant_admin(tenant)?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    sqlx::query("UPDATE core.projects SET status = 'archived' WHERE id = $1")
        .bind(project.as_uuid())
        .execute(&mut **tx)
        .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "project.archive")
            .scope(tenant, Some(project))
            .subject("project", project),
    )
    .await?;
    tx.commit().await?;
    invalidate_project(st, project).await;
    Ok(json!({ "id": project, "status": "archived" }))
}

// ---------------------------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------------------------

pub async fn list_members(st: &AppState, caller: &Caller, project: ProjectId) -> AppResult<Value> {
    // Readable by every member (analysts assign cases to colleagues); mutations need project_admin.
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT jsonb_build_object('user_id', m.user_id, 'role', m.role, 'email', u.email::text, \
                'full_name', u.full_name, 'is_active', u.is_active, 'created_at', m.created_at) \
         FROM core.project_members m JOIN core.app_users u ON u.id = m.user_id \
         WHERE m.project_id = $1 ORDER BY u.email",
    )
    .bind(project.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(json!({ "items": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() }))
}

pub async fn upsert_member(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    user_id: Uuid,
    role: &str,
) -> AppResult<Value> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    if ProjectRole::parse(role).is_none() {
        return Err(AppError::field(
            "role",
            "project_admin | approver | analyst | viewer",
        ));
    }
    let same_tenant: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM core.app_users WHERE id = $1 AND tenant_id = $2")
            .bind(user_id)
            .bind(tenant.as_uuid())
            .fetch_optional(&st.pool)
            .await?;
    if same_tenant.is_none() {
        return Err(AppError::field("user_id", "user not found in this tenant"));
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    sqlx::query(
        "INSERT INTO core.project_members (tenant_id, project_id, user_id, role) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(user_id)
    .bind(role)
    .execute(&mut **tx)
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "project.member.upsert")
            .scope(tenant, Some(project))
            .subject("user", user_id)
            .after(json!({ "role": role })),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({ "user_id": user_id, "role": role }))
}

pub async fn remove_member(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    user_id: Uuid,
) -> AppResult<()> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let res = sqlx::query("DELETE FROM core.project_members WHERE project_id = $1 AND user_id = $2")
        .bind(project.as_uuid())
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("member not found"));
    }
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "project.member.remove")
            .scope(tenant, Some(project))
            .subject("user", user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

pub async fn get_settings(st: &AppState, caller: &Caller, project: ProjectId) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(String, Value)> =
        sqlx::query_as("SELECT key, value FROM core.project_settings WHERE project_id = $1")
            .bind(project.as_uuid())
            .fetch_all(&mut **tx)
            .await?;
    tx.commit().await?;
    let s = ProjectSettings::from_rows(rows.iter().map(|(k, v)| (k.as_str(), v)));
    Ok(s.to_json())
}

pub async fn put_setting(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    key: &str,
    value: &Value,
) -> AppResult<Value> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    if !settings::KEYS.contains(&key) {
        return Err(AppError::not_found(format!("unknown settings key `{key}`")));
    }
    settings::validate(key, value).map_err(AppError::validation)?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let before: Option<(Value,)> =
        sqlx::query_as("SELECT value FROM core.project_settings WHERE project_id = $1 AND key = $2")
            .bind(project.as_uuid())
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
    sqlx::query(
        "INSERT INTO core.project_settings (tenant_id, project_id, key, value, updated_by) VALUES ($1,$2,$3,$4,$5) \
         ON CONFLICT (project_id, key) DO UPDATE SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, \
            updated_at = now()",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(key)
    .bind(value)
    .bind(caller.actor_user_id().map(|u| u.as_uuid()))
    .execute(&mut **tx)
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "project.settings.update")
            .scope(tenant, Some(project))
            .subject("setting", key)
            .before(before.map(|b| b.0))
            .after(value),
    )
    .await?;
    tx.commit().await?;
    invalidate_project(st, project).await;
    Ok(json!({ "key": key, "value": value }))
}
