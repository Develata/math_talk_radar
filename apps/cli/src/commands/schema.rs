use crate::cli::SchemaArgs;
use crate::output::{ScanOutput, write_stdout_raw};
use crate::runtime::CliError;
use schemars::schema_for;

/// H4: `math_talk_radar schema` prints the JSON Schema for the scan output
/// envelope (§30, §64), not an empty `ScanOutput` instance. The schema is
/// derived from the Rust model via `schemars` so it stays in lockstep with the
/// serialized types. CI checks the generated output against a golden file
/// (`docs/reference/output-schema.json`) to catch drift (§09:38).
pub async fn run(_args: SchemaArgs) -> Result<(), CliError> {
    let schema = schema_for!(ScanOutput);
    let json = serde_json::to_string_pretty(&schema)
        .map_err(|e| CliError::serialization(format!("encode JSON schema: {e}")))?;
    write_stdout_raw(&json)?;
    Ok(())
}
