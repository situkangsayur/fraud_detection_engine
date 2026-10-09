//! Command-line helpers shared by the service binaries.
//!
//! Each binary exposes the same sub-commands so compose/Kubernetes manifests look the same for
//! every service:
//!
//! ```text
//! service serve                  # default — run the HTTP server
//! service migrate                # core-api only: run db/migrations as the `migrator` role
//! service healthcheck <url>      # exit 0 if GET <url> returns 2xx (images are distroless: no curl)
//! ```
//!
//! Typical binary:
//!
//! ```ignore
//! #[derive(clap::Parser)]
//! struct Cli {
//!     #[command(subcommand)]
//!     command: Option<platform::cli::Command>,
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     let cli = Cli::parse();
//!     match cli.command.unwrap_or_default() {
//!         Command::Healthcheck { url } => platform::cli::healthcheck(&url),
//!         Command::Serve => run_server(),
//!         Command::Migrate { dir } => run_migrate(dir),
//!     }
//! }
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug, Clone, Default, clap::Subcommand)]
pub enum Command {
    /// Run the HTTP server (default).
    #[default]
    Serve,
    /// Apply SQL migrations from a directory and exit.
    Migrate {
        #[arg(long, env = "MIGRATIONS_DIR", default_value = "/app/migrations")]
        dir: PathBuf,
    },
    /// HTTP GET the URL; exit 0 on 2xx, 1 otherwise.
    Healthcheck {
        #[arg(default_value = "http://127.0.0.1:8080/health/live")]
        url: String,
    },
}

/// Synchronous health probe (creates its own small runtime; used by Docker HEALTHCHECK).
pub fn healthcheck(url: &str) -> ExitCode {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return ExitCode::FAILURE,
    };
    let ok = rt.block_on(async {
        match reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    });
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
