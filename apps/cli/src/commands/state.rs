use crate::cli::{StateAction, StateArgs};
use crate::runtime::CliError;

pub async fn run(args: StateArgs) -> Result<(), CliError> {
    match args.action {
        StateAction::Migrate { from, to } => {
            radar_state::file_migration::migrate_copy(&from, &to)
                .map_err(|error| CliError::state(format!("state migration failed: {error}")))?;
            println!(
                "Migrated state to {}. Original retained at {} for rollback.",
                to.display(),
                from.display()
            );
            println!(
                "Select the new file with scan --state. Later scans are not copied back to the original."
            );
            Ok(())
        }
    }
}
