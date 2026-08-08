use crate::cli::ScanArgs;
use crate::runtime::CliError;

pub async fn run(args: ScanArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("scan"))
}
