//! core-api configuration (env vars, see `.env.example` / docker-compose.yml).

use platform::config::{BaseConfig, Secret};

#[derive(Debug, Clone, clap::Parser)]
#[command(
    name = "core-api",
    about = "core-api server configuration (read from the environment)"
)]
pub struct ServeConfig {
    #[command(flatten)]
    pub base: BaseConfig,

    #[arg(long, env = "JWT_TTL_MINUTES", default_value_t = 60)]
    pub jwt_ttl_minutes: u64,

    #[arg(long, env = "REFRESH_TTL_DAYS", default_value_t = 14)]
    pub refresh_ttl_days: i64,

    /// Platform pepper for tenant-scoped PII fingerprints. Never rotate without re-hashing.
    #[arg(long, env = "PII_PEPPER")]
    pub pii_pepper: Secret,

    #[arg(long, env = "ADMIN_EMAIL", default_value = "admin@fraud.local")]
    pub admin_email: String,

    #[arg(long, env = "ADMIN_PASSWORD")]
    pub admin_password: Option<Secret>,

    #[arg(long, env = "SEED_DEMO", default_value_t = false, action = clap::ArgAction::Set)]
    pub seed_demo: bool,

    #[arg(long, env = "DEMO_USER_PASSWORD")]
    pub demo_user_password: Option<Secret>,

    #[arg(long, env = "RULE_SERVICE_URL", default_value = "http://rule-service:8081")]
    pub rule_service_url: String,

    #[arg(long, env = "GRAPH_SERVICE_URL", default_value = "http://graph-service:8082")]
    pub graph_service_url: String,

    #[arg(long, env = "ML_SERVICE_URL", default_value = "http://ml-service:8001")]
    pub ml_service_url: String,

    #[arg(long, env = "LLM_SERVICE_URL", default_value = "http://llm-service:8002")]
    pub llm_service_url: String,

    #[arg(
        long,
        env = "INGEST_SERVICE_URL",
        default_value = "http://ingest-service:8003"
    )]
    pub ingest_service_url: String,
}

/// Treats an empty secret env var (`DEMO_USER_PASSWORD=`) as absent.
pub fn non_empty(s: &Option<Secret>) -> Option<&Secret> {
    s.as_ref().filter(|s| !s.expose().trim().is_empty())
}
