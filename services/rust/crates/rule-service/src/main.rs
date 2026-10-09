//! `rule-service` binary: `serve` (default) or `healthcheck <url>`.

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use platform::cli::{self, Command};
use platform::http::ServiceClient;
use platform::server::{self, HttpReadiness, PgReadiness, ServerOptions};
use platform::{db, telemetry};
use rule_service::adapters::data_provider::GraphClient;
use rule_service::config::{Config, Settings};
use rule_service::state::AppState;

#[derive(Debug, clap::Parser)]
#[command(name = "rule-service", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or_default() {
        Command::Healthcheck { url } => cli::healthcheck(&url),
        Command::Migrate { .. } => {
            eprintln!("rule-service does not run migrations (core-api `migrate` owns the schema)");
            ExitCode::FAILURE
        }
        Command::Serve => {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("cannot start runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match runtime.block_on(serve()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = ?e, "rule-service failed");
                    eprintln!("rule-service failed: {e:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

async fn serve() -> anyhow::Result<()> {
    // Configuration comes from the environment only (so `healthcheck` needs no config).
    let config = Config::parse_from(["rule-service"]);
    telemetry::init_tracing(&config.base.log_level);
    let metrics = telemetry::init_metrics()?;
    let pool = db::connect_pool(&config.base).await?;

    let graph_client = ServiceClient::new(
        "graph-service",
        config.graph_service_url.clone(),
        config.base.internal_api_token.clone(),
        Duration::from_millis(config.graph_timeout_ms),
    )?;
    let graph = GraphClient::new(graph_client, Duration::from_millis(config.graph_timeout_ms));
    let state = AppState::new(
        pool.clone(),
        &config.base.jwt_secret,
        &config.base.internal_api_token,
        Some(graph),
        Settings::from(&config),
        Duration::from_secs(config.serving_cache_ttl_secs),
        Duration::from_secs(config.reference_cache_ttl_secs),
    );
    let options = ServerOptions::from_config(&config.base)
        .with_readiness(PgReadiness(pool))
        .with_readiness(HttpReadiness::new(
            "graph-service",
            &config.graph_service_url,
            false,
        ))
        .with_metrics(metrics);
    tracing::info!(bind = %config.base.bind_addr, "starting rule-service");
    server::serve(rule_service::api::router(state), options).await
}
