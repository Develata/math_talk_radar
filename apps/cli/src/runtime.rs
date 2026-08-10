//! Runtime dispatch, logging setup, and exit-code mapping (§32).
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command, LogFormat};
use crate::commands;

/// A command-level error carrying a stable exit code (§32).
#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub message: String,
}

impl CliError {
    /// §32 exit 2: usage error. Returned by stubs for commands not yet
    /// implemented; exit 1 is deliberately absent from the §32 enum.
    pub fn not_implemented(cmd: &str) -> Self {
        Self {
            code: 2,
            message: format!("{cmd}: not implemented in M0"),
        }
    }

    /// §32 exit 3: config or schema error.
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            code: 3,
            message: message.into(),
        }
    }

    /// §32 exit 4: zero usable sources.
    pub fn zero_sources() -> Self {
        Self {
            code: 4,
            message: "no enabled sources to scan".into(),
        }
    }

    /// §32 exit 5: state fatal.
    pub fn state(message: impl Into<String>) -> Self {
        Self {
            code: 5,
            message: message.into(),
        }
    }

    /// §32 exit 6: output serialization fatal.
    pub fn serialization(message: impl Into<String>) -> Self {
        Self {
            code: 6,
            message: message.into(),
        }
    }

    /// §32 exit 10: update fatal.
    pub fn update(message: impl Into<String>) -> Self {
        Self {
            code: 10,
            message: message.into(),
        }
    }

    /// §32 exit 11: uninstall fatal.
    pub fn uninstall(message: impl Into<String>) -> Self {
        Self {
            code: 11,
            message: message.into(),
        }
    }
}

pub fn init_logging(verbose: u8, log_format: Option<LogFormat>) {
    let default_level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    match log_format {
        Some(LogFormat::Json) => {
            let _ = subscriber.json().try_init();
        }
        _ => {
            let _ = subscriber.try_init();
        }
    }
}

pub async fn run(cli: Cli) -> ExitCode {
    let result = match cli.command {
        Command::Scan(a) => commands::scan::run(a).await,
        Command::Sources(a) => commands::sources::run(a).await,
        Command::Doctor(a) => commands::doctor::run(a).await,
        Command::Update(a) => commands::update::run(a).await,
        Command::Uninstall(a) => commands::uninstall::run(a).await,
        Command::Schema(a) => commands::schema::run(a).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", e.message);
            ExitCode::from(e.code)
        }
    }
}
