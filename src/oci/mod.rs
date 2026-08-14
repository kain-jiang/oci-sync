//! OCI registry interaction — the only module allowed to talk to registries.
//!
//! Backed by `oci-distribution` for push/pull/tags, plus a small raw-HTTP
//! layer (via `reqwest`) for the registry catalog endpoint (`/v2/_catalog`)
//! and manifest deletion, which `oci-distribution` does not expose.
//!
//! Manifest contract (compatible with the Go version):
//! - OCI image manifest, schemaVersion 2
//! - config: `{}` with mediaType `application/vnd.oci.image.config.v1+json`
//! - layer: the (possibly encrypted) payload, mediaType `application/octet-stream`
//! - annotations: `io.oci-sync.encrypted` ("true"/"false"), `io.oci-sync.version`,
//!   plus user labels (any key not prefixed `io.oci-sync.`)
//!
//! Auth precedence: config `auths.<host>` > Docker credential store.

pub mod client;

pub use client::OciClient;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const ANNOTATION_ENCRYPTED: &str = "io.oci-sync.encrypted";
pub const ANNOTATION_VERSION: &str = "io.oci-sync.version";
pub const ANNOTATION_PREFIX: &str = "io.oci-sync.";
pub const MEDIA_TYPE_LAYER: &str = "application/octet-stream";
pub const MEDIA_TYPE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// Metadata about one artifact discovered by `list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    /// `<registry>/<repo>:<tag>`
    pub full_name: String,
    pub repo: String,
    pub tag: String,
    pub digest: String,
    pub encrypted: bool,
    /// oci-sync version that pushed the artifact
    pub version: String,
    pub size: i64,
    /// user labels (annotation keys not prefixed `io.oci-sync.`)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
}

/// Payload + encryption flag returned by `pull`.
#[derive(Debug, Clone)]
pub struct PullResult {
    pub data: Vec<u8>,
    pub encrypted: bool,
}

/// Result of parsing a remote reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRef {
    /// Registry host (may include port).
    pub host: String,
    /// Repository path; empty when the reference is a bare registry.
    pub repo: String,
    /// Tag; None when absent.
    pub tag: Option<String>,
}

/// Parse a remote reference: `<registry>/<repository>[:<tag>]` or a bare
/// `<registry>`.
///
/// Tag detection follows the OCI convention: the last `:` is a tag separator
/// only when a `/` exists in the reference AND no `/` appears after it. A bare
/// registry (`localhost:5000`) is never tag-split, so host ports survive.
pub fn parse_ref(input: &str) -> Result<ParsedRef, client::OciError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(client::OciError::InvalidReference {
            ref_str: input.to_string(),
            reason: "reference is empty".to_string(),
        });
    }

    let (body, tag) = if s.contains('/') {
        match s.rfind(':') {
            Some(idx) if !s[idx + 1..].contains('/') => {
                let t = &s[idx + 1..];
                if t.is_empty() {
                    return Err(client::OciError::InvalidReference {
                        ref_str: input.to_string(),
                        reason: "tag is empty (trailing ':')".to_string(),
                    });
                }
                (&s[..idx], Some(t.to_string()))
            }
            _ => (s, None),
        }
    } else {
        (s, None)
    };

    match body.find('/') {
        Some(idx) => {
            let host = &body[..idx];
            let repo = &body[idx + 1..];
            if host.is_empty() {
                return Err(client::OciError::InvalidReference {
                    ref_str: input.to_string(),
                    reason: "registry host is empty".to_string(),
                });
            }
            if repo.is_empty() {
                return Err(client::OciError::InvalidReference {
                    ref_str: input.to_string(),
                    reason: "repository is empty".to_string(),
                });
            }
            Ok(ParsedRef {
                host: host.to_string(),
                repo: repo.to_string(),
                tag,
            })
        }
        None => Ok(ParsedRef {
            host: body.to_string(),
            repo: String::new(),
            tag,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_with_tag() {
        let r = parse_ref("registry.example.com/myteam/files:v1").unwrap();
        assert_eq!(r.host, "registry.example.com");
        assert_eq!(r.repo, "myteam/files");
        assert_eq!(r.tag.as_deref(), Some("v1"));
    }

    #[test]
    fn parses_repo_without_tag() {
        let r = parse_ref("registry.example.com/myteam/files").unwrap();
        assert_eq!(r.host, "registry.example.com");
        assert_eq!(r.repo, "myteam/files");
        assert_eq!(r.tag, None);
    }

    #[test]
    fn parses_host_with_port() {
        let r = parse_ref("localhost:5000/myrepo:v2").unwrap();
        assert_eq!(r.host, "localhost:5000");
        assert_eq!(r.repo, "myrepo");
        assert_eq!(r.tag.as_deref(), Some("v2"));
    }

    #[test]
    fn parses_bare_registry() {
        let r = parse_ref("registry.example.com").unwrap();
        assert_eq!(r.host, "registry.example.com");
        assert!(r.repo.is_empty());
        assert_eq!(r.tag, None);
    }

    #[test]
    fn rejects_empty_and_bad_refs() {
        assert!(parse_ref("").is_err());
        assert!(parse_ref("reg/repo:").is_err());
        assert!(parse_ref("/repo").is_err());
        assert!(parse_ref("reg/").is_err());
    }
}
