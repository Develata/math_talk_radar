use crate::cli::SchemaArgs;
use crate::runtime::CliError;

pub async fn run(args: SchemaArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("schema"))
}
