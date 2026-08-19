use crate::cli::SourcesArgs;
use crate::config_loader::load_sources;
use crate::output::write_stdout;
use crate::runtime::CliError;

pub async fn run(args: SourcesArgs) -> Result<(), CliError> {
    let config = load_sources(None)?;
    match args.action {
        crate::cli::SourcesAction::List => {
            if config.sources.is_empty() {
                write_stdout("no sources configured")?;
                return Ok(());
            }
            write_stdout(&format!(
                "{:<24} {:<32} {:<6} {:<10} ENABLED",
                "ID", "NAME", "TIER", "ADAPTER"
            ))?;
            for s in &config.sources {
                write_stdout(&format!(
                    "{:<24} {:<32} {:<6} {:<10} {}",
                    s.id,
                    s.name,
                    s.tier,
                    s.adapter,
                    s.enabled
                ))?;
            }
            Ok(())
        }
        crate::cli::SourcesAction::Check { source: _ } => {
            Err(CliError::not_implemented("sources check"))
        }
    }
}
