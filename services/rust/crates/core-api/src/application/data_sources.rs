//! Data sources, mapping versions (with preview and activation) and the field catalog.

use std::collections::BTreeMap;

use contracts::catalog::{self, BUILTIN_FIELDS};
use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::error::{AppError, AppResult, FieldError};
use platform::pagination::{Page, PageParams};
use platform::{pii, ProjectId, TenantId};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::adapters::crypto::{hash_secret, new_api_key};
use crate::application::context::{invalidate_source, project_ctx};
use crate::domain::mapping::{
    apply_mapping, referenced_paths, validate_mapping, Mapping, MappingCtx, Transform,
};
use crate::domain::normalize::normalize_event;
use crate::state::AppState;

use super::util::{collect_page, valid_source_slug};

pub const KINDS: &[&str] = &["webhook", "file", "postgres", "mysql"];

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct DataSourceIn {
    pub slug: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub default_event_type: Option<String>,
    pub mode: Option<String>,
    #[schema(value_type = Object)]
    pub connection: Option<Value>,
    pub is_active: Option<bool>,
}

/// Rejects any key that looks like an inline secret (secrets are referenced via `*_env`).
fn find_inline_secret(v: &Value, path: &str) -> Option<String> {
    match v {
        Value::Object(m) => m.iter().find_map(|(k, val)| {
            let lk = k.to_lowercase();
            let p = format!("{path}.{k}");
            if (lk.contains("password") || lk.contains("secret") || lk.contains("token"))
                && !lk.ends_with("_env")
            {
                Some(p)
            } else {
                find_inline_secret(val, &p)
            }
        }),
        Value::Array(a) => a.iter().find_map(|x| find_inline_secret(x, path)),
        _ => None,
    }
}

fn validate(input: &DataSourceIn, creating: bool) -> Vec<FieldError> {
    let mut e = Vec::new();
    if creating {
        if !input.slug.as_deref().is_some_and(valid_source_slug) {
            e.push(FieldError::new(
                "slug",
                "lower-case letters, digits, '-' and '_', 2-63 characters",
            ));
        }
        if input.slug.as_deref() == Some("canonical") {
            e.push(FieldError::new("slug", "`canonical` is reserved"));
        }
        if !input.kind.as_deref().is_some_and(|k| KINDS.contains(&k)) {
            e.push(FieldError::new("kind", "webhook | file | postgres | mysql"));
        }
        if input.name.as_deref().map(str::trim).unwrap_or("").is_empty() {
            e.push(FieldError::new("name", "required"));
        }
    }
    if input.name.as_ref().is_some_and(|n| n.len() > 200) {
        e.push(FieldError::new("name", "max 200 characters"));
    }
    if let Some(m) = &input.mode {
        if m != "score" && m != "load_only" {
            e.push(FieldError::new("mode", "score | load_only"));
        }
    }
    if let Some(t) = &input.default_event_type {
        if !contracts::EventType::is_valid(t) {
            e.push(FieldError::new(
                "default_event_type",
                "lower snake case event type",
            ));
        }
    }
    if let Some(c) = &input.connection {
        if !c.is_object() {
            e.push(FieldError::new("connection", "must be an object"));
        } else if let Some(p) = find_inline_secret(c, "connection") {
            e.push(FieldError::new(
                p,
                "secrets must not be stored; reference an env var with `password_env`",
            ));
        }
    }
    e
}

const DS_COLS: &str = "id, slug, name, description, kind, default_event_type, mode, connection, cursor_state, \
                       inferred_schema, api_key_prefix, is_active, created_at, updated_at, \
                       (SELECT max(version) FILTER (WHERE status = 'active') FROM core.data_source_mappings m \
                         WHERE m.data_source_id = ds.id) AS active_mapping_version";

pub async fn list(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(&format!(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM \
           (SELECT {DS_COLS} FROM core.data_sources ds WHERE project_id = $1 ORDER BY created_at) t \
         LIMIT $2 OFFSET $3"
    ))
    .bind(project.as_uuid())
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}

async fn get_in(tx: &mut TenantTx<'_>, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let row: Option<(Value,)> = sqlx::query_as(&format!(
        "SELECT to_jsonb(t) FROM (SELECT {DS_COLS} FROM core.data_sources ds WHERE project_id = $1 AND id = $2) t"
    ))
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut ***tx)
    .await?;
    row.map(|r| r.0)
        .ok_or_else(|| AppError::not_found("data source not found"))
}

pub async fn get(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let v = get_in(&mut tx, project, id).await?;
    tx.commit().await?;
    Ok(v)
}

pub async fn create(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    input: &DataSourceIn,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let errors = validate(input, true);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let (api_key, prefix, hash) = if input.kind.as_deref() == Some("webhook") {
        let (k, p) = new_api_key();
        let h = hash_secret(&k)?;
        (Some(k), Some(p), Some(h))
    } else {
        (None, None, None)
    };
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.data_sources (tenant_id, project_id, slug, name, description, kind, default_event_type, \
                                        mode, connection, api_key_prefix, api_key_hash, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7, COALESCE($8,'score'), COALESCE($9,'{}'::jsonb), $10, $11, $12) RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(&input.slug)
    .bind(input.name.as_deref().map(str::trim))
    .bind(&input.description)
    .bind(&input.kind)
    .bind(&input.default_event_type)
    .bind(&input.mode)
    .bind(&input.connection)
    .bind(&prefix)
    .bind(&hash)
    .bind(caller.actor_user_id().map(|u| u.as_uuid()))
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| match AppError::from(e) {
        AppError::Conflict(_) => AppError::Conflict("a data source with this slug already exists".into()),
        other => other,
    })?;
    let mut out = get_in(&mut tx, project, id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "data_source.create")
            .scope(tenant, Some(project))
            .subject("data_source", id)
            .after(&out),
    )
    .await?;
    tx.commit().await?;
    if let (Some(k), Some(m)) = (api_key, out.as_object_mut()) {
        m.insert("api_key".into(), json!(k));
    }
    Ok(out)
}

pub async fn patch(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    input: &DataSourceIn,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    if input.slug.is_some() || input.kind.is_some() {
        return Err(AppError::BadRequest("slug and kind cannot be changed".into()));
    }
    let errors = validate(input, false);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let before = get_in(&mut tx, project, id).await?;
    sqlx::query(
        "UPDATE core.data_sources SET name = COALESCE($3, name), description = COALESCE($4, description), \
            default_event_type = COALESCE($5, default_event_type), mode = COALESCE($6, mode), \
            connection = COALESCE($7, connection), is_active = COALESCE($8, is_active) \
         WHERE project_id = $1 AND id = $2",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(input.name.as_deref().map(str::trim))
    .bind(&input.description)
    .bind(&input.default_event_type)
    .bind(&input.mode)
    .bind(&input.connection)
    .bind(input.is_active)
    .execute(&mut **tx)
    .await?;
    let after = get_in(&mut tx, project, id).await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "data_source.update")
            .scope(tenant, Some(project))
            .subject("data_source", id)
            .before(&before)
            .after(&after),
    )
    .await?;
    tx.commit().await?;
    invalidate_source(st, id).await;
    Ok(after)
}

pub async fn delete(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<()> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let before = get_in(&mut tx, project, id).await?;
    if before.get("slug").and_then(Value::as_str) == Some("canonical") {
        return Err(AppError::Conflict(
            "the canonical data source cannot be deleted".into(),
        ));
    }
    let has_events: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM core.events WHERE data_source_id = $1 LIMIT 1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    if has_events.is_some() {
        return Err(AppError::Conflict(
            "data source has events; deactivate it instead (PATCH is_active=false)".into(),
        ));
    }
    sqlx::query("DELETE FROM core.data_sources WHERE project_id = $1 AND id = $2")
        .bind(project.as_uuid())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "data_source.delete")
            .scope(tenant, Some(project))
            .subject("data_source", id)
            .before(&before),
    )
    .await?;
    tx.commit().await?;
    invalidate_source(st, id).await;
    Ok(())
}

pub async fn rotate_key(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    let (key, prefix) = new_api_key();
    let hash = hash_secret(&key)?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let res = sqlx::query(
        "UPDATE core.data_sources SET api_key_prefix = $3, api_key_hash = $4 \
         WHERE project_id = $1 AND id = $2 AND kind = 'webhook'",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(&prefix)
    .bind(hash)
    .execute(&mut **tx)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("webhook data source not found"));
    }
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "data_source.rotate_key")
            .scope(tenant, Some(project))
            .subject("data_source", id),
    )
    .await?;
    tx.commit().await?;
    // Previously verified keys stay cached for at most the cache TTL; clear them now.
    st.caches.api_keys.invalidate_all();
    Ok(json!({ "id": id, "api_key": key, "api_key_prefix": prefix }))
}

// ---------------------------------------------------------------------------------------------
// Mappings
// ---------------------------------------------------------------------------------------------

pub async fn list_mappings(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT jsonb_build_object('version', version, 'status', status, 'mapping', mapping, \
                'created_by', created_by, 'created_at', created_at, 'activated_at', activated_at) \
         FROM core.data_source_mappings WHERE project_id = $1 AND data_source_id = $2 ORDER BY version DESC",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(json!({ "items": rows.into_iter().map(|r| r.0).collect::<Vec<_>>() }))
}

pub async fn create_mapping(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    mapping: &Value,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    validate_mapping(mapping).map_err(|errs| AppError::Validation {
        detail: Some("invalid mapping".into()),
        errors: errs,
    })?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let _ = get_in(&mut tx, project, id).await?;
    let (version,): (i32,) = sqlx::query_as(
        "INSERT INTO core.data_source_mappings (tenant_id, project_id, data_source_id, version, mapping, created_by) \
         VALUES ($1, $2, $3, COALESCE((SELECT max(version) FROM core.data_source_mappings \
                                       WHERE data_source_id = $3), 0) + 1, $4, $5) \
         RETURNING version",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(id)
    .bind(mapping)
    .bind(caller.actor_user_id().map(|u| u.as_uuid()))
    .fetch_one(&mut **tx)
    .await?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "mapping.create")
            .scope(tenant, Some(project))
            .subject("data_source", id)
            .after(json!({ "version": version })),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({ "data_source_id": id, "version": version, "status": "draft" }))
}

/// Applies a (draft) mapping to sample records without persisting anything.
pub async fn preview(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    mapping: &Value,
    records: &[Value],
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    if records.len() > 100 {
        return Err(AppError::field("records", "at most 100 records"));
    }
    let m = validate_mapping(mapping).map_err(|errs| AppError::Validation {
        detail: Some("invalid mapping".into()),
        errors: errs,
    })?;
    let pctx = project_ctx(st, tenant, project).await?;
    let src = get(st, caller, project, id).await?;
    let ctx = MappingCtx {
        pepper: pii::tenant_pepper(&st.cfg.pii_pepper, tenant),
        default_tz: pctx.timezone,
        default_event_type: src
            .get("default_event_type")
            .and_then(Value::as_str)
            .map(String::from),
    };
    let results: Vec<Value> = records
        .iter()
        .map(|r| {
            match apply_mapping(&m, r, &ctx)
                .and_then(|out| normalize_event(out.event, &ctx.pepper).map(|n| (n, out.label)))
            {
                Ok((n, label)) => {
                    let mut event = serde_json::to_value(&n.event).unwrap_or(Value::Null);
                    let customer = event.get("customer").cloned().unwrap_or(Value::Null);
                    if let Some(o) = event.as_object_mut() {
                        o.remove("customer");
                    }
                    json!({ "ok": true, "event": event, "customer": customer, "label": label })
                }
                Err(errs) => json!({ "ok": false, "errors": errs }),
            }
        })
        .collect();
    Ok(json!({ "items": results }))
}

fn catalog_type(inferred: &str) -> &'static str {
    match inferred {
        "integer" => "integer",
        "number" | "float" | "decimal" => "number",
        "bool" | "boolean" => "bool",
        "datetime" | "date" | "timestamp" => "datetime",
        "array" => "array",
        "object" => "object",
        _ => "string",
    }
}

/// Paths that must not be exposed as `source.*` (dropped or hashed PII).
fn hidden_paths(m: &Mapping) -> Vec<String> {
    let mut out = m.drop_fields.clone();
    for (key, spec) in &m.event {
        let pii_target = key == "card_number" || key == "account_number";
        let hashing = spec.transform.iter().any(|t| {
            matches!(
                t,
                Transform::HashPan | Transform::HashAccount | Transform::PanBin { .. } | Transform::PanLast4
            )
        });
        if pii_target || hashing {
            if let Some(f) = &spec.from {
                match f {
                    crate::domain::mapping::FromSpec::One(p) => out.push(p.clone()),
                    crate::domain::mapping::FromSpec::Many(ps) => out.extend(ps.iter().cloned()),
                }
            }
        }
    }
    out
}

pub async fn activate_mapping(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    version: i32,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let src = get_in(&mut tx, project, id).await?;
    let row: Option<(Value,)> = sqlx::query_as(
        "SELECT mapping FROM core.data_source_mappings WHERE project_id = $1 AND data_source_id = $2 AND version = $3",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(version)
    .fetch_optional(&mut **tx)
    .await?;
    let mapping_doc = row
        .map(|r| r.0)
        .ok_or_else(|| AppError::not_found("mapping version not found"))?;
    let mapping = validate_mapping(&mapping_doc).map_err(AppError::validation)?;
    sqlx::query(
        "UPDATE core.data_source_mappings SET status = 'archived' \
         WHERE data_source_id = $1 AND status = 'active' AND version <> $2",
    )
    .bind(id)
    .bind(version)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE core.data_source_mappings SET status = 'active', activated_at = now(), activated_by = $3 \
         WHERE data_source_id = $1 AND version = $2",
    )
    .bind(id)
    .bind(version)
    .bind(caller.actor_user_id().map(|u| u.as_uuid()))
    .execute(&mut **tx)
    .await?;

    // Register source fields: inferred schema (typed) + every referenced path.
    let hidden = hidden_paths(&mapping);
    let mut fields: BTreeMap<String, (String, bool, Value)> = BTreeMap::new();
    if let Some(list) = src
        .get("inferred_schema")
        .and_then(|s| s.get("fields"))
        .and_then(Value::as_array)
    {
        for f in list {
            let Some(path) = f.get("path").and_then(Value::as_str) else {
                continue;
            };
            let ty = f
                .get("inferred_type")
                .and_then(Value::as_str)
                .map(catalog_type)
                .unwrap_or("string");
            let pii_flag = f.get("pii").is_some_and(|p| !p.is_null() && p != &json!(false));
            let samples = f.get("sample_values").cloned().unwrap_or(Value::Null);
            fields.insert(path.to_string(), (ty.to_string(), pii_flag, samples));
        }
    }
    for p in referenced_paths(&mapping) {
        fields.entry(p).or_insert(("string".into(), false, Value::Null));
    }
    let mut registered = 0;
    for (path, (ty, pii_flag, samples)) in &fields {
        if hidden
            .iter()
            .any(|h| h == path || path.starts_with(&format!("{h}.")))
            || path.contains("[]")
        {
            continue;
        }
        sqlx::query(
            "INSERT INTO core.field_catalog (tenant_id, project_id, path, data_type, data_source_id, pii, sample_values) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT (project_id, path) DO UPDATE SET data_type = EXCLUDED.data_type, \
                data_source_id = EXCLUDED.data_source_id, pii = EXCLUDED.pii, \
                sample_values = COALESCE(EXCLUDED.sample_values, core.field_catalog.sample_values)",
        )
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(format!("source.{path}"))
        .bind(ty)
        .bind(id)
        .bind(*pii_flag)
        .bind(if samples.is_null() { None } else { Some(samples.clone()) })
        .execute(&mut **tx)
        .await?;
        registered += 1;
    }
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "mapping.activate")
            .scope(tenant, Some(project))
            .subject("data_source", id)
            .after(json!({ "version": version, "registered_fields": registered })),
    )
    .await?;
    tx.commit().await?;
    invalidate_source(st, id).await;
    Ok(
        json!({ "data_source_id": id, "version": version, "status": "active", "registered_fields": registered }),
    )
}

pub async fn list_errors(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM \
           (SELECT id, job_id, record, reason, created_at FROM core.ingest_errors \
            WHERE project_id = $1 AND data_source_id = $2 ORDER BY id DESC) t LIMIT $3 OFFSET $4",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}

// ---------------------------------------------------------------------------------------------
// Field catalog
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct CatalogQuery {
    pub entity: Option<String>,
    pub source_id: Option<Uuid>,
    pub velocity_enabled: Option<bool>,
    pub q: Option<String>,
}

/// Built-ins (from `contracts::catalog`) merged with the project's `source.*` fields.
pub async fn field_catalog(
    st: &AppState,
    tenant: TenantId,
    project: ProjectId,
    q: &CatalogQuery,
) -> AppResult<Value> {
    let mut items: Vec<Value> = Vec::new();
    if q.source_id.is_none() {
        for f in BUILTIN_FIELDS {
            items.push(json!({
                "path": f.path, "entity": f.entity, "data_type": f.data_type, "description": f.description,
                "velocity_enabled": f.velocity_enabled, "pii": f.pii, "builtin": true,
                "velocity_column": catalog::velocity_column(f.path),
            }));
        }
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT jsonb_build_object('path', path, 'entity', 'source', 'data_type', data_type, \
                'description', description, 'velocity_enabled', velocity_enabled, 'pii', pii, \
                'data_source_id', data_source_id, 'sample_values', sample_values, 'builtin', false) \
         FROM core.field_catalog WHERE project_id = $1 AND ($2::uuid IS NULL OR data_source_id = $2) ORDER BY path",
    )
    .bind(project.as_uuid())
    .bind(q.source_id)
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    items.extend(rows.into_iter().map(|r| r.0));
    let needle = q.q.as_deref().map(str::to_lowercase);
    items.retain(|f| {
        q.entity
            .as_deref()
            .is_none_or(|e| f.get("entity").and_then(Value::as_str) == Some(e))
            && q.velocity_enabled
                .is_none_or(|v| f.get("velocity_enabled").and_then(Value::as_bool) == Some(v))
            && needle.as_deref().is_none_or(|n| {
                f.get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| p.to_lowercase().contains(n))
            })
    });
    Ok(json!({ "items": items, "total": items.len() }))
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct CatalogPatch {
    pub velocity_enabled: Option<bool>,
    pub description: Option<String>,
    pub pii: Option<bool>,
}

pub async fn patch_field(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    path: &str,
    input: &CatalogPatch,
) -> AppResult<Value> {
    let tenant = caller
        .require_project_role(project, ProjectRole::ProjectAdmin)
        .await?;
    if !path.starts_with("source.") {
        return Err(AppError::BadRequest("built-in fields cannot be changed".into()));
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value,)> = sqlx::query_as(
        "UPDATE core.field_catalog SET velocity_enabled = COALESCE($3, velocity_enabled), \
            description = COALESCE($4, description), pii = COALESCE($5, pii) \
         WHERE project_id = $1 AND path = $2 \
         RETURNING jsonb_build_object('path', path, 'data_type', data_type, 'velocity_enabled', velocity_enabled, \
                                      'description', description, 'pii', pii)",
    )
    .bind(project.as_uuid())
    .bind(path)
    .bind(input.velocity_enabled)
    .bind(&input.description)
    .bind(input.pii)
    .fetch_optional(&mut **tx)
    .await?;
    let out = row
        .map(|r| r.0)
        .ok_or_else(|| AppError::not_found("field not found"))?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "field_catalog.update")
            .scope(tenant, Some(project))
            .subject("field", path)
            .after(&out),
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}
