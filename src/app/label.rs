//! `label set/unset` orchestration → oci::update_annotations.

use anyhow::{Result, anyhow};
use tracing::info;

use crate::cache::{self, Activity, ActivityType};
use crate::cli::{LabelSetArgs, LabelUnsetArgs};
use crate::config::Config;
use crate::oci::{self, OciClient};

pub async fn set(cfg: &Config, args: &LabelSetArgs) -> Result<()> {
    let (client, repo, tag) = prepare(cfg, &args.remote)?;

    let mut updates = std::collections::HashMap::new();
    for l in &args.labels {
        match l.split_once('=') {
            Some((k, v)) => {
                if k.is_empty() {
                    return Err(anyhow!("label key cannot be empty: {l:?}"));
                }
                updates.insert(k.to_string(), v.to_string());
            }
            None => {
                return Err(anyhow!(
                    "invalid label {l:?}: expected format KEY=VALUE (value may be empty)"
                ));
            }
        }
    }

    info!(ref = args.remote, labels = ?updates.keys().collect::<Vec<_>>(), "Setting labels...");
    client
        .update_annotations(&repo, &tag, &updates, &[])
        .await
        .map_err(|e| anyhow!("label set failed: {e:#}"))?;
    info!(ref = args.remote, "Labels updated ✓");

    record(args.remote.clone(), args.labels.clone());
    Ok(())
}

pub async fn unset(cfg: &Config, args: &LabelUnsetArgs) -> Result<()> {
    let (client, repo, tag) = prepare(cfg, &args.remote)?;

    info!(ref = args.remote, keys = ?args.keys, "Removing labels...");
    client
        .update_annotations(&repo, &tag, &std::collections::HashMap::new(), &args.keys)
        .await
        .map_err(|e| anyhow!("label unset failed: {e:#}"))?;
    info!(ref = args.remote, "Labels removed ✓");

    record(args.remote.clone(), args.keys.clone());
    Ok(())
}

fn prepare(cfg: &Config, remote_ref: &str) -> Result<(OciClient, String, String)> {
    let parsed = oci::parse_ref(remote_ref).map_err(|e| anyhow!("{e}"))?;
    let tag = parsed.tag.clone().ok_or_else(|| {
        anyhow!("remote reference must include a tag, e.g. <registry>/<repo>:<tag>")
    })?;
    if parsed.repo.is_empty() {
        return Err(anyhow!("remote reference must include a repository path"));
    }
    let client = OciClient::new(&parsed.host, cfg)?;
    Ok((client, parsed.repo, tag))
}

fn record(remote_ref: String, labels: Vec<String>) {
    let _ = cache::add(Activity {
        kind: ActivityType::Label,
        timestamp: chrono::Local::now(),
        remote_ref,
        local_path: None,
        labels,
        success: true,
        error: None,
    });
}
