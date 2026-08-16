//! Source registry config model (§17). Loaded from `config/sources.toml`.
use serde::{Deserialize, Serialize};
use url::Url;

/// R9-M02: semantic config validation error. `SourcesConfig::parse` only
/// performs TOML deserialization — it cannot catch duplicate source IDs or
/// an `HtmlConfig` source missing its required selector fields. Those are
/// semantic invariants (source ID is the key for dedup / manifest / change
/// detection; the HTML adapter's four required selectors are non-optional per
/// `HtmlSelectors`'s contract) and were previously surfaced only as a late
/// runtime failure inside the adapter mid-scan. `validate()` checks them at
/// load time so a malformed config fails fast with a precise message.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("duplicate source id '{0}'")]
    DuplicateSourceId(String),
    #[error("source '{0}': adapter = HtmlConfig requires a [selectors] table")]
    MissingSelectors(String),
    #[error("source '{0}': selectors.{1} must be a non-empty string")]
    EmptySelector(String, &'static str),
    #[error("source '{0}': {1} must be a non-empty string")]
    EmptyField(String, &'static str),
    #[error("source '{0}': enabled source requires an entrypoint URL")]
    MissingEntrypoint(String),
    #[error("source '{0}': entrypoint must be http or https, got '{1}'")]
    InvalidEntrypointScheme(String, String),
    #[error("source '{0}': allowed_hosts contains an empty string")]
    EmptyAllowedHost(String),
    #[error("source '{0}': max_depth must be >= 1, got {1}")]
    InvalidMaxDepth(String, u8),
    #[error("source '{0}': request_budget must be >= 1, got {1}")]
    InvalidRequestBudget(String, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTier {
    S,
    A,
    B,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    InstitutionCalendar,
    ConferenceSeries,
    RssFeed,
    IcsFeed,
    Indico,
    JsonLd,
    MediaArchive,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Rss,
    Ics,
    JsonLd,
    Indico,
    HtmlConfig,
    HtmlGeneric,
    #[default]
    None,
}

/// CSS selectors for the configured HTML adapter (SRC-004, ADR-0005). Carried
/// on `SourceSpec::selectors`; only consulted by `AdapterKind::HtmlConfig`.
/// `list`/`list_link`/`detail_title`/`detail_date` are required when the
/// adapter is in use; the `detail_*` optional fields default to `None` when
/// absent from TOML.
///
/// `list_title` and `list_date` (§P-5, added post-M6) are optional overrides
/// for sites where the event title or date does not live on the link element
/// itself (e.g. AMS Calendar puts the title in a sibling `dd.event_title`, MIT
/// puts it in a `td > strong`). When `list_title` is absent, the adapter falls
/// back to the link's own text — preserving the original contract. Per §7 /
/// §64, adding optional fields is schema-compatible in v0.x.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HtmlSelectors {
    pub list: String,
    pub list_link: String,
    pub detail_title: String,
    pub detail_date: String,
    #[serde(default)]
    pub detail_location: Option<String>,
    #[serde(default)]
    pub detail_description: Option<String>,
    #[serde(default)]
    pub detail_speaker: Option<String>,
    /// Override the event title source on the list page. Selector is matched
    /// within each `list` container; the first match's text wins. When absent,
    /// the `list_link` element's text is used (legacy behavior).
    #[serde(default)]
    pub list_title: Option<String>,
    /// Override the event date source on the list page. Selector is matched
    /// within each `list` container; the first match's text is fed to
    /// `parse_date` and stored as `EventStub::date_hint`. When absent, no
    /// date hint is extracted at discovery.
    #[serde(default)]
    pub list_date: Option<String>,
}

/// A source entry loaded from `config/sources.toml` (§17). Adapters and the
/// fetch coordinator consume this; it is the source registry's runtime shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tier: SourceTier,
    #[serde(default)]
    pub kind: SourceKind,
    #[serde(default)]
    pub adapter: AdapterKind,
    #[serde(default)]
    pub entrypoint: Option<Url>,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: u8,
    #[serde(default = "default_request_budget")]
    pub request_budget: u32,
    #[serde(default)]
    pub media_strategy: Option<String>,
    #[serde(default)]
    pub dynamic: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub selectors: Option<HtmlSelectors>,
}

fn default_max_depth() -> u8 {
    2
}

fn default_request_budget() -> u32 {
    20
}

/// Wrapper for the `sources.toml` file shape: a top-level `[[sources]]` array.
/// Loaded by the CLI at startup (§33). The embedded default (CFG-001) ships
/// with the binary so the CLI works without a user config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcesConfig {
    #[serde(default)]
    pub sources: Vec<SourceSpec>,
}

impl SourcesConfig {
    /// Parse a `sources.toml` document. Returns a config with an empty source
    /// list for empty input — the caller decides whether zero sources is an
    /// error (it is, for `scan`: HTTP-005 exit 4).
    pub fn parse(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// The embedded default source registry shipped with the binary (CFG-001).
    /// M0 ships an empty list; M6 promotes audited sources here.
    ///
    /// B5: returns `Result` instead of panicking. The embedded TOML is part of
    /// the source tree and verified by `cfg_001_embedded_default_config_parses`
    /// in CI, but a malformed file would have panicked at runtime with a
    /// misleading "compile time" message. Callers now handle the error.
    pub fn embedded() -> Result<Self, toml::de::Error> {
        Self::parse(include_str!("../../../config/sources.toml"))
    }

    /// Only sources with `enabled = true`.
    pub fn enabled(&self) -> Vec<&SourceSpec> {
        self.sources.iter().filter(|s| s.enabled).collect()
    }

    /// R9-M02: validate semantic invariants that TOML deserialization cannot
    /// enforce. Call after `parse` (or `embedded`) to fail fast at load time
    /// with a precise message instead of a late runtime failure mid-scan.
    /// Checks: (1) source IDs are unique, (2) `HtmlConfig` sources carry a
    /// `[selectors]` table with all four required fields non-empty, (3) id
    /// and name are non-empty, (4) enabled sources have an http/https
    /// entrypoint, (5) allowed_hosts has no empty entries, (6) max_depth and
    /// request_budget are >= 1.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut seen_ids = std::collections::HashSet::new();
        for source in &self.sources {
            if !seen_ids.insert(source.id.as_str()) {
                return Err(ConfigError::DuplicateSourceId(source.id.clone()));
            }
            if source.id.trim().is_empty() {
                return Err(ConfigError::EmptyField(source.id.clone(), "id"));
            }
            if source.name.trim().is_empty() {
                return Err(ConfigError::EmptyField(source.id.clone(), "name"));
            }
            if source.enabled {
                let entrypoint = source
                    .entrypoint
                    .as_ref()
                    .ok_or_else(|| ConfigError::MissingEntrypoint(source.id.clone()))?;
                let scheme = entrypoint.scheme();
                if scheme != "http" && scheme != "https" {
                    return Err(ConfigError::InvalidEntrypointScheme(
                        source.id.clone(),
                        scheme.to_string(),
                    ));
                }
            }
            if source.allowed_hosts.iter().any(|h| h.trim().is_empty()) {
                return Err(ConfigError::EmptyAllowedHost(source.id.clone()));
            }
            if source.max_depth < 1 {
                return Err(ConfigError::InvalidMaxDepth(
                    source.id.clone(),
                    source.max_depth,
                ));
            }
            if source.request_budget < 1 {
                return Err(ConfigError::InvalidRequestBudget(
                    source.id.clone(),
                    source.request_budget,
                ));
            }
            if source.adapter == AdapterKind::HtmlConfig {
                let selectors = source
                    .selectors
                    .as_ref()
                    .ok_or_else(|| ConfigError::MissingSelectors(source.id.clone()))?;
                for (field, value) in [
                    ("list", &selectors.list),
                    ("list_link", &selectors.list_link),
                    ("detail_title", &selectors.detail_title),
                    ("detail_date", &selectors.detail_date),
                ] {
                    if value.trim().is_empty() {
                        return Err(ConfigError::EmptySelector(source.id.clone(), field));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SourceSpec;

    const MINIMAL_TOML: &str = r#"
id = "mpim-bonn"
name = "MPIM Bonn"
entrypoint = "https://www.mpim-bonn.mpg.de/events"
"#;

    #[test]
    fn source_spec_deserializes_without_selectors_field() {
        let source: SourceSpec = toml::from_str(MINIMAL_TOML).expect("minimal SourceSpec parses");
        assert_eq!(source.id, "mpim-bonn");
        assert_eq!(source.name, "MPIM Bonn");
        assert!(source.fixture.is_none());
    }

    const SELECTORS_TOML: &str = r#"
id = "mpim-bonn"
name = "MPIM Bonn"
entrypoint = "https://www.mpim-bonn.mpg.de/events"

[selectors]
list = ".event-list .event"
list_link = "a.event-link"
detail_title = "h1.event-title"
detail_date = ".event-date"
detail_location = ".event-location"
detail_description = ".event-abstract"
detail_speaker = ".event-speaker"
"#;

    #[test]
    fn source_spec_deserializes_with_selectors_section() {
        let source: SourceSpec =
            toml::from_str(SELECTORS_TOML).expect("SourceSpec with selectors parses");
        let selectors = source
            .selectors
            .as_ref()
            .expect("selectors field is Some when [selectors] present");
        assert_eq!(selectors.list, ".event-list .event");
        assert_eq!(selectors.list_link, "a.event-link");
        assert_eq!(selectors.detail_title, "h1.event-title");
        assert_eq!(selectors.detail_date, ".event-date");
        assert_eq!(
            selectors.detail_location.as_deref(),
            Some(".event-location")
        );
        assert_eq!(
            selectors.detail_description.as_deref(),
            Some(".event-abstract")
        );
        assert_eq!(selectors.detail_speaker.as_deref(), Some(".event-speaker"));
    }

    #[test]
    fn source_spec_rejects_non_table_selectors_gracefully() {
        let toml = r#"
id = "mpim-bonn"
name = "MPIM Bonn"
selectors = "not_a_table"
"#;
        let result: Result<SourceSpec, toml::de::Error> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "non-table selectors must fail to deserialize, not panic or silently accept"
        );
    }

    #[test]
    fn source_spec_selectors_optional_fields_default_to_none() {
        let toml = r#"
id = "mpim-bonn"
name = "MPIM Bonn"

[selectors]
list = "li.event"
list_link = "a"
detail_title = "h1"
detail_date = "time"
"#;
        let source: SourceSpec = toml::from_str(toml).expect("minimal selectors parse");
        let selectors = source.selectors.expect("selectors present");
        assert_eq!(selectors.list, "li.event");
        assert!(selectors.detail_location.is_none());
        assert!(selectors.detail_description.is_none());
        assert!(selectors.detail_speaker.is_none());
    }

    // CFG-001: embedded default config exists and parses.
    #[test]
    fn cfg_001_embedded_default_config_parses() {
        let config = super::SourcesConfig::embedded().expect("embedded sources.toml parses");
        // M0 ships an empty list; M6 promotes audited sources. The contract
        // is that the embedded file parses without error, not that it has
        // a minimum source count.
        let _ = config.sources.len();
    }

    #[test]
    fn sources_config_parses_empty_as_zero_sources() {
        let config = super::SourcesConfig::parse("").expect("empty TOML parses");
        assert!(config.sources.is_empty());
    }

    #[test]
    fn sources_config_parses_array_of_sources() {
        let toml = r#"
[[sources]]
id = "s1"
name = "Source 1"
enabled = true

[[sources]]
id = "s2"
name = "Source 2"
enabled = false
"#;
        let config = super::SourcesConfig::parse(toml).expect("two-source TOML parses");
        assert_eq!(config.sources.len(), 2);
        let enabled = config.enabled();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "s1");
    }

    // R9-M02: validate() rejects duplicate source IDs.
    #[test]
    fn validate_rejects_duplicate_source_id() {
        let toml = r#"
[[sources]]
id = "dup"
name = "First"

[[sources]]
id = "dup"
name = "Second"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("duplicate id must fail");
        assert!(matches!(err, super::ConfigError::DuplicateSourceId(ref id) if id == "dup"));
    }

    // R9-M02: validate() rejects HtmlConfig source without a [selectors] table.
    #[test]
    fn validate_rejects_html_config_missing_selectors() {
        let toml = r#"
[[sources]]
id = "no-sel"
name = "No Selectors"
adapter = "html_config"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("missing selectors must fail");
        assert!(matches!(err, super::ConfigError::MissingSelectors(ref id) if id == "no-sel"));
    }

    // R9-M02: validate() rejects HtmlConfig source with an empty required
    // selector field (whitespace-only counts as empty).
    #[test]
    fn validate_rejects_html_config_empty_required_selector() {
        let toml = r#"
[[sources]]
id = "empty-sel"
name = "Empty Selector"
adapter = "html_config"

[sources.selectors]
list = "   "
list_link = "a"
detail_title = "h1"
detail_date = "time"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("empty selector must fail");
        assert!(
            matches!(err, super::ConfigError::EmptySelector(ref id, field) if id == "empty-sel" && field == "list")
        );
    }

    // R9-M02: validate() accepts a well-formed HtmlConfig source with all
    // four required selectors non-empty and optional selectors absent.
    #[test]
    fn validate_accepts_well_formed_html_config() {
        let toml = r#"
[[sources]]
id = "good"
name = "Good"
adapter = "html_config"

[sources.selectors]
list = ".event"
list_link = "a"
detail_title = "h1"
detail_date = "time"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        config.validate().expect("well-formed config must pass");
    }

    // R9-M02: validate() accepts non-HtmlConfig sources without selectors.
    #[test]
    fn validate_accepts_rss_source_without_selectors() {
        let toml = r#"
[[sources]]
id = "feed"
name = "RSS Feed"
adapter = "rss"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        config
            .validate()
            .expect("RSS source without selectors must pass");
    }

    // R9-M02: the embedded default config must pass semantic validation.
    // If this fails, the shipped config is broken and every `scan` would
    // fail at load time.
    #[test]
    fn cfg_embedded_default_passes_validation() {
        let config = super::SourcesConfig::embedded().expect("embedded sources.toml parses");
        config
            .validate()
            .expect("embedded sources.toml must pass semantic validation");
    }

    // R9-M02: validate() rejects an empty id.
    #[test]
    fn validate_rejects_empty_id() {
        let toml = r#"
[[sources]]
id = ""
name = "Has Name"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("empty id must fail");
        assert!(
            matches!(err, super::ConfigError::EmptyField(ref id, f) if id.is_empty() && f == "id")
        );
    }

    // R9-M02: validate() rejects a whitespace-only id (trim before check).
    #[test]
    fn validate_rejects_whitespace_id() {
        let toml = r#"
[[sources]]
id = "   "
name = "Has Name"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("whitespace id must fail");
        assert!(matches!(err, super::ConfigError::EmptyField(_, f) if f == "id"));
    }

    // R9-M02: validate() rejects an empty name.
    #[test]
    fn validate_rejects_empty_name() {
        let toml = r#"
[[sources]]
id = "no-name"
name = ""
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("empty name must fail");
        assert!(
            matches!(err, super::ConfigError::EmptyField(ref id, f) if id == "no-name" && f == "name")
        );
    }

    // R9-M02: validate() rejects an enabled source without an entrypoint.
    #[test]
    fn validate_rejects_enabled_without_entrypoint() {
        let toml = r#"
[[sources]]
id = "no-ep"
name = "No Entrypoint"
enabled = true
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config
            .validate()
            .expect_err("enabled without entrypoint must fail");
        assert!(matches!(err, super::ConfigError::MissingEntrypoint(ref id) if id == "no-ep"));
    }

    // R9-M02: validate() accepts a disabled source without an entrypoint.
    #[test]
    fn validate_accepts_disabled_without_entrypoint() {
        let toml = r#"
[[sources]]
id = "no-ep-disabled"
name = "Disabled No EP"
enabled = false
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        config
            .validate()
            .expect("disabled source without entrypoint must pass");
    }

    // R9-M02: validate() rejects an enabled source with a non-http(s) scheme.
    #[test]
    fn validate_rejects_invalid_entrypoint_scheme() {
        let toml = r#"
[[sources]]
id = "ftp-src"
name = "FTP Source"
enabled = true
entrypoint = "ftp://example.com/"
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("ftp scheme must fail");
        assert!(
            matches!(err, super::ConfigError::InvalidEntrypointScheme(ref id, ref s) if id == "ftp-src" && s == "ftp")
        );
    }

    // R9-M02: validate() rejects allowed_hosts containing an empty string.
    #[test]
    fn validate_rejects_empty_allowed_host() {
        let toml = r#"
[[sources]]
id = "bad-hosts"
name = "Bad Hosts"
entrypoint = "https://example.com/"
allowed_hosts = ["example.com", ""]
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("empty allowed_host must fail");
        assert!(matches!(err, super::ConfigError::EmptyAllowedHost(ref id) if id == "bad-hosts"));
    }

    // R9-M02: validate() rejects max_depth = 0.
    #[test]
    fn validate_rejects_zero_max_depth() {
        let toml = r#"
[[sources]]
id = "zero-depth"
name = "Zero Depth"
entrypoint = "https://example.com/"
max_depth = 0
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("max_depth=0 must fail");
        assert!(
            matches!(err, super::ConfigError::InvalidMaxDepth(ref id, d) if id == "zero-depth" && d == 0)
        );
    }

    // R9-M02: validate() rejects request_budget = 0.
    #[test]
    fn validate_rejects_zero_request_budget() {
        let toml = r#"
[[sources]]
id = "zero-budget"
name = "Zero Budget"
entrypoint = "https://example.com/"
request_budget = 0
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        let err = config.validate().expect_err("request_budget=0 must fail");
        assert!(
            matches!(err, super::ConfigError::InvalidRequestBudget(ref id, b) if id == "zero-budget" && b == 0)
        );
    }

    // R9-M02: deny_unknown_fields rejects an unknown key in a source.
    #[test]
    fn deny_unknown_fields_rejects_unknown_source_key() {
        let toml = r#"
[[sources]]
id = "bad"
name = "Bad"
typo_field = "oops"
"#;
        let result = super::SourcesConfig::parse(toml);
        assert!(
            result.is_err(),
            "unknown field must fail at parse time, not silently ignored"
        );
    }

    // R9-M02: deny_unknown_fields rejects an unknown key in [selectors].
    #[test]
    fn deny_unknown_fields_rejects_unknown_selector_key() {
        let toml = r#"
[[sources]]
id = "bad-sel"
name = "Bad Selectors"
adapter = "html_config"

[sources.selectors]
list = ".e"
list_link = "a"
detail_title = "h1"
detail_date = "time"
bogus_selector = "x"
"#;
        let result = super::SourcesConfig::parse(toml);
        assert!(
            result.is_err(),
            "unknown selector field must fail at parse time"
        );
    }

    // R9-M02: a fully well-formed enabled source passes all checks.
    #[test]
    fn validate_accepts_complete_enabled_source() {
        let toml = r#"
[[sources]]
id = "good-enabled"
name = "Good Enabled"
adapter = "rss"
entrypoint = "https://example.com/feed"
allowed_hosts = ["example.com"]
max_depth = 3
request_budget = 30
enabled = true
"#;
        let config = super::SourcesConfig::parse(toml).expect("TOML parses");
        config
            .validate()
            .expect("complete enabled source must pass");
    }
}
