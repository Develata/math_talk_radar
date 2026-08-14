use crate::cli::UninstallArgs;
use crate::lifecycle;
use crate::output::write_stdout;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<(), CliError> {
    let message = lifecycle::uninstall::run(args).await?;
    write_stdout(&message)?;
    Ok(())
}
