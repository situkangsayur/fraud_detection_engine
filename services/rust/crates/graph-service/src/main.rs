//! graph-service binary: `graph-service [serve|healthcheck <url>]`.

use std::process::ExitCode;

use clap::Parser;
use graph_service::{router, AppState};
use platform::cli::{healthcheck, Command};
use platform::config::BaseConfig;
use platform::server::{serve, PgReadiness, ServerOptions};
use platform::telemetry::{init_metrics, init_tracing};

#[derive(Debug, Parser)]
#[command(name = "graph-service", version, about = "Fraud platform graph engine")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Runtime configuration, read from the environment (see `.env.example` / docker-compose.yml).
#[derive(Debug, Parser)]
struct ServeConfig {
    #[command(flatten)]
    base: BaseConfig,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or_default() {
        Command::Healthcheck { url } => healthcheck(&url),
        Command::Migrate { .. } => {
            eprintln!("graph-service has no migrations; run `core-api migrate`");
            ExitCode::FAILURE
        }
        Command::Serve => {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("failed to start runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(run()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = %format!("{e:#}"), "graph-service failed");
                    eprintln!("graph-service failed: {e:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

async fn run() -> anyhow::Result<()> {
    // Parse config from the environment only (the binary name is the sole argv entry).
    let cfg = ServeConfig::try_parse_from(["graph-service"])?;
    init_tracing(&cfg.base.log_level);
    let metrics = init_metrics()?;
    let pool = platform::db::connect_pool(&cfg.base).await?;
    let state = AppState::new(pool.clone(), &cfg.base.jwt_secret, &cfg.base.internal_api_token);
    let opts = ServerOptions::from_config(&cfg.base)
        .with_readiness(PgReadiness(pool))
        .with_metrics(metrics);
    serve(router(state), opts).await
}
