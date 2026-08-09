use std::path::Path;

use radar_core::SourcesConfig;

/// Resolve the effective sources config (§33). Priority:
///   1. `--sources <path>` override (must parse, else exit 3)
///   2. embedded default shipped with the binary (CFG-001)
///
/// `--sources` pointing to a missing or unparseable file fails closed (CFG-002).
pub fn load_sources(path: Option<&Path>) -> Result<SourcesConfig, crate::runtime::CliError> {
    match path {
        Some(p) => {
            let contents = std::fs::read_to_string(p).map_err(|e| {
                crate::runtime::CliError::config(format!(
                    "cannot read sources {}: {e}",
                    p.display()
                ))
            })?;
            SourcesConfig::parse(&contents).map_err(|e| {
                crate::runtime::CliError::config(format!("sources {}: {e}", p.display()))
            })
        }
        None => Ok(SourcesConfig::embedded()),
    }
}
