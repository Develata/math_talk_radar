use crate::cli::ScanArgs;
use crate::output::render;
use crate::runtime::CliError;
use crate::scan_engine::run_scan;

pub async fn run(args: ScanArgs) -> Result<(), CliError> {
    let output = run_scan(args).await?;
    let rendered = render(&output, crate::cli::OutputFormat::Json)
        .map_err(|e| CliError::serialization(format!("encode output: {e}")))?;
    println!("{rendered}");
    Ok(())
}
