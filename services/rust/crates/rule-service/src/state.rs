//! Shared application state, cloned into every handler by axum.
//!
//! This plays the role of the Spring application context: the long-lived collaborators (pool, caches,
//! clients) are created once in `main` and **passed in** (dependency injection by constructor). No globals,
//! no service locator, so tests build their own `AppState` against a test database.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::FromRef;
use platform::auth::{AuthState, PgProjectDirectory};
use platform::config::Secret;
use sqlx::PgPool;

use crate::adapters::catalog::CatalogCache;
use crate::adapters::data_provider::GraphClient;
use crate::adapters::provider_cache::{new_ref_cache, RefCache};
use crate::adapters::serving::ServingCache;
use crate::config::Settings;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: AuthState,
    pub catalogs: CatalogCache,
    pub serving: ServingCache,
    pub ref_cache: RefCache,
    pub graph: Option<GraphClient>,
    pub settings: Arc<Settings>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        jwt_secret: &Secret,
        internal_token: &Secret,
        graph: Option<GraphClient>,
        settings: Settings,
        serving_ttl: Duration,
        reference_ttl: Duration,
    ) -> Self {
        let directory = Arc::new(PgProjectDirectory::new(pool.clone()));
        Self {
            auth: AuthState::new(jwt_secret, internal_token, directory),
            catalogs: CatalogCache::new(pool.clone()),
            serving: ServingCache::new(pool.clone(), serving_ttl),
            ref_cache: new_ref_cache(reference_ttl),
            graph,
            settings: Arc::new(settings),
            pool,
        }
    }
}

impl FromRef<AppState> for AuthState {
    fn from_ref(s: &AppState) -> Self {
        s.auth.clone()
    }
}
