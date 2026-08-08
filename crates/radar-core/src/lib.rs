//! Pure domain model and deterministic algorithms for `math_talk_radar`.
//!
//! `radar-core` contains no I/O: no HTTP client, no persistence, no parsing of
//! external documents. It defines the canonical domain model (events, talks,
//! people, topics, dates, media) and the pure algorithms that operate on
//! already-fetched data (normalization, matching, deduplication, ranking).
//!
//! Boundary rules (see crate `AGENTS.md`):
//! - no `reqwest`, no `redb`, no `scraper` here;
//! - all behavior is deterministic given identical inputs.
#![forbid(unsafe_code)]

pub mod adapter;
pub mod config;
pub mod date;
pub mod dedup;
pub mod document;
pub mod model;
pub mod normalize;
pub mod people;
pub mod ranking;
pub mod topics;

pub use adapter::{AdapterError, EventCandidate, EventStub, SourceAdapter};
pub use config::{AdapterKind, SourceKind, SourceSpec, SourceTier};
pub use date::{
    DateError, DatePrecision, DateTimeOrDate, DateTimeRange, EventDate, parse_timezone,
};
pub use dedup::DedupSignal;
pub use document::{FetchPlan, FetchedDocument};
pub use model::{
    AccessInfo, Event, EventId, EventStatus, EventType, Location, MediaId, MediaResource,
    MediaType, OnlineAvailability, PublicAccess, SourceEvidence, SourceHealth, SourceStatus, Talk,
    TalkId, deterministic_id,
};
pub use normalize::normalize_text;
pub use people::{PersonHit, PersonRole, ScholarRecord};
pub use ranking::{
    MAX_ACCESS, MAX_COMPLETENESS, MAX_MEDIA, MAX_PEOPLE, MAX_SOURCE_TIER, MAX_TOPIC,
    ScoreComponents,
};
pub use topics::{TopicMatch, TopicRecord};
