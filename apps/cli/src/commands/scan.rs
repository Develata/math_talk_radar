use crate::cli::ScanArgs;
use crate::output::render;
use crate::runtime::CliError;
use crate::scan_engine::run_scan;

pub async fn run(args: ScanArgs) -> Result<(), CliError> {
    let format = args.format;
    let detail = args.detail;
    let output = run_scan(args).await?;
    let rendered = render(&output, format, detail)
        .map_err(|e| CliError::serialization(format!("encode output: {e}")))?;
    println!("{rendered}");
    Ok(())
}
