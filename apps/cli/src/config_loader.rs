use std::path::Path;

use radar_core::SourcesConfig;

/// Resolve the effective sources config (§33). Priority:
///   1. `--sources <path>` override (must parse, else exit 3)
///   2. embedded default shipped with the binary (CFG-001)
///
/// `--sources` pointing to a missing or unparseable file fails closed (CFG-002).
/// R9-M02: after parsing, run semantic validation (duplicate IDs, HtmlConfig
/// required selectors) so a malformed config fails fast at load time with a
/// precise message instead of a late runtime failure mid-scan.
pub fn load_sources(path: Option<&Path>) -> Result<SourcesConfig, crate::runtime::CliError> {
    let config = match path {
        Some(p) => {
            let contents = std::fs::read_to_string(p).map_err(|e| {
                crate::runtime::CliError::config(format!(
                    "cannot read sources {}: {e}",
                    p.display()
                ))
            })?;
            SourcesConfig::parse(&contents).map_err(|e| {
                crate::runtime::CliError::config(format!("sources {}: {e}", p.display()))
            })?
        }
        None => SourcesConfig::embedded()
            .map_err(|e| crate::runtime::CliError::config(format!("embedded sources.toml: {e}")))?,
    };
    config.validate().map_err(|e| {
        let ctx = match path {
            Some(p) => format!("sources {}", p.display()),
            None => "embedded sources.toml".to_string(),
        };
        crate::runtime::CliError::config(format!("{ctx}: {e}"))
    })?;
    Ok(config)
}
