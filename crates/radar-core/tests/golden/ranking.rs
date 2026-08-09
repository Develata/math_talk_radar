//! Ranking score composer golden tests — parses `ranking.toml` and evaluates
//! each case against `radar_core::ranking::score_event`.

use std::collections::HashMap;

use radar_core::config::SourceTier;
use radar_core::date::{DatePrecision, EventDate};
use radar_core::model::{
    AccessInfo, Event, EventId, EventStatus, EventType, Location, MediaId, MediaResource,
    MediaType, OnlineAvailability, PublicAccess, SourceEvidence, Talk, TalkId,
};
use radar_core::people::{PersonHit, PersonRole};
use radar_core::ranking::{InterestWeights, ScoreComponents, score_event};
use radar_core::topics::TopicMatch;
use serde::Deserialize;
use url::Url;

// --- TOML schema -----------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct Cases {
    cases: Vec<RankingCase>,
}

#[derive(Deserialize)]
pub(crate) struct RankingCase {
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    media: Vec<MediaSpec>,
    #[serde(default)]
    access_online: String,
    #[serde(default)]
    source_tiers: Vec<String>,
    #[serde(default)]
    people: Vec<PersonSpec>,
    #[serde(default)]
    has_description: bool,
    #[serde(default)]
    has_abstract: bool,
    #[serde(default)]
    has_location: bool,
    #[serde(default)]
    talk_count: usize,
    #[serde(default)]
    interests: Option<HashMap<String, f64>>,
    expected_topic: u8,
    expected_media: u8,
    expected_access: u8,
    expected_source_tier: u8,
    expected_people: u8,
    expected_completeness: u8,
    expected_total: f32,
}

#[derive(Deserialize)]
pub(crate) struct MediaSpec {
    #[serde(rename = "type")]
    media_type: String,
    access: String,
}

#[derive(Deserialize)]
pub(crate) struct PersonSpec {
    canonical: String,
    role: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Default)]
pub struct RankingStats {
    pub total: usize,
    pub passed: usize,
    pub failures: Vec<String>,
}

// --- Helpers ----------------------------------------------------------------

fn empty_source_evidence() -> SourceEvidence {
    SourceEvidence {
        source_id: String::new(),
        source_url: Url::parse("https://example.com").expect("static URL"),
        evidence: None,
        captured_at: None,
        native_id: None,
    }
}

fn empty_event() -> Event {
    Event {
        id: EventId(String::new()),
        title: String::new(),
        url: None,
        event_type: EventType::Unknown,
        status: EventStatus::Unknown,
        date: EventDate {
            start: None,
            end: None,
            timezone: None,
            original_text: String::new(),
            precision: DatePrecision::Unknown,
        },
        location: None,
        description: None,
        topics: Vec::new(),
        people: Vec::new(),
        talks: Vec::new(),
        media: Vec::new(),
        access: AccessInfo {
            access: PublicAccess::Unknown,
            online: OnlineAvailability::Unknown,
        },
        sources: Vec::new(),
        score: 0.0,
        score_components: ScoreComponents::default(),
        rank_reasons: Vec::new(),
        first_seen_at: None,
        last_seen_at: None,
    }
}

fn parse_media_type(s: &str) -> MediaType {
    match s {
        "video" => MediaType::Video,
        "audio" => MediaType::Audio,
        "slides" => MediaType::Slides,
        "lecture_notes" => MediaType::LectureNotes,
        "transcript" => MediaType::Transcript,
        "program_pdf" => MediaType::ProgramPdf,
        "abstract_pdf" => MediaType::AbstractPdf,
        "livestream" => MediaType::Livestream,
        "playlist" => MediaType::Playlist,
        _ => MediaType::Other,
    }
}

fn parse_public_access(s: &str) -> PublicAccess {
    match s {
        "open" => PublicAccess::Open,
        "registration_required" => PublicAccess::RegistrationRequired,
        "institution_login" => PublicAccess::InstitutionLogin,
        "paywalled" => PublicAccess::Paywalled,
        "in_person_only" => PublicAccess::InPersonOnly,
        _ => PublicAccess::Unknown,
    }
}

fn parse_online_availability(s: &str) -> OnlineAvailability {
    match s {
        "livestream" => OnlineAvailability::Livestream,
        "hybrid" => OnlineAvailability::Hybrid,
        "recording_available" => OnlineAvailability::RecordingAvailable,
        "recording_expected" => OnlineAvailability::RecordingExpected,
        "no_online_access" => OnlineAvailability::NoOnlineAccess,
        _ => OnlineAvailability::Unknown,
    }
}

fn parse_person_role(s: &str) -> PersonRole {
    match s {
        "speaker" => PersonRole::Speaker,
        "lecturer" => PersonRole::Lecturer,
        "organizer" => PersonRole::Organizer,
        "participant" => PersonRole::Participant,
        "panelist" => PersonRole::Panelist,
        "honoree" => PersonRole::Honoree,
        "series_namesake" => PersonRole::SeriesNamesake,
        "title_mention" => PersonRole::TitleMention,
        _ => PersonRole::Unknown,
    }
}

fn source_tier_map() -> HashMap<String, SourceTier> {
    let mut m = HashMap::new();
    m.insert("s-tier-src".to_string(), SourceTier::S);
    m.insert("a-tier-src".to_string(), SourceTier::A);
    m.insert("b-tier-src".to_string(), SourceTier::B);
    m.insert("unknown-src".to_string(), SourceTier::Unknown);
    m
}

fn make_topic_match(topic_id: &str) -> TopicMatch {
    TopicMatch {
        topic_id: topic_id.to_string(),
        canonical_name: topic_id.to_string(),
        matched_text: topic_id.to_string(),
        confidence: 1.0,
    }
}

fn make_media(spec: &MediaSpec) -> MediaResource {
    MediaResource {
        id: MediaId("m1".to_string()),
        media_type: parse_media_type(&spec.media_type),
        title: None,
        url: Url::parse("https://example.com").expect("static URL"),
        platform: None,
        public_access: parse_public_access(&spec.access),
        published_at: None,
        source: empty_source_evidence(),
    }
}

fn make_person(spec: &PersonSpec) -> PersonHit {
    PersonHit {
        canonical_name: spec.canonical.clone(),
        matched_text: spec.canonical.clone(),
        role: parse_person_role(&spec.role),
        evidence: None,
        confidence: 0.5,
        scholar_tags: spec.tags.clone(),
    }
}

fn make_talk(has_abstract: bool) -> Talk {
    Talk {
        id: TalkId("t1".to_string()),
        title: "Talk".to_string(),
        speaker: Vec::new(),
        date_time: None,
        abstract_text: has_abstract.then(|| "Abstract text".to_string()),
        topics: Vec::new(),
        media: Vec::new(),
        source: empty_source_evidence(),
    }
}

fn build_event(case: &RankingCase) -> Event {
    let mut event = empty_event();

    event.topics = case.topics.iter().map(|t| make_topic_match(t)).collect();
    event.media = case.media.iter().map(make_media).collect();
    event.access = AccessInfo {
        access: PublicAccess::Unknown,
        online: parse_online_availability(&case.access_online),
    };
    event.sources = case
        .source_tiers
        .iter()
        .map(|s| SourceEvidence {
            source_id: s.clone(),
            source_url: Url::parse("https://example.com").expect("static URL"),
            evidence: None,
            captured_at: None,
            native_id: None,
        })
        .collect();
    event.people = case.people.iter().map(make_person).collect();

    if case.has_description {
        event.description = Some("Description text".to_string());
    }
    if case.has_location {
        event.location = Some(Location {
            name: "Location".to_string(),
            city: None,
            country: None,
            venue: None,
        });
    }

    let talk_count = if case.has_abstract {
        case.talk_count.max(1)
    } else {
        case.talk_count
    };
    event.talks = (0..talk_count)
        .map(|i| make_talk(i == 0 && case.has_abstract))
        .collect();

    event
}

// --- Runner -----------------------------------------------------------------

pub fn run(data: &str) -> RankingStats {
    let parsed: Cases = toml::from_str(data).expect("ranking.toml parses");
    let total = parsed.cases.len();
    let mut passed = 0usize;
    let mut failures = Vec::new();
    let tiers = source_tier_map();

    for case in &parsed.cases {
        let event = build_event(case);
        let interests = case.interests.as_ref().map(|m| InterestWeights(m.clone()));
        let (total_score, components, _) = score_event(&event, &tiers, interests.as_ref());

        let ok = components.topic == case.expected_topic
            && components.media == case.expected_media
            && components.access == case.expected_access
            && components.source_tier == case.expected_source_tier
            && components.people == case.expected_people
            && components.completeness == case.expected_completeness
            && (total_score - case.expected_total).abs() < 1e-6;

        if ok {
            passed += 1;
        } else {
            failures.push(format!(
                "{}: expected (t={},m={},a={},s={},p={},c={},total={}); \
                 got (t={},m={},a={},s={},p={},c={},total={})",
                case.id,
                case.expected_topic,
                case.expected_media,
                case.expected_access,
                case.expected_source_tier,
                case.expected_people,
                case.expected_completeness,
                case.expected_total,
                components.topic,
                components.media,
                components.access,
                components.source_tier,
                components.people,
                components.completeness,
                total_score
            ));
        }
    }

    RankingStats {
        total,
        passed,
        failures,
    }
}
