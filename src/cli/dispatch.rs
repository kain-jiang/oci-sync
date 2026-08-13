//! Command dispatch: parse the `Command` enum and call the app layer.
//!
//! Dynamic shortcut commands (`oci-sync <name> <sub> ...`) are captured by
//! clap's `external_subcommand` as `Vec<String>` and re-parsed here against
//! the dedicated shortcut arg structs.

use anyhow::{Result, bail};
use clap::Parser;

use crate::app;
use crate::config;
use crate::output;

use super::args::*;
use super::args::{Cli, Command};

pub async fn dispatch(cli: Cli) -> Result<()> {
    let cfg = config::load()?;

    match cli.command {
        Command::Push(a) => app::push::run(&cfg, &a).await,
        Command::Pull(a) => app::pull::run(&cfg, &a).await,
        Command::Delete(a) => app::delete::run(&cfg, &a).await,
        Command::List(a) => {
            let artifacts = app::list::run(&cfg, &a).await?;
            output::render_artifacts(&artifacts, &a.labels, a.format)
        }
        Command::Label(l) => match l.command {
            LabelCommand::Set(a) => app::label::set(&cfg, &a).await,
            LabelCommand::Unset(a) => app::label::unset(&cfg, &a).await,
        },
        Command::Alias(a) => match a.command {
            AliasCommand::List => output::render_shortcuts(cfg.all_shortcuts()),
            AliasCommand::Add(a) => app::alias::add(&cfg, &a),
            AliasCommand::Remove(a) => app::alias::remove(&cfg, &a),
        },
        Command::Recent(r) => app::recent::run(&r),
        Command::Tui => app::tui::run().await,
        Command::Completion(c) => app::completion::run(c.shell),
        Command::Shortcut(raw) => dispatch_shortcut(&cfg, raw).await,
    }
}

/// Re-parse `[<name>, <sub>, ...flags]` into the matching shortcut subcommand.
async fn dispatch_shortcut(cfg: &config::Config, raw: Vec<String>) -> Result<()> {
    let Some(name) = raw.first() else {
        bail!("shortcut command requires a name, e.g. `oci-sync <name> push`");
    };
    let Some(sub) = raw.get(1) else {
        bail!("shortcut {name:?} requires a subcommand: push | pull | list | delete");
    };
    let rest: Vec<String> = raw[2..].to_vec();

    match sub.as_str() {
        "push" => {
            let a =
                ShortcutPushArgs::try_parse_from(std::iter::once("push".to_string()).chain(rest))?;
            let remote = cfg.shortcut_remote_ref(name, &a.tag)?;
            app::push::run_ref(
                cfg,
                &remote,
                &a.local,
                a.passphrase.as_deref(),
                &a.labels,
                a.verify,
            )
            .await
        }
        "pull" => {
            let a =
                ShortcutPullArgs::try_parse_from(std::iter::once("pull".to_string()).chain(rest))?;
            let remote = cfg.shortcut_remote_ref(name, &a.tag)?;
            app::pull::run_ref(cfg, &remote, &a.local, a.passphrase.as_deref(), a.force).await
        }
        "list" => {
            let a =
                ShortcutListArgs::try_parse_from(std::iter::once("list".to_string()).chain(rest))?;
            let repo = cfg.shortcut_repo(name)?;
            let artifacts = app::list::run_repo(cfg, &repo).await?;
            output::render_artifacts(&artifacts, &a.labels, a.format)
        }
        "delete" => {
            let a = ShortcutDeleteArgs::try_parse_from(
                std::iter::once("delete".to_string()).chain(rest),
            )?;
            let remote = cfg.shortcut_remote_ref(name, &a.tag)?;
            app::delete::run_ref(cfg, &remote, a.yes).await
        }
        other => bail!(
            "shortcut {name:?}: unknown subcommand {other:?} (expected push | pull | list | delete)"
        ),
    }
}
