//! `list` orchestration.
//!
//! Distinguishes repository-level (host/repo) from registry-level (bare host)
//! listing. Filtering by label/tag happens in the output layer.

use anyhow::{Result, anyhow};

use crate::cli::ListArgs;
use crate::config::Config;
use crate::oci::{self, ArtifactInfo, OciClient};

pub async fn run(cfg: &Config, args: &ListArgs) -> Result<Vec<ArtifactInfo>> {
    run_ref(cfg, &args.remote).await
}

/// List one repository (used by shortcut list).
pub async fn run_repo(cfg: &Config, repo: &str) -> Result<Vec<ArtifactInfo>> {
    run_ref(cfg, repo).await
}

async fn run_ref(cfg: &Config, remote: &str) -> Result<Vec<ArtifactInfo>> {
    let parsed = oci::parse_ref(remote).map_err(|e| anyhow!("{e}"))?;
    let client = OciClient::new(&parsed.host, cfg)?;

    if parsed.repo.is_empty() {
        // Bare registry: scan the catalog.
        client.list_registry().await
    } else {
        client.list_repo(&parsed.repo).await
    }
}
