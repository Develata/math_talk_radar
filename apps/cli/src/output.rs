//! Public JSON output contract (§29, §31, §64).
use chrono::{DateTime, Utc};
use radar_core::{Event, SourceHealth};
use radar_state::ChangeRecord;
use serde::{Deserialize, Serialize};

/// Public output schema version (§64). v0.x may add optional fields; renaming
/// or removing a field requires bumping this.
pub const OUTPUT_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOutput {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub query: QuerySpec,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub changes: Vec<ChangeRecord>,
    #[serde(default)]
    pub source_health: Vec<SourceHealth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySpec {
    pub mode: String,
    pub before_days: u32,
    pub after_days: u32,
}

/// Render a `ScanOutput` to the requested format. `Json` pretty-prints the full
/// envelope; `Jsonl` is reserved for the streaming path (M4).
pub fn render(output: &ScanOutput, format: crate::cli::OutputFormat) -> anyhow::Result<String> {
    match format {
        crate::cli::OutputFormat::Json => Ok(serde_json::to_string_pretty(output)?),
        crate::cli::OutputFormat::Jsonl => Ok(serde_json::to_string(output)?),
    }
}
