//! Database access: connection pool, tenant-scoped transactions, migrations.
//!
//! ## Tenant isolation
//!
//! Every tenant table has Row-Level Security `tenant_id = core.current_tenant()`, where
//! `core.current_tenant()` reads the transaction-local setting `app.tenant_id`
//! (docs/technical/multi-tenancy.md §3). [`TenantTx`] is the **only** sanctioned way to touch tenant
//! data: it opens a transaction and sets `app.tenant_id` with `is_local = true`, so the setting
//! disappears at commit/rollback and a pooled connection can never leak one tenant's context into
//! the next request.
//!
//! This follows the RAII pattern (like Java try-with-resources, but automatic): if a `TenantTx`
//! is dropped without `commit()`, sqlx rolls the transaction back.
//!
//! Queries must still filter by `project_id` explicitly. RLS is the safety net, not the filter.

use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use std::str::FromStr;

use crate::config::BaseConfig;
use crate::error::{AppError, AppResult};
use crate::ids::TenantId;

/// Connection-level settings applied to every pooled connection.
#[derive(Debug, Clone)]
pub struct DbOptions {
    pub max_connections: u32,
    /// Shown in `pg_stat_activity.application_name` (e.g. `rule-service`).
    pub application_name: String,
    /// Server-side `statement_timeout`; `0` = no limit. Protects the pool from runaway queries.
    pub statement_timeout_ms: u64,
    /// Server-side `idle_in_transaction_session_timeout`; `0` = no limit. Kills sessions that hold
    /// a transaction (and its locks) open without doing anything, e.g. after a crashed handler.
    pub idle_in_transaction_timeout_ms: u64,
}

impl DbOptions {
    pub fn from_config(cfg: &BaseConfig) -> Self {
        Self {
            max_connections: cfg.database_max_connections,
            application_name: cfg.resolved_service_name(),
            statement_timeout_ms: cfg.database_statement_timeout_ms,
            idle_in_transaction_timeout_ms: cfg.database_idle_in_transaction_timeout_ms,
        }
    }
}

/// Creates the connection pool for the service's role, with the configured server-side timeouts
/// (`DATABASE_STATEMENT_TIMEOUT_MS`, `DATABASE_IDLE_IN_TRANSACTION_TIMEOUT_MS`) and
/// `application_name` = service name.
pub async fn connect_pool(cfg: &BaseConfig) -> anyhow::Result<PgPool> {
    connect_with(cfg.database_url.expose(), &DbOptions::from_config(cfg)).await
}

/// Pool **without** statement timeouts — for migrations and tests, where long DDL is expected.
pub async fn connect_url(url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    connect_with(
        url,
        &DbOptions {
            max_connections,
            application_name: crate::config::default_service_name(),
            statement_timeout_ms: 0,
            idle_in_transaction_timeout_ms: 0,
        },
    )
    .await
}

/// Pool with explicit [`DbOptions`]. The timeouts are sent as startup parameters (`-c key=value`),
/// so they apply to every connection from the first statement on — no `after_connect` round trip.
pub async fn connect_with(url: &str, db: &DbOptions) -> anyhow::Result<PgPool> {
    let opts = PgConnectOptions::from_str(url)?
        .application_name(&db.application_name)
        .options([
            ("statement_timeout", db.statement_timeout_ms.to_string()),
            (
                "idle_in_transaction_session_timeout",
                db.idle_in_transaction_timeout_ms.to_string(),
            ),
        ]);
    let pool = PgPoolOptions::new()
        .max_connections(db.max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .connect_with(opts)
        .await?;
    Ok(pool)
}

/// A transaction bound to one tenant. Execute queries with `.fetch_all(tx.conn())`
/// (the older `&mut ***tx` via `Deref` keeps working).
#[derive(Debug)]
pub struct TenantTx<'c> {
    tx: Transaction<'c, Postgres>,
    tenant: TenantId,
}

impl TenantTx<'static> {
    pub async fn begin(pool: &PgPool, tenant: TenantId) -> AppResult<Self> {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(Self { tx, tenant })
    }
}

impl<'c> TenantTx<'c> {
    pub fn tenant(&self) -> TenantId {
        self.tenant
    }

    /// The underlying connection, usable as a sqlx executor: `query.fetch_one(tx.conn()).await`.
    pub fn conn(&mut self) -> &mut PgConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> AppResult<()> {
        self.tx.commit().await.map_err(AppError::from)
    }

    pub async fn rollback(self) -> AppResult<()> {
        self.tx.rollback().await.map_err(AppError::from)
    }
}

impl<'c> Deref for TenantTx<'c> {
    type Target = Transaction<'c, Postgres>;
    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

impl DerefMut for TenantTx<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tx
    }
}

/// Runs the SQL migrations found in `dir` (default `/app/migrations` in images).
/// Used by `core-api migrate` with the `migrator` role (owner of all schemas).
pub async fn run_migrations(pool: &PgPool, dir: &Path) -> anyhow::Result<()> {
    let migrator = sqlx::migrate::Migrator::new(dir).await?;
    tracing::info!(dir = %dir.display(), count = migrator.iter().count(), "running migrations");
    migrator.run(pool).await?;
    tracing::info!("migrations complete");
    Ok(())
}

/// Readiness probe helper: `SELECT 1`.
pub async fn ping(pool: &PgPool) -> AppResult<()> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
