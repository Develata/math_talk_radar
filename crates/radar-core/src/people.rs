//! Person model and scholar matching (§6, §6.1, §6.2).
//!
//! Role protection (§P-2, §6.2): a name in body text can yield at most
//! `TitleMention` / `Unknown`. Structured person fields or strong
//! name-in-context evidence are required for `Speaker` / `Organizer` / etc.
//! The matcher itself lands in M1.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonRole {
    Speaker,
    Lecturer,
    Organizer,
    Participant,
    Panelist,
    Honoree,
    SeriesNamesake,
    TitleMention,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonHit {
    pub canonical_name: String,
    pub matched_text: String,
    pub role: PersonRole,
    #[serde(default)]
    pub evidence: Option<String>,
    pub confidence: f32,
    #[serde(default)]
    pub scholar_tags: Vec<String>,
}

/// A curated scholar entry loaded from `config/scholars.toml` (§6.1).
/// Kept decoupled from any parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScholarRecord {
    pub id: String,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
