use crate::cli::DoctorArgs;
use crate::runtime::CliError;

pub async fn run(args: DoctorArgs) -> Result<(), CliError> {
    let _ = args;
    Err(CliError::not_implemented("doctor"))
}
