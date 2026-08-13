//! Terminal output: human tables (tabled), JSON/YAML serialization,
//! label filtering, confirmation prompts and byte formatting.
//!
//! - `table` is the default human format; `json`/`yaml` are machine formats
//!   and contain the FULL field set (no truncation)
//! - confirmation prompts only appear when stdin is a TTY; `--yes` skips them

use std::io::IsTerminal;

use anyhow::{Result, bail};

use crate::cli::OutputFormat;
use crate::oci::ArtifactInfo;

/// Filter artifacts by label rules: `key=value` exact match, bare `key` = presence.
pub fn filter_by_labels(artifacts: &[ArtifactInfo], rules: &[String]) -> Vec<ArtifactInfo> {
    if rules.is_empty() {
        return artifacts.to_vec();
    }
    artifacts
        .iter()
        .filter(|a| {
            rules.iter().all(|rule| match rule.split_once('=') {
                Some((k, v)) => a.labels.get(k).is_some_and(|got| got == v),
                None => a.labels.contains_key(rule),
            })
        })
        .cloned()
        .collect()
}

/// Filter artifacts by tag list (empty = no filter).
pub fn filter_by_tags(artifacts: &[ArtifactInfo], tags: &[String]) -> Vec<ArtifactInfo> {
    if tags.is_empty() {
        return artifacts.to_vec();
    }
    artifacts
        .iter()
        .filter(|a| tags.iter().any(|t| t == &a.tag))
        .cloned()
        .collect()
}

/// Render an artifact list in the requested format (table / json / yaml).
pub fn render_artifacts(
    artifacts: &[ArtifactInfo],
    label_rules: &[String],
    format: OutputFormat,
) -> Result<()> {
    let filtered = filter_by_tags(&filter_by_labels(artifacts, label_rules), &[]);
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&filtered)?);
        }
        OutputFormat::Yaml => {
            print!("{}", serde_yml::to_string(&filtered)?);
        }
        OutputFormat::Table => {
            if filtered.is_empty() {
                println!("No oci-sync artifacts found");
                return Ok(());
            }
            let mut builder = tabled::builder::Builder::default();
            builder.push_record([
                "REPO",
                "TAG",
                "ENCRYPTED",
                "VERSION",
                "SIZE",
                "DIGEST",
                "LABELS",
            ]);
            for a in &filtered {
                let digest = if a.digest.len() > 32 {
                    format!("{}...", &a.digest[..32])
                } else {
                    a.digest.clone()
                };
                let labels = a
                    .labels
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                builder.push_record([
                    a.repo.clone(),
                    a.tag.clone(),
                    if a.encrypted { "yes" } else { "no" }.to_string(),
                    a.version.clone(),
                    format_bytes(a.size),
                    digest,
                    labels,
                ]);
            }
            let table = builder
                .build()
                .with(tabled::settings::Style::rounded())
                .to_string();
            let repo_display = artifacts
                .first()
                .and_then(|a| a.full_name.rsplit_once(':'))
                .map(|(n, _)| n.to_string())
                .or_else(|| artifacts.first().map(|a| a.full_name.clone()))
                .unwrap_or_default();
            println!("\n  Repository: {repo_display}\n");
            println!("{table}");
            println!("  Total: {} artifact(s)", filtered.len());
        }
    }
    Ok(())
}

/// Render shortcut list for `alias list`.
pub fn render_shortcuts(shortcuts: Vec<(String, String)>) -> Result<()> {
    if shortcuts.is_empty() {
        println!("No shortcuts configured");
        return Ok(());
    }
    let mut builder = tabled::builder::Builder::default();
    builder.push_record(["NAME", "REPO"]);
    for (name, repo) in &shortcuts {
        builder.push_record([name.clone(), repo.clone()]);
    }
    let table = builder
        .build()
        .with(tabled::settings::Style::rounded())
        .to_string();
    if let Some(path) = crate::config::Config::file_used() {
        println!("\n  Config: {}\n", path.display());
    }
    println!("{table}");
    println!("  Total: {} shortcut(s)", shortcuts.len());
    Ok(())
}

/// Ask `question` on the terminal; returns true on y/Y/yes. Errors when stdin
/// is not a TTY (callers must gate destructive ops on this).
pub fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("confirmation required but stdin is not a TTY (use --yes to skip)");
    }
    eprint!("{question} [y/N] ");
    std::io::Write::flush(&mut std::io::stderr())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let ans = line.trim().to_lowercase();
    Ok(ans == "y" || ans == "yes")
}

/// Human-readable byte size (1024-based, e.g. `1.5 MiB`).
pub fn format_bytes(n: i64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mk(labels: &[(&str, &str)]) -> ArtifactInfo {
        ArtifactInfo {
            full_name: "reg/r:t".into(),
            repo: "r".into(),
            tag: "t".into(),
            digest: "sha256:abc".into(),
            encrypted: false,
            version: "0.7.0".into(),
            size: 1,
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn label_filter_exact_and_presence() {
        let arts = vec![
            mk(&[("app", "web"), ("env", "prod")]),
            mk(&[("app", "cli")]),
        ];
        let r = filter_by_labels(&arts, &["app=web".into()]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].labels["env"], "prod");
        let r = filter_by_labels(&arts, &["env".into()]);
        assert_eq!(r.len(), 1);
        let r = filter_by_labels(&arts, &["app=web".into(), "env=prod".into()]);
        assert_eq!(r.len(), 1);
        let r = filter_by_labels(&arts, &["app=web".into(), "env=staging".into()]);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn tag_filter() {
        let mut a1 = mk(&[]);
        a1.tag = "v1".into();
        let mut a2 = mk(&[]);
        a2.tag = "v2".into();
        let arts = vec![a1, a2];
        let r = filter_by_tags(&arts, &["v1".into()]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].tag, "v1");
    }
}
