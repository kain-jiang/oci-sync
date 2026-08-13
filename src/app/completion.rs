//! `completion` — generate shell completion scripts via clap_complete.

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, shells};

use crate::cli::{Cli, Shell};

pub fn run(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    match shell {
        Shell::Bash => generate(shells::Bash, &mut cmd, name, &mut std::io::stdout()),
        Shell::Zsh => generate(shells::Zsh, &mut cmd, name, &mut std::io::stdout()),
        Shell::Fish => generate(shells::Fish, &mut cmd, name, &mut std::io::stdout()),
        Shell::PowerShell => generate(shells::PowerShell, &mut cmd, name, &mut std::io::stdout()),
    }
    Ok(())
}
