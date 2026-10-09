//! Per-project graph configuration (`core.projects.graph_config`), cached for 60 s.
//!
//! The cache key is `(tenant, project)`: a project id presented under the wrong tenant never hits
//! another tenant's cached entry, and RLS makes the lookup itself return nothing.

use std::sync::Arc;
use std::time::Duration;

use contracts::graph::LinkKind;
use moka::future::Cache;
use platform::db::TenantTx;
use platform::{AppError, AppResult, ProjectId, TenantId};
use serde::Deserialize;

use crate::adapters::postgres;
use crate::domain::model::{limits, TraversalParams};

/// Mirrors the `graph_config` JSON documented in multi-tenancy.md §2. Missing keys take defaults.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
    pub link_kinds: Vec<String>,
    pub include_similar: bool,
    pub max_depth: u32,
    pub supernode_degree_cap: u32,
    pub similarity_threshold: f32,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            link_kinds: LinkKind::DEFAULT.iter().map(|k| k.as_str().to_string()).collect(),
            include_similar: true,
            max_depth: 3,
            supernode_degree_cap: 50,
            similarity_threshold: 0.85,
        }
    }
}

/// Request-level overrides of the project defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    pub link_kinds: Option<Vec<LinkKind>>,
    pub include_similar: Option<bool>,
    pub max_depth: Option<u32>,
}

impl GraphConfig {
    /// Configured kinds (unknown names ignored; empty → defaults).
    pub fn kinds(&self) -> Vec<LinkKind> {
        let kinds: Vec<LinkKind> = self
            .link_kinds
            .iter()
            .filter_map(|k| LinkKind::parse(k))
            .collect();
        if kinds.is_empty() {
            LinkKind::DEFAULT.to_vec()
        } else {
            kinds
        }
    }

    pub fn supernode_cap(&self) -> u32 {
        self.supernode_degree_cap.max(2)
    }

    pub fn threshold(&self) -> f32 {
        self.similarity_threshold.clamp(0.0, 1.0)
    }

    /// Traversal parameters: project defaults, then request overrides, then hard limits.
    pub fn params(&self, o: &Overrides) -> TraversalParams {
        TraversalParams {
            link_kinds: o.link_kinds.clone().unwrap_or_else(|| self.kinds()),
            include_similar: o.include_similar.unwrap_or(self.include_similar),
            max_depth: o.max_depth.unwrap_or(self.max_depth).clamp(1, limits::MAX_DEPTH),
            supernode_cap: self.supernode_cap(),
            similarity_threshold: self.threshold(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectConfigs {
    cache: Cache<(TenantId, ProjectId), Arc<GraphConfig>>,
}

impl Default for ProjectConfigs {
    fn default() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
        }
    }
}

impl ProjectConfigs {
    /// Loads (or returns cached) config; 404 when the project is not visible to the tenant.
    pub async fn get(
        &self,
        tx: &mut TenantTx<'static>,
        tenant: TenantId,
        project: ProjectId,
    ) -> AppResult<Arc<GraphConfig>> {
        if let Some(c) = self.cache.get(&(tenant, project)).await {
            return Ok(c);
        }
        let raw = postgres::project_graph_config(tx, project)
            .await?
            .ok_or_else(|| AppError::not_found("project not found"))?;
        let cfg = Arc::new(parse_config(raw));
        self.cache.insert((tenant, project), cfg.clone()).await;
        Ok(cfg)
    }
}

/// Invalid config JSON falls back to defaults (logged) rather than breaking scoring.
pub fn parse_config(raw: serde_json::Value) -> GraphConfig {
    serde_json::from_value(raw).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "invalid graph_config, using defaults");
        GraphConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn parses_partial_config_and_applies_overrides_and_limits() {
        let cfg = parse_config(json!({"link_kinds": ["card", "bogus", "ip"], "max_depth": 2}));
        assert_eq!(cfg.kinds(), vec![LinkKind::Card, LinkKind::Ip]);
        assert!(cfg.include_similar);
        let p = cfg.params(&Overrides {
            max_depth: Some(99),
            include_similar: Some(false),
            ..Overrides::default()
        });
        assert_eq!(p.max_depth, limits::MAX_DEPTH);
        assert!(!p.include_similar);
        assert_eq!(p.link_kinds, vec![LinkKind::Card, LinkKind::Ip]);
    }

    #[test]
    fn garbage_config_falls_back_to_defaults() {
        assert_eq!(parse_config(json!("nope")), GraphConfig::default());
        assert_eq!(
            GraphConfig {
                link_kinds: vec![],
                ..GraphConfig::default()
            }
            .kinds()
            .len(),
            7
        );
    }
}
