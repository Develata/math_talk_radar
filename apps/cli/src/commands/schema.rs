use crate::cli::SchemaArgs;
use crate::output::OUTPUT_SCHEMA_VERSION;
use crate::output::ScanOutput;
use crate::runtime::CliError;

pub async fn run(_args: SchemaArgs) -> Result<(), CliError> {
    let skeleton = ScanOutput {
        schema_version: OUTPUT_SCHEMA_VERSION.to_string(),
        generated_at: chrono::Utc::now(),
        query: crate::output::QuerySpec {
            mode: "both".into(),
            before_days: 30,
            after_days: 180,
        },
        events: Vec::new(),
        changes: Vec::new(),
        source_health: Vec::new(),
    };
    let json = serde_json::to_string_pretty(&skeleton)
        .map_err(|e| CliError::serialization(format!("encode schema skeleton: {e}")))?;
    println!("{json}");
    Ok(())
}
