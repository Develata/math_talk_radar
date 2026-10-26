use std::path::Path;

use radar_core::SourcesConfig;

/// TOML's Display includes input snippets and sometimes values in its message.
/// Emit only a context and source position at the CLI logging boundary.
pub(crate) fn parse_error(
    context: &str,
    input: &str,
    span: Option<std::ops::Range<usize>>,
) -> crate::runtime::CliError {
    let location = span
        .and_then(|span| input.get(..span.start))
        .map(|prefix| {
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!(" at line {line}, column {column}")
        })
        .unwrap_or_default();
    crate::runtime::CliError::config(format!("{context}: invalid TOML or field type{location}"))
}

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
                parse_error(&format!("sources {}", p.display()), &contents, e.span())
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
        use radar_core::config::ConfigError;
        let reason = match e {
            ConfigError::DuplicateSourceId(_) => "duplicate source id",
            ConfigError::MissingSelectors(_) => "missing selectors table",
            ConfigError::EmptySelector(_, field) | ConfigError::EmptyField(_, field) => field,
            ConfigError::MissingEntrypoint(_) => "missing entrypoint",
            ConfigError::InvalidEntrypointScheme(_, _) => "entrypoint must use HTTP(S)",
            ConfigError::EmptyAllowedHost(_) => "empty allowed host",
            ConfigError::InvalidMaxDepth(_, _) => "max_depth must be positive",
            ConfigError::InvalidRequestBudget(_, _) => "request_budget must be positive",
            ConfigError::InvalidMediaStrategy(_, _) => "unsupported media_strategy",
        };
        crate::runtime::CliError::config(format!("{ctx}: invalid source configuration: {reason}"))
    })?;
    Ok(config)
}
