//! XDG base directory resolution (via the `dirs` crate).
//!
//! Honors `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` / `XDG_DATA_HOME` and falls
//! back to the platform defaults.

use std::path::PathBuf;

/// `$XDG_CONFIG_HOME` or `~/.config`
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"))
}

/// `$XDG_CACHE_HOME` or `~/.cache`
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from("~/.cache"))
}

/// `$XDG_DATA_HOME` or `~/.local/share`
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"))
}
