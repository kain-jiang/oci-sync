//! `OciClient`: one instance per registry host, resolving credentials once.
//!
//! Uses `oci-distribution` for push/pull/tags/manifest operations and a raw
//! `reqwest` layer for the catalog endpoint and manifest deletion (not
//! exposed by `oci-distribution`). The raw layer supports Basic auth and the
//! standard Bearer token flow (`WWW-Authenticate` challenge → token endpoint).

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use oci_distribution::Reference;
use oci_distribution::client::{
    ClientConfig, ClientProtocol, Config as OciImageConfig, ImageLayer,
};
use oci_distribution::manifest::{OciImageManifest, OciManifest};
use oci_distribution::secrets::RegistryAuth;
use thiserror::Error;

use crate::config::Config;
use crate::oci::{
    ANNOTATION_ENCRYPTED, ANNOTATION_PREFIX, ANNOTATION_VERSION, ArtifactInfo, MEDIA_TYPE_LAYER,
    PullResult,
};

/// Errors produced by the OCI layer.
#[derive(Debug, Error)]
pub enum OciError {
    #[error("invalid reference {ref_str:?}: {reason}")]
    InvalidReference { ref_str: String, reason: String },

    #[error("registry error: {0}")]
    Registry(#[from] oci_distribution::errors::OciDistributionError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("not an oci-sync artifact (missing {ANNOTATION_VERSION} annotation)")]
    NotOciSyncArtifact,
}

/// Async OCI client bound to one registry host.
#[derive(Clone)]
pub struct OciClient {
    host: String,
    client: oci_distribution::Client,
    auth: RegistryAuth,
    http: reqwest::Client,
    scheme: &'static str,
}

impl OciClient {
    /// Create a client for `host`, resolving credentials with the precedence:
    /// config `auths` first, then the Docker credential store, then anonymous.
    pub fn new(host: &str, cfg: &Config) -> Result<Self> {
        let auth = resolve_auth(host, cfg)?;

        let scheme = if host.starts_with("localhost")
            || host.starts_with("127.0.0.1")
            || host.starts_with("[::1]")
            || host.starts_with("0.0.0.0")
        {
            "http"
        } else {
            "https"
        };

        let client = oci_distribution::Client::new(ClientConfig {
            protocol: if scheme == "http" {
                ClientProtocol::Http
            } else {
                ClientProtocol::Https
            },
            ..Default::default()
        });

        let http = reqwest::Client::builder()
            .user_agent(format!("oci-sync/{}", crate::version()))
            .build()
            .context("build http client")?;

        Ok(Self {
            host: host.to_string(),
            client,
            auth,
            http,
            scheme,
        })
    }

    fn reference(&self, repo: &str, tag: &str) -> Reference {
        Reference::with_tag(self.host.clone(), repo.to_string(), tag.to_string())
    }

    fn repo_ref(&self, repo: &str) -> Reference {
        Reference::with_tag(self.host.clone(), repo.to_string(), "latest".to_string())
    }

    /// Push a payload as a new artifact under `<repo>:<tag>`.
    pub async fn push(
        &self,
        repo: &str,
        tag: &str,
        data: &[u8],
        encrypted: bool,
        labels: &HashMap<String, String>,
    ) -> Result<()> {
        let reference = self.reference(repo, tag);

        let layer = ImageLayer::new(data.to_vec(), MEDIA_TYPE_LAYER.to_string(), None);
        let config = OciImageConfig::oci_v1(b"{}".to_vec(), None);

        let mut annotations = HashMap::new();
        annotations.insert(ANNOTATION_VERSION.to_string(), crate::version().to_string());
        annotations.insert(
            ANNOTATION_ENCRYPTED.to_string(),
            if encrypted { "true" } else { "false" }.to_string(),
        );
        for (k, v) in labels {
            annotations.insert(k.clone(), v.clone());
        }

        let manifest =
            OciImageManifest::build(std::slice::from_ref(&layer), &config, Some(annotations));

        self.client
            .push(&reference, &[layer], config, &self.auth, Some(manifest))
            .await
            .map_err(OciError::Registry)?;

        Ok(())
    }

    /// Read only the manifest and report the encryption flag.
    pub async fn is_encrypted(&self, repo: &str, tag: &str) -> Result<bool> {
        let reference = self.reference(repo, tag);
        let (manifest, _digest) = self
            .client
            .pull_manifest(&reference, &self.auth)
            .await
            .map_err(OciError::Registry)?;
        Ok(annotations_of(&manifest)
            .and_then(|a| a.get(ANNOTATION_ENCRYPTED))
            .is_some_and(|v| v == "true"))
    }

    /// Fetch the layer payload of an artifact.
    pub async fn pull(&self, repo: &str, tag: &str) -> Result<PullResult> {
        let reference = self.reference(repo, tag);
        let accepted = vec![
            MEDIA_TYPE_LAYER,
            "application/vnd.oci.image.layer.v1.tar",
            "application/vnd.oci.image.layer.v1.tar+gzip",
        ];
        let image = self
            .client
            .pull(&reference, &self.auth, accepted)
            .await
            .map_err(OciError::Registry)?;

        let Some(layer) = image.layers.into_iter().next() else {
            return Err(anyhow!("artifact has no layers"));
        };

        let encrypted = image
            .manifest
            .as_ref()
            .and_then(|m| m.annotations.as_ref())
            .is_some_and(|a| a.get(ANNOTATION_ENCRYPTED).is_some_and(|v| v == "true"));

        Ok(PullResult {
            data: layer.data,
            encrypted,
        })
    }

    /// Resolve the digest of the manifest referenced by `<repo>:<tag>`.
    pub async fn manifest_digest(&self, repo: &str, tag: &str) -> Result<String> {
        let reference = self.reference(repo, tag);
        let digest = self
            .client
            .fetch_manifest_digest(&reference, &self.auth)
            .await
            .map_err(OciError::Registry)?;
        Ok(digest)
    }

    /// Delete an artifact by tag (raw HTTP DELETE with auth).
    pub async fn delete(&self, repo: &str, tag: &str) -> Result<()> {
        let reference = self.reference(repo, tag);
        let digest = self
            .client
            .fetch_manifest_digest(&reference, &self.auth)
            .await
            .map_err(OciError::Registry)?;

        let url = format!(
            "{scheme}://{host}/v2/{repo}/manifests/{digest}",
            scheme = self.scheme,
            host = self.host
        );
        let scope = format!("repository:{repo}:pull,push");
        self.raw_delete(&url, &scope).await
    }

    /// List oci-sync artifacts in one repository (all tags).
    pub async fn list_repo(&self, repo: &str) -> Result<Vec<ArtifactInfo>> {
        let reference = self.repo_ref(repo);
        let resp = self
            .client
            .list_tags(&reference, &self.auth, None, None)
            .await
            .map_err(OciError::Registry)?;

        let mut results = Vec::new();
        for tag in resp.tags {
            let reference = self.reference(repo, &tag);
            let Ok((manifest, digest)) = self.client.pull_manifest(&reference, &self.auth).await
            else {
                continue;
            };
            let Some(info) = artifact_info(&manifest, &digest, &self.host, repo, &tag) else {
                continue;
            };
            results.push(info);
        }
        Ok(results)
    }

    /// List oci-sync artifacts across the whole registry (catalog endpoint).
    pub async fn list_registry(&self) -> Result<Vec<ArtifactInfo>> {
        let mut results = Vec::new();
        let mut last: Option<String> = None;

        loop {
            let url = match &last {
                Some(l) => format!(
                    "{scheme}://{host}/v2/_catalog?n=100&last={l}",
                    scheme = self.scheme,
                    host = self.host
                ),
                None => format!(
                    "{scheme}://{host}/v2/_catalog?n=100",
                    scheme = self.scheme,
                    host = self.host
                ),
            };
            let body = self.raw_get(&url, "registry:catalog:*").await?;
            let json: serde_json::Value =
                serde_json::from_str(&body).context("parse catalog response")?;
            let repos = json["repositories"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            for repo in &repos {
                let Ok(infos) = self.list_repo(repo).await else {
                    continue;
                };
                results.extend(infos);
            }

            if repos.len() < 100 {
                break;
            }
            last = repos.last().cloned();
        }

        Ok(results)
    }

    /// Set/remove manifest annotations and re-point the tag at the new
    /// manifest (used by `label set/unset`).
    pub async fn update_annotations(
        &self,
        repo: &str,
        tag: &str,
        updates: &HashMap<String, String>,
        removes: &[String],
    ) -> Result<()> {
        let reference = self.reference(repo, tag);
        let (manifest, _digest) = self
            .client
            .pull_manifest(&reference, &self.auth)
            .await
            .map_err(OciError::Registry)?;

        let OciManifest::Image(mut image_manifest) = manifest else {
            return Err(anyhow!("cannot update annotations on an image index"));
        };

        let annotations = image_manifest.annotations.get_or_insert_with(HashMap::new);
        for (k, v) in updates {
            annotations.insert(k.clone(), v.clone());
        }
        for k in removes {
            annotations.remove(k);
        }

        self.client
            .push_manifest(&reference, &OciManifest::Image(image_manifest))
            .await
            .map_err(OciError::Registry)?;
        Ok(())
    }

    // ------------------------------------------------------------ raw http

    /// GET with auth, returning the response body on 2xx.
    async fn raw_get(&self, url: &str, scope: &str) -> Result<String> {
        let mut req = self.http.get(url);
        if let Some(header) = self.auth_header() {
            req = req.header(reqwest::header::AUTHORIZATION, header);
        }
        let resp = req.send().await.map_err(OciError::Http)?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Bearer token challenge?
            let challenge = resp
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if let Some(token) = self.try_bearer_challenge(&challenge, scope).await? {
                let resp = self
                    .http
                    .get(url)
                    .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await
                    .map_err(OciError::Http)?;
                return resp
                    .error_for_status()
                    .map_err(OciError::Http)?
                    .text()
                    .await
                    .map_err(Into::into);
            }
            return Err(anyhow!(
                "authentication required for {} (docker login {} or set auths.{} in config)",
                self.host,
                self.host,
                self.host
            ));
        }

        resp.error_for_status()
            .map_err(OciError::Http)?
            .text()
            .await
            .map_err(Into::into)
    }

    /// DELETE with auth (Basic or Bearer token flow).
    async fn raw_delete(&self, url: &str, scope: &str) -> Result<()> {
        let mut req = self.http.delete(url);
        if let Some(header) = self.auth_header() {
            req = req.header(reqwest::header::AUTHORIZATION, header);
        }
        let resp = req.send().await.map_err(OciError::Http)?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let challenge = resp
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if let Some(token) = self.try_bearer_challenge(&challenge, scope).await? {
                let resp = self
                    .http
                    .delete(url)
                    .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await
                    .map_err(OciError::Http)?;
                resp.error_for_status().map_err(OciError::Http)?;
                return Ok(());
            }
            return Err(anyhow!(
                "authentication required for {} (docker login {} or set auths.{} in config)",
                self.host,
                self.host,
                self.host
            ));
        }

        resp.error_for_status().map_err(OciError::Http)?;
        Ok(())
    }

    fn auth_header(&self) -> Option<String> {
        match &self.auth {
            RegistryAuth::Anonymous => None,
            RegistryAuth::Basic(user, pass) => {
                let encoded =
                    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
                Some(format!("Basic {encoded}"))
            }
        }
    }

    /// Handle a `WWW-Authenticate: Bearer realm=...,service=...` challenge:
    /// fetch a token and return it. Returns None when the challenge is not a
    /// Bearer challenge or no token could be obtained.
    async fn try_bearer_challenge(
        &self,
        challenge: &Option<String>,
        scope: &str,
    ) -> Result<Option<String>> {
        let Some(challenge) = challenge else {
            return Ok(None);
        };
        if !challenge.trim_start().starts_with("Bearer") {
            return Ok(None);
        }

        let realm = parse_challenge_param(challenge, "realm");
        let service = parse_challenge_param(challenge, "service");
        let Some(realm) = realm else {
            return Ok(None);
        };

        let mut url = format!("{realm}?scope={scope}");
        if let Some(service) = service {
            url.push_str(&format!("&service={service}"));
        }

        let resp = self.http.get(&url).send().await.map_err(OciError::Http)?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let json: serde_json::Value = resp.json().await.map_err(OciError::Http)?;
        let token = json["token"]
            .as_str()
            .or_else(|| json["access_token"].as_str())
            .map(|s| s.to_string());
        Ok(token)
    }
}

/// Resolve credentials for a host: config `auths` → Docker credential store
/// → anonymous.
fn resolve_auth(host: &str, cfg: &Config) -> Result<RegistryAuth> {
    if let Some(auth) = cfg.registry_auth(host) {
        if !auth.username.is_empty() || !auth.password.is_empty() {
            return Ok(RegistryAuth::Basic(
                auth.username.clone(),
                auth.password.clone(),
            ));
        }
    }

    use docker_credential::CredentialRetrievalError;
    match docker_credential::get_credential(host) {
        Ok(docker_credential::DockerCredential::UsernamePassword(u, p)) => {
            Ok(RegistryAuth::Basic(u, p))
        }
        Ok(_) => Ok(RegistryAuth::Anonymous),
        Err(
            CredentialRetrievalError::NoCredentialConfigured
            | CredentialRetrievalError::ConfigNotFound
            | CredentialRetrievalError::ConfigReadError,
        ) => Ok(RegistryAuth::Anonymous),
        Err(e) => Err(anyhow!("docker credential store error: {e}")),
    }
}

/// Extract `key="value"` (or `key=value`) from a challenge header.
fn parse_challenge_param(challenge: &str, key: &str) -> Option<String> {
    let key_eq = format!("{key}=");
    let idx = challenge.find(&key_eq)?;
    let rest = &challenge[idx + key_eq.len()..];
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest.find(',').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn annotations_of(manifest: &OciManifest) -> Option<&HashMap<String, String>> {
    match manifest {
        OciManifest::Image(m) => m.annotations.as_ref(),
        OciManifest::ImageIndex(_) => None,
    }
}

/// Build `ArtifactInfo` from a manifest, or None when the manifest carries no
/// `io.oci-sync.version` annotation (i.e. not an oci-sync artifact).
fn artifact_info(
    manifest: &OciManifest,
    digest: &str,
    host: &str,
    repo: &str,
    tag: &str,
) -> Option<ArtifactInfo> {
    let OciManifest::Image(m) = manifest else {
        return None;
    };
    let annotations = m.annotations.as_ref()?;
    let version = annotations.get(ANNOTATION_VERSION)?;

    let size = m.layers.first().map(|l| l.size).unwrap_or(0);
    let encrypted = annotations
        .get(ANNOTATION_ENCRYPTED)
        .is_some_and(|v| v == "true");

    let labels = annotations
        .iter()
        .filter(|(k, _)| !k.starts_with(ANNOTATION_PREFIX))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<HashMap<_, _>>();

    Some(ArtifactInfo {
        full_name: format!("{host}/{repo}:{tag}"),
        repo: repo.to_string(),
        tag: tag.to_string(),
        digest: digest.to_string(),
        encrypted,
        version: version.clone(),
        size,
        labels,
    })
}
