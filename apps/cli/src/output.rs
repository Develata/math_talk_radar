//! Public JSON output contract (§29, §31, §64).
use chrono::{DateTime, Utc};
use radar_core::{Event, SourceHealth};
use radar_state::ChangeRecord;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Public output schema version (§64). v0.x may add optional fields; renaming
/// or removing a field requires bumping this.
pub const OUTPUT_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanOutput {
    /// Always "1.0" in v0.x (§64). Constrained in the JSON schema via
    /// `#[schemars(schema_with = ...)]` so consumers can validate compatibility.
    #[schemars(schema_with = "const_schema_1_0")]
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

/// H4-1: JSON schema fragment constraining `schema_version` to exactly "1.0".
fn const_schema_1_0(_gen: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({
        "type": "string",
        "const": "1.0"
    }))
    .unwrap_or(schemars::schema::Schema::Bool(true))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuerySpec {
    /// H4-1: constrained to "upcoming", "recordings", or "both".
    #[schemars(schema_with = "mode_enum_schema")]
    pub mode: String,
    pub before_days: u32,
    pub after_days: u32,
    /// H5-2: the IANA timezone used for date interpretation (§27.2). Empty
    /// string means UTC was used. Adding an optional field is schema-compatible
    /// in v0.x (§64).
    #[serde(default)]
    pub timezone: String,
}

/// H4-1: JSON schema fragment constraining `mode` to the three valid scan modes.
fn mode_enum_schema(_gen: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({
        "type": "string",
        "enum": ["upcoming", "recordings", "both"]
    }))
    .unwrap_or(schemars::schema::Schema::Bool(true))
}

/// Render a `ScanOutput` to the requested format and detail level (§31) and
/// stream it to `out` without buffering the full serialization in memory.
/// `Json` pretty-prints the full envelope; `Jsonl` emits one JSON object per
/// event. Detail level truncates long text fields in place: compact ≤1200
/// chars, full ≤8000. Takes `output` by value so truncation mutates in place
/// without cloning the (potentially large) event vector.
///
/// A `BrokenPipe` on `out` is the normal Unix signal that the downstream
/// consumer (e.g. `| head`) is done; per CLI-23 / §32 it is treated as
/// success (exit 0), not a §32 exit 6. Any other I/O or serialization error
/// surfaces as `CliError::serialization`.
pub fn render_to<W: std::io::Write>(
    mut output: ScanOutput,
    format: crate::cli::OutputFormat,
    detail: crate::cli::DetailLevel,
    out: &mut W,
) -> Result<(), crate::runtime::CliError> {
    truncate_for_detail(&mut output, detail);
    match format {
        crate::cli::OutputFormat::Json => {
            serde_json::to_writer_pretty(&mut *out, &output).or_else(ser_to_cli)?;
            out.write_all(b"\n").or_else(io_to_cli)?;
        }
        crate::cli::OutputFormat::Jsonl => render_jsonl_to(&output, &mut *out)?,
    }
    out.flush().or_else(io_to_cli)?;
    Ok(())
}

fn ser_to_cli(e: serde_json::Error) -> Result<(), crate::runtime::CliError> {
    if e.io_error_kind() == Some(std::io::ErrorKind::BrokenPipe) {
        Ok(())
    } else {
        Err(crate::runtime::CliError::serialization(format!(
            "encode output: {e}"
        )))
    }
}

fn io_to_cli(e: std::io::Error) -> Result<(), crate::runtime::CliError> {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        Ok(())
    } else {
        Err(crate::runtime::CliError::serialization(format!(
            "write stdout: {e}"
        )))
    }
}

fn render_jsonl_to<W: std::io::Write>(
    output: &ScanOutput,
    out: &mut W,
) -> Result<(), crate::runtime::CliError> {
    let envelope = serde_json::json!({
        "kind": "scan",
        "schema_version": output.schema_version,
        "generated_at": output.generated_at,
        "query": output.query,
        "source_health": output.source_health,
        "changes": output.changes,
    });
    serde_json::to_writer(&mut *out, &envelope).or_else(ser_to_cli)?;
    out.write_all(b"\n").or_else(io_to_cli)?;
    for event in &output.events {
        serde_json::to_writer(&mut *out, event).or_else(ser_to_cli)?;
        out.write_all(b"\n").or_else(io_to_cli)?;
    }
    Ok(())
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

/// Write `content` + a trailing newline to stdout, treating `BrokenPipe` as
/// success (the downstream consumer closed the pipe, e.g. `| head`). Any other
/// I/O error surfaces as §32 exit 6. Management commands must route stdout
/// through this helper — a bare `println!` panics on a closed pipe and exits
/// 101, which is not in the §32 enum.
pub fn write_stdout(content: &str) -> Result<(), crate::runtime::CliError> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let result = handle
        .write_all(content.as_bytes())
        .and_then(|()| handle.write_all(b"\n"))
        .and_then(|()| handle.flush());
    if let Err(e) = result
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(crate::runtime::CliError::serialization(format!(
            "write stdout: {e}"
        )));
    }
    Ok(())
}

/// Like [`write_stdout`] but without a trailing newline — for callers that
/// composed exact bytes (e.g. pretty JSON into `| jq`).
pub fn write_stdout_raw(content: &str) -> Result<(), crate::runtime::CliError> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let result = handle
        .write_all(content.as_bytes())
        .and_then(|()| handle.flush());
    if let Err(e) = result
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(crate::runtime::CliError::serialization(format!(
            "write stdout: {e}"
        )));
    }
    Ok(())
}
