//! Canonical domain model (§5, §6, §9, §21, §43).
//!
//! Types here are the public JSON contract surface. Field names use
//! `#[serde(rename_all = "snake_case")]` via per-enum attributes so the wire
//! shape matches the engineering contract exactly.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::date::{DateTimeRange, EventDate};
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

// ---- Event (§5.1) --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub title: String,
    pub event_type: EventType,
    pub status: EventStatus,
    pub date: EventDate,
    pub location: Option<Location>,
    pub description: Option<String>,
    pub topics: Vec<TopicMatch>,
    pub people: Vec<PersonHit>,
    pub talks: Vec<Talk>,
    pub media: Vec<MediaResource>,
    pub access: AccessInfo,
    pub sources: Vec<SourceEvidence>,
    pub score: f32,
    pub score_components: ScoreComponents,
    pub rank_reasons: Vec<String>,
    pub first_seen_at: Option<DateTime<Utc>>,
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
