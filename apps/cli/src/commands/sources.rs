use crate::cli::SourcesArgs;
use crate::runtime::CliError;

pub async fn run(args: SourcesArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("sources"))
}
