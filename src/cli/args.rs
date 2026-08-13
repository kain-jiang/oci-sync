//! clap argument definitions — the single source of truth for the CLI surface.

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "oci-sync",
    version,
    about = "Sync local files or directories to OCI-compatible image registries",
    long_about = "Pack, compress and optionally encrypt local files or directories, then push them \
as OCI artifacts to any OCI-compatible registry. Authentication uses config-file \
credentials or the Docker credential store (docker login)."
)]
pub struct Cli {
    /// Omit informational output (errors are always shown)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Push local files or directories to an OCI registry
    Push(PushArgs),
    /// Pull files or directories from an OCI registry to a local path
    Pull(PullArgs),
    /// Delete an artifact from a registry (asks for confirmation)
    Delete(DeleteArgs),
    /// List oci-sync artifacts in a repository or a whole registry
    List(ListArgs),
    /// Manage labels on existing artifacts
    Label(LabelArgs),
    /// Manage configured shortcuts
    Alias(AliasArgs),
    /// Show recent activity history
    Recent(RecentArgs),
    /// Launch the full-screen interactive TUI
    Tui,
    /// Generate shell completion scripts
    Completion(CompletionArgs),
    /// Dynamic shortcut commands (from config `shortcuts.<name>.repo`).
    /// Usage: `oci-sync <name> <push|pull|list|delete> [flags]`
    #[command(external_subcommand)]
    Shortcut(Vec<String>),
}

// ---------------------------------------------------------------- push

#[derive(Debug, Args)]
pub struct PushArgs {
    /// Local file or directory path
    #[arg(short, long)]
    pub local: String,

    /// Remote OCI reference, format: <registry>/<repository>:<tag>
    #[arg(short, long)]
    pub remote: String,

    /// Passphrase for AES-256-GCM encryption (omit for plaintext)
    #[arg(long)]
    pub passphrase: Option<String>,

    /// Label to set on the artifact (key=value, repeatable, value may be empty)
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,

    /// After pushing, pull the artifact back and verify the content digest
    #[arg(long)]
    pub verify: bool,
}

// ---------------------------------------------------------------- pull

#[derive(Debug, Args)]
pub struct PullArgs {
    /// Remote OCI reference, format: <registry>/<repository>:<tag>
    #[arg(short, long)]
    pub remote: String,

    /// Local destination directory
    #[arg(short, long)]
    pub local: String,

    /// Passphrase for decryption (required when the content is encrypted)
    #[arg(long)]
    pub passphrase: Option<String>,

    /// Overwrite existing files in the destination
    #[arg(short, long)]
    pub force: bool,
}

// ---------------------------------------------------------------- delete

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Remote OCI reference, format: <registry>/<repository>:<tag>
    #[arg(short, long)]
    pub remote: String,

    /// Skip the confirmation prompt
    #[arg(short, long)]
    pub yes: bool,
}

// ---------------------------------------------------------------- list

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Remote reference: <registry>/<repository> or a bare <registry> to scan everything
    #[arg(short, long)]
    pub remote: String,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Filter by label (key=value exact match, or bare key for presence), repeatable
    #[arg(long = "label", value_name = "KEY[=VALUE]")]
    pub labels: Vec<String>,

    /// Only show artifacts carrying this tag (repeatable for multiple)
    #[arg(short, long)]
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------- label

#[derive(Debug, Args)]
pub struct LabelArgs {
    #[command(subcommand)]
    pub command: LabelCommand,
}

#[derive(Debug, Subcommand)]
pub enum LabelCommand {
    /// Set or update labels (key=value, value may be empty)
    Set(LabelSetArgs),
    /// Remove labels by key
    Unset(LabelUnsetArgs),
}

#[derive(Debug, Args)]
pub struct LabelSetArgs {
    /// Remote OCI reference, format: <registry>/<repository>:<tag>
    #[arg(short, long)]
    pub remote: String,

    /// Labels to set, format key=value (repeatable)
    #[arg(value_name = "KEY=VALUE", required = true)]
    pub labels: Vec<String>,
}

#[derive(Debug, Args)]
pub struct LabelUnsetArgs {
    /// Remote OCI reference, format: <registry>/<repository>:<tag>
    #[arg(short, long)]
    pub remote: String,

    /// Label keys to remove (repeatable)
    #[arg(value_name = "KEY", required = true)]
    pub keys: Vec<String>,
}

// ---------------------------------------------------------------- alias

#[derive(Debug, Args)]
pub struct AliasArgs {
    #[command(subcommand)]
    pub command: AliasCommand,
}

#[derive(Debug, Subcommand)]
pub enum AliasCommand {
    /// List all configured shortcuts
    List,
    /// Add a shortcut
    Add(AliasAddArgs),
    /// Remove a shortcut
    Remove(AliasRemoveArgs),
}

#[derive(Debug, Args)]
pub struct AliasAddArgs {
    /// Shortcut name (becomes `oci-sync <name> push|pull|list|delete`)
    #[arg(value_name = "NAME")]
    pub name: String,

    /// Repository for the shortcut, format: <registry>/<repository> (no tag)
    #[arg(short, long)]
    pub repo: String,
}

#[derive(Debug, Args)]
pub struct AliasRemoveArgs {
    /// Shortcut name to remove
    #[arg(value_name = "NAME")]
    pub name: String,
}

// ---------------------------------------------------------------- recent

#[derive(Debug, Args)]
pub struct RecentArgs {
    /// Maximum number of activities to show
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Clear all activity history
    #[arg(long)]
    pub clear: bool,

    /// Show aggregate statistics (totals per operation type)
    #[arg(long)]
    pub stats: bool,
}

// ---------------------------------------------------------------- completion

#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// Shell to generate completion for
    #[arg(value_enum)]
    pub shell: Shell,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

// ---------------------------------------------------------------- shortcut

#[derive(Debug, Parser)]
pub struct ShortcutPushArgs {
    #[arg(short, long)]
    pub local: String,
    #[arg(short, long)]
    pub tag: String,
    #[arg(long)]
    pub passphrase: Option<String>,
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
    #[arg(long)]
    pub verify: bool,
}

#[derive(Debug, Parser)]
pub struct ShortcutPullArgs {
    #[arg(short, long)]
    pub tag: String,
    #[arg(short, long)]
    pub local: String,
    #[arg(long)]
    pub passphrase: Option<String>,
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Parser)]
pub struct ShortcutListArgs {
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
    #[arg(long = "label", value_name = "KEY[=VALUE]")]
    pub labels: Vec<String>,
    #[arg(short, long)]
    pub tags: Vec<String>,
}

#[derive(Debug, Parser)]
pub struct ShortcutDeleteArgs {
    #[arg(short, long)]
    pub tag: String,
    #[arg(short, long)]
    pub yes: bool,
}

// ---------------------------------------------------------------- shared

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}
