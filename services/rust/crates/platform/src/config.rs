//! Configuration from environment variables (12-factor).
//!
//! Every service declares its own config struct with `#[derive(clap::Args)]` and embeds
//! [`BaseConfig`] with `#[command(flatten)]`. That is composition in place of a Java
//! `@ConfigurationProperties` hierarchy. `clap` reads each field from the listed `env` var, so the
//! same struct also works as CLI flags in local development and tests.
//!
//! Secrets are wrapped in [`Secret`] so they are never printed by `Debug` (for example in a
//! startup log line).

use std::fmt;
use std::str::FromStr;

/// A string that never prints its content.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl FromStr for Secret {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

/// Settings every Rust service needs.
#[derive(Debug, Clone, clap::Args)]
pub struct BaseConfig {
    /// Postgres connection string (the service's own least-privilege role).
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: Secret,

    #[arg(long, env = "DATABASE_MAX_CONNECTIONS", default_value_t = 20)]
    pub database_max_connections: u32,

    /// Server-side `statement_timeout` in ms (0 = none).
    #[arg(long, env = "DATABASE_STATEMENT_TIMEOUT_MS", default_value_t = 5000)]
    pub database_statement_timeout_ms: u64,

    /// Server-side `idle_in_transaction_session_timeout` in ms (0 = none).
    #[arg(
        long,
        env = "DATABASE_IDLE_IN_TRANSACTION_TIMEOUT_MS",
        default_value_t = 10000
    )]
    pub database_idle_in_transaction_timeout_ms: u64,

    /// Service name for logs and `pg_stat_activity.application_name`. Defaults to the binary name
    /// (images name every binary `/app/service`, so compose sets `SERVICE_NAME`).
    #[arg(long, env = "SERVICE_NAME", default_value = "")]
    pub service_name: String,

    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind_addr: String,

    /// HS256 secret shared by all services to verify user JWTs.
    #[arg(long, env = "JWT_SECRET")]
    pub jwt_secret: Secret,

    /// Service-to-service bearer token.
    #[arg(long, env = "INTERNAL_API_TOKEN")]
    pub internal_api_token: Secret,

    #[arg(long, env = "RUST_LOG", default_value = "info")]
    pub log_level: String,

    /// Comma-separated list of allowed CORS origins. Empty = no CORS headers (same-origin via gateway).
    #[arg(long, env = "CORS_ORIGINS", default_value = "")]
    pub cors_origins: String,

    /// Default request timeout in seconds.
    #[arg(long, env = "REQUEST_TIMEOUT_SECS", default_value_t = 30)]
    pub request_timeout_secs: u64,

    /// Maximum request body size in bytes.
    #[arg(long, env = "MAX_BODY_BYTES", default_value_t = 5 * 1024 * 1024)]
    pub max_body_bytes: usize,
}

/// Binary file name (`core-api` when run via cargo), falling back to `fraud-platform`.
pub fn default_service_name() -> String {
    std::env::var("SERVICE_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "fraud-platform".into())
}

impl BaseConfig {
    pub fn resolved_service_name(&self) -> String {
        if self.service_name.is_empty() {
            default_service_name()
        } else {
            self.service_name.clone()
        }
    }

    pub fn cors_origin_list(&self) -> Vec<String> {
        self.cors_origins
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(s.expose(), "hunter2");
    }
}
