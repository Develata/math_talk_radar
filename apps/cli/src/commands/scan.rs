use crate::cli::ScanArgs;
use crate::output::render_to;
use crate::runtime::CliError;
use crate::scan_engine::run_scan;

pub async fn run(args: ScanArgs) -> Result<(), CliError> {
    let format = args.format;
    let detail = args.detail;
    let output = run_scan(args).await?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    render_to(output, format, detail, &mut handle)
}
