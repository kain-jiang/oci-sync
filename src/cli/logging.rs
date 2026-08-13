//! Logging setup (tracing-subscriber).
//!
//! - Default level: INFO
//! - `--quiet`: ERROR only
//! - `RUST_LOG` env var overrides everything
//!
//! All log messages in this codebase MUST be in English (project convention).

use tracing_subscriber::EnvFilter;

pub fn init(quiet: bool) {
    let default_level = if quiet { "error" } else { "info" };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
