//! oci-sync — sync local files or directories to OCI-compatible image registries.
//!
//! Rust rewrite of the original Go tool. Keeps the same feature set
//! (push / pull / delete / list / label / alias / recent / shortcuts / TUI)
//! with a redesigned CLI & TUI interaction layer.
//!
//! See `docs/` for architecture, feature specs, interaction design and the
//! implementation guide.

pub mod app;
pub mod archive;
pub mod cache;
pub mod cli;
pub mod config;
pub mod crypto;
pub mod oci;
pub mod output;
pub mod tui;
pub mod xdg;

/// Tool version (from Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tool version as a static string (used in manifest annotations and UA).
pub fn version() -> &'static str {
    VERSION
}
