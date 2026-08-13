//! `delete` orchestration — the only destructive remote operation.
//!
//! Interaction design: always ask for confirmation on a TTY unless `--yes`.

use anyhow::{Result, anyhow};
use tracing::info;

use crate::cache::{self, Activity, ActivityType};
use crate::cli::DeleteArgs;
use crate::config::Config;
use crate::oci::{self, OciClient};

pub async fn run(cfg: &Config, args: &DeleteArgs) -> Result<()> {
    run_ref(cfg, &args.remote, args.yes).await
}

/// Shared implementation (also used by shortcut delete).
pub async fn run_ref(cfg: &Config, remote_ref: &str, yes: bool) -> Result<()> {
    let parsed = oci::parse_ref(remote_ref).map_err(|e| anyhow!("{e}"))?;
    let tag = parsed.tag.clone().ok_or_else(|| {
        anyhow!("remote reference must include a tag, e.g. <registry>/<repo>:<tag>")
    })?;
    if parsed.repo.is_empty() {
        return Err(anyhow!("remote reference must include a repository path"));
    }

    let client = OciClient::new(&parsed.host, cfg)?;

    // Resolve the digest so the user sees exactly what will be deleted.
    let digest = match client.manifest_digest(&parsed.repo, &tag).await {
        Ok(d) => d,
        Err(e) => {
            // Do not block deletion on digest resolution; surface it and ask.
            eprintln!("warning: could not resolve digest: {e:#}");
            "unknown".to_string()
        }
    };

    if !yes {
        crate::output::confirm(&format!("Will delete: {remote_ref} ({digest})\nContinue?"))?;
    }

    info!(ref = remote_ref, "Deleting artifact...");
    client
        .delete(&parsed.repo, &tag)
        .await
        .with_context_delete(remote_ref)?;

    info!(ref = remote_ref, "Delete successful ✓");

    let _ = cache::add(Activity {
        kind: ActivityType::Delete,
        timestamp: chrono::Local::now(),
        remote_ref: remote_ref.to_string(),
        local_path: None,
        labels: vec![],
        success: true,
        error: None,
    });

    Ok(())
}

trait DeleteCtx {
    fn with_context_delete(self, ref_str: &str) -> Result<()>;
}

impl DeleteCtx for Result<()> {
    fn with_context_delete(self, ref_str: &str) -> Result<()> {
        self.map_err(|e| anyhow!("delete failed for {ref_str}: {e:#}"))
    }
}
