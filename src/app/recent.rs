//! `recent` — activity history output. Pure local read; no registry access.

use anyhow::Result;

use crate::cache;
use crate::cli::{OutputFormat, RecentArgs};

pub fn run(args: &RecentArgs) -> Result<()> {
    if args.clear {
        cache::clear()?;
        println!("Activity history cleared");
        return Ok(());
    }
    if args.stats {
        let stats = cache::stats()?;
        if stats.is_empty() {
            println!("No activities recorded");
            return Ok(());
        }
        let mut builder = tabled::builder::Builder::default();
        builder.push_record(["TYPE", "COUNT"]);
        let mut total = 0usize;
        for (kind, count) in &stats {
            builder.push_record([kind.clone(), count.to_string()]);
            total += count;
        }
        builder.push_record(["total".to_string(), total.to_string()]);
        let table = builder
            .build()
            .with(tabled::settings::Style::rounded())
            .to_string();
        println!("{table}");
        return Ok(());
    }

    let activities = cache::recent(args.limit)?;
    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&activities)?),
        OutputFormat::Yaml => print!("{}", serde_yml::to_string(&activities)?),
        OutputFormat::Table => {
            if activities.is_empty() {
                println!("No recent activities");
                return Ok(());
            }
            let mut builder = tabled::builder::Builder::default();
            builder.push_record(["TYPE", "TIME", "REMOTE", "LOCAL", "RESULT"]);
            for a in &activities {
                builder.push_record([
                    a.kind.as_str().to_string(),
                    a.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                    a.remote_ref.clone(),
                    a.local_path.clone().unwrap_or_default(),
                    if a.success { "ok" } else { "failed" }.to_string(),
                ]);
            }
            let table = builder
                .build()
                .with(tabled::settings::Style::rounded())
                .to_string();
            println!("{table}");
        }
    }
    Ok(())
}
