//! Source registry config model (§17). Loaded from `config/sources.toml`.
use serde::{Deserialize, Serialize};
use url::Url;

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
    pub fn embedded() -> Self {
        Self::parse(include_str!("../../../config/sources.toml"))
            .expect("embedded sources.toml must parse at compile time")
    }

    /// Only sources with `enabled = true`.
    pub fn enabled(&self) -> Vec<&SourceSpec> {
        self.sources.iter().filter(|s| s.enabled).collect()
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
        let config = super::SourcesConfig::embedded();
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
}
