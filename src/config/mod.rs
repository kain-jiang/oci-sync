//! YAML configuration loading and accessors.
//!
//! Search order:
//! 1. `./oci-sync.yaml` (current working directory)
//! 2. `~/.config/oci-sync/oci-sync.yaml` (XDG config dir)
//!
//! Auth precedence at the OCI layer: config `auths` > Docker credential store.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::xdg;

pub const CONFIG_FILE_NAME: &str = "oci-sync.yaml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Shortcut {
    pub repo: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub auths: HashMap<String, RegistryAuth>,
    #[serde(default)]
    pub shortcuts: HashMap<String, Shortcut>,
}

impl Config {
    /// Candidate config paths in search order.
    pub fn candidate_paths() -> Vec<PathBuf> {
        vec![
            PathBuf::from(".").join(CONFIG_FILE_NAME),
            xdg::config_dir().join("oci-sync").join(CONFIG_FILE_NAME),
        ]
    }

    /// First existing candidate path, if any.
    pub fn file_used() -> Option<PathBuf> {
        Self::candidate_paths().into_iter().find(|p| p.is_file())
    }

    /// Credentials for a registry host (exact host match).
    pub fn registry_auth(&self, host: &str) -> Option<&RegistryAuth> {
        self.auths.get(host)
    }

    /// Resolve the repository for a shortcut, validating that it carries no
    /// tag and no digest.
    pub fn shortcut_repo(&self, name: &str) -> Result<String> {
        let repo = &self
            .shortcuts
            .get(name)
            .ok_or_else(|| {
                anyhow!("shortcut {name:?} not found (add `shortcuts.{name}.repo` to config)")
            })?
            .repo;
        if repo.is_empty() {
            return Err(anyhow!("shortcut {name:?} has an empty repo"));
        }
        if repo.contains('@') {
            return Err(anyhow!(
                "shortcut {name:?} repository must not be a digest reference (contains '@')"
            ));
        }
        if let Some(c) = repo.rfind(':') {
            let last_slash = repo.rfind('/');
            if last_slash.is_none_or(|s| c > s) {
                return Err(anyhow!(
                    "shortcut {name:?} repository must not include a tag (found ':' after last '/')"
                ));
            }
        }
        Ok(repo.clone())
    }

    /// Build `<repo>:<tag>` for shortcut commands.
    pub fn shortcut_remote_ref(&self, name: &str, tag: &str) -> Result<String> {
        let tag = tag.trim();
        if tag.is_empty() {
            return Err(anyhow!("tag cannot be empty for shortcut {name:?}"));
        }
        Ok(format!("{}:{tag}", self.shortcut_repo(name)?))
    }

    /// All shortcuts (name + repo) sorted by name, for `alias list` and TUI.
    pub fn all_shortcuts(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .shortcuts
            .iter()
            .map(|(k, s)| (k.clone(), s.repo.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}

/// Load the first existing config file. A missing config is NOT an error.
pub fn load() -> Result<Config> {
    let Some(path) = Config::file_used() else {
        return Ok(Config::default());
    };
    load_from(&path)
}

pub fn load_from(path: &Path) -> Result<Config> {
    let data =
        std::fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    let cfg: Config =
        serde_yml::from_str(&data).with_context(|| format!("parse config {}", path.display()))?;
    Ok(cfg)
}

/// Persist the config to the given path, creating parent directories.
pub fn save_to(cfg: &Config, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    let data = serde_yml::to_string(cfg).context("serialize config")?;
    std::fs::write(path, data).with_context(|| format!("write config {}", path.display()))?;
    Ok(())
}

/// Persist to the default user location `~/.config/oci-sync/oci-sync.yaml`.
pub fn save_user(cfg: &Config) -> Result<PathBuf> {
    let path = xdg::config_dir().join("oci-sync").join(CONFIG_FILE_NAME);
    save_to(cfg, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let yaml = r#"
shortcuts:
  x:
    repo: registry.example.com/myteam/files
auths:
  registry.example.com:
    username: myuser
    password: mytoken
"#;
        let cfg: Config = serde_yml::from_str(yaml).unwrap();
        assert_eq!(cfg.shortcuts["x"].repo, "registry.example.com/myteam/files");
        assert_eq!(cfg.auths["registry.example.com"].username, "myuser");
    }

    #[test]
    fn empty_config_defaults() {
        let cfg = Config::default();
        assert!(cfg.auths.is_empty());
        assert!(cfg.shortcuts.is_empty());
        assert!(cfg.registry_auth("x").is_none());
    }

    #[test]
    fn shortcut_repo_rejects_tag() {
        let cfg = Config {
            shortcuts: HashMap::from([(
                "x".into(),
                Shortcut {
                    repo: "reg/repo:tag".into(),
                },
            )]),
            ..Default::default()
        };
        assert!(cfg.shortcut_repo("x").is_err());
    }

    #[test]
    fn shortcut_repo_rejects_digest() {
        let cfg = Config {
            shortcuts: HashMap::from([(
                "x".into(),
                Shortcut {
                    repo: "reg/repo@sha256:abc".into(),
                },
            )]),
            ..Default::default()
        };
        assert!(cfg.shortcut_repo("x").is_err());
    }

    #[test]
    fn shortcut_repo_accepts_plain() {
        let cfg = Config {
            shortcuts: HashMap::from([(
                "x".into(),
                Shortcut {
                    repo: "reg/repo".into(),
                },
            )]),
            ..Default::default()
        };
        assert_eq!(cfg.shortcut_repo("x").unwrap(), "reg/repo");
        assert_eq!(cfg.shortcut_remote_ref("x", "v1").unwrap(), "reg/repo:v1");
    }

    #[test]
    fn shortcut_repo_port_in_host_ok() {
        // host:port/repo must NOT be treated as a tag
        let cfg = Config {
            shortcuts: HashMap::from([(
                "x".into(),
                Shortcut {
                    repo: "localhost:5000/repo".into(),
                },
            )]),
            ..Default::default()
        };
        assert_eq!(cfg.shortcut_repo("x").unwrap(), "localhost:5000/repo");
    }

    #[test]
    fn missing_shortcut_errors() {
        let cfg = Config::default();
        assert!(cfg.shortcut_repo("nope").is_err());
    }

    #[test]
    fn config_search_prefers_cwd_over_user_dir() {
        use std::sync::Mutex;
        // Serializes with other config tests: this test changes the process
        // working directory and XDG_CONFIG_HOME.
        static LOCK: Mutex<()> = Mutex::new(());

        let _guard = LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("oci-sync-config-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("xdg/oci-sync")).unwrap();

        // cwd config: shortcut "cwdsc"
        std::fs::write(
            dir.join("oci-sync.yaml"),
            "shortcuts:\n  cwdsc:\n    repo: reg/cwd-repo\n",
        )
        .unwrap();
        // user config (XDG): shortcut "usersc"
        std::fs::write(
            dir.join("xdg/oci-sync/oci-sync.yaml"),
            "shortcuts:\n  usersc:\n    repo: reg/user-repo\n",
        )
        .unwrap();

        let prev_dir = std::env::current_dir().unwrap();
        // SAFETY: guarded by LOCK; no other test relies on cwd concurrently.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.join("xdg")) };
        std::env::set_current_dir(&dir).unwrap();

        let cfg = load().unwrap();

        std::env::set_current_dir(prev_dir).unwrap();

        assert!(cfg.shortcuts.contains_key("cwdsc"), "cwd config must win");
        assert!(
            !cfg.shortcuts.contains_key("usersc"),
            "user config must be ignored when cwd config exists"
        );
        assert_eq!(cfg.shortcut_repo("cwdsc").unwrap(), "reg/cwd-repo");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
