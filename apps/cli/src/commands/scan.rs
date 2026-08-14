use crate::cli::ScanArgs;
use crate::output::render;
use crate::runtime::CliError;
use crate::scan_engine::run_scan;
use std::io::Write;

pub async fn run(args: ScanArgs) -> Result<(), CliError> {
    let format = args.format;
    let detail = args.detail;
    let output = run_scan(args).await?;
    let rendered = render(output, format, detail)
        .map_err(|e| CliError::serialization(format!("encode output: {e}")))?;
    // CLI-23: use a single write_all instead of println! so a closed pipe
    // (e.g. `| head`) yields a clean exit 0, not a panic exit 101. The §32
    // contract does not define exit 101; BrokenPipe on stdout is the normal
    // Unix signal that the downstream consumer is done and we should stop.
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let result = handle
        .write_all(rendered.as_bytes())
        .and_then(|()| handle.write_all(b"\n"))
        .and_then(|()| handle.flush());
    if let Err(e) = result
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(CliError::serialization(format!("write stdout: {e}")));
    }
    Ok(())
}
