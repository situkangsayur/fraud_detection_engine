//! Application state shared by all handlers (axum `State`).
//!
//! Dependency injection without a container: `main` builds one [`AppState`] and axum clones it
//! (cheaply; everything inside is `Arc`) into each request. Tests build the same struct with fake
//! engine clients. This is the moral equivalent of Spring wiring beans, done explicitly in
//! code, where the compiler checks that every dependency is provided.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::FromRef;
use chrono_tz::Tz;
use contracts::graph::LinkKind;
use moka::future::Cache;
use platform::auth::AuthState;
use platform::config::Secret;
use platform::http::ServiceClient;
use platform::{ProjectId, TenantId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::{GraphClient, MlClient, RuleEngineClient};
use crate::domain::mapping::Mapping;
use crate::domain::settings::ProjectSettings;

/// Downstream engines (ports).
#[derive(Clone)]
pub struct Engines {
    pub rules: Arc<dyn RuleEngineClient>,
    pub graph: Arc<dyn GraphClient>,
    pub ml: Arc<dyn MlClient>,
}

impl std::fmt::Debug for Engines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engines")
    }
}

/// Settings that are fixed for the process lifetime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub jwt_ttl: Duration,
    pub refresh_ttl_days: i64,
    pub pii_pepper: Secret,
    /// Service-to-service token (for the `/v1/internal` extractor).
    pub internal_token: Secret,
}

/// Graph parameters of a project (`core.projects.graph_config`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphParams {
    pub link_kinds: Option<Vec<LinkKind>>,
    pub include_similar: Option<bool>,
    pub max_depth: Option<u8>,
}

/// Per-project data the pipeline needs on every event (cached ~30 s).
#[derive(Debug, Clone)]
pub struct ProjectCtx {
    pub tenant: TenantId,
    pub project: ProjectId,
    pub slug: String,
    pub timezone: Tz,
    pub status: String,
    pub settings: ProjectSettings,
    pub graph: GraphParams,
}

/// A data source with its active mapping (cached ~30 s, invalidated on activation).
#[derive(Debug, Clone)]
pub struct SourceCfg {
    pub id: Uuid,
    pub tenant: TenantId,
    pub project: ProjectId,
    pub slug: String,
    pub kind: String,
    pub mode: String,
    pub default_event_type: Option<String>,
    pub is_active: bool,
    pub mapping: Option<Arc<Mapping>>,
}

/// Result of a verified webhook key.
#[derive(Debug, Clone)]
pub struct SourceAuth {
    pub tenant: TenantId,
    pub project: ProjectId,
    pub source_id: Uuid,
    pub slug: String,
}

#[derive(Debug, Clone)]
pub struct Caches {
    pub projects: Cache<Uuid, Arc<ProjectCtx>>,
    pub sources: Cache<Uuid, Arc<SourceCfg>>,
    /// sha256(api key) → verified source (argon2 verification is deliberately slow).
    pub api_keys: Cache<String, SourceAuth>,
    /// (project, path) of `source.*` paths already registered in `core.field_catalog`.
    pub known_paths: Cache<(Uuid, String), ()>,
}

impl Default for Caches {
    fn default() -> Self {
        Self {
            projects: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            sources: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            api_keys: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
            known_paths: Cache::builder()
                .max_capacity(200_000)
                .time_to_live(Duration::from_secs(600))
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: AuthState,
    pub cfg: Arc<RuntimeConfig>,
    pub engines: Engines,
    pub caches: Arc<Caches>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl FromRef<AppState> for AuthState {
    fn from_ref(s: &AppState) -> AuthState {
        s.auth.clone()
    }
}

/// Builds the HTTP engine adapters from base URLs.
pub fn http_engines(
    token: &Secret,
    rule_url: &str,
    graph_url: &str,
    ml_url: &str,
) -> anyhow::Result<Engines> {
    use crate::adapters::engines::{HttpGraph, HttpMl, HttpRuleEngine};
    let mk = |name: &'static str, url: &str| {
        ServiceClient::new(name, url, token.clone(), Duration::from_secs(5))
            .map_err(|e| anyhow::anyhow!("{e}"))
    };
    Ok(Engines {
        rules: Arc::new(HttpRuleEngine(mk("rule-service", rule_url)?)),
        graph: Arc::new(HttpGraph(mk("graph-service", graph_url)?)),
        ml: Arc::new(HttpMl(mk("ml-service", ml_url)?)),
    })
}
