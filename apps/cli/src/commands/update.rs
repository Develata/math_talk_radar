use crate::cli::UpdateArgs;
use crate::lifecycle;
use crate::runtime::CliError;

pub async fn run(args: UpdateArgs) -> Result<(), CliError> {
    let message = if args.check {
        lifecycle::update::check().await?
    } else {
        lifecycle::update::run().await?
    };
    println!("{message}");
    Ok(())
}
