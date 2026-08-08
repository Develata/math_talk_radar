use crate::cli::UpdateArgs;
use crate::runtime::CliError;

pub async fn run(args: UpdateArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("update"))
}
