//! Loading (and caching) per-project context and data-source configuration.

use std::sync::Arc;

use chrono_tz::Tz;
use contracts::graph::LinkKind;
use platform::db::TenantTx;
use platform::error::{AppError, AppResult};
use platform::{ProjectId, TenantId};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::domain::mapping::{validate_mapping, Mapping};
use crate::domain::settings::ProjectSettings;
use crate::state::{AppState, GraphParams, ProjectCtx, SourceCfg};

pub fn parse_graph_params(v: &Value) -> GraphParams {
    GraphParams {
        link_kinds: v.get("link_kinds").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(LinkKind::parse)
                .collect()
        }),
        include_similar: v.get("include_similar").and_then(Value::as_bool),
        max_depth: v
            .get("max_depth")
            .and_then(Value::as_u64)
            .and_then(|d| u8::try_from(d).ok()),
    }
}

/// Loads project context inside an existing tenant transaction (no cache).
pub async fn load_project_ctx(tx: &mut TenantTx<'_>, project: ProjectId) -> AppResult<ProjectCtx> {
    let row = sqlx::query("SELECT slug, timezone, status, graph_config FROM core.projects WHERE id = $1")
        .bind(project.as_uuid())
        .fetch_optional(&mut ***tx)
        .await?
        .ok_or_else(|| AppError::not_found("project not found"))?;
    let rows: Vec<(String, Value)> =
        sqlx::query_as("SELECT key, value FROM core.project_settings WHERE project_id = $1")
            .bind(project.as_uuid())
            .fetch_all(&mut ***tx)
            .await?;
    let tz_name: String = row.get("timezone");
    Ok(ProjectCtx {
        tenant: tx.tenant(),
        project,
        slug: row.get("slug"),
        timezone: tz_name.parse::<Tz>().unwrap_or(chrono_tz::Asia::Jakarta),
        status: row.get("status"),
        settings: ProjectSettings::from_rows(rows.iter().map(|(k, v)| (k.as_str(), v))),
        graph: parse_graph_params(&row.get::<Value, _>("graph_config")),
    })
}

/// Cached project context.
pub async fn project_ctx(st: &AppState, tenant: TenantId, project: ProjectId) -> AppResult<Arc<ProjectCtx>> {
    if let Some(c) = st.caches.projects.get(&project.as_uuid()).await {
        if c.tenant == tenant {
            return Ok(c);
        }
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let ctx = Arc::new(load_project_ctx(&mut tx, project).await?);
    tx.commit().await?;
    st.caches.projects.insert(project.as_uuid(), ctx.clone()).await;
    Ok(ctx)
}

pub async fn invalidate_project(st: &AppState, project: ProjectId) {
    st.caches.projects.invalidate(&project.as_uuid()).await;
}

/// Loads a data source + its active mapping.
pub async fn load_source(tx: &mut TenantTx<'_>, project: ProjectId, source_id: Uuid) -> AppResult<SourceCfg> {
    let row = sqlx::query(
        "SELECT ds.id, ds.slug, ds.kind, ds.mode, ds.default_event_type, ds.is_active, m.mapping \
         FROM core.data_sources ds \
         LEFT JOIN core.data_source_mappings m ON m.data_source_id = ds.id AND m.status = 'active' \
         WHERE ds.project_id = $1 AND ds.id = $2",
    )
    .bind(project.as_uuid())
    .bind(source_id)
    .fetch_optional(&mut ***tx)
    .await?
    .ok_or_else(|| AppError::not_found("data source not found"))?;
    let mapping: Option<Value> = row.get("mapping");
    let mapping: Option<Arc<Mapping>> = match mapping {
        Some(v) => Some(Arc::new(validate_mapping(&v).map_err(|errs| {
            AppError::Validation {
                detail: Some("the active mapping is invalid".into()),
                errors: errs,
            }
        })?)),
        None => None,
    };
    Ok(SourceCfg {
        id: row.get("id"),
        tenant: tx.tenant(),
        project,
        slug: row.get("slug"),
        kind: row.get("kind"),
        mode: row.get("mode"),
        default_event_type: row.get("default_event_type"),
        is_active: row.get("is_active"),
        mapping,
    })
}

pub async fn source_cfg(
    st: &AppState,
    tenant: TenantId,
    project: ProjectId,
    source_id: Uuid,
) -> AppResult<Arc<SourceCfg>> {
    if let Some(c) = st.caches.sources.get(&source_id).await {
        if c.tenant == tenant && c.project == project {
            return Ok(c);
        }
    }
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let cfg = Arc::new(load_source(&mut tx, project, source_id).await?);
    tx.commit().await?;
    st.caches.sources.insert(source_id, cfg.clone()).await;
    Ok(cfg)
}

pub async fn invalidate_source(st: &AppState, source_id: Uuid) {
    st.caches.sources.invalidate(&source_id).await;
}

/// The internal `canonical` data source of a project.
pub async fn canonical_source_id(tx: &mut TenantTx<'_>, project: ProjectId) -> AppResult<Uuid> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM core.data_sources WHERE project_id = $1 AND slug = 'canonical'")
            .bind(project.as_uuid())
            .fetch_optional(&mut ***tx)
            .await?;
    row.map(|r| r.0)
        .ok_or_else(|| AppError::internal("project has no canonical data source"))
}
