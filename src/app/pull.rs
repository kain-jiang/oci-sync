//! `pull` orchestration.
//!
//! Flow: oci::is_encrypted (fail fast when passphrase missing) → oci::pull →
//! optional crypto::decrypt → archive::unpack → cache::add.
//! `--force` controls overwriting of existing destination files.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use tracing::{info, warn};

use crate::cache::{self, Activity, ActivityType};
use crate::cli::PullArgs;
use crate::config::Config;
use crate::crypto;
use crate::oci::{self, OciClient};
use crate::output::format_bytes;

pub async fn run(cfg: &Config, args: &PullArgs) -> Result<()> {
    run_ref(
        cfg,
        &args.remote,
        &args.local,
        args.passphrase.as_deref(),
        args.force,
    )
    .await
}

/// Shared implementation (also used by shortcut pull).
pub async fn run_ref(
    cfg: &Config,
    remote_ref: &str,
    local: &str,
    passphrase: Option<&str>,
    force: bool,
) -> Result<()> {
    let parsed = oci::parse_ref(remote_ref).map_err(|e| anyhow!("{e}"))?;
    let tag = parsed.tag.clone().ok_or_else(|| {
        anyhow!("remote reference must include a tag, e.g. <registry>/<repo>:<tag>")
    })?;
    if parsed.repo.is_empty() {
        return Err(anyhow!("remote reference must include a repository path"));
    }

    let client = OciClient::new(&parsed.host, cfg)?;

    // Fail fast on encrypted content without a passphrase (no data download).
    info!(ref = remote_ref, "Checking encryption status...");
    let encrypted = client
        .is_encrypted(&parsed.repo, &tag)
        .await
        .with_context(|| format!("failed to check encryption status for {remote_ref}"))?;

    let has_passphrase = passphrase.is_some_and(|p| !p.is_empty());
    if encrypted && !has_passphrase {
        return Err(anyhow!(
            "content is encrypted, provide a decryption passphrase via --passphrase"
        ));
    }
    if !encrypted && has_passphrase {
        warn!("content is not encrypted, ignoring --passphrase flag");
    }

    info!(ref = remote_ref, "Pulling from registry...");
    let result = client
        .pull(&parsed.repo, &tag)
        .await
        .with_context(|| format!("pull failed for {remote_ref}"))?;
    info!(
        size = format_bytes(result.data.len() as i64).as_str(),
        encrypted = result.encrypted,
        "Pull complete"
    );

    let data = if result.encrypted {
        info!("Decrypting...");
        let plain = crypto::decrypt(&result.data, passphrase.unwrap_or_default())
            .context("decryption failed")?;
        info!("Decryption complete");
        plain
    } else {
        result.data
    };

    // Destination handling: create if missing; refuse to overwrite existing
    // content unless --force is given.
    let dest = Path::new(local);
    if dest.exists() && !force {
        return Err(anyhow!(
            "destination {} already exists (use --force to overwrite)",
            dest.display()
        ));
    }

    info!(dest = local, "Unpacking files...");
    crate::archive::unpack(&data, dest).with_context(|| format!("unpack failed to {}", local))?;

    info!(dest = local, "Pull successful ✓");

    let _ = cache::add(Activity {
        kind: ActivityType::Pull,
        timestamp: chrono::Local::now(),
        remote_ref: remote_ref.to_string(),
        local_path: Some(local.to_string()),
        labels: vec![],
        success: true,
        error: None,
    });

    Ok(())
}
