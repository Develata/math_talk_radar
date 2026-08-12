//! Canonical domain model (§5, §6, §9, §21, §43).
//!
//! Types here are the public JSON contract surface. Field names use
//! `#[serde(rename_all = "snake_case")]` via per-enum attributes so the wire
//! shape matches the engineering contract exactly.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::date::{DateTimeRange, EventDate};
use crate::normalize::normalize_name;
use crate::people::PersonHit;
use crate::ranking::ScoreComponents;
use crate::topics::TopicMatch;

// ---- Stable deterministic IDs (§24) -------------------------------------

/// Stable event identity. Constructed via [`deterministic_id`] over normalized
/// `title + organizer/domain + start_date`. Never random, never time-based.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TalkId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(pub String);

/// Deterministic identity hash (§24): BLAKE3 over joined, normalized fields.
/// Joined with the ASCII unit separator `\x1f` so `("a","bc")` != `("ab","c")`.
pub fn deterministic_id(parts: &[&str]) -> String {
    let joined = parts.join("\x1f");
    let hash = blake3::hash(joined.as_bytes());
    format!("blake3:{hash}")
}

/// Compute a stable [`EventId`] from an event's title and canonical URL,
/// normalizing the title (NFC + lowercase + whitespace collapse) before
/// hashing. All adapters MUST use this function so the same event discovered
/// via different adapter kinds produces the same id (§24 cross-adapter
/// identity consistency).
///
/// The URL is canonicalized before hashing: fragment is stripped and a
/// trailing slash on the path is removed, so `…/e/1`, `…/e/1/`, and
/// `…/e/1#top` produce the same id. Query parameters are preserved (they may
/// be semantically meaningful for identity). Malformed URLs fall back to the
/// raw string to preserve backward compatibility.
pub fn event_id(title: &str, url: &str) -> EventId {
    let normalized = normalize_name(title);
    let canon_url = canonicalize_url_for_id(url);
    EventId(deterministic_id(&[&normalized, &canon_url]))
}

fn canonicalize_url_for_id(url: &str) -> String {
    let mut parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return url.to_string(),
    };
    parsed.set_fragment(None);
    let path = parsed.path().to_string();
    if path.len() > 1 && path.ends_with('/') {
        let trimmed = &path[..path.len() - 1];
        parsed.set_path(if trimmed.is_empty() { "/" } else { trimmed });
    }
    parsed.to_string()
}

// ---- Event (§5.1) --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub title: String,
    /// Event's canonical detail-page URL (from the discovering stub). Used by
    /// the §25 `CanonicalUrl` dedup signal. Per §64, adding an optional field
    /// is schema-compatible in v0.x.
    #[serde(default)]
    pub url: Option<Url>,
    pub event_type: EventType,
    pub status: EventStatus,
    pub date: EventDate,
    #[serde(default)]
    pub location: Option<Location>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub topics: Vec<TopicMatch>,
    #[serde(default)]
    pub people: Vec<PersonHit>,
    #[serde(default)]
    pub talks: Vec<Talk>,
    #[serde(default)]
    pub media: Vec<MediaResource>,
    pub access: AccessInfo,
    #[serde(default)]
    pub sources: Vec<SourceEvidence>,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub score_components: ScoreComponents,
    #[serde(default)]
    pub rank_reasons: Vec<String>,
    #[serde(default)]
    pub first_seen_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Conference,
    Workshop,
    ResearchProgram,
    PublicLecture,
    DistinguishedLecture,
    LectureSeries,
    SummerSchool,
    MiniCourse,
    Colloquium,
    Panel,
    AwardLecture,
    MemorialConference,
    Seminar,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Announced,
    RegistrationOpen,
    Upcoming,
    Ongoing,
    Completed,
    MediaPending,
    MediaAvailable,
    Archived,
    Cancelled,
    Unknown,
}

// ---- Talk (§5.3) ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Talk {
    pub id: TalkId,
    pub title: String,
    pub speaker: Vec<PersonHit>,
    pub date_time: Option<DateTimeRange>,
    pub abstract_text: Option<String>,
    pub topics: Vec<TopicMatch>,
    pub media: Vec<MediaResource>,
    pub source: SourceEvidence,
}

// ---- Media (§5.4, §20) ---------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Video,
    Audio,
    Slides,
    LectureNotes,
    Transcript,
    ProgramPdf,
    AbstractPdf,
    Livestream,
    Playlist,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaResource {
    pub id: MediaId,
    pub media_type: MediaType,
    pub title: Option<String>,
    pub url: Url,
    pub platform: Option<String>,
    pub public_access: PublicAccess,
    pub published_at: Option<DateTime<Utc>>,
    pub source: SourceEvidence,
}

// ---- Access (§21) --------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicAccess {
    Open,
    RegistrationRequired,
    InstitutionLogin,
    Paywalled,
    InPersonOnly,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineAvailability {
    Livestream,
    Hybrid,
    RecordingAvailable,
    RecordingExpected,
    NoOnlineAccess,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessInfo {
    pub access: PublicAccess,
    pub online: OnlineAvailability,
}

// ---- Location ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub name: String,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub venue: Option<String>,
}

// ---- Evidence (§P-4) -----------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub source_id: String,
    pub source_url: Url,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    pub captured_at: Option<DateTime<Utc>>,
    /// Source-declared canonical event id (e.g. Indico event id, ICS UID). Used
    /// by the §25 `SourceCanonicalId` dedup signal. Optional: most sources do
    /// not declare one. Per §64, adding an optional field is schema-compatible
    /// in v0.x (no `schema_version` bump).
    #[serde(default)]
    pub native_id: Option<String>,
}

// ---- Source health (§43) -------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Ok,
    Partial,
    Timeout,
    HttpError,
    ParseError,
    RobotsDenied,
    DynamicUnsupported,
    BudgetExhausted,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceHealth {
    pub source: String,
    pub status: SourceStatus,
    pub duration_ms: u64,
    pub requests: u32,
    pub events: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_id_canonicalizes_fragment() {
        let a = event_id("Talk", "https://example.com/e/1");
        let b = event_id("Talk", "https://example.com/e/1#top");
        assert_eq!(a, b, "fragment must not affect event_id");
    }

    #[test]
    fn event_id_canonicalizes_trailing_slash() {
        let a = event_id("Talk", "https://example.com/e/1");
        let b = event_id("Talk", "https://example.com/e/1/");
        assert_eq!(a, b, "trailing slash must not affect event_id");
    }

    #[test]
    fn event_id_preserves_query() {
        let a = event_id("Talk", "https://example.com/e/1?session=abc");
        let b = event_id("Talk", "https://example.com/e/1?session=def");
        assert_ne!(a, b, "different query params must produce different ids");
    }

    #[test]
    fn event_id_root_path_keeps_slash() {
        let a = event_id("Talk", "https://example.com/");
        let b = event_id("Talk", "https://example.com");
        assert_eq!(a, b, "root path '/' and empty path must collide");
    }

    #[test]
    fn event_id_malformed_url_falls_back_to_raw() {
        let id = event_id("Talk", "not a url");
        assert!(id.0.starts_with("blake3:"));
    }
}
