//! rule-service configuration (env vars, 12-factor). Composition, not inheritance: the shared
//! [`platform::config::BaseConfig`] is embedded with `#[command(flatten)]`.

use std::time::Duration;

use platform::config::BaseConfig;

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "rule-service", about = "Rule service configuration")]
pub struct Config {
    #[command(flatten)]
    pub base: BaseConfig,

    /// graph-service base URL (graph rules).
    #[arg(long, env = "GRAPH_SERVICE_URL", default_value = "http://graph-service:8082")]
    pub graph_service_url: String,

    /// Timeout of one graph-metric call, in milliseconds.
    #[arg(long, env = "GRAPH_TIMEOUT_MS", default_value_t = 100)]
    pub graph_timeout_ms: u64,

    /// Default per-rule evaluation budget in milliseconds (a slower rule is `trapped: timeout`).
    #[arg(long, env = "RULE_TIMEOUT_MS", default_value_t = 50)]
    pub rule_timeout_ms: u64,

    /// Write rule hits/counters in the background after `/evaluate` responds (lower latency).
    #[arg(long, env = "EVAL_ASYNC_WRITES", default_value_t = true, action = clap::ArgAction::Set)]
    pub eval_async_writes: bool,

    /// TTL of the per-project serving (active rulesets) cache, in seconds.
    #[arg(long, env = "SERVING_CACHE_TTL_SECS", default_value_t = 10)]
    pub serving_cache_ttl_secs: u64,

    /// TTL of cached reference-list lookups, in seconds.
    #[arg(long, env = "REFERENCE_CACHE_TTL_SECS", default_value_t = 5)]
    pub reference_cache_ttl_secs: u64,

    /// Concurrent events replayed by one backtest.
    #[arg(long, env = "BACKTEST_CONCURRENCY", default_value_t = 8)]
    pub backtest_concurrency: usize,
}

/// Runtime settings derived from [`Config`] (also constructed directly by tests).
#[derive(Debug, Clone)]
pub struct Settings {
    pub rule_timeout: Duration,
    pub eval_async_writes: bool,
    pub backtest_concurrency: usize,
}

impl From<&Config> for Settings {
    fn from(c: &Config) -> Self {
        Self {
            rule_timeout: Duration::from_millis(c.rule_timeout_ms),
            eval_async_writes: c.eval_async_writes,
            backtest_concurrency: c.backtest_concurrency.max(1),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rule_timeout: Duration::from_millis(50),
            eval_async_writes: false,
            backtest_concurrency: 4,
        }
    }
}
