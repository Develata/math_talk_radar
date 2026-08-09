use crate::cli::UninstallArgs;
use crate::lifecycle;
use crate::runtime::CliError;

pub async fn run(args: UninstallArgs) -> Result<(), CliError> {
    let message = lifecycle::uninstall::run(args).await?;
    println!("{message}");
    Ok(())
}
