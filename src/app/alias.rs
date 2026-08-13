//! `alias` management: read-modify-write of the config file.

use anyhow::{Result, anyhow};
use tracing::info;

use crate::cli::{AliasAddArgs, AliasRemoveArgs};
use crate::config::{self, Config, Shortcut};
use crate::oci;

pub fn add(cfg: &Config, args: &AliasAddArgs) -> Result<()> {
    // Validate the repo reference up front: must be <registry>/<repository>
    // without a tag (mirrors config::shortcut_repo validation at add time).
    let parsed = oci::parse_ref(&args.repo).map_err(|e| anyhow!("invalid repo: {e}"))?;
    if parsed.repo.is_empty() {
        return Err(anyhow!(
            "repo must include a repository path, e.g. registry.example.com/myteam/files"
        ));
    }
    if parsed.tag.is_some() {
        return Err(anyhow!("repo must not include a tag (got {})", args.repo));
    }

    let mut new_cfg = cfg.clone();
    new_cfg.shortcuts.insert(
        args.name.clone(),
        Shortcut {
            repo: args.repo.clone(),
        },
    );

    // Always persist to the user config file so the change is visible to
    // every invocation (cwd config wins on load, but falls back to user file).
    let path = config::save_user(&new_cfg)?;
    info!(
        name = args.name,
        repo = args.repo,
        path = path.display().to_string().as_str(),
        "Shortcut added ✓"
    );
    Ok(())
}

pub fn remove(cfg: &Config, args: &AliasRemoveArgs) -> Result<()> {
    if !cfg.shortcuts.contains_key(&args.name) {
        return Err(anyhow!(
            "shortcut {:?} not found (run `oci-sync alias list` to see configured shortcuts)",
            args.name
        ));
    }

    let mut new_cfg = cfg.clone();
    new_cfg.shortcuts.remove(&args.name);

    let path = config::save_user(&new_cfg)?;
    info!(
        name = args.name,
        path = path.display().to_string().as_str(),
        "Shortcut removed ✓"
    );
    Ok(())
}
