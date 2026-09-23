//! core-api binary: `serve` (default), `migrate`, `healthcheck <url>`.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use core_api::config::ServeConfig;
use core_api::state::{http_engines, AppState, Caches, RuntimeConfig};
use platform::auth::{AuthState, PgProjectDirectory};
use platform::cli::Command;
use platform::server::{HttpReadiness, PgReadiness, ServerOptions};

#[derive(Debug, Parser)]
#[command(name = "core-api", version, about = "Fraud platform core API")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or_default() {
        Command::Healthcheck { url } => platform::cli::healthcheck(&url),
        Command::Migrate { dir } => run(migrate(dir)),
        Command::Serve => run(serve()),
    }
}

fn run(fut: impl std::future::Future<Output = anyhow::Result<()>>) -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(fut) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %format!("{e:#}"), "fatal");
            eprintln!("fatal: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn migrate(dir: std::path::PathBuf) -> anyhow::Result<()> {
    platform::telemetry::init_tracing("info");
    let url = std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set"))?;
    let mut last = None;
    // Postgres may still be starting in compose; retry the connection for ~60 s.
    for _ in 0..30 {
        match platform::db::connect_url(&url, 2).await {
            Ok(pool) => {
                platform::db::run_migrations(&pool, &dir).await?;
                return Ok(());
            }
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("could not connect")))
}

async fn serve() -> anyhow::Result<()> {
    let cfg = ServeConfig::try_parse_from(["core-api"])?;
    platform::telemetry::init_tracing(&cfg.base.log_level);
    let metrics = platform::telemetry::init_metrics()?;
    let pool = platform::db::connect_pool(&cfg.base).await?;
    let engines = http_engines(
        &cfg.base.internal_api_token,
        &cfg.rule_service_url,
        &cfg.graph_service_url,
        &cfg.ml_service_url,
    )?;
    let state = AppState {
        pool: pool.clone(),
        auth: AuthState::new(
            &cfg.base.jwt_secret,
            &cfg.base.internal_api_token,
            Arc::new(PgProjectDirectory::new(pool.clone())),
        ),
        cfg: Arc::new(RuntimeConfig {
            jwt_ttl: Duration::from_secs(cfg.jwt_ttl_minutes * 60),
            refresh_ttl_days: cfg.refresh_ttl_days,
            pii_pepper: cfg.pii_pepper.clone(),
            internal_token: cfg.base.internal_api_token.clone(),
        }),
        engines,
        caches: Arc::new(Caches::default()),
    };
    core_api::bootstrap::run(&state, &cfg).await?;

    let opts = ServerOptions::from_config(&cfg.base)
        .with_readiness(PgReadiness(pool))
        .with_readiness(HttpReadiness::new("rules", &cfg.rule_service_url, false))
        .with_readiness(HttpReadiness::new("graph", &cfg.graph_service_url, false))
        .with_readiness(HttpReadiness::new("ml", &cfg.ml_service_url, false))
        .with_readiness(HttpReadiness::new("llm", &cfg.llm_service_url, false))
        .with_readiness(HttpReadiness::new("ingest", &cfg.ingest_service_url, false))
        .with_metrics(metrics);
    platform::server::serve(core_api::router(state), opts).await
}
