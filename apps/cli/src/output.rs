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
/// `Json` pretty-prints the full envelope; `Jsonl` emits one JSON object per
/// event. Detail level truncates long text fields in place: compact ≤1200
/// chars, full ≤8000. Takes `output` by value so truncation mutates in place
/// without cloning the (potentially large) event vector.
pub fn render(
    mut output: ScanOutput,
    format: crate::cli::OutputFormat,
    detail: crate::cli::DetailLevel,
) -> anyhow::Result<String> {
    truncate_for_detail(&mut output, detail);
    match format {
        crate::cli::OutputFormat::Json => Ok(serde_json::to_string_pretty(&output)?),
        crate::cli::OutputFormat::Jsonl => Ok(render_jsonl(&output)?),
    }
}

/// §31 JSONL: one JSON object per line — the envelope metadata first, then one
/// line per event.
fn render_jsonl(output: &ScanOutput) -> anyhow::Result<String> {
    let mut out = String::new();
    let envelope = serde_json::json!({
        "kind": "scan",
        "schema_version": output.schema_version,
        "generated_at": output.generated_at,
        "query": output.query,
        "source_health": output.source_health,
        "changes": output.changes,
    });
    out.push_str(&envelope.to_string());
    out.push('\n');
    for event in &output.events {
        out.push_str(&serde_json::to_string(event)?);
        out.push('\n');
    }
    Ok(out)
}

fn truncate_for_detail(output: &mut ScanOutput, detail: crate::cli::DetailLevel) {
    let limit = match detail {
        crate::cli::DetailLevel::Compact => 1200,
        crate::cli::DetailLevel::Full => 8000,
    };
    for event in &mut output.events {
        if let Some(d) = &event.description
            && let Some(t) = truncate_if_longer(d, limit)
        {
            event.description = Some(t);
        }
        for talk in &mut event.talks {
            if let Some(a) = &talk.abstract_text
                && let Some(t) = truncate_if_longer(a, limit)
            {
                talk.abstract_text = Some(t);
            }
        }
    }
}

fn truncate_if_longer(s: &str, limit: usize) -> Option<String> {
    if s.len() <= limit {
        return None;
    }
    s.char_indices()
        .nth(limit)
        .map(|(idx, _)| s[..idx].to_string())
}
