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
}

fn default_max_depth() -> u8 {
    2
}

fn default_request_budget() -> u32 {
    20
}
