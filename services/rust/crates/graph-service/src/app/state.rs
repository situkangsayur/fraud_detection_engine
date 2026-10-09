//! Shared application state, cloned into every handler (all fields are cheap `Arc` clones).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::FromRef;
use moka::future::Cache;
use platform::auth::{AuthState, PgProjectDirectory};
use platform::config::Secret;
use platform::{ProjectId, TenantId};
use sqlx::PgPool;

use super::project_config::ProjectConfigs;
use crate::domain::components::Component;

/// Components per project, cached for 5 minutes (api-contract §C). Invalidated on label changes.
pub type ComponentsCache = Cache<(TenantId, ProjectId), Arc<Vec<Component>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: AuthState,
    pub configs: ProjectConfigs,
    pub components: ComponentsCache,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

impl AppState {
    pub fn new(pool: PgPool, jwt_secret: &Secret, internal_token: &Secret) -> Self {
        let directory = Arc::new(PgProjectDirectory::new(pool.clone()));
        Self {
            auth: AuthState::new(jwt_secret, internal_token, directory),
            pool,
            configs: ProjectConfigs::default(),
            components: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(300))
                .build(),
        }
    }
}

/// Lets the platform auth extractors (`Caller`, `AuthUser`) find their dependencies.
impl FromRef<AppState> for AuthState {
    fn from_ref(s: &AppState) -> AuthState {
        s.auth.clone()
    }
}
