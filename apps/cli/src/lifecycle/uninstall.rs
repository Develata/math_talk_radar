//! Uninstall (§35). Full implementation lands in Todo 4.
use crate::cli::UninstallArgs;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<String, CliError> {
    let _ = args;
    Err(CliError::not_implemented("uninstall"))
}
