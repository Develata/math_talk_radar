use crate::cli::UninstallArgs;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("uninstall"))
}
