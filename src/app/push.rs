//! `push` orchestration.
//!
//! Flow: archive::pack → optional crypto::encrypt → oci::push →
//! optional `--verify` (pull back + compare) → cache::add.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use tracing::info;

use crate::cache::{self, Activity, ActivityType};
use crate::cli::PushArgs;
use crate::config::Config;
use crate::crypto;
use crate::oci::{self, OciClient};
use crate::output::format_bytes;

pub async fn run(cfg: &Config, args: &PushArgs) -> Result<()> {
    run_ref(
        cfg,
        &args.remote,
        &args.local,
        args.passphrase.as_deref(),
        &args.labels,
        args.verify,
    )
    .await
}

/// Shared implementation (also used by shortcut push).
pub async fn run_ref(
    cfg: &Config,
    remote_ref: &str,
    local: &str,
    passphrase: Option<&str>,
    labels: &[String],
    verify: bool,
) -> Result<()> {
    let started = Instant::now();
    let parsed = oci::parse_ref(remote_ref).map_err(|e| anyhow!("{e}"))?;
    let tag = parsed.tag.clone().ok_or_else(|| {
        anyhow!("remote reference must include a tag, e.g. <registry>/<repo>:<tag>")
    })?;
    if parsed.repo.is_empty() {
        return Err(anyhow!("remote reference must include a repository path"));
    }

    let label_map = parse_labels(labels)?;

    info!(path = local, "Packing files...");
    let data = crate::archive::pack(Path::new(local))
        .with_context(|| format!("pack failed for {}", local))?;
    info!(
        size = format_bytes(data.len() as i64).as_str(),
        "Pack complete"
    );

    let encrypted = passphrase.is_some_and(|p| !p.is_empty());
    let data = if encrypted {
        info!("Encrypting...");
        let cipher =
            crypto::encrypt(&data, passphrase.unwrap_or_default()).context("encryption failed")?;
        info!(
            size = format_bytes(cipher.len() as i64).as_str(),
            "Encryption complete"
        );
        cipher
    } else {
        data
    };

    info!(ref = remote_ref, "Pushing to registry...");
    let client = OciClient::new(&parsed.host, cfg)?;
    client
        .push(&parsed.repo, &tag, &data, encrypted, &label_map)
        .await
        .with_context(|| format!("push failed for {remote_ref}"))?;

    if verify {
        info!("Verifying pushed artifact...");
        let result = client
            .pull(&parsed.repo, &tag)
            .await
            .with_context(|| format!("verify pull failed for {remote_ref}"))?;
        if result.data != data {
            return Err(anyhow!(
                "verify failed: pulled content digest does not match"
            ));
        }
        if result.encrypted != encrypted {
            return Err(anyhow!("verify failed: encryption flag mismatch"));
        }
        info!("Verification passed ✓");
    }

    info!(
        ref = remote_ref,
        seconds = format!("{:.1}", started.elapsed().as_secs_f64()).as_str(),
        "Push successful ✓"
    );

    let _ = cache::add(Activity {
        kind: ActivityType::Push,
        timestamp: chrono::Local::now(),
        remote_ref: remote_ref.to_string(),
        local_path: Some(local.to_string()),
        labels: labels.to_vec(),
        success: true,
        error: None,
    });

    Ok(())
}

/// Parse `key=value` label flags; entries without `=` are rejected with a
/// clear message.
pub fn parse_labels(labels: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    for l in labels {
        match l.split_once('=') {
            Some((k, v)) => {
                if k.is_empty() {
                    return Err(anyhow!("label key cannot be empty: {l:?}"));
                }
                map.insert(k.to_string(), v.to_string());
            }
            None => {
                return Err(anyhow!(
                    "invalid label {l:?}: expected format KEY=VALUE (value may be empty)"
                ));
            }
        }
    }
    Ok(map)
}
