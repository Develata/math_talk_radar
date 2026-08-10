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

/// Render a `ScanOutput` to the requested format and detail level (§31).
/// `Json` pretty-prints the full envelope; `Jsonl` emits one JSON object.
/// Detail level truncates long text fields: compact ≤1200 chars, full ≤8000.
pub fn render(
    output: &ScanOutput,
    format: crate::cli::OutputFormat,
    detail: crate::cli::DetailLevel,
) -> anyhow::Result<String> {
    let mut output = output.clone();
    truncate_for_detail(&mut output, detail);
    match format {
        crate::cli::OutputFormat::Json => Ok(serde_json::to_string_pretty(&output)?),
        crate::cli::OutputFormat::Jsonl => Ok(serde_json::to_string(&output)?),
    }
}

fn truncate_for_detail(output: &mut ScanOutput, detail: crate::cli::DetailLevel) {
    let limit = match detail {
        crate::cli::DetailLevel::Compact => 1200,
        crate::cli::DetailLevel::Full => 8000,
    };
    for event in &mut output.events {
        if let Some(d) = &event.description
            && d.chars().count() > limit
        {
            event.description = Some(d.chars().take(limit).collect());
        }
        for talk in &mut event.talks {
            if let Some(a) = &talk.abstract_text
                && a.chars().count() > limit
            {
                talk.abstract_text = Some(a.chars().take(limit).collect());
            }
        }
    }
}
