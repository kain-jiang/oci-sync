//! CLI entry: argument parsing, logging setup, command dispatch.

mod args;
mod dispatch;
mod logging;

pub use args::*;

use anyhow::Result;
use clap::Parser;

/// Parse args, initialize logging/config and dispatch to the app layer.
pub async fn run() -> Result<()> {
    let cli = args::Cli::parse();
    logging::init(cli.quiet);
    dispatch::dispatch(cli).await
}
