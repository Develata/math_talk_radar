use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ScanMode {
    Upcoming,
    Recordings,
    Both,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OutputFormat {
    Json,
    Jsonl,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum DetailLevel {
    Compact,
    Full,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "math_talk_radar",
    version,
    about = "Radar for public mathematics conferences, talks, lecture series, and recordings",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Increase verbosity (-v info, -vv debug).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Log output format.
    #[arg(long, value_enum, global = true)]
    pub log_format: Option<LogFormat>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan sources for upcoming/recorded mathematics events.
    Scan(ScanArgs),
    /// Inspect the source registry and per-source health.
    Sources(SourcesArgs),
    /// Diagnose the local installation.
    Doctor(DoctorArgs),
    /// Self-update against the official GitHub release origin.
    Update(UpdateArgs),
    /// Remove the binary and optionally its data.
    Uninstall(UninstallArgs),
    /// Print the current public JSON output schema.
    Schema(SchemaArgs),
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Discovery mode.
    #[arg(long, value_enum, default_value_t = ScanMode::Both)]
    pub mode: ScanMode,
    /// Window before `--today` in days.
    #[arg(long, default_value_t = 30)]
    pub before: u32,
    /// Window after `--today` in days.
    #[arg(long, default_value_t = 180)]
    pub after: u32,
    /// Concurrent fetch jobs.
    #[arg(long, default_value_t = 8)]
    pub jobs: u32,
    /// Cap on emitted events.
    #[arg(long)]
    pub max_events: Option<u32>,
    /// Cap on emitted talks.
    #[arg(long)]
    pub max_talks: Option<u32>,
    /// Override the local IANA timezone.
    #[arg(long, value_name = "IANA")]
    pub timezone: Option<String>,
    /// Inject `today` for deterministic runs (YYYY-MM-DD).
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub today: Option<String>,
    /// Path to the main config file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Path to the source registry TOML.
    #[arg(long, value_name = "PATH")]
    pub sources: Option<PathBuf>,
    /// Path to the scholars TOML.
    #[arg(long, value_name = "PATH")]
    pub scholars: Option<PathBuf>,
    /// Path to the user interests TOML.
    #[arg(long, value_name = "PATH")]
    pub interests: Option<PathBuf>,
    /// Path to the state database.
    #[arg(long, value_name = "PATH")]
    pub state: Option<PathBuf>,
    /// Run without reading or writing state.
    #[arg(long)]
    pub no_state: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    pub format: OutputFormat,
    /// Output detail level.
    #[arg(long, value_enum, default_value_t = DetailLevel::Compact)]
    pub detail: DetailLevel,
}

#[derive(Debug, Args)]
pub struct SourcesArgs {
    #[command(subcommand)]
    pub action: SourcesAction,
}

#[derive(Debug, Subcommand)]
pub enum SourcesAction {
    /// List configured sources.
    List,
    /// Check all or a single source.
    Check {
        /// Optional source id; omit to check all.
        source: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Include network connectivity checks.
    #[arg(long)]
    pub network: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Only check for a newer release; do not modify any file.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Print the deletion plan and change nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Remove binary + config + cache, keep persistent data.
    #[arg(long)]
    pub keep_data: bool,
    /// Remove everything including persistent data.
    #[arg(long)]
    pub purge: bool,
    /// Noninteractive confirmation (required when not a TTY).
    #[arg(long)]
    pub yes: bool,
    /// Allow uninstall from an unmanaged/development binary.
    #[arg(long)]
    pub force_unmanaged: bool,
}

#[derive(Debug, Args)]
pub struct SchemaArgs {}
