//! Project field catalogue: `contracts::catalog` built-ins + the project's `source.*` fields from
//! `core.field_catalog` (registered by core-api when a data-source mapping is activated).
//!
//! It implements the rule engine's [`FieldCatalog`] port (used by the validator) and is also the **allow-list**
//! for SQL identifiers in [`super::velocity_sql`]: a history field can only reach SQL if this catalogue says it is
//! velocity-enabled.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use platform::db::TenantTx;
use platform::{AppResult, ProjectId, TenantId};
use rule_engine::FieldCatalog;
use sqlx::PgPool;

#[derive(Debug, Clone, Default)]
pub struct ProjectCatalog {
    /// `source.*` path → velocity_enabled.
    source_fields: HashMap<String, bool>,
}

impl ProjectCatalog {
    /// Built-ins only (tests, templates).
    pub fn builtins_only() -> Self {
        Self::default()
    }

    pub fn with_source_fields(fields: impl IntoIterator<Item = (String, bool)>) -> Self {
        Self {
            source_fields: fields.into_iter().collect(),
        }
    }

    /// Whether a `source.*` field may be used in velocity group-by / aggregates / history filters.
    pub fn source_velocity_enabled(&self, path: &str) -> bool {
        self.source_fields.get(path).copied().unwrap_or(false)
    }
}

impl FieldCatalog for ProjectCatalog {
    fn is_known_path(&self, path: &str) -> bool {
        contracts::catalog::lookup(path).is_some() || self.source_fields.contains_key(path)
    }

    fn is_velocity_field(&self, field: &str) -> bool {
        if field.starts_with("source.") {
            self.source_velocity_enabled(field)
        } else {
            contracts::catalog::velocity_column(field).is_some()
        }
    }
}

/// Loads and caches catalogues per project (60 s TTL; field registration is rare).
#[derive(Clone)]
pub struct CatalogCache {
    pool: PgPool,
    cache: Cache<ProjectId, Arc<ProjectCatalog>>,
}

impl std::fmt::Debug for CatalogCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogCache").finish_non_exhaustive()
    }
}

impl CatalogCache {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
        }
    }

    pub async fn get(&self, tenant: TenantId, project: ProjectId) -> AppResult<Arc<ProjectCatalog>> {
        if let Some(hit) = self.cache.get(&project).await {
            return Ok(hit);
        }
        let mut tx = TenantTx::begin(&self.pool, tenant).await?;
        let rows: Vec<(String, bool)> =
            sqlx::query_as("SELECT path, velocity_enabled FROM core.field_catalog WHERE project_id = $1")
                .bind(project.as_uuid())
                .fetch_all(&mut **tx)
                .await?;
        tx.commit().await?;
        let catalog = Arc::new(ProjectCatalog::with_source_fields(rows));
        self.cache.insert(project, catalog.clone()).await;
        Ok(catalog)
    }

    pub async fn invalidate(&self, project: ProjectId) {
        self.cache.invalidate(&project).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_and_source_fields() {
        let c = ProjectCatalog::with_source_fields([
            ("source.order.total".to_string(), true),
            ("source.note".to_string(), false),
        ]);
        assert!(c.is_known_path("event.amount"));
        assert!(c.is_known_path("features.cust_cnt_1h"));
        assert!(c.is_known_path("source.note"));
        assert!(!c.is_known_path("event.nope"));
        assert!(c.is_velocity_field("amount"));
        assert!(c.is_velocity_field("customer_id"));
        assert!(c.is_velocity_field("source.order.total"));
        assert!(!c.is_velocity_field("source.note"));
        assert!(!c.is_velocity_field("source.unknown"));
        assert!(!c.is_velocity_field("card_last4"));
        assert!(!c.is_velocity_field("amount; DROP TABLE x"));
    }
}
