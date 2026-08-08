//! Cross-source deduplication signals (§25).
//!
//! Conservative deterministic dedup. Priority: canonical URL → source-declared
//! canonical ID → normalized title+date+organizer → normalized
//! title+date+location. Prefer keeping a suspected duplicate over merging two
//! distinct events. Fuzzy/semantic dedup is deferred. Full algorithm lands in
//! M3; this module establishes the signal ordering.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupSignal {
    /// Identical canonical URL.
    CanonicalUrl,
    /// Source declares the same canonical ID (e.g. Indico event id).
    SourceCanonicalId,
    /// Normalized title + start date + organizer match.
    TitleDateOrganizer,
    /// Normalized title + start date + location match.
    TitleDateLocation,
}
